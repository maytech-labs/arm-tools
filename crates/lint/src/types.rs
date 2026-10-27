//! Gradual types for assembly (docs/decisions/0039-types.md). Comments
//! declare function signatures and register types:
//!
//! ```asm
//! ; armtool: fn _sum(x0: ptr<i32>, x1: u64) -> i64
//! ; armtool: x9: ptr<u8>
//! ```
//!
//! Types flow through the control-flow graph from those annotations: across
//! moves, loads, pointer arithmetic, extends, and calls to annotated
//! functions. A register whose type isn't known, or differs between the paths
//! that reach a point, is unknown, and nothing is ever reported about an
//! unknown type, so code without annotations gets no findings.

use std::collections::HashMap;
use std::ops::Range;

use arm_isa::{Imm, Operand};
use arm_sema::{Analysis, Instruction};
use arm_syntax::SyntaxKind;

use crate::Finding;
use crate::flow::{self, Cfg, register_id};

/// A value's type.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Ty {
    Int {
        signed: bool,
        bits: u8,
    },
    Float {
        bits: u8,
    },
    /// A pointer, to a known type or to anything (`ptr`).
    Ptr(Option<Box<Ty>>),
}

impl Ty {
    /// Parses `i32`, `u8`, `f64`, `ptr`, `ptr<ptr<u8>>`.
    pub fn parse(text: &str) -> Result<Ty, String> {
        let text = text.trim();
        if let Some(inner) = text.strip_prefix("ptr") {
            let inner = inner.trim();
            if inner.is_empty() {
                return Ok(Ty::Ptr(None));
            }
            let Some(pointee) = inner.strip_prefix('<').and_then(|i| i.strip_suffix('>')) else {
                return Err(format!("'{text}': write ptr or ptr<T>"));
            };
            return Ok(Ty::Ptr(Some(Box::new(Ty::parse(pointee)?))));
        }
        let (kind, bits) = text.split_at(text.len().min(1));
        let bits: Option<u8> = bits.parse().ok();
        match (kind, bits) {
            ("i", Some(b @ (8 | 16 | 32 | 64))) => Ok(Ty::Int {
                signed: true,
                bits: b,
            }),
            ("u", Some(b @ (8 | 16 | 32 | 64))) => Ok(Ty::Int {
                signed: false,
                bits: b,
            }),
            ("f", Some(b @ (32 | 64))) => Ok(Ty::Float { bits: b }),
            _ => Err(format!(
                "unknown type '{text}': use i8 to i64, u8 to u64, f32, f64, ptr, or ptr<T>"
            )),
        }
    }

    /// Size in bytes.
    pub fn size(&self) -> u8 {
        match self {
            Ty::Int { bits, .. } | Ty::Float { bits } => bits / 8,
            Ty::Ptr(_) => 8,
        }
    }

    fn pointee(&self) -> Option<&Ty> {
        match self {
            Ty::Ptr(Some(t)) => Some(t),
            _ => None,
        }
    }

    fn is_float(&self) -> bool {
        matches!(self, Ty::Float { .. })
    }

    /// The type as it reads in a 32-bit register.
    fn narrowed(&self) -> Option<Ty> {
        match self {
            Ty::Int { signed, bits } => Some(Ty::Int {
                signed: *signed,
                bits: (*bits).min(32),
            }),
            _ => None,
        }
    }
}

impl std::fmt::Display for Ty {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Ty::Int { signed: true, bits } => write!(f, "i{bits}"),
            Ty::Int {
                signed: false,
                bits,
            } => write!(f, "u{bits}"),
            Ty::Float { bits } => write!(f, "f{bits}"),
            Ty::Ptr(None) => write!(f, "ptr"),
            Ty::Ptr(Some(t)) => write!(f, "ptr<{t}>"),
        }
    }
}

/// A function's signature: the registers it takes, and what it returns.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Signature {
    pub name: String,
    /// Register id (`x0`, `v0`) and type.
    pub params: Vec<(String, Ty)>,
    pub variadic: bool,
    /// None for `-> void`; also None, with `returns_known` false, when the
    /// annotation doesn't say.
    pub ret: Option<Ty>,
    pub returns_known: bool,
}

/// What the annotations in a file say.
#[derive(Debug, Default)]
pub struct Annotations {
    pub signatures: HashMap<String, Signature>,
    /// Register types asserted at a text offset: (offset, register id, type).
    pub registers: Vec<(usize, String, Ty)>,
    /// Malformed annotations: where, and what's wrong.
    pub errors: Vec<(Range<usize>, String)>,
}

/// The register id of a name as an annotation writes it: `x0`, `w0`, `d0`.
fn annotated_register(name: &str) -> Option<String> {
    let register = arm_isa::Register::lookup(name.trim()).next()?;
    register_id(&Operand::Register(arm_isa::RegisterOperand {
        register,
        suffix: None,
        shift: None,
    }))
}

/// Reads every `armtool: fn ...` and `armtool: x0: T` comment.
pub fn annotations(analysis: &Analysis) -> Annotations {
    let mut out = Annotations::default();
    for element in analysis.parse().syntax().descendants_with_tokens() {
        let Some(token) = element.into_token() else {
            continue;
        };
        if token.kind() != SyntaxKind::Comment {
            continue;
        }
        let text = token.text();
        let Some(at) = text.find("armtool:") else {
            continue;
        };
        let start = usize::from(token.text_range().start());
        let body = text[at + "armtool:".len()..].trim();
        let range = start + at..start + text.trim_end().len();
        if let Some(signature) = body.strip_prefix("fn ") {
            match parse_signature(signature) {
                Ok(signature) => {
                    out.signatures.insert(signature.name.clone(), signature);
                }
                Err(error) => out.errors.push((range, error)),
            }
            continue;
        }
        // `x9: ptr<u8>, x10: u64`. Other armtool comments (allow, platform)
        // don't start with a register.
        let Some((first, _)) = body.split_once(':') else {
            continue;
        };
        if annotated_register(first).is_none() {
            continue;
        }
        for part in split_top(body) {
            let Some((register, ty)) = part.split_once(':') else {
                out.errors
                    .push((range.clone(), format!("'{part}': write register: type")));
                continue;
            };
            match (annotated_register(register), Ty::parse(ty)) {
                (Some(id), Ok(ty)) => out.registers.push((start, id, ty)),
                (None, _) => out.errors.push((
                    range.clone(),
                    format!("'{}' isn't a register", register.trim()),
                )),
                (_, Err(error)) => out.errors.push((range.clone(), error)),
            }
        }
    }
    out
}

/// Splits at commas outside `< >`.
fn split_top(text: &str) -> Vec<String> {
    let mut parts = Vec::new();
    let mut depth = 0;
    let mut current = String::new();
    for c in text.chars() {
        match c {
            '<' => depth += 1,
            '>' => depth -= 1,
            ',' if depth == 0 => {
                parts.push(std::mem::take(&mut current).trim().to_string());
                continue;
            }
            _ => {}
        }
        current.push(c);
    }
    if !current.trim().is_empty() {
        parts.push(current.trim().to_string());
    }
    parts
}

