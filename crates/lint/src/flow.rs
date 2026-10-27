//! Functions, branches, register reads and writes, and stack pointer
//! tracking over a file's items.

use std::collections::{HashMap, HashSet};
use std::ops::Range;

use arm_isa::{Imm, Operand, RegisterKind};
use arm_sema::{Analysis, Instruction, Item, SymbolKind};

/// A function: a label that starts one, and the code after it.
#[derive(Debug, Clone)]
pub(crate) struct Function {
    pub name: String,
    pub name_range: Range<usize>,
    /// The function's first instruction, as an index into `Cfg::insns`.
    pub entry: Option<usize>,
    /// Instructions up to the next function label or data section, as indices
    /// into `Cfg::insns`. The code reachable from `entry` can extend beyond.
    pub span: Range<usize>,
    /// The next function's name, if one follows directly.
    pub next: Option<String>,
}

/// The file's instructions as a control-flow graph. Labels that start
/// functions are exported or called with `bl`; a branch to one leaves the
/// current function, like a tail call.
pub(crate) struct Cfg<'a> {
    pub insns: Vec<&'a Instruction>,
    /// Symbol of a label to the index of the instruction after it.
    label_at: HashMap<usize, usize>,
    /// Instruction index where each function starts.
    function_at: HashSet<usize>,
    /// Whether instruction i can fall through to i + 1: no data section or
    /// function label between them.
    falls: Vec<bool>,
    /// Whether code we can't see (a macro call) or operands we couldn't read
    /// sit at or right after instruction i.
    opaque: Vec<bool>,
    pub functions: Vec<Function>,
}

/// Where control goes after an instruction.
pub(crate) struct Successors {
    pub next: Vec<usize>,
    /// The instruction leaves the function: ret, or a branch out (tail call).
    pub exits: bool,
}

impl<'a> Cfg<'a> {
    pub fn new(analysis: &'a Analysis) -> Cfg<'a> {
        let items = analysis.items();
        let called: HashSet<usize> = items
            .iter()
            .filter_map(|item| match item {
                Item::Instruction(insn) if insn.mnemonic.head == "bl" => target(analysis, insn),
                _ => None,
            })
            .collect();
        let starts_function = |symbol: usize| {
            let s = &analysis.symbols()[symbol];
            s.kind == SymbolKind::Label && (s.exported || called.contains(&symbol))
        };
        let mut cfg = Cfg {
            insns: Vec::new(),
            label_at: HashMap::new(),
            function_at: HashSet::new(),
            falls: Vec::new(),
            opaque: Vec::new(),
            functions: Vec::new(),
        };
        let mut in_text = true;
        // A barrier since the last instruction: nothing falls across it.
        let mut barrier = false;
        let mut opaque = false;
        let mut open: Option<(usize, Function)> = None;
        let close =
            |cfg: &mut Cfg<'a>, open: &mut Option<(usize, Function)>, next: Option<String>| {
                if let Some((_, mut function)) = open.take() {
                    function.span.end = cfg.insns.len();
                    function.next = next;
                    cfg.functions.push(function);
                }
            };
        for item in items {
            match item {
                Item::Section { text, .. } => {
                    in_text = *text;
                    barrier = true;
                    if !in_text {
                        close(&mut cfg, &mut open, None);
                    }
                }
                Item::Label(symbol) => {
                    cfg.label_at.insert(*symbol, cfg.insns.len());
                    if in_text && starts_function(*symbol) {
                        let name = analysis.symbols()[*symbol].name.clone();
                        close(&mut cfg, &mut open, Some(name));
                        barrier = true;
                        cfg.function_at.insert(cfg.insns.len());
                        let s = &analysis.symbols()[*symbol];
                        open = Some((
                            cfg.insns.len(),
                            Function {
                                name: s.name.clone(),
                                name_range: s.range.clone(),
                                entry: None,
                                span: cfg.insns.len()..cfg.insns.len(),
                                next: None,
                            },
                        ));
                    }
                }
                Item::MacroCall(_) => opaque = true,
                Item::Data(_) => {}
                Item::Instruction(insn) => {
                    if let Some(last) = cfg.falls.last_mut() {
                        *last = !barrier;
                    }
                    if let Some(last) = cfg.opaque.last_mut() {
                        *last |= opaque;
                    }
                    if let Some((_, function)) = open.as_mut() {
                        function.entry.get_or_insert(cfg.insns.len());
                    }
                    cfg.insns.push(insn);
                    cfg.falls.push(false);
                    cfg.opaque.push(insn.operands.is_none());
                    barrier = !in_text;
                    opaque = false;
                }
            }
        }
        if let Some(last) = cfg.opaque.last_mut() {
            *last |= opaque;
        }
        close(&mut cfg, &mut open, None);
        cfg
    }

