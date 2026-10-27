//! Matching an instruction's operands against its forms, the way clang's
//! generated matcher does (docs/decisions/0017-operand-matching.md).
//!
//! Clang parses some operands differently depending on the class it expects:
//! `#1, lsl #12` is one shifted immediate where an add/sub immediate goes,
//! `x1, lsl #3` is one register in an SVE address, `#0.0` is the tokens `#0`
//! and `.0` after `fcmp`, and a bare name is a symbol or a keyword. The
//! matcher tries each reading, so the operand list can be built without
//! knowing the form.

use crate::operand::{Check, Imm, Operand, RegisterOperand};
use crate::predicates::check;
use crate::{ClassKind, Form, Mnemonic, OperandClass, RegisterKind, Special, forms};

/// Why an instruction's operands fit none of its forms.
#[derive(Debug, Clone, PartialEq)]
pub struct Mismatch {
    /// The operand that failed in the form that got furthest, as an index into
    /// the operands given (not counting tokens from the mnemonic). `None` when
    /// the problem is the count: too few or too many operands.
    pub operand: Option<usize>,
    /// What's wrong, in clang's words where clang has them.
    pub message: String,
    /// The form that got furthest, to show what was expected.
    pub closest: Option<Form>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum MatchResult {
    Matched(Form),
    Mismatch(Mismatch),
    /// The operands fit a form, but it needs CPU features that aren't on.
    /// Each inner list is one requirement, met by any of its features.
    MissingFeatures {
        form: Form,
        needs: Vec<Vec<&'static str>>,
    },
    /// The matcher can't judge this instruction: clang handles it in C++
    /// (SYS aliases, bitfield aliases, `lsl` by an immediate, `ldr =`).
    Unchecked,
}

/// Matches `operands` against the forms of `mnemonic`, generic syntax and
/// Apple's NEON syntax, in clang's order, ignoring CPU features.
pub fn match_instruction(mnemonic: &Mnemonic, operands: &[Operand]) -> MatchResult {
    match_for(mnemonic, operands, None)
}

/// Like [`match_instruction`], but a form only fits if `features` has what it
/// needs, as clang decides with its target's features.
pub fn match_for(
    mnemonic: &Mnemonic,
    operands: &[Operand],
    features: Option<&crate::FeatureSet>,
) -> MatchResult {
    if mnemonic.special().is_some() && mnemonic.special() != Some(Special::Bitfield) {
        return MatchResult::Unchecked;
    }
    if mnemonic.special() == Some(Special::Bitfield)
        || (mnemonic.head == "lsl" && matches!(operands.last(), Some(Operand::Imm(_))))
    {
        return MatchResult::Unchecked;
    }
    // Tokens clang makes from the mnemonic: `b.eq` is `.` and a condition;
    // `fadd.4s` has the suffix `.4s`.
    let mut all = Vec::new();
    if let Some(condition) = &mnemonic.condition {
        all.push(Operand::Token(".".into()));
        all.push(Operand::CondCode(condition.clone()));
    }
    all.extend(mnemonic.suffixes.iter().map(|s| Operand::Token(s.clone())));
    let prefix = all.len();
    all.extend(cpp_rewrites(&mnemonic.head, operands));

    let mut best: Option<Failure> = None;
    let mut any = false;
    let mut needs_features: Option<(Form, Vec<Vec<&'static str>>)> = None;
    for form in forms(&mnemonic.head) {
        any = true;
        let classes: Vec<OperandClass> = form.operands().collect();
        match match_from(&classes, &all, 0, 0) {
            Ok(()) => {
                let missing = features.map(|f| f.missing(form)).unwrap_or_default();
                if missing.is_empty() {
                    return MatchResult::Matched(form);
                }
                needs_features.get_or_insert((form, missing));
            }
            Err(failure) => {
                let failure = Failure { form, ..failure };
                if best.as_ref().is_none_or(|b| failure.better_than(b)) {
                    best = Some(failure);
                }
            }
        }
    }
    if let Some((form, needs)) = needs_features {
        return MatchResult::MissingFeatures { form, needs };
    }
    let Some(best) = best.filter(|_| any) else {
        return MatchResult::Unchecked;
    };
    MatchResult::Mismatch(best.into_mismatch(prefix, operands.len()))
}

#[derive(Debug, Clone)]
struct Failure {
    form: Form,
    /// Operands matched before the failure.
    matched: usize,
    /// Index into the full operand list where it failed, or None if it ran out
    /// of operands or classes.
    at: Option<usize>,
    class: Option<OperandClass>,
    near: bool,
}

impl Failure {
    fn better_than(&self, other: &Failure) -> bool {
        (self.matched, self.near) > (other.matched, other.near)
    }

    fn into_mismatch(self, prefix: usize, given: usize) -> Mismatch {
        let operand = self
            .at
            .and_then(|at| at.checked_sub(prefix))
            .filter(|&i| i < given);
        let message = match (self.at, self.class) {
            (Some(_), Some(class)) if self.near => class
                .message()
                .unwrap_or("invalid operand for instruction")
                .trim_end_matches('.')
                .to_string(),
            (Some(_), Some(_)) => "invalid operand for instruction".to_string(),
            (Some(_), None) => "too many operands for instruction".to_string(),
            (None, _) => "too few operands for instruction".to_string(),
        };
        Mismatch {
            operand,
            message,
            closest: Some(self.form),
        }
    }
}

/// Matches classes from `c` against operands from `o`, trying every reading
/// of each operand. Returns how far the best attempt got on failure.
fn match_from(
    classes: &[OperandClass],
    ops: &[Operand],
    c: usize,
    o: usize,
) -> Result<(), Failure> {
    let fail = |at: Option<usize>, class: Option<OperandClass>, near: bool| Failure {
        // Filled in by the caller.
        form: Form(0),
        matched: o,
        at,
        class,
        near,
    };
    if c == classes.len() {
        return if o == ops.len() {
            Ok(())
        } else {
            Err(fail(Some(o), None, false))
        };
    }
    let class = classes[c];
    if o == ops.len() {
        return Err(fail(None, Some(class), false));
    }
    let mut best = fail(Some(o), Some(class), false);
    let keep = |result: Result<(), Failure>, best: &mut Failure| match result {
        Ok(()) => true,
        Err(f) => {
            if (f.matched, f.near) > (best.matched, best.near) {
                *best = f;
            }
            false
        }
    };

    // `#0.0` after fcmp and friends: one operand, two token classes.
    if class.kind() == ClassKind::Token
        && class.name() == "#0"
        && classes.get(c + 1).is_some_and(|n| n.name() == ".0")
        && is_zero(&ops[o])
        && keep(match_from(classes, ops, c + 2, o + 1), &mut best)
    {
        return Ok(());
    }

    // A consecutive register pair, `x0, x1`, which clang reads as one operand.
    if let Some(check) = seq_pair(class, &ops[o], ops.get(o + 1)) {
        match check {
            Check::Match => {
                if keep(match_from(classes, ops, c + 1, o + 2), &mut best) {
                    return Ok(());
                }
            }
            _ => {
                if best.matched <= o {
                    best = fail(Some(o), Some(class), check == Check::NearMiss);
                }
            }
        }
        return Err(best);
    }

    // Two operands that clang reads as one in this class.
    if let Some(merged) = ops.get(o + 1).and_then(|next| merge(class, &ops[o], next)) {
        let near = match check_class(class, &merged) {
            Check::Match | Check::Unknown => {
                if keep(match_from(classes, ops, c + 1, o + 2), &mut best) {
                    return Ok(());
                }
                false
            }
            Check::NearMiss => true,
            Check::NoMatch => false,
        };
        if near && !best.near && best.matched <= o {
            best = fail(Some(o), Some(class), true);
        }
    }

    match check_class(class, &ops[o]) {
        Check::Match | Check::Unknown => {
            if keep(match_from(classes, ops, c + 1, o + 1), &mut best) {
                return Ok(());
            }
        }
        Check::NearMiss => {
            if best.matched <= o {
                best = fail(Some(o), Some(class), true);
            }
        }
        Check::NoMatch => {}
    }
    Err(best)
}

/// Clang's one-operand reading of two adjacent operands, if this class asks
/// for it.
fn merge(class: OperandClass, first: &Operand, second: &Operand) -> Option<Operand> {
    // `[x0, #0]` where only a base register is allowed: clang's GPR64sp0
    // parser takes the register and an optional `#0` as one operand.
    if class.name() == "GPR64sp0"
        && matches!(first, Operand::Register(_))
        && second == &Operand::Imm(Imm::Constant(0))
    {
        return Some(first.clone());
    }
    let Operand::ShiftExtend { kind, amount, .. } = second else {
        return None;
    };
    let predicate = class.predicate();
    let takes_shifted_imm = ["isAddSubImm", "isSVEAddSubImm", "isSVECpyImm"]
        .iter()
        .any(|p| predicate.starts_with(p));
    let takes_shifted_reg = [
        "isGPR64WithShiftExtend",
        "isSVEDataVectorRegWithShiftExtend",
    ]
    .iter()
    .any(|p| predicate.starts_with(p));
    match first {
        Operand::Imm(value) if takes_shifted_imm && kind == "lsl" => Some(Operand::ShiftedImm {
            value: value.clone(),
            shift: u32::try_from((*amount)?).ok()?,
        }),
        Operand::Register(register) if takes_shifted_reg && register.shift.is_none() => {
            Some(Operand::Register(RegisterOperand {
                shift: Some((kind.clone(), *amount)),
                ..register.clone()
            }))
        }
        _ => None,
    }
}

/// For a `WSeqPair` or `XSeqPair` class: whether two operands are a pair
/// clang accepts, an even register and the next one, both W or both X. None
/// for other classes.
fn seq_pair(class: OperandClass, first: &Operand, second: Option<&Operand>) -> Option<Check> {
    let width = match class.predicate() {
        "isWSeqPair" => 'W',
        "isXSeqPair" => 'X',
        _ => return None,
    };
    let number = |operand: Option<&Operand>| match operand {
        Some(Operand::Register(r)) if r.register.kind() == RegisterKind::Scalar => {
            gpr_number(r.register.llvm_name(), width)
        }
        _ => None,
    };
    Some(match (number(Some(first)), number(second)) {
        (Some(a), Some(b)) if a % 2 == 0 && b == a + 1 => Check::Match,
        (Some(_), Some(_)) => Check::NearMiss,
        _ => Check::NoMatch,
    })
}

/// A general-purpose register's number for pairing: X0 to X28, then FP 29,
/// LR 30, and XZR 31, as in LLVM's GPR64 order. W registers likewise.
fn gpr_number(llvm_name: &str, width: char) -> Option<u32> {
    if width == 'X' && llvm_name == "FP" {
        return Some(29);
    }
    if width == 'X' && llvm_name == "LR" {
        return Some(30);
    }
    match llvm_name.strip_prefix(width)? {
        "ZR" => Some(31),
        rest => rest.parse().ok().filter(|n| *n <= 30),
    }
}

/// Register rewrites clang makes in C++ before matching
/// (`AArch64AsmParser::MatchAndEmitInstruction`): the extend aliases take a
/// W source where the forms want X, and `uxtb`/`uxth` take an X destination.
fn cpp_rewrites(head: &str, operands: &[Operand]) -> Vec<Operand> {
    let mut out = operands.to_vec();
    let is_x = |o: &Operand| matches!(o, Operand::Register(r) if r.register.kind() == RegisterKind::Scalar && scalar_width(r.register.llvm_name()) == Some('X'));
    if out.len() == 2 {
        match head {
            "sxtw" | "uxtw" => out[1] = resized(&out[1], 'X'),
            "sxtb" | "sxth" if is_x(&out[0]) => out[1] = resized(&out[1], 'X'),
            "uxtb" | "uxth" if is_x(&out[0]) => out[0] = resized(&out[0], 'W'),
            _ => {}
        }
    }
    out
}

/// 'X' or 'W' for a general-purpose register's LLVM name.
fn scalar_width(llvm_name: &str) -> Option<char> {
    match llvm_name {
        "SP" | "XZR" | "FP" | "LR" => Some('X'),
        "WSP" | "WZR" => Some('W'),
        _ => {
            let width = llvm_name.chars().next()?;
            let rest = &llvm_name[1..];
            (matches!(width, 'X' | 'W') && rest.parse::<u32>().is_ok()).then_some(width)
        }
    }
}

/// The same general-purpose register at another width; other operands as they are.
fn resized(operand: &Operand, width: char) -> Operand {
    let Operand::Register(r) = operand else {
        return operand.clone();
    };
    if r.register.kind() != RegisterKind::Scalar {
        return operand.clone();
    }
    let name = r.register.llvm_name();
    let number = match name {
        "XZR" | "WZR" => "zr".to_string(),
        "SP" | "WSP" => "sp".to_string(),
        "FP" => "29".to_string(),
        "LR" => "30".to_string(),
        _ if scalar_width(name).is_some() => name[1..].to_string(),
        _ => return operand.clone(),
    };
    let target = match (width, number.as_str()) {
        ('X', "sp") => "sp".to_string(),
        ('W', "sp") => "wsp".to_string(),
        (w, n) => format!("{}{n}", w.to_ascii_lowercase()),
    };
    match crate::Register::lookup_kind(&target, RegisterKind::Scalar) {
        Some(register) => Operand::Register(RegisterOperand {
            register,
            ..r.clone()
        }),
        None => operand.clone(),
    }
}

fn is_zero(operand: &Operand) -> bool {
    match operand {
        Operand::FpImm(value) => *value == 0.0,
        Operand::Imm(Imm::Constant(0)) => true,
        _ => false,
    }
}

/// Checks one operand against one class, trying each reading of a bare name:
/// as itself, as a condition code, and as a symbol.
fn check_class(class: OperandClass, operand: &Operand) -> Check {
    let direct = check_one(class, operand);
    if matches!(direct, Check::Match | Check::Unknown) {
        return direct;
    }
    // adr and adrp parse their operand as an expression, so a label named
    // like a register (`adr x0, s0`) is a symbol there. Branches don't do
    // this: `b d1` is an error even with a label d1.
    if let Operand::Register(r) = operand {
        let label_class = matches!(class.predicate(), "isAdrLabel" | "isAdrpLabel");
        if label_class && r.suffix.is_none() && r.shift.is_none() {
            let symbol = Operand::Imm(Imm::Symbol {
                name: r.register.name().to_string(),
                variant: None,
                specifier: None,
                addend: 0,
            });
            return check_one(class, &symbol);
        }
    }
    if let Operand::Ident(name) = operand {
        let lower = name.to_ascii_lowercase();
        let readings = [
            crate::CONDITION_CODES
                .contains(&lower.as_str())
                .then(|| Operand::CondCode(lower.clone())),
            Some(Operand::Imm(Imm::Symbol {
                name: name.clone(),
                variant: None,
                specifier: None,
                addend: 0,
            })),
        ];
        let mut result = direct;
        for reading in readings.into_iter().flatten() {
            match check_one(class, &reading) {
                found @ (Check::Match | Check::Unknown) => return found,
                Check::NearMiss => result = Check::NearMiss,
                Check::NoMatch => {}
            }
        }
        return result;
    }
    direct
}

fn check_one(class: OperandClass, operand: &Operand) -> Check {
    match class.kind() {
        ClassKind::Invalid => Check::NoMatch,
        ClassKind::Optional => Check::Match,
        ClassKind::Token => check_token(class.name(), operand),
        // Clang checks membership for a register of any kind: `za` fits MPR.
        ClassKind::Register => match operand {
            Operand::Register(r) if r.shift.is_none() => {
                if r.register.fits(class) {
                    Check::Match
                } else {
                    Check::NoMatch
                }
            }
            _ => Check::NoMatch,
        },
        ClassKind::Custom => match class.predicate() {
            "" => Check::Unknown,
            predicate => check(predicate, operand),
        },
    }
}

/// Token classes: literal text, or `#N`, which clang matches against an
/// immediate equal to N.
fn check_token(text: &str, operand: &Operand) -> Check {
    if let Some(number) = text.strip_prefix('#') {
        let Ok(expected) = number.parse::<i64>() else {
            return Check::Unknown;
        };
        return match operand {
            Operand::Imm(Imm::Constant(value)) if *value == expected => Check::Match,
            Operand::Imm(Imm::Constant(_)) => Check::NearMiss,
            Operand::Token(t) if t == text => Check::Match,
            _ => Check::NoMatch,
        };
    }
    let matches = match operand {
        Operand::Token(t) | Operand::Ident(t) => t.eq_ignore_ascii_case(text),
        // `smstart za`: the keyword is also a register name.
        Operand::Register(r) => r.suffix.is_none() && r.register.name().eq_ignore_ascii_case(text),
        _ => false,
    };
    if matches {
        Check::Match
    } else {
        Check::NoMatch
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Register;

    fn reg(name: &str) -> Operand {
        let (base, suffix) = match name.split_once('.') {
            Some((b, s)) => (b, Some(s.to_string())),
            None => (name, None),
        };
        let register = Register::lookup(base).next().expect("register");
        Operand::Register(RegisterOperand {
            register,
            suffix,
            shift: None,
        })
    }

    fn imm(value: i64) -> Operand {
        Operand::Imm(Imm::Constant(value))
    }

    fn tok(text: &str) -> Operand {
        Operand::Token(text.into())
    }

    fn matched(mnemonic: &str, operands: &[Operand]) -> Option<&'static str> {
        match match_instruction(&Mnemonic::parse(mnemonic), operands) {
            MatchResult::Matched(form) => Some(form.opcode()),
            _ => None,
        }
    }

    fn mismatch(mnemonic: &str, operands: &[Operand]) -> Mismatch {
        match match_instruction(&Mnemonic::parse(mnemonic), operands) {
            MatchResult::Mismatch(m) => m,
            other => panic!("expected a mismatch, got {other:?}"),
        }
    }

    #[test]
    fn registers_must_fit_their_class() {
        assert_eq!(
            matched("add", &[reg("x0"), reg("x1"), reg("x2")]),
            Some("ADDXrs")
        );
        assert_eq!(
            matched("add", &[reg("w0"), reg("w1"), reg("w2")]),
            Some("ADDWrs")
        );
        let m = mismatch("add", &[reg("x0"), reg("w1"), reg("x2")]);
        assert_eq!(m.operand, Some(1));
        assert_eq!(m.message, "invalid operand for instruction");
    }

    #[test]
    fn operand_counts() {
        let m = mismatch("add", &[reg("x0"), reg("x1")]);
        assert_eq!(m.message, "too few operands for instruction");
        let m = mismatch("ret", &[reg("x0"), reg("x1")]);
        assert_eq!(m.message, "too many operands for instruction");
        assert_eq!(matched("ret", &[]), Some("RET"));
    }

    #[test]
    fn memory_tokens() {
        let ops = [
            reg("x0"),
            reg("x1"),
            tok("["),
            reg("sp"),
            imm(16),
            tok("]"),
            tok("!"),
        ];
        assert_eq!(matched("ldp", &ops), Some("LDPXpre"));
        let ops = [reg("x0"), tok("["), reg("x1"), tok("]")];
        assert_eq!(matched("ldr", &ops), Some("LDRXui"));
    }

    #[test]
    fn condition_from_the_mnemonic() {
        let label = Operand::Ident("_f".into());
        assert_eq!(matched("b.eq", std::slice::from_ref(&label)), Some("Bcc"));
        assert_eq!(matched("bne", &[label]), Some("Bcc"));
    }

    #[test]
    fn bare_condition_codes() {
        let ops = [reg("x0"), reg("x1"), reg("x2"), Operand::Ident("EQ".into())];
        assert_eq!(matched("csel", &ops), Some("CSELXr"));
    }

    #[test]
    fn fcmp_with_zero() {
        assert_eq!(
            matched("fcmp", &[reg("s0"), Operand::FpImm(0.0)]),
            Some("FCMPSri")
        );
        assert_eq!(matched("fcmp", &[reg("d1"), imm(0)]), Some("FCMPDri"));
    }

    #[test]
    fn hash_tokens_match_immediates() {
        // ld1 with a post-index of exactly the list size.
        let list = Operand::VectorList {
            first: Register::lookup("v0").next().expect("v0"),
            count: 4,
            stride: 1,
            suffix: Some("16b".into()),
        };
        let ops = [list.clone(), tok("["), reg("x0"), tok("]"), imm(64)];
        assert!(matches!(
            match_instruction(&Mnemonic::parse("ld1"), &ops),
            MatchResult::Matched(_)
        ));
    }

    #[test]
    fn extend_aliases_take_w_sources() {
        assert_eq!(matched("sxtw", &[reg("x1"), reg("w2")]), Some("SBFMXri"));
        assert_eq!(matched("sxtb", &[reg("x1"), reg("w2")]), Some("SBFMXri"));
        assert!(matched("uxtb", &[reg("x1"), reg("w2")]).is_some());
        assert_eq!(matched("sxtb", &[reg("w1"), reg("w2")]), Some("SBFMWri"));
    }

    #[test]
    fn base_register_with_zero_offset() {
        let ops = [reg("x4"), tok("["), reg("sp"), imm(0), tok("]")];
        assert_eq!(matched("ldar", &ops), Some("LDARX"));
        let ops = [reg("x4"), tok("["), reg("sp"), imm(8), tok("]")];
        assert!(matched("ldar", &ops).is_none());
    }

    #[test]
    fn adr_reads_register_names_as_labels() {
        assert_eq!(matched("adr", &[reg("x0"), reg("s0")]), Some("ADR"));
        // Branches don't: clang rejects `b d1` even with a label d1.
        assert!(matched("b", &[reg("d1")]).is_none());
    }

    #[test]
    fn register_pairs() {
        let ops = [
            reg("x0"),
            reg("x1"),
            reg("x2"),
            reg("x3"),
            tok("["),
            reg("x4"),
            tok("]"),
        ];
        assert_eq!(matched("casp", &ops), Some("CASPX"));
        let ops = [
            reg("w4"),
            reg("w5"),
            reg("w6"),
            reg("w7"),
            tok("["),
            reg("sp"),
            tok("]"),
        ];
        assert_eq!(matched("casp", &ops), Some("CASPW"));
        let odd = [
            reg("x1"),
            reg("x2"),
            reg("x2"),
            reg("x3"),
            tok("["),
            reg("x4"),
            tok("]"),
        ];
        assert_eq!(mismatch("casp", &odd).operand, Some(0));
    }

    #[test]
    fn keywords_that_are_register_names() {
        assert!(matches!(
            match_instruction(&Mnemonic::parse("smstart"), &[reg("za")]),
            MatchResult::Matched(_)
        ));
    }

    #[test]
    fn features_gate_forms() {
        let ops = [reg("x0"), reg("x1")];
        let apple = crate::FeatureSet::apple_default();
        let abs = match_for(&Mnemonic::parse("abs"), &ops, Some(&apple));
        assert!(
            matches!(&abs, MatchResult::MissingFeatures { needs, .. } if needs == &[vec!["cssc"]]),
            "{abs:?}"
        );
        let mut cssc = apple.clone();
        cssc.modify("cssc").expect("known");
        assert!(matches!(
            match_for(&Mnemonic::parse("abs"), &ops, Some(&cssc)),
            MatchResult::Matched(_)
        ));
        // Without a feature set, features aren't checked.
        assert!(matches!(
            match_instruction(&Mnemonic::parse("abs"), &ops),
            MatchResult::Matched(_)
        ));
    }

    #[test]
    fn handled_in_cpp_is_unchecked() {
        for name in ["ubfx", "tlbi", "dc"] {
            assert_eq!(
                match_instruction(&Mnemonic::parse(name), &[]),
                MatchResult::Unchecked,
                "{name}"
            );
        }
        let ops = [reg("x0"), reg("x1"), imm(3)];
        assert_eq!(
            match_instruction(&Mnemonic::parse("lsl"), &ops),
            MatchResult::Unchecked
        );
    }
}