/// `_sum(x0: ptr<i32>, x1: u64) -> i64`.
fn parse_signature(text: &str) -> Result<Signature, String> {
    let open = text.find('(').ok_or("write fn name(x0: T, ...) -> T")?;
    let close = text
        .rfind(')')
        .ok_or("the parameter list needs a closing )")?;
    let name = text[..open].trim().to_string();
    if name.is_empty() || name.contains(char::is_whitespace) {
        return Err("write fn name(x0: T, ...) -> T".into());
    }
    let mut params = Vec::new();
    let mut variadic = false;
    for part in split_top(&text[open + 1..close]) {
        if part == "..." {
            variadic = true;
            continue;
        }
        let (register, ty) = part
            .split_once(':')
            .ok_or_else(|| format!("'{part}': write register: type, like x0: u64"))?;
        let id = annotated_register(register)
            .ok_or_else(|| format!("'{}' isn't a register", register.trim()))?;
        params.push((id, Ty::parse(ty)?));
    }
    let rest = text[close + 1..].trim();
    let (ret, returns_known) = match rest.strip_prefix("->").map(str::trim) {
        None if rest.is_empty() => (None, false),
        None => return Err(format!("'{rest}': write -> T after the parameters")),
        Some("void") => (None, true),
        Some(ty) => (Some(Ty::parse(ty)?), true),
    };
    Ok(Signature {
        name,
        params,
        variadic,
        ret,
        returns_known,
    })
}

/// Register types at a point: only the known ones.
pub type Types = HashMap<String, Ty>;

/// Which compare set the flags: signed or unsigned operands, if known.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Flags {
    Signed(usize),
    Unsigned(usize),
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
struct State {
    types: Types,
    flags: Option<Flags>,
    /// Registers holding a label's address (`adr`, or `adrp` and its low
    /// 12 bits), or its page (`adrp` alone, marked with a leading `@`).
    addresses: HashMap<String, String>,
}

impl State {
    /// What's the same on both paths.
    fn meet(&self, other: &State) -> State {
        State {
            types: self
                .types
                .iter()
                .filter(|(r, t)| other.types.get(*r) == Some(t))
                .map(|(r, t)| (r.clone(), t.clone()))
                .collect(),
            flags: (self.flags == other.flags).then_some(self.flags).flatten(),
            addresses: self
                .addresses
                .iter()
                .filter(|(r, l)| other.addresses.get(*r) == Some(l))
                .map(|(r, l)| (r.clone(), l.clone()))
                .collect(),
        }
    }
}

/// The registers a call may change, besides those it returns in.
fn caller_saved(id: &str) -> bool {
    match id.split_at(1) {
        ("x", n) => n.parse::<u32>().is_ok_and(|n| n <= 18),
        ("v", n) => n.parse::<u32>().is_ok_and(|n| !(8..=15).contains(&n)),
        _ => false,
    }
}

/// The type a register operand reads as: a W register reads the low half.
fn operand_type(types: &Types, operand: &Operand) -> Option<Ty> {
    let Operand::Register(r) = operand else {
        return None;
    };
    let ty = types.get(&register_id(operand)?)?;
    if r.register.llvm_name().starts_with('W') {
        ty.narrowed()
    } else {
        Some(ty.clone())
    }
}

/// Whether a register operand is a 32-bit general-purpose register.
fn is_w(operand: &Operand) -> bool {
    matches!(operand, Operand::Register(r) if r.register.llvm_name().starts_with('W'))
}

/// Bytes a load or store moves per register.
fn access_size(insn: &Instruction) -> Option<u8> {
    let head = insn.mnemonic.head.as_str();
    let by_suffix = match head.trim_start_matches("ld").trim_start_matches("st") {
        "rb" | "rsb" | "urb" | "ursb" => Some(1),
        "rh" | "rsh" | "urh" | "ursh" => Some(2),
        "rsw" | "ursw" | "psw" => Some(4),
        _ => None,
    };
    if by_suffix.is_some() {
        return by_suffix;
    }
    if !matches!(
        head,
        "ldr" | "str" | "ldur" | "stur" | "ldp" | "stp" | "ldnp" | "stnp"
    ) {
        return None;
    }
    let Some(Operand::Register(r)) = flow::operands(insn).first() else {
        return None;
    };
    Some(match r.register.llvm_name().as_bytes().first()? {
        b'W' | b'S' => 4,
        b'X' | b'D' => 8,
        b'Q' => 16,
        b'H' => 2,
        b'B' => 1,
        _ => return None,
    })
}

/// The type a load gives its register, from the pointee: `ldrsw` widens an
/// i32 to i64.
fn loaded(insn: &Instruction, pointee: &Ty) -> Ty {
    match (insn.mnemonic.head.as_str(), pointee) {
        ("ldrsw" | "ldpsw", Ty::Int { .. }) => Ty::Int {
            signed: true,
            bits: 64,
        },
        _ => pointee.clone(),
    }
}

/// Signed and unsigned condition codes that read the same flags.
fn condition_family(condition: &str) -> Option<(bool, &'static str)> {
    // (is a signed test, the test for the other kind)
    Some(match condition {
        "gt" => (true, "hi"),
        "ge" => (true, "hs"),
        "lt" => (true, "lo"),
        "le" => (true, "ls"),
        "hi" => (false, "gt"),
        "hs" | "cs" => (false, "ge"),
        "lo" | "cc" => (false, "lt"),
        "ls" => (false, "le"),
        _ => return None,
    })
}

/// The condition an instruction tests: from `b.gt`, or a `csel` operand.
fn condition(insn: &Instruction) -> Option<String> {
    if let Some(c) = &insn.mnemonic.condition {
        return Some(c.clone());
    }
    flow::operands(insn).iter().find_map(|o| match o {
        Operand::CondCode(c) => Some(c.to_ascii_lowercase()),
        Operand::Ident(n) if arm_isa::is_condition(n) => Some(n.to_ascii_lowercase()),
        _ => None,
    })
}

/// Whether a value of type `actual` can go where `expected` is wanted: the
/// same type, pointers where one side is `ptr`, or integers no wider than
/// wanted that don't need sign-extending.
fn fits(expected: &Ty, actual: &Ty) -> bool {
    match (expected, actual) {
        (Ty::Ptr(None), Ty::Ptr(_)) | (Ty::Ptr(_), Ty::Ptr(None)) => true,
        // `char *` and `unsigned char *` alike: same-width integers.
        (Ty::Ptr(Some(a)), Ty::Ptr(Some(b))) => {
            a == b
                || matches!((&**a, &**b), (Ty::Int { bits: x, .. }, Ty::Int { bits: y, .. }) if x == y)
        }
        (Ty::Int { bits: want, .. }, Ty::Int { signed, bits }) => {
            // A narrower signed value needs sxtw first; an unsigned one is
            // already zero-extended by the W write that made it.
            bits >= want || !signed
        }
        (Ty::Float { bits: a }, Ty::Float { bits: b }) => a == b,
        _ => false,
    }
}