    pub fn successors(&self, analysis: &Analysis, i: usize) -> Successors {
        let insn = self.insns[i];
        let head = insn.mnemonic.head.as_str();
        let local = target(analysis, insn)
            .and_then(|s| self.label_at.get(&s).copied())
            .filter(|t| !self.function_at.contains(t) && *t < self.insns.len());
        let fall = self.falls[i].then_some(i + 1);
        let unconditional_branch = head == "b" && insn.mnemonic.condition.is_none();
        if unconditional_branch {
            return Successors {
                next: local.into_iter().collect(),
                exits: local.is_none(),
            };
        }
        if is_terminator(insn) {
            let exits = !matches!(head, "brk" | "udf" | "hlt");
            return Successors {
                next: Vec::new(),
                exits,
            };
        }
        let mut next: Vec<usize> = fall.into_iter().collect();
        if is_branch(insn) {
            next.extend(local);
        }
        Successors { next, exits: false }
    }

    /// Instructions reachable from `entry`, in order, and whether any is
    /// next to code we can't see.
    pub fn reachable(&self, analysis: &Analysis, entry: usize) -> (Vec<usize>, bool) {
        let mut seen = vec![false; self.insns.len()];
        let mut work = vec![entry];
        let mut opaque = false;
        while let Some(i) = work.pop() {
            if std::mem::replace(&mut seen[i], true) {
                continue;
            }
            opaque |= self.opaque[i];
            work.extend(self.successors(analysis, i).next);
        }
        let reached = (0..self.insns.len()).filter(|&i| seen[i]).collect();
        (reached, opaque)
    }
}

fn is_branch(insn: &Instruction) -> bool {
    let head = insn.mnemonic.head.as_str();
    ((head == "b" || head == "bc") && insn.mnemonic.condition.is_some())
        || matches!(head, "cbz" | "cbnz" | "tbz" | "tbnz")
}

pub(crate) fn operands(insn: &Instruction) -> &[Operand] {
    insn.operands.as_deref().unwrap_or_default()
}

/// Unconditional transfers that never fall through.
pub(crate) fn is_terminator(insn: &Instruction) -> bool {
    let head = insn.mnemonic.head.as_str();
    (head == "b" && insn.mnemonic.condition.is_none())
        || matches!(
            head,
            "br" | "braa"
                | "brab"
                | "braaz"
                | "brabz"
                | "ret"
                | "retaa"
                | "retab"
                | "eret"
                | "eretaa"
                | "eretab"
                | "brk"
                | "udf"
                | "hlt"
        )
}

/// Whether an instruction sets the condition flags (NZCV).
pub(crate) fn sets_flags(insn: &Instruction, platform: arm_isa::Platform) -> bool {
    let head = insn.mnemonic.head.as_str();
    matches!(
        head,
        "adds" | "subs" | "ands" | "bics" | "adcs" | "sbcs" | "negs" | "ngcs" | "cmp" | "cmn"
            | "tst" | "ccmp" | "ccmn" | "fcmp" | "fcmpe" | "fccmp" | "fccmpe" | "setf8"
            | "setf16" | "rmif" | "cfinv" | "axflag" | "xaflag"
    ) || (head == "msr"
        && operands(insn)
            .first()
            .is_some_and(|o| matches!(o, Operand::Ident(n) if n.eq_ignore_ascii_case("nzcv"))))
        // Darwin's system calls report errors in the carry flag.
        || (head == "svc" && platform == arm_isa::Platform::Darwin)
}