/// What to do about a value that doesn't fit.
fn advice(expected: &Ty, actual: &Ty, register: &str) -> String {
    match (expected, actual) {
        (Ty::Int { bits: 64, .. }, Ty::Int { signed: true, bits }) if *bits < 64 => {
            let n = register.trim_start_matches('x');
            format!(": sign-extend it first, like sxtw x{n}, w{n}")
        }
        _ => String::new(),
    }
}

/// Types inferred through one function, and the findings.
struct Checker<'a> {
    analysis: &'a Analysis,
    cfg: &'a Cfg<'a>,
    annotations: &'a Annotations,
    /// Instruction index to register types asserted after it, and before it.
    after: HashMap<usize, Vec<(String, Ty)>>,
    before: HashMap<usize, Vec<(String, Ty)>>,
    /// The inferred state before each instruction of the function being
    /// checked, for looking back from a call.
    states: std::cell::RefCell<HashMap<usize, State>>,
}

impl Checker<'_> {
    fn callee(&self, insn: &Instruction) -> Option<&Signature> {
        let range = insn.operand_ranges.first()?;
        let name = &self.analysis.reference_at(range.start)?.name;
        self.annotations.signatures.get(name)
    }

    /// The state after instruction `i`, given the state before it.
    fn transfer(&self, i: usize, state: &State) -> State {
        let insn = self.cfg.insns[i];
        let ops = flow::operands(insn);
        let head = insn.mnemonic.head.as_str();
        let mut out = state.clone();
        let written = flow::writes(insn);
        let base = memory_base(ops);
        // Most writes make a register's type unknown; the cases below say
        // what it is.
        for register in &written {
            if Some(register) != base.as_ref() {
                out.types.remove(register);
            }
        }
        let dest = ops.first();
        let dest_id = dest.and_then(register_id);
        let set = |out: &mut State, ty: Option<Ty>| {
            if let (Some(id), Some(ty)) = (&dest_id, ty) {
                let ty = if dest.is_some_and(is_w) {
                    ty.narrowed()
                } else {
                    Some(ty)
                };
                if let Some(ty) = ty {
                    out.types.insert(id.clone(), ty);
                }
            }
        };
        match head {
            "mov" if ops.len() == 2 => set(
                &mut out,
                ops.get(1).and_then(|o| operand_type(&state.types, o)),
            ),
            "add" | "sub" => {
                let lhs = ops.get(1).and_then(|o| operand_type(&state.types, o));
                let rhs = ops.get(2).and_then(|o| operand_type(&state.types, o));
                let ty = match (lhs, rhs) {
                    (Some(p @ Ty::Ptr(_)), None | Some(Ty::Int { .. })) => Some(p),
                    (Some(Ty::Int { .. }) | None, Some(p @ Ty::Ptr(_))) if head == "add" => Some(p),
                    (Some(Ty::Ptr(_)), Some(Ty::Ptr(_))) if head == "sub" => Some(Ty::Int {
                        signed: true,
                        bits: 64,
                    }),
                    (Some(a @ Ty::Int { .. }), Some(b)) if a == b => Some(a),
                    _ => None,
                };
                // `add x0, x0, #4` on an untyped pointer stays unknown.
                if ops.get(2).is_some_and(|o| matches!(o, Operand::Imm(_))) || ty.is_some() {
                    set(&mut out, ty);
                }
            }
            "sxtw" => set(
                &mut out,
                Some(Ty::Int {
                    signed: true,
                    bits: 64,
                }),
            ),
            "sxtb" | "sxth" => set(
                &mut out,
                Some(Ty::Int {
                    signed: true,
                    bits: if dest.is_some_and(is_w) { 32 } else { 64 },
                }),
            ),
            "uxtb" | "uxth" => set(
                &mut out,
                Some(Ty::Int {
                    signed: false,
                    bits: 32,
                }),
            ),
            "udiv" => set(
                &mut out,
                Some(Ty::Int {
                    signed: false,
                    bits: 64,
                }),
            ),
            "sdiv" => set(
                &mut out,
                Some(Ty::Int {
                    signed: true,
                    bits: 64,
                }),
            ),
            "scvtf" | "ucvtf" => set(&mut out, float_of(dest)),
            "fcvtzs" | "fcvtas" | "fcvtms" | "fcvtns" | "fcvtps" => set(
                &mut out,
                Some(Ty::Int {
                    signed: true,
                    bits: 64,
                }),
            ),
            "fcvtzu" | "fcvtau" | "fcvtmu" | "fcvtnu" | "fcvtpu" => set(
                &mut out,
                Some(Ty::Int {
                    signed: false,
                    bits: 64,
                }),
            ),
            _ if access_size(insn).is_some() && head.starts_with("ld") => {
                let pointee = base
                    .as_ref()
                    .and_then(|b| state.types.get(b))
                    .and_then(Ty::pointee)
                    .cloned();
                if let Some(pointee) = pointee {
                    let ty = loaded(insn, &pointee);
                    let pair = matches!(head, "ldp" | "ldnp" | "ldpsw");
                    for operand in ops.iter().take(if pair { 2 } else { 1 }) {
                        if let Some(id) = register_id(operand) {
                            let ty = if is_w(operand) {
                                ty.narrowed()
                            } else {
                                Some(ty.clone())
                            };
                            if let Some(ty) = ty {
                                out.types.insert(id, ty);
                            }
                        }
                    }
                }
            }
            _ => {}
        }
        if flow::is_call(insn) {
            out.types.retain(|r, _| !caller_saved(r));
            if let Some(signature) = self.callee(insn).filter(|_| head == "bl")
                && let Some(ret) = &signature.ret
            {
                let id = if ret.is_float() { "v0" } else { "x0" };
                out.types.insert(id.into(), ret.clone());
            }
        }
        // Flags: who set them, and with what kind of operands.
        if flow::sets_flags(insn, self.analysis.platform()) {
            let first = ops.get(usize::from(!matches!(
                head,
                "cmp" | "cmn" | "tst" | "ccmp" | "ccmn"
            )));
            out.flags = match (head, first.and_then(|o| operand_type(&state.types, o))) {
                ("tst", _) => None,
                (_, Some(Ty::Int { signed: true, .. })) => Some(Flags::Signed(i)),
                (_, Some(Ty::Int { signed: false, .. } | Ty::Ptr(_))) => Some(Flags::Unsigned(i)),
                _ => None,
            };
        } else if flow::is_call(insn) {
            out.flags = None;
        }
        // Label addresses, for format strings.
        for register in &written {
            out.addresses.remove(register);
        }
        if flow::is_call(insn) {
            out.addresses.retain(|r, _| !caller_saved(r));
        }
        let symbol = |o: &Operand| match o {
            Operand::Imm(Imm::Symbol {
                name,
                variant,
                specifier,
                ..
            }) => Some((name.clone(), variant.clone(), specifier.clone())),
            Operand::Ident(name) => Some((name.clone(), None, None)),
            _ => None,
        };
        if let (Some(id), Some((name, variant, specifier))) =
            (&dest_id, ops.get(1).and_then(symbol))
        {
            match (head, variant.as_deref(), specifier.as_deref()) {
                ("adr", None, None) => {
                    out.addresses.insert(id.clone(), name);
                }
                ("adrp", Some("page") | None, None) => {
                    out.addresses.insert(id.clone(), format!("@{name}"));
                }
                _ => {}
            }
        }
        if head == "add"
            && let (Some(id), Some(base), Some((name, variant, specifier))) = (
                &dest_id,
                ops.get(1).and_then(register_id),
                ops.get(2).and_then(symbol),
            )
            && (variant.as_deref() == Some("pageoff") || specifier.as_deref() == Some("lo12"))
            && state.addresses.get(&base) == Some(&format!("@{name}"))
        {
            out.addresses.insert(id.clone(), name);
        }
        for (register, ty) in self.after.get(&i).into_iter().flatten() {
            out.types.insert(register.clone(), ty.clone());
        }
        out
    }

    /// Findings at instruction `i`, given the state before it.
    fn check(
        &self,
        i: usize,
        state: &State,
        signature: Option<&Signature>,
        out: &mut Vec<Finding>,
    ) {
        let insn = self.cfg.insns[i];
        let ops = flow::operands(insn);
        let head = insn.mnemonic.head.as_str();
        let at = |k: usize| {
            insn.operand_ranges
                .get(k)
                .cloned()
                .unwrap_or(insn.range.clone())
        };
        // A pointer read through a W register loses its top half.
        let reads_from = usize::from(!matches!(
            head,
            "cmp" | "cmn" | "tst" | "str" | "strb" | "strh" | "stp" | "stur"
        ));
        for (k, operand) in ops.iter().enumerate().skip(reads_from) {
            if is_w(operand)
                && let Some(Ty::Ptr(_)) = register_id(operand).and_then(|id| state.types.get(&id))
            {
                out.push(finding(
                    "types/pointer-width",
                    at(k),
                    format!(
                        "{} is a pointer, but this reads its low 32 bits only. Use the X register",
                        register_name(operand)
                    ),
                ));
            }
        }
        // Loads and stores against the pointee.
        if let (Some(size), Some(base)) = (access_size(insn), memory_base(ops)) {
            let base_at = ops
                .iter()
                .position(|o| matches!(o, Operand::Token(t) if t == "["))
                .map_or(insn.range.clone(), |p| at(p + 1));
            // SIMD and FP registers load several elements at once.
            let vector = matches!(
                ops.first(),
                Some(Operand::Register(r)) if matches!(r.register.llvm_name().as_bytes().first(), Some(b'Q' | b'D' | b'S' | b'H' | b'B'))
            );
            match state.types.get(&base) {
                Some(Ty::Ptr(Some(pointee))) => {
                    let whole = vector && size % pointee.size() == 0;
                    if pointee.size() != size && !whole {
                        out.push(finding(
                            "types/memory",
                            insn.range.clone(),
                            format!(
                                "{base} points to {pointee} ({} bytes), but this {} {size} bytes{}",
                                pointee.size(),
                                if head.starts_with("ld") {
                                    "loads"
                                } else {
                                    "stores"
                                },
                                if size > pointee.size() {
                                    ", past the value"
                                } else {
                                    ""
                                }
                            ),
                        ));
                    }
                    if let Some(offset) = memory_offset(ops)
                        && offset % i64::from(pointee.size()) != 0
                    {
                        out.push(finding(
                            "types/memory",
                            insn.range.clone(),
                            format!(
                                "offset {offset} from {base} isn't a multiple of {pointee}'s {} bytes, so it's not at an element",
                                pointee.size()
                            ),
                        ));
                    }
                }
                Some(other @ (Ty::Int { .. } | Ty::Float { .. })) => out.push(finding(
                    "types/memory",
                    base_at,
                    format!("{base} holds {other}, not a pointer, but this uses it as an address"),
                )),
                _ => {}
            }
        }
        // Pointer arithmetic by part of an element.
        if matches!(head, "add" | "sub")
            && let (Some(Ty::Ptr(Some(pointee))), Some(Operand::Imm(Imm::Constant(n)))) = (
                ops.get(1).and_then(|o| operand_type(&state.types, o)),
                ops.get(2),
            )
            && n % i64::from(pointee.size()) != 0
        {
            out.push(finding(
                "types/memory",
                at(2),
                format!(
                    "{} points to {pointee}, so moving it by {n} bytes lands between elements. Use a multiple of {}",
                    register_name(&ops[1]),
                    pointee.size()
                ),
            ));
        }
        // Signed tests on unsigned compares, and the reverse.
        if flow::reads_flags(insn)
            && let (Some(flags), Some(cond)) = (state.flags, condition(insn))
            && let Some((signed_test, other)) = condition_family(&cond)
        {
            let (signed_values, setter) = match flags {
                Flags::Signed(s) => (true, s),
                Flags::Unsigned(s) => (false, s),
            };
            if signed_test != signed_values {
                let line = self.line(self.cfg.insns[setter].range.start);
                out.push(finding(
                    "types/signedness",
                    insn.range.clone(),
                    format!(
                        "the compare on line {line} is of {} values, but {cond} is {} test. Use {other}",
                        if signed_values { "signed" } else { "unsigned" },
                        if signed_test { "a signed" } else { "an unsigned" }
                    ),
                ));
            }
        }
        if matches!(head, "sdiv" | "udiv") {
            let want_signed = head == "sdiv";
            for (k, operand) in ops.iter().enumerate().skip(1) {
                if let Some(Ty::Int { signed, .. }) = operand_type(&state.types, operand)
                    && signed != want_signed
                {
                    out.push(finding(
                        "types/signedness",
                        at(k),
                        format!(
                            "{} is {}, but {head} divides {} values. Use {}",
                            register_name(operand),
                            if signed { "signed" } else { "unsigned" },
                            if want_signed { "signed" } else { "unsigned" },
                            if want_signed { "udiv" } else { "sdiv" }
                        ),
                    ));
                    break;
                }
            }
        }
        if head == "bl" {
            self.check_format(i, state, out);
        }
        // Calls against the callee's signature.
        if head == "bl"
            && let Some(callee) = self.callee(insn)
        {
            for (register, expected) in &callee.params {
                if let Some(actual) = state.types.get(register)
                    && !fits(expected, actual)
                {
                    out.push(finding(
                        "types/call",
                        insn.range.clone(),
                        format!(
                            "{} takes {register} as {expected}, but {register} holds {actual} here{}",
                            callee.name,
                            advice(expected, actual, register)
                        ),
                    ));
                }
            }
        }
        // Returns against this function's signature.
        if matches!(head, "ret" | "retaa" | "retab")
            && let Some(Signature {
                ret: Some(expected),
                name,
                ..
            }) = signature
        {
            let register = if expected.is_float() { "v0" } else { "x0" };
            if let Some(actual) = state.types.get(register)
                && !fits(expected, actual)
            {
                out.push(finding(
                    "types/call",
                    insn.range.clone(),
                    format!(
                        "{name} returns {expected}, but {register} holds {actual} here{}",
                        advice(expected, actual, register)
                    ),
                ));
            }
        }
    }

    /// A printf-family call against its format string.
    fn check_format(&self, i: usize, state: &State, out: &mut Vec<Finding>) {
        let insn = self.cfg.insns[i];
        let Some(callee) = self.callee(insn) else {
            return;
        };
        let darwin = self.analysis.platform() == arm_isa::Platform::Darwin;
        let base = callee
            .name
            .strip_prefix('_')
            .filter(|_| darwin)
            .unwrap_or(&callee.name);
        let format_register = match base {
            "printf" => 0,
            "fprintf" | "sprintf" | "dprintf" => 1,
            "snprintf" => 2,
            _ => return,
        };
        let Some(label) = state.addresses.get(&format!("x{format_register}")) else {
            return;
        };
        let Some(format) = string_at(self.analysis, label) else {
            return;
        };
        let wanted = conversions(&format);
        let shown = format.escape_debug().to_string();
        if darwin {
            // Variadic arguments go on the stack, 8 bytes each.
            let slots = self.stack_slots(i, state);
            if !wanted.is_empty() && slots.is_empty() {
                let in_registers = (format_register + 1..8)
                    .any(|n| self.written_before(i, &format!("x{n}")))
                    || (0..8).any(|n| self.written_before(i, &format!("v{n}")));
                if in_registers {
                    out.push(finding(
                        "types/format",
                        insn.range.clone(),
                        format!(
                            "\"{shown}\" takes {} argument{}, which on Darwin go on the stack, not in registers: store them at [sp], [sp, #8], and on",
                            wanted.len(),
                            if wanted.len() == 1 { "" } else { "s" }
                        ),
                    ));
                }
                return;
            }
            for (k, kind) in wanted.iter().enumerate() {
                match slots.get(&(k as i64 * 8)) {
                    Some(Some(actual)) if !kind.accepts(actual) => out.push(finding(
                        "types/format",
                        insn.range.clone(),
                        format!(
                            "\"{shown}\" wants {} for argument {}, but [sp, #{}] holds {actual}",
                            kind.describe(),
                            k + 1,
                            k * 8
                        ),
                    )),
                    None if !slots.is_empty() => {
                        out.push(finding(
                            "types/format",
                            insn.range.clone(),
                            format!(
                                "\"{shown}\" takes {} argument{}, but nothing is stored at [sp, #{}] for argument {}",
                                wanted.len(),
                                if wanted.len() == 1 { "" } else { "s" },
                                k * 8,
                                k + 1
                            ),
                        ));
                        break;
                    }
                    _ => {}
                }
            }
            return;
        }
        // Linux: integers and pointers in the next x registers, floats in v.
        let (mut next_x, mut next_v) = (format_register + 1, 0);
        for (k, kind) in wanted.iter().enumerate() {
            let register = if matches!(kind, Conversion::Float) {
                next_v += 1;
                format!("v{}", next_v - 1)
            } else {
                next_x += 1;
                format!("x{}", next_x - 1)
            };
            if next_x > 8 || next_v > 8 {
                break;
            }
            if let Some(actual) = state.types.get(&register)
                && !kind.accepts(actual)
            {
                out.push(finding(
                    "types/format",
                    insn.range.clone(),
                    format!(
                        "\"{shown}\" wants {} for argument {}, in {register}, but {register} holds {actual}",
                        kind.describe(),
                        k + 1
                    ),
                ));
            }
        }
    }

    /// Whether a register is written in the straight-line code before
    /// instruction `i`.
    fn written_before(&self, i: usize, register: &str) -> bool {
        self.block_before(i).iter().any(|&j| {
            flow::writes(self.cfg.insns[j])
                .iter()
                .any(|r| r == register)
        })
    }

    /// The instructions before `i` that surely run just before it, nearest
    /// first: back to a label, a branch, or a call, at most 24.
    fn block_before(&self, i: usize) -> Vec<usize> {
        let text = self.analysis.text();
        let insns = &self.cfg.insns;
        let mut out = Vec::new();
        let mut j = i;
        while j > 0 && out.len() < 24 {
            let prev = j - 1;
            let between = text
                .get(insns[prev].range.end..insns[j].range.start)
                .unwrap_or("");
            let labeled = between.lines().any(|l| {
                l.split_whitespace()
                    .next()
                    .is_some_and(|w| w.ends_with(':'))
            });
            let insn = insns[prev];
            let jumps = flow::is_call(insn)
                || flow::is_terminator(insn)
                || insn.mnemonic.condition.is_some()
                || matches!(insn.mnemonic.head.as_str(), "cbz" | "cbnz" | "tbz" | "tbnz");
            if labeled || jumps {
                break;
            }
            out.push(prev);
            j = prev;
        }
        out
    }

    /// Stores to the stack in the straight-line code before a call, by
    /// their offset from sp at the call, with the stored value's type when
    /// known. Follows sp as it moves: `str w1, [sp, #-32]!` makes room and
    /// stores the first argument at once.
    fn stack_slots(&self, i: usize, _state: &State) -> HashMap<i64, Option<Ty>> {
        let mut slots = HashMap::new();
        // How far sp moves between the instruction and the call.
        let mut below = 0i64;
        for j in self.block_before(i) {
            let insn = self.cfg.insns[j];
            let change = match flow::sp_change(insn) {
                flow::SpChange::By(n) => n,
                flow::SpChange::None => 0,
                // sp set some other way: nothing before it can be placed.
                _ => break,
            };
            let ops = flow::operands(insn);
            if insn.mnemonic.head.starts_with("st") && memory_base(ops).as_deref() == Some("sp") {
                let offset = memory_offset(ops).unwrap_or(0);
                let pre = ops
                    .iter()
                    .any(|o| matches!(o, Operand::Token(t) if t == "!"));
                // A post-indexed store writes at sp before it moves; plain
                // and pre-indexed ones at sp plus the offset.
                let post = change != 0 && !pre;
                let at = if post { 0 } else { offset } - change - below;
                let pair = insn.mnemonic.head == "stp";
                let size = i64::from(access_size(insn).unwrap_or(8)).max(8);
                let types_before = self.types_before(j);
                for (n, operand) in ops.iter().take(if pair { 2 } else { 1 }).enumerate() {
                    let slot = at + n as i64 * size;
                    // The frame record (x29 and x30) is never an argument.
                    let frame = matches!(register_id(operand).as_deref(), Some("x29" | "x30"));
                    if slot >= 0 && !frame {
                        let ty = types_before.as_ref().and_then(|t| operand_type(t, operand));
                        slots.entry(slot).or_insert(ty);
                    }
                }
            }
            below += change;
        }
        slots
    }

    /// Register types before instruction `j`, as the latest inference left
    /// them. Filled in during checking.
    fn types_before(&self, j: usize) -> Option<Types> {
        self.states.borrow().get(&j).map(|s| s.types.clone())
    }

    fn line(&self, offset: usize) -> u32 {
        self.analysis
            .line_index()
            .line_col(self.analysis.text(), offset, arm_sema::Encoding::Utf8)
            .line
            + 1
    }
}