/// Whether an instruction reads the condition flags.
pub(crate) fn reads_flags(insn: &Instruction) -> bool {
    let head = insn.mnemonic.head.as_str();
    insn.mnemonic.condition.is_some()
        || matches!(
            head,
            "csel"
                | "csinc"
                | "csinv"
                | "csneg"
                | "cset"
                | "csetm"
                | "cinc"
                | "cinv"
                | "cneg"
                | "fcsel"
                | "ccmp"
                | "ccmn"
                | "fccmp"
                | "fccmpe"
                | "adc"
                | "adcs"
                | "sbc"
                | "sbcs"
                | "ngc"
                | "ngcs"
                | "cfinv"
        )
        || (head == "mrs"
            && operands(insn)
                .get(1)
                .is_some_and(|o| matches!(o, Operand::Ident(n) if n.eq_ignore_ascii_case("nzcv"))))
}

/// Where `svc` takes the system call number: x16 on Darwin, x8 on Linux.
pub(crate) fn syscall_register(platform: arm_isa::Platform) -> &'static str {
    match platform {
        arm_isa::Platform::Darwin => "x16",
        arm_isa::Platform::Linux => "x8",
    }
}

pub(crate) fn is_call(insn: &Instruction) -> bool {
    matches!(
        insn.mnemonic.head.as_str(),
        "bl" | "blr" | "blraa" | "blrab" | "blraaz" | "blrabz"
    )
}

/// The symbol a branch or call goes to, if it's defined in this file.
pub(crate) fn target(analysis: &Analysis, insn: &Instruction) -> Option<usize> {
    let range = insn.operand_ranges.last()?;
    analysis.reference_at(range.start)?.symbol
}

/// A general-purpose or SIMD register's identity, ignoring width: `x19` for
/// x19 and w19, `v8` for v8, q8, d8, s8, h8, b8. None for other registers.
pub(crate) fn register_id(operand: &Operand) -> Option<String> {
    let Operand::Register(r) = operand else {
        return None;
    };
    let name = r.register.llvm_name();
    match name {
        "FP" => return Some("x29".into()),
        "LR" => return Some("x30".into()),
        "SP" | "WSP" => return Some("sp".into()),
        "XZR" | "WZR" => return None,
        _ => {}
    }
    let (prefix, number) = name.split_at(1);
    let number: u32 = number.parse().ok()?;
    match (r.register.kind(), prefix) {
        (RegisterKind::Scalar, "X" | "W") => Some(format!("x{number}")),
        (RegisterKind::Scalar | RegisterKind::NeonVector, "Q" | "D" | "S" | "H" | "B") => {
            Some(format!("v{number}"))
        }
        _ => None,
    }
}

/// Mnemonics whose first register operand is a source, not a destination.
const FIRST_OPERAND_READ: &[&str] = &[
    "cmp", "cmn", "tst", "ccmp", "ccmn", "fcmp", "fcmpe", "fccmp", "fccmpe", "cbz", "cbnz", "tbz",
    "tbnz", "b", "bl", "br", "blr", "ret", "prfm", "prfum", "msr", "dc", "ic", "at", "tlbi", "sys",
];

/// Store-exclusives write their status register, the first operand.
const STORE_EXCLUSIVE: &[&str] = &[
    "stxr", "stxrb", "stxrh", "stlxr", "stlxrb", "stlxrh", "stxp", "stlxp",
];

/// The registers an instruction writes, by `register_id`. Covers the
/// destination operands and base register writeback.
/// LSE atomics, which don't follow the destination-first pattern.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Atomic {
    /// `ldadd Rs, Rt, [Xn]` and `swp`: read Rs, write Rt.
    LoadOp,
    /// `cas Rs, Rt, [Xn]`: read both, write Rs. `casp` reads and writes pairs.
    CompareAndSwap,
}