/// What a printf conversion wants.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Conversion {
    Int,
    Float,
    /// `%s`, `%p`, `%n`.
    Pointer,
}

impl Conversion {
    fn accepts(self, ty: &Ty) -> bool {
        matches!(
            (self, ty),
            (Conversion::Int, Ty::Int { .. })
                | (Conversion::Float, Ty::Float { bits: 64 })
                | (Conversion::Pointer, Ty::Ptr(_))
        )
    }

    fn describe(self) -> &'static str {
        match self {
            Conversion::Int => "an integer",
            Conversion::Float => "a double",
            Conversion::Pointer => "a pointer",
        }
    }
}

/// The arguments a printf format string takes, in order: `%*d` takes two.
fn conversions(format: &str) -> Vec<Conversion> {
    let mut out = Vec::new();
    let mut chars = format.chars().peekable();
    while let Some(c) = chars.next() {
        if c != '%' {
            continue;
        }
        if chars.peek() == Some(&'%') {
            chars.next();
            continue;
        }
        // Flags, width, precision, and length, then the conversion.
        while let Some(&c) = chars.peek() {
            if c == '*' {
                out.push(Conversion::Int);
                chars.next();
            } else if "-+ #0123456789.hljztqL'".contains(c) {
                chars.next();
            } else {
                break;
            }
        }
        match chars.next() {
            Some('d' | 'i' | 'u' | 'x' | 'X' | 'o' | 'c' | 'b') => out.push(Conversion::Int),
            Some('f' | 'F' | 'e' | 'E' | 'g' | 'G' | 'a' | 'A') => out.push(Conversion::Float),
            Some('s' | 'p' | 'n') => out.push(Conversion::Pointer),
            _ => {}
        }
    }
    out
}

/// The text of the string a label marks, from `.asciz`, `.string`, or
/// `.ascii` right after it, with C escapes decoded.
fn string_at(analysis: &Analysis, label: &str) -> Option<String> {
    let symbol = analysis
        .symbols()
        .iter()
        .find(|s| s.name == label && s.kind == arm_sema::SymbolKind::Label && s.origin.is_none())?;
    let after = analysis.text().get(symbol.range.end..)?;
    let after = after.strip_prefix(':').unwrap_or(after);
    let directive = after.split_whitespace().next()?.to_ascii_lowercase();
    if !matches!(directive.as_str(), ".asciz" | ".string" | ".ascii") {
        return None;
    }
    let open = after.find('"')?;
    let mut out = String::new();
    let mut chars = after[open + 1..].chars();
    while let Some(c) = chars.next() {
        match c {
            '"' => return Some(out),
            '\\' => match chars.next()? {
                'n' => out.push('\n'),
                't' => out.push('\t'),
                '0' => out.push('\0'),
                other => out.push(other),
            },
            '\n' => return None,
            c => out.push(c),
        }
    }
    None
}

fn float_of(dest: Option<&Operand>) -> Option<Ty> {
    let Some(Operand::Register(r)) = dest else {
        return None;
    };
    match r.register.llvm_name().as_bytes().first()? {
        b'S' => Some(Ty::Float { bits: 32 }),
        b'D' => Some(Ty::Float { bits: 64 }),
        _ => None,
    }
}