fn atomic(head: &str) -> Option<Atomic> {
    const LOAD_OPS: &[&str] = &[
        "ldadd", "ldclr", "ldeor", "ldset", "ldsmax", "ldsmin", "ldumax", "ldumin", "swp",
    ];
    if LOAD_OPS.iter().any(|op| head.starts_with(op)) {
        Some(Atomic::LoadOp)
    } else if head.starts_with("cas") {
        Some(Atomic::CompareAndSwap)
    } else {
        None
    }
}

pub(crate) fn writes(insn: &Instruction) -> Vec<String> {
    let head = insn.mnemonic.head.as_str();
    let ops = operands(insn);
    let registers: Vec<String> = ops
        .iter()
        .take_while(|o| !matches!(o, Operand::Token(t) if t == "["))
        .filter_map(register_id)
        .collect();
    let mut out = Vec::new();
    match atomic(head) {
        Some(Atomic::LoadOp) => {
            out.extend(registers.get(1).cloned());
            return out;
        }
        Some(Atomic::CompareAndSwap) => {
            let pair = head.starts_with("casp");
            out.extend(registers.iter().take(if pair { 2 } else { 1 }).cloned());
            return out;
        }
        None => {}
    }
    let is_store = head.starts_with("st") && !STORE_EXCLUSIVE.contains(&head);
    if STORE_EXCLUSIVE.contains(&head) {
        out.extend(registers.first().cloned());
    } else if !is_store && !FIRST_OPERAND_READ.contains(&head) {
        let pair = matches!(head, "ldp" | "ldnp" | "ldpsw" | "ldxp" | "ldaxp" | "ldiapp");
        out.extend(registers.iter().take(if pair { 2 } else { 1 }).cloned());
    }
    if head.starts_with("ld") {
        out.extend(ops.iter().flat_map(list_registers));
    }
    if let Some(base) = writeback_base(ops) {
        out.push(base);
    }
    out
}

/// The registers an instruction reads: every register operand except the
/// destinations `writes` counts, plus x16 for `svc` (Darwin's syscall
/// number) and x30 for `ret`. A destination that's also read, like `movk`'s,
/// isn't counted: that can only miss a read, never invent one.
pub(crate) fn reads(insn: &Instruction, platform: arm_isa::Platform) -> Vec<String> {
    let head = insn.mnemonic.head.as_str();
    let ops = operands(insn);
    let is_store = head.starts_with("st") && !STORE_EXCLUSIVE.contains(&head);
    let destinations = if STORE_EXCLUSIVE.contains(&head) {
        1
    } else if is_store || FIRST_OPERAND_READ.contains(&head) {
        0
    } else if matches!(head, "ldp" | "ldnp" | "ldpsw" | "ldxp" | "ldaxp" | "ldiapp") {
        2
    } else {
        1
    };
    let mut skipped = 0;
    let mut out = Vec::new();
    let mut in_memory = false;
    // `ldadd Rs, Rt, [Xn]` reads Rs and Xn; cas reads everything.
    let skip_second = atomic(head) == Some(Atomic::LoadOp);
    let destinations = if atomic(head).is_some() {
        0
    } else {
        destinations
    };
    let mut position = 0;
    for operand in ops {
        if let Operand::Token(t) = operand {
            in_memory |= t == "[";
            continue;
        }
        let Some(id) = register_id(operand) else {
            continue;
        };
        position += 1;
        if skip_second && !in_memory && position == 2 {
            continue;
        }
        if !in_memory && skipped < destinations {
            skipped += 1;
            continue;
        }
        out.push(id);
    }
    if head == "svc" {
        out.push(syscall_register(platform).into());
    }
    if head == "ret" && ops.is_empty() {
        out.push("x30".into());
    }
    // Instructions that keep part of their destination read it too.
    if keeps_destination(head, ops)
        && let Some(dest) = ops.first().and_then(register_id)
    {
        out.push(dest);
    }
    // `{v0.4s, v1.4s}`: a store or table lookup reads the list.
    if !head.starts_with("ld") {
        out.extend(ops.iter().flat_map(list_registers));
    }
    out
}

/// Instructions that write only part of their destination, or accumulate
/// into it, so its old value matters.
fn keeps_destination(head: &str, ops: &[Operand]) -> bool {
    const ACCUMULATE: &[&str] = &[
        "movk", "bfi", "bfxil", "bfm", "bfc", "fmla", "fmls", "mla", "mls", "bsl", "bit", "bif",
        "sli", "sri", "ins", "umlal", "umlal2", "smlal", "smlal2", "umlsl", "umlsl2", "smlsl",
        "smlsl2", "sdot", "udot", "usdot", "sudot", "fmlal", "fmlal2", "fmlsl", "fmlsl2", "bfdot",
        "bfmmla", "bfmlalb", "bfmlalt", "smmla", "ummla", "usmmla", "sqrdmlah", "sqrdmlsh",
        "fcmla", "sqdmlal", "sqdmlal2", "sqdmlsl", "sqdmlsl2", "sadalp", "uadalp", "usra", "ssra",
        "ursra", "srsra", "saba", "uaba", "sabal", "sabal2", "uabal", "uabal2", "tbx", "xtn2",
        "sqxtn2", "uqxtn2", "sqxtun2", "shrn2", "rshrn2", "fcvtn2", "fcvtxn2",
    ];
    if ACCUMULATE.contains(&head) {
        return true;
    }
    // `mov v0.s[1], w0` writes one lane: an index before the source.
    let source = ops
        .iter()
        .skip(1)
        .position(|o| matches!(o, Operand::Register(_)))
        .map_or(ops.len(), |p| p + 1);
    matches!(head, "mov" | "ld1" | "ld2" | "ld3" | "ld4")
        && ops[..source.min(ops.len())]
            .iter()
            .any(|o| matches!(o, Operand::VectorIndex(_)))
}

/// The registers of a vector list: `{v0.4s, v1.4s}` is v0 and v1.
pub(crate) fn list_registers(operand: &Operand) -> Vec<String> {
    let Operand::VectorList {
        first,
        count,
        stride,
        ..
    } = operand
    else {
        return Vec::new();
    };
    if first.kind() != RegisterKind::NeonVector {
        return Vec::new();
    }
    let llvm = first.llvm_name();
    let Ok(n) = llvm
        .trim_start_matches(|c: char| c.is_ascii_alphabetic())
        .parse::<u32>()
    else {
        return Vec::new();
    };
    (0..*count)
        .map(|k| format!("v{}", (n + k * stride) % 32))
        .collect()
}

/// The registers an instruction stores to memory.
/// The registers a load from the stack (`[sp, ...]` or `[x29, ...]`)
/// brings back: how a function restores what its prologue saved.
pub(crate) fn restored(insn: &Instruction) -> Vec<String> {
    let head = insn.mnemonic.head.as_str();
    if !head.starts_with("ld") || atomic(head).is_some() {
        return Vec::new();
    }
    let ops = operands(insn);
    let base = ops
        .iter()
        .skip_while(|o| !matches!(o, Operand::Token(t) if t == "["))
        .find_map(register_id);
    if !matches!(base.as_deref(), Some("sp" | "x29")) {
        return Vec::new();
    }
    ops.iter()
        .take_while(|o| !matches!(o, Operand::Token(t) if t == "["))
        .filter_map(register_id)
        .collect()
}

pub(crate) fn stored(insn: &Instruction) -> Vec<String> {
    let head = insn.mnemonic.head.as_str();
    if !head.starts_with("st") {
        return Vec::new();
    }
    let skip = usize::from(STORE_EXCLUSIVE.contains(&head));
    operands(insn)
        .iter()
        .take_while(|o| !matches!(o, Operand::Token(t) if t == "["))
        .filter_map(register_id)
        .skip(skip)
        .collect()
}