fn register_name(operand: &Operand) -> String {
    match operand {
        Operand::Register(r) => r.register.name().to_string(),
        _ => "it".into(),
    }
}

/// The base register of a memory operand: `x0` in `[x0, #8]`.
fn memory_base(ops: &[Operand]) -> Option<String> {
    let open = ops
        .iter()
        .position(|o| matches!(o, Operand::Token(t) if t == "["))?;
    register_id(ops.get(open + 1)?)
}

/// A constant offset inside the brackets: 8 in `[x0, #8]`.
fn memory_offset(ops: &[Operand]) -> Option<i64> {
    let open = ops
        .iter()
        .position(|o| matches!(o, Operand::Token(t) if t == "["))?;
    match ops.get(open + 2) {
        Some(Operand::Imm(Imm::Constant(n))) => Some(*n),
        _ => None,
    }
}

fn finding(rule: &'static str, range: Range<usize>, message: String) -> Finding {
    Finding {
        rule,
        range,
        message,
        fix: None,
    }
}

/// Infers types through every function and checks them. Malformed
/// annotations are findings too.
pub(crate) fn run(
    analysis: &Analysis,
    cfg: &Cfg,
    externals: &HashMap<String, Signature>,
    out: &mut Vec<Finding>,
) {
    let mut annotations = annotations(analysis);
    for (range, error) in &annotations.errors {
        out.push(finding(
            "types/annotation",
            range.clone(),
            format!("this type annotation can't be read: {error}"),
        ));
    }
    // Calls to C functions are checked even without annotations; what the
    // file says wins.
    for (name, signature) in externals {
        annotations
            .signatures
            .entry(name.clone())
            .or_insert_with(|| signature.clone());
    }
    let checker = Checker::new(analysis, cfg, &annotations);
    for function in &cfg.functions {
        let Some(entry) = function.entry else {
            continue;
        };
        let signature = annotations.signatures.get(&function.name);
        let states = infer(&checker, entry, entry_state(signature));
        let mut indices: Vec<usize> = states.keys().copied().collect();
        indices.sort_unstable();
        *checker.states.borrow_mut() = states.clone();
        for i in indices {
            checker.check(i, &states[&i], signature, out);
        }
    }
}

/// Register types at a function's entry, from its signature.
fn entry_state(signature: Option<&Signature>) -> State {
    let mut state = State::default();
    for (register, ty) in signature.iter().flat_map(|s| &s.params) {
        state.types.insert(register.clone(), ty.clone());
    }
    state
}