/// The base register of a pre-indexed (`[x0, #8]!`) or post-indexed
/// (`[x0], #8`) memory operand, which the instruction updates.
fn writeback_base(ops: &[Operand]) -> Option<String> {
    let open = ops
        .iter()
        .position(|o| matches!(o, Operand::Token(t) if t == "["))?;
    let close = open
        + ops[open..]
            .iter()
            .position(|o| matches!(o, Operand::Token(t) if t == "]"))?;
    let base = register_id(ops.get(open + 1)?)?;
    let pre = matches!(ops.get(close + 1), Some(Operand::Token(t)) if t == "!");
    let post = matches!(ops.get(close + 1), Some(Operand::Imm(_)));
    (pre || post).then_some(base)
}

/// How an instruction changes sp, in bytes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum SpChange {
    None,
    By(i64),
    /// sp is set in a way we don't follow, like `mov sp, x29`.
    Unknown,
}

pub(crate) fn sp_change(insn: &Instruction) -> SpChange {
    let ops = operands(insn);
    let is_sp = |o: Option<&Operand>| o.and_then(register_id).as_deref() == Some("sp");
    let head = insn.mnemonic.head.as_str();
    // add/sub sp, sp, #n{, lsl #12}
    if matches!(head, "add" | "sub") && is_sp(ops.first()) {
        if !is_sp(ops.get(1)) {
            return SpChange::Unknown;
        }
        let shift = match ops.get(3) {
            Some(Operand::ShiftExtend {
                kind,
                amount: Some(a),
                ..
            }) if kind == "lsl" => *a,
            None => 0,
            _ => return SpChange::Unknown,
        };
        return match ops.get(2) {
            Some(Operand::Imm(Imm::Constant(n))) => {
                let n = n << shift;
                SpChange::By(if head == "add" { n } else { -n })
            }
            _ => SpChange::Unknown,
        };
    }
    // Writeback to sp: `[sp, #-16]!` and `[sp], #16`.
    if let Some(open) = ops
        .iter()
        .position(|o| matches!(o, Operand::Token(t) if t == "["))
        && is_sp(ops.get(open + 1))
    {
        let close = ops[open..]
            .iter()
            .position(|o| matches!(o, Operand::Token(t) if t == "]"))
            .map(|p| open + p);
        if let Some(close) = close {
            match (ops.get(close + 1), ops.get(close - 1)) {
                (Some(Operand::Token(t)), Some(Operand::Imm(Imm::Constant(n)))) if t == "!" => {
                    return SpChange::By(*n);
                }
                (Some(Operand::Imm(Imm::Constant(n))), _) => return SpChange::By(*n),
                (Some(Operand::Token(t)), _) if t == "!" => return SpChange::Unknown,
                (Some(Operand::Imm(_)), _) => return SpChange::Unknown,
                _ => {}
            }
        }
    }
    if writes(insn).iter().any(|r| r == "sp") {
        return SpChange::Unknown;
    }
    SpChange::None
}

#[cfg(test)]
mod tests {
    use super::*;

    fn insn(text: &str) -> Instruction {
        let analysis = Analysis::new(text);
        analysis
            .items()
            .iter()
            .find_map(|i| match i {
                Item::Instruction(insn) => Some(insn.clone()),
                _ => None,
            })
            .expect("an instruction")
    }

    #[test]
    fn sp_changes() {
        assert_eq!(sp_change(&insn("sub sp, sp, #32")), SpChange::By(-32));
        assert_eq!(
            sp_change(&insn("add sp, sp, #1, lsl #12")),
            SpChange::By(4096)
        );
        assert_eq!(
            sp_change(&insn("stp x29, x30, [sp, #-16]!")),
            SpChange::By(-16)
        );
        assert_eq!(
            sp_change(&insn("ldp x29, x30, [sp], #16")),
            SpChange::By(16)
        );
        assert_eq!(sp_change(&insn("ldp x29, x30, [sp, #16]")), SpChange::None);
        assert_eq!(sp_change(&insn("mov sp, x29")), SpChange::Unknown);
        assert_eq!(sp_change(&insn("mov x29, sp")), SpChange::None);
        assert_eq!(sp_change(&insn("str x0, [x1], #8")), SpChange::None);
    }