impl<'a> Checker<'a> {
    /// Places each register annotation: after the instruction on its line,
    /// or before the next one.
    fn new(analysis: &'a Analysis, cfg: &'a Cfg<'a>, annotations: &'a Annotations) -> Checker<'a> {
        let text = analysis.text();
        let line_of = |offset: usize| text[..offset.min(text.len())].matches('\n').count();
        let mut after: HashMap<usize, Vec<(String, Ty)>> = HashMap::new();
        let mut before: HashMap<usize, Vec<(String, Ty)>> = HashMap::new();
        for (offset, register, ty) in &annotations.registers {
            let line = line_of(*offset);
            let same = cfg
                .insns
                .iter()
                .position(|i| line_of(i.range.start) == line && i.range.start < *offset);
            match same {
                Some(i) => after
                    .entry(i)
                    .or_default()
                    .push((register.clone(), ty.clone())),
                None => {
                    if let Some(j) = cfg.insns.iter().position(|i| i.range.start > *offset) {
                        before
                            .entry(j)
                            .or_default()
                            .push((register.clone(), ty.clone()));
                    }
                }
            }
        }
        Checker {
            analysis,
            cfg,
            annotations,
            after,
            before,
            states: std::cell::RefCell::new(HashMap::new()),
        }
    }
}

/// The state before each instruction reachable from `entry`.
fn infer(checker: &Checker, entry: usize, initial: State) -> HashMap<usize, State> {
    let mut before: HashMap<usize, State> = HashMap::new();
    let mut work = vec![(entry, initial)];
    while let Some((i, incoming)) = work.pop() {
        let mut merged = match before.get(&i) {
            None => incoming,
            Some(old) => old.meet(&incoming),
        };
        for (register, ty) in checker.before.get(&i).into_iter().flatten() {
            merged.types.insert(register.clone(), ty.clone());
        }
        if before.get(&i) == Some(&merged) {
            continue;
        }
        before.insert(i, merged.clone());
        let after = checker.transfer(i, &merged);
        for next in checker.cfg.successors(checker.analysis, i).next {
            work.push((next, after.clone()));
        }
    }
    before
}

/// The known register types before the instruction at `offset`, for hover.
pub fn types_at(analysis: &Analysis, offset: usize) -> Option<Types> {
    let cfg = Cfg::new(analysis);
    let annotations = annotations(analysis);
    let at = cfg
        .insns
        .iter()
        .position(|i| i.range.start <= offset && offset <= i.range.end)?;
    let checker = Checker::new(analysis, &cfg, &annotations);
    for function in &cfg.functions {
        let (Some(entry), true) = (function.entry, function.span.contains(&at)) else {
            continue;
        };
        let signature = annotations.signatures.get(&function.name);
        return infer(&checker, entry, entry_state(signature))
            .remove(&at)
            .map(|s| s.types);
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Config, lint};

    /// (rule, first line of the flagged text), types rules only.
    fn types(text: &str) -> Vec<(&'static str, String)> {
        lint(&Analysis::new(text), &Config::default())
            .into_iter()
            .filter(|d| d.code.starts_with("types/"))
            .map(|d| {
                (
                    d.code,
                    text[d.range]
                        .lines()
                        .next()
                        .unwrap_or("")
                        .trim()
                        .to_string(),
                )
            })
            .collect()
    }

    #[test]
    fn parses_types_and_signatures() {
        assert_eq!(
            Ty::parse("ptr<ptr<u8>>").map(|t| t.to_string()),
            Ok("ptr<ptr<u8>>".into())
        );
        assert_eq!(Ty::parse("ptr"), Ok(Ty::Ptr(None)));
        assert_eq!(Ty::parse("f32").map(|t| t.size()), Ok(4));
        assert!(Ty::parse("i33").is_err() && Ty::parse("ptr<").is_err());
        let s = parse_signature("_printf(x0: ptr<i8>, ...) -> i32").expect("parses");
        assert_eq!(
            s.params,
            [(
                "x0".to_string(),
                Ty::Ptr(Some(Box::new(Ty::Int {
                    signed: true,
                    bits: 8
                })))
            )]
        );
        assert!(s.variadic && s.returns_known);
        let s = parse_signature("_f(w0: i32, d0: f64) -> void").expect("parses");
        assert_eq!(s.params[0].0, "x0");
        assert_eq!(s.params[1].0, "v0");
        assert_eq!((s.ret, s.returns_known), (None, true));
        assert!(parse_signature("_f(q: i32)").is_err());
        assert!(parse_signature("_f(x0: i32) => i32").is_err());
    }

    #[test]
    fn nothing_without_annotations() {
        let text = "_f:\n    ldr x0, [x1]\n    cmp x0, #0\n    b.gt 1f\n1:  ret\n";
        assert_eq!(types(text), []);
    }

    #[test]
    fn a_correct_function_is_clean() {
        let text = "\
; armtool: fn _sum(x0: ptr<i32>, x1: u64) -> i64
.globl _sum
_sum:
    mov     x2, #0
1:  ldr     w3, [x0], #4
    sxtw    x3, w3
    add     x2, x2, x3
    subs    x1, x1, #1
    b.hi    1b
    mov     x0, x2
    ret
";
        assert_eq!(types(text), []);
        // Hover sees the inferred types.
        let analysis = Analysis::new(text);
        let at = text.find("sxtw").expect("sxtw");
        let known = types_at(&analysis, at).expect("in a function");
        assert_eq!(
            known.get("x3"),
            Some(&Ty::Int {
                signed: true,
                bits: 32
            })
        );
        assert_eq!(
            known.get("x0").map(|t| t.to_string()).as_deref(),
            Some("ptr<i32>")
        );
    }

    #[test]
    fn signedness() {
        let text = "\
; armtool: fn _f(x0: u64, x1: i64) -> void
.globl _f
_f:
    cmp     x0, #10
    b.gt    1f
    cmp     x1, #0
    b.lo    1f
    cset    x2, lt
    udiv    x3, x1, x1
1:  ret
";
        let found = types(text);
        let rules: Vec<&str> = found.iter().map(|(r, _)| *r).collect();
        assert_eq!(rules, ["types/signedness"; 3], "{found:?}");
        assert_eq!(found[0].1, "b.gt    1f");
        assert_eq!(found[1].1, "b.lo    1f");
        assert_eq!(found[2].1, "x1");
        let message = lint(&Analysis::new(text), &Config::default())
            .into_iter()
            .find(|d| d.code == "types/signedness")
            .map(|d| d.message)
            .unwrap_or_default();
        assert!(
            message.contains("unsigned values") && message.contains("Use hi"),
            "{message}"
        );
    }

    #[test]
    fn memory() {
        let text = "\
; armtool: fn _f(x0: ptr<i32>, x1: u64) -> void
.globl _f
_f:
    ldr     x2, [x0]
    ldr     w3, [x0, #6]
    ldr     w4, [x0, #8]
    add     x5, x0, #2
    add     x6, x0, #4
    ldr     w7, [x6]
    ldr     x8, [x1]
    ret
";
        let found = types(text);
        let lines: Vec<&str> = found.iter().map(|(_, l)| l.as_str()).collect();
        assert_eq!(
            lines,
            ["ldr     x2, [x0]", "ldr     w3, [x0, #6]", "#2", "x1"],
            "{found:?}"
        );
        assert!(found.iter().all(|(r, _)| *r == "types/memory"));
    }

    #[test]
    fn pointer_width() {
        let text = "\
; armtool: fn _f(x0: ptr<u8>) -> void
.globl _f
_f:
    add     w1, w0, #1
    mov     x2, x0
    ret
";
        assert_eq!(types(text), [("types/pointer-width", "w0".to_string())]);
    }

    #[test]
    fn calls_and_returns() {
        let text = "\
; armtool: fn _strlen(x0: ptr<u8>) -> u64
; armtool: fn _scale(x0: i64, d0: f64) -> f64
; armtool: fn _f(x0: ptr<u8>, w1: i32) -> ptr<u8>
.globl _f
_f:
    stp     x29, x30, [sp, #-16]!
    mov     x29, sp
    mov     x19, x0
    mov     w20, w1
    mov     x0, x20
    bl      _strlen
    mov     x0, x20
    bl      _scale
    sxtw    x0, w20
    bl      _scale
    mov     x21, x0
    mov     x0, x20
    ldp     x29, x30, [sp], #16
    ret
";
        let messages: Vec<String> = lint(&Analysis::new(text), &Config::default())
            .into_iter()
            .filter(|d| d.code == "types/call")
            .map(|d| d.message)
            .collect();
        assert_eq!(messages.len(), 3, "{messages:?}");
        assert!(
            messages[0].contains("_strlen takes x0 as ptr<u8>, but x0 holds i32"),
            "{messages:?}"
        );
        assert!(
            messages[1].contains("_scale takes x0 as i64, but x0 holds i32 here: sign-extend it first, like sxtw x0, w0"),
            "{messages:?}"
        );
        assert!(
            messages[2].contains("_f returns ptr<u8>, but x0 holds i32"),
            "{messages:?}"
        );
        // The return value's type flows on: _scale returns f64 in v0.
        let analysis = Analysis::new(text);
        let at = text.find("mov     x21").expect("mov");
        let known = types_at(&analysis, at).expect("types");
        assert_eq!(known.get("v0"), Some(&Ty::Float { bits: 64 }));
        assert_eq!(
            known.get("x19").map(|t| t.to_string()).as_deref(),
            Some("ptr<u8>")
        );
    }

    #[test]
    fn register_annotations_and_errors() {
        let text = "\
.globl _f
_f:
    bl      _malloc             ; armtool: x0: ptr<u16>
    ldr     w1, [x0]
    ; armtool: x2: ptr<u64>
    ldrb    w3, [x2]
    ret
; armtool: x9: i128
; armtool: fn _broken(x0 i32)
";
        let found = types(text);
        let rules: Vec<&str> = found.iter().map(|(r, _)| *r).collect();
        assert_eq!(
            rules,
            [
                "types/memory",
                "types/memory",
                "types/annotation",
                "types/annotation"
            ],
            "{found:?}"
        );
    }

    #[test]
    fn printf_formats_on_darwin() {
        let text = "\
.globl _f
_f:
    stp     x29, x30, [sp, #-16]!
    mov     x29, sp
    adrp    x0, fmt@PAGE
    add     x0, x0, fmt@PAGEOFF
    mov     x1, #42
    bl      _printf
    sub     sp, sp, #16
    mov     x9, #42
    str     x9, [sp]
    adrp    x0, fmt@PAGE
    add     x0, x0, fmt@PAGEOFF
    bl      _printf
    adrp    x0, two@PAGE
    add     x0, x0, two@PAGEOFF
    str     x9, [sp]
    bl      _printf
    add     sp, sp, #16
    ldp     x29, x30, [sp], #16
    ret
fmt:    .asciz \"%ld\\n\"
two:    .asciz \"%ld and %s\\n\"
";
        let found: Vec<String> = lint(&Analysis::new(text), &Config::default())
            .into_iter()
            .filter(|d| d.code == "types/format")
            .map(|d| d.message)
            .collect();
        assert_eq!(found.len(), 2, "{found:?}");
        assert!(
            found[0].contains("on Darwin go on the stack, not in registers"),
            "{found:?}"
        );
        assert!(
            found[1].contains("nothing is stored at [sp, #8] for argument 2"),
            "{found:?}"
        );
    }

    #[test]
    fn printf_formats_on_linux() {
        let text = "\
// armtool: fn main(x0: i64, x1: ptr<ptr<i8>>) -> i32
.globl main
main:
    stp     x29, x30, [sp, #-16]!
    mov     x29, sp
    mov     x19, x0
    adrp    x0, fmt
    add     x0, x0, :lo12:fmt
    mov     x1, x19
    mov     x2, x19
    bl      printf
    mov     w0, #0
    ldp     x29, x30, [sp], #16
    ret
fmt:    .asciz \"%d args, first %s\\n\"
";
        let found: Vec<String> = lint(
            &Analysis::for_platform(text, arm_isa::Platform::Linux),
            &Config::default(),
        )
        .into_iter()
        .filter(|d| d.code == "types/format")
        .map(|d| d.message)
        .collect();
        assert_eq!(found.len(), 1, "{found:?}");
        assert!(
            found[0].contains("wants a pointer for argument 2, in x2, but x2 holds i64"),
            "{found:?}"
        );
    }

    #[test]
    fn darwin_slots_follow_sp() {
        // hellosilicon's pattern: make room and store the first argument at once.
        let text = "\
.globl _f
_f:
    stp     x29, x30, [sp, #-16]!
    mov     x29, sp
    adrp    x0, fmt@PAGE
    add     x0, x0, fmt@PAGEOFF
    mov     w1, #1
    mov     w2, #2
    str     w1, [sp, #-16]!
    str     w2, [sp, #8]
    bl      _printf
    add     sp, sp, #16
    ldp     x29, x30, [sp], #16
    ret
fmt:    .asciz \"%d %d\\n\"
";
        assert_eq!(types(text), []);
    }

    #[test]
    fn libc_calls_are_checked_without_annotations() {
        // strlen takes a pointer; this passes the result of strlen, a u64.
        let text = "_f:\n    bl _strlen\n    bl _strlen\n    ret\n.globl _f\n";
        let found = types(text);
        assert_eq!(
            found,
            [("types/call", "bl _strlen".to_string())],
            "{found:?}"
        );
    }

    #[test]
    fn format_parsing() {
        use Conversion::*;
        assert_eq!(
            conversions("%d %5.2f %s %% %*d %lu %p %c"),
            [Int, Float, Pointer, Int, Int, Int, Pointer, Int]
        );
        assert_eq!(conversions("100%%"), []);
    }

    #[test]
    fn paths_that_disagree_are_unknown() {
        let text = "\
; armtool: fn _f(x0: ptr<u8>, x1: u64) -> void
.globl _f
_f:
    cbz     x1, 1f
    mov     x0, x1
1:  ldr     x2, [x0]
    ret
";
        assert_eq!(types(text), []);
    }
}