    #[test]
    fn writes_and_stores() {
        assert_eq!(writes(&insn("add x19, x0, #1")), ["x19"]);
        assert_eq!(
            writes(&insn("ldp x19, x20, [sp], #16")),
            ["x19", "x20", "sp"]
        );
        assert_eq!(writes(&insn("str x19, [sp, #8]")), Vec::<String>::new());
        assert_eq!(writes(&insn("stxr w1, x0, [x2]")), ["x1"]);
        assert_eq!(writes(&insn("cmp x19, #0")), Vec::<String>::new());
        assert_eq!(writes(&insn("fmov d8, #1.0")), ["v8"]);
        assert_eq!(stored(&insn("stp x20, x19, [sp, #-32]!")), ["x20", "x19"]);
        assert_eq!(stored(&insn("stxr w1, x0, [x2]")), ["x0"]);
    }

    #[test]
    fn reads() {
        assert_eq!(
            super::reads(&insn("add x0, x9, x10"), arm_isa::Platform::Darwin),
            ["x9", "x10"]
        );
        assert_eq!(
            super::reads(&insn("str x9, [sp, #8]"), arm_isa::Platform::Darwin),
            ["x9", "sp"]
        );
        assert_eq!(
            super::reads(&insn("ldr x0, [x9]"), arm_isa::Platform::Darwin),
            ["x9"]
        );
        assert_eq!(
            super::reads(&insn("ldp x0, x1, [x9], #16"), arm_isa::Platform::Darwin),
            ["x9"]
        );
        assert_eq!(
            super::reads(&insn("cmp x9, #0"), arm_isa::Platform::Darwin),
            ["x9"]
        );
        assert_eq!(
            super::reads(&insn("stxr w1, x0, [x2]"), arm_isa::Platform::Darwin),
            ["x0", "x2"]
        );
        assert_eq!(
            super::reads(&insn("svc #0x80"), arm_isa::Platform::Darwin),
            ["x16"]
        );
        assert_eq!(
            super::reads(&insn("svc #0"), arm_isa::Platform::Linux),
            ["x8"]
        );
        assert_eq!(
            super::reads(
                &insn("fadd v16.4s, v17.4s, v18.4s"),
                arm_isa::Platform::Darwin
            ),
            ["v17", "v18"]
        );
    }

    #[test]
    fn functions_start_at_exported_or_called_labels() {
        let text = ".globl _a\n_a:\n bl helper\n ret\nhelper:\n ret\nnot_a_function:\n ret\n.data\n_x: .quad 0\n";
        let analysis = Analysis::new(text);
        let cfg = Cfg::new(&analysis);
        let names: Vec<_> = cfg.functions.iter().map(|f| f.name.as_str()).collect();
        assert_eq!(names, ["_a", "helper"]);
    }

    #[test]
    fn reachable_code_follows_branches_not_layout() {
        // `two` sits between `one`'s return and the code `_start` jumps to.
        let text =
            ".globl _start\n_start:\n bl one\n b 1f\none:\n ret\n1:\n bl two\n ret\ntwo:\n ret\n";
        let analysis = Analysis::new(text);
        let cfg = Cfg::new(&analysis);
        let reach = |name: &str| {
            let f = cfg
                .functions
                .iter()
                .find(|f| f.name == name)
                .expect("function");
            let (insns, _) = cfg.reachable(&analysis, f.entry.expect("entry"));
            insns.len()
        };
        // _start: bl one, b 1f, bl two, ret. one: ret. two: ret.
        assert_eq!(reach("_start"), 4);
        assert_eq!(reach("one"), 1);
        assert_eq!(reach("two"), 1);
    }
}
