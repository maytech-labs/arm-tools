//! Instructions clang handles in C++ before its match table
//! (`AArch64AsmParser::MatchAndEmitInstruction` and `parseSysAlias` in LLVM
//! 21), checked the same way (docs/decisions/0037-cpp-aliases.md).
//!
//! The bitfield aliases (`ubfx`, `bfi`, ...) and `lsl` by an immediate are
//! range-checked, rewritten to `ubfm`, `sbfm`, or `bfm` as clang does, and
//! the rewrite goes through the matcher. The SYS aliases (`dc`, `ic`, `at`,
//! `tlbi`, ...) are checked for whether their operation takes a register.

use crate::matcher::{MatchResult, Mismatch, match_for};
use crate::operand::{Imm, Operand, RegisterOperand};
use crate::{FeatureSet, Mnemonic, Register, RegisterKind};

/// How an alias is written, for signature help: `ubfx Rd, Rn, #lsb, #width`.
pub fn alias_signature(head: &str) -> Option<&'static str> {
    Some(match head {
        "ubfx" => "ubfx Rd, Rn, #lsb, #width",
        "sbfx" => "sbfx Rd, Rn, #lsb, #width",
        "bfxil" => "bfxil Rd, Rn, #lsb, #width",
        "ubfiz" => "ubfiz Rd, Rn, #lsb, #width",
        "sbfiz" => "sbfiz Rd, Rn, #lsb, #width",
        "bfi" => "bfi Rd, Rn, #lsb, #width",
        "bfc" => "bfc Rd, #lsb, #width",
        "dc" => "dc <op>, Xt",
        "ic" => "ic <op>, {Xt}",
        "at" => "at <op>, Xt",
        "tlbi" => "tlbi <op>, {Xt}",
        "cfp" | "dvp" | "cpp" | "cosp" => return sys_signature(head),
        _ => return None,
    })
}

fn sys_signature(head: &str) -> Option<&'static str> {
    Some(match head {
        "cfp" => "cfp rctx, Xt",
        "dvp" => "dvp rctx, Xt",
        "cpp" => "cpp rctx, Xt",
        "cosp" => "cosp rctx, Xt",
        _ => return None,
    })
}

/// An operation a SYS alias names, and whether it takes a register.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SysOp {
    pub name: &'static str,
    pub register: bool,
}

/// The operations armls knows for a SYS alias, common ones first. Clang
/// knows more (MTE, RME, and range TLBI operations among them); names not
/// here aren't judged.
pub fn sys_operations(head: &str) -> &'static [SysOp] {
    match head {
        "dc" => &[
            SysOp {
                name: "civac",
                register: true,
            },
            SysOp {
                name: "cvac",
                register: true,
            },
            SysOp {
                name: "cvau",
                register: true,
            },
            SysOp {
                name: "ivac",
                register: true,
            },
            SysOp {
                name: "zva",
                register: true,
            },
            SysOp {
                name: "cvap",
                register: true,
            },
            SysOp {
                name: "cvadp",
                register: true,
            },
            SysOp {
                name: "isw",
                register: true,
            },
            SysOp {
                name: "csw",
                register: true,
            },
            SysOp {
                name: "cisw",
                register: true,
            },
        ],
        "ic" => &[
            SysOp {
                name: "ivau",
                register: true,
            },
            SysOp {
                name: "iallu",
                register: false,
            },
            SysOp {
                name: "ialluis",
                register: false,
            },
        ],
        "at" => &[
            SysOp {
                name: "s1e0r",
                register: true,
            },
            SysOp {
                name: "s1e0w",
                register: true,
            },
            SysOp {
                name: "s1e1r",
                register: true,
            },
            SysOp {
                name: "s1e1w",
                register: true,
            },
            SysOp {
                name: "s1e1rp",
                register: true,
            },
            SysOp {
                name: "s1e1wp",
                register: true,
            },
            SysOp {
                name: "s1e2r",
                register: true,
            },
            SysOp {
                name: "s1e2w",
                register: true,
            },
            SysOp {
                name: "s1e3r",
                register: true,
            },
            SysOp {
                name: "s1e3w",
                register: true,
            },
            SysOp {
                name: "s12e0r",
                register: true,
            },
            SysOp {
                name: "s12e0w",
                register: true,
            },
            SysOp {
                name: "s12e1r",
                register: true,
            },
            SysOp {
                name: "s12e1w",
                register: true,
            },
        ],
        "tlbi" => &[
            SysOp {
                name: "vmalle1",
                register: false,
            },
            SysOp {
                name: "vmalle1is",
                register: false,
            },
            SysOp {
                name: "vmalle1os",
                register: false,
            },
            SysOp {
                name: "alle1",
                register: false,
            },
            SysOp {
                name: "alle1is",
                register: false,
            },
            SysOp {
                name: "alle1os",
                register: false,
            },
            SysOp {
                name: "alle2",
                register: false,
            },
            SysOp {
                name: "alle2is",
                register: false,
            },
            SysOp {
                name: "alle2os",
                register: false,
            },
            SysOp {
                name: "alle3",
                register: false,
            },
            SysOp {
                name: "alle3is",
                register: false,
            },
            SysOp {
                name: "alle3os",
                register: false,
            },
            SysOp {
                name: "vmalls12e1",
                register: false,
            },
            SysOp {
                name: "vmalls12e1is",
                register: false,
            },
            SysOp {
                name: "vmalls12e1os",
                register: false,
            },
            SysOp {
                name: "vae1",
                register: true,
            },
            SysOp {
                name: "vae1is",
                register: true,
            },
            SysOp {
                name: "vae1os",
                register: true,
            },
            SysOp {
                name: "vale1",
                register: true,
            },
            SysOp {
                name: "vale1is",
                register: true,
            },
            SysOp {
                name: "vale1os",
                register: true,
            },
            SysOp {
                name: "vaae1",
                register: true,
            },
            SysOp {
                name: "vaae1is",
                register: true,
            },
            SysOp {
                name: "vaae1os",
                register: true,
            },
            SysOp {
                name: "vaale1",
                register: true,
            },
            SysOp {
                name: "vaale1is",
                register: true,
            },
            SysOp {
                name: "vaale1os",
                register: true,
            },
            SysOp {
                name: "aside1",
                register: true,
            },
            SysOp {
                name: "aside1is",
                register: true,
            },
            SysOp {
                name: "aside1os",
                register: true,
            },
            SysOp {
                name: "vae2",
                register: true,
            },
            SysOp {
                name: "vae2is",
                register: true,
            },
            SysOp {
                name: "vale2",
                register: true,
            },
            SysOp {
                name: "vale2is",
                register: true,
            },
            SysOp {
                name: "vae3",
                register: true,
            },
            SysOp {
                name: "vae3is",
                register: true,
            },
            SysOp {
                name: "vale3",
                register: true,
            },
            SysOp {
                name: "vale3is",
                register: true,
            },
            SysOp {
                name: "ipas2e1",
                register: true,
            },
            SysOp {
                name: "ipas2e1is",
                register: true,
            },
            SysOp {
                name: "ipas2le1",
                register: true,
            },
            SysOp {
                name: "ipas2le1is",
                register: true,
            },
        ],
        "cfp" | "dvp" | "cpp" | "cosp" => &[SysOp {
            name: "rctx",
            register: true,
        }],
        _ => &[],
    }
}

fn mismatch(operand: Option<usize>, message: String) -> MatchResult {
    MatchResult::Mismatch(Mismatch {
        operand,
        message,
        closest: None,
    })
}

/// `dc civac, x0` and friends. Unknown operation names aren't judged.
pub(crate) fn check_sys(mnemonic: &Mnemonic, operands: &[Operand]) -> MatchResult {
    let head = mnemonic.head.as_str();
    let upper = head.to_ascii_uppercase();
    let name = match operands.first() {
        Some(Operand::Ident(name)) => name.to_ascii_lowercase(),
        Some(Operand::Imm(_) | Operand::Register(_)) => {
            return mismatch(Some(0), format!("invalid operand for {upper} instruction"));
        }
        _ => return MatchResult::Unchecked,
    };
    if operands.len() > 2 {
        return mismatch(
            Some(2),
            "unexpected token in argument list: these take an operation and at most one register"
                .into(),
        );
    }
    let register = operands.get(1);
    if let Some(operand) = register {
        let fits = matches!(operand, Operand::Register(RegisterOperand { register, .. })
            if crate::operand_class("GPR64").is_some_and(|c| register.fits(c)));
        if !fits {
            return mismatch(
                Some(1),
                "invalid operand for instruction: the register must be an X register or xzr".into(),
            );
        }
    }
    let Some(known) = sys_operations(head).iter().find(|o| o.name == name) else {
        return MatchResult::Unchecked;
    };
    match (known.register, register.is_some()) {
        (true, false) => mismatch(
            None,
            format!("specified {head} op requires a register: {head} {name}, Xt"),
        ),
        (false, true) => mismatch(
            Some(1),
            format!("specified {head} op does not use a register: write {head} {name}"),
        ),
        _ => MatchResult::Unchecked,
    }
}

/// A constant immediate, or None for a register, a symbol, or an
/// expression clang can't fold while parsing.
fn constant(operand: &Operand) -> Option<i64> {
    match operand {
        Operand::Imm(Imm::Constant(value)) => Some(*value),
        _ => None,
    }
}

/// 64 for a register in clang's GPR64all class (x0 to x30, xzr, sp), else 32.
fn width(operand: &Operand) -> Option<u64> {
    let Operand::Register(r) = operand else {
        return None;
    };
    (r.register.kind() == RegisterKind::Scalar).then(|| {
        let x = crate::operand_class("GPR64all").is_some_and(|c| r.register.fits(c))
            || matches!(r.register.llvm_name(), "SP" | "XZR");
        if x { 64 } else { 32 }
    })
}

fn imm(value: u64) -> Operand {
    Operand::Imm(Imm::Constant(value as i64))
}

/// `lsl Rd, Rn, #shift` with a constant shift: clang rewrites it to
/// `ubfm Rd, Rn, #((width - shift) % width), #(width - 1 - shift)`. None if
/// it isn't that shape, so the match table judges it.
pub(crate) fn check_lsl(
    operands: &[Operand],
    features: Option<&FeatureSet>,
) -> Option<MatchResult> {
    let [rd, rn, shift] = operands else {
        return None;
    };
    let shift = constant(shift)? as u64;
    let bits = width(rn)?;
    // The rewrite's imms is out of range exactly when the shift is; say so
    // here, since the matcher can't judge -1 and 64 for 64-bit registers.
    if shift >= bits {
        return Some(mismatch(
            Some(2),
            format!("immediate must be an integer in range [0, {}]", bits - 1),
        ));
    }
    let immr = bits.wrapping_sub(shift) & (bits - 1);
    let imms = (bits - 1).wrapping_sub(shift);
    let rewritten = [rd.clone(), rn.clone(), imm(immr), imm(imms)];
    Some(remap(
        match_for(&Mnemonic::parse("ubfm"), &rewritten, features),
        |i| i.min(2),
    ))
}

/// The bitfield aliases, as clang checks and rewrites them.
pub(crate) fn check_bitfield(
    mnemonic: &Mnemonic,
    operands: &[Operand],
    features: Option<&FeatureSet>,
) -> MatchResult {
    let head = mnemonic.head.as_str();
    let clear = head == "bfc";
    let count = if clear { 3 } else { 4 };
    let shape = alias_signature(head).unwrap_or(head);
    let unrecognized = || {
        mismatch(
            None,
            format!(
                "unrecognized instruction mnemonic: clang only knows {head} as `{shape}`, with constant #lsb and #width"
            ),
        )
    };
    if operands.len() != count {
        return unrecognized();
    }
    // Where the lsb and width are.
    let (at_lsb, at_width) = if clear { (1, 2) } else { (2, 3) };
    let Some(bits) = width(&operands[0]) else {
        return unrecognized();
    };
    let (Some(lsb), Some(field)) = (constant(&operands[at_lsb]), constant(&operands[at_width]))
    else {
        // Something we can't fold, like `.set n` below this line, which
        // clang can't either: then it has no reading. Expressions we can't
        // evaluate aren't judged.
        let opaque = |o: &Operand| matches!(o, Operand::Imm(Imm::Other));
        if opaque(&operands[at_lsb]) || opaque(&operands[at_width]) {
            return MatchResult::Unchecked;
        }
        return unrecognized();
    };
    let (lsb, field) = (lsb as u64, field as u64);
    if lsb >= bits {
        return mismatch(
            Some(at_lsb),
            format!(
                "expected integer in range [0, {}]: #lsb is a bit number in a {bits}-bit register",
                bits - 1
            ),
        );
    }
    if field < 1 || field > bits {
        return mismatch(
            Some(at_width),
            format!("expected integer in range [1, {bits}]: #width is how many bits"),
        );
    }
    let insert = matches!(head, "bfi" | "sbfiz" | "ubfiz" | "bfc");
    let (immr, imms) = if insert {
        let immr = bits.wrapping_sub(lsb) & (bits - 1);
        let imms = field - 1;
        if immr != 0 && imms >= immr {
            return mismatch(
                Some(at_width),
                format!(
                    "requested insert overflows register: #lsb + #width must be at most {bits}"
                ),
            );
        }
        (immr, imms)
    } else {
        let imms = field + lsb - 1;
        if imms >= bits || imms < lsb {
            return mismatch(
                Some(at_width),
                format!(
                    "requested extract overflows register: #lsb + #width must be at most {bits}"
                ),
            );
        }
        (lsb, imms)
    };
    let target = match head {
        "ubfx" | "ubfiz" => "ubfm",
        "sbfx" | "sbfiz" => "sbfm",
        _ => "bfm",
    };
    let mut rewritten = vec![operands[0].clone()];
    if clear {
        let zero = if bits == 64 { "xzr" } else { "wzr" };
        let Some(register) = Register::lookup_kind(zero, RegisterKind::Scalar) else {
            return MatchResult::Unchecked;
        };
        rewritten.push(Operand::Register(RegisterOperand {
            register,
            suffix: None,
            shift: None,
        }));
    } else {
        rewritten.push(operands[1].clone());
    }
    rewritten.extend([imm(immr), imm(imms)]);
    // `bfc` has one operand fewer than the rewrite: map back.
    remap(
        match_for(&Mnemonic::parse(target), &rewritten, features),
        |i| if clear && i >= 2 { i - 1 } else { i },
    )
}

/// Moves a mismatch's operand index from the rewrite back to what was written.
fn remap(result: MatchResult, map: impl Fn(usize) -> usize) -> MatchResult {
    match result {
        MatchResult::Mismatch(mut m) => {
            m.operand = m.operand.map(map);
            MatchResult::Mismatch(m)
        }
        other => other,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::match_instruction;

    fn reg(name: &str) -> Operand {
        Operand::Register(RegisterOperand {
            register: Register::lookup(name).next().expect("register"),
            suffix: None,
            shift: None,
        })
    }

    fn n(value: i64) -> Operand {
        Operand::Imm(Imm::Constant(value))
    }

    fn sym(name: &str) -> Operand {
        Operand::Imm(Imm::Symbol {
            name: name.into(),
            variant: None,
            specifier: None,
            addend: 0,
        })
    }

    fn id(name: &str) -> Operand {
        Operand::Ident(name.into())
    }

    /// The operand index and start of the message, or "ok".
    fn judge(mnemonic: &str, operands: &[Operand]) -> (Option<usize>, String) {
        match match_instruction(&Mnemonic::parse(mnemonic), operands) {
            MatchResult::Mismatch(m) => (m.operand, m.message),
            MatchResult::Matched(_) | MatchResult::Unchecked => (None, "ok".into()),
            other => (None, format!("{other:?}")),
        }
    }

    fn ok(mnemonic: &str, operands: &[Operand]) {
        assert_eq!(judge(mnemonic, operands).1, "ok", "{mnemonic} {operands:?}");
    }

    fn err(mnemonic: &str, operands: &[Operand], at: Option<usize>, starts: &str) {
        let (operand, message) = judge(mnemonic, operands);
        assert!(message.starts_with(starts), "{mnemonic}: {message}");
        assert_eq!(operand, at, "{mnemonic}: {message}");
    }

    // Each case below was checked against clang 17 (Xcode), LLVM 21's parser.

    #[test]
    fn bitfield_extracts() {
        ok("ubfx", &[reg("x0"), reg("x1"), n(4), n(8)]);
        ok("ubfx", &[reg("w0"), reg("w1"), n(4), n(8)]);
        ok("ubfx", &[reg("xzr"), reg("x1"), n(4), n(8)]);
        ok("ubfx", &[reg("w0"), reg("w1"), n(0), n(32)]);
        ok("ubfx", &[reg("x0"), reg("x1"), n(0), n(64)]);
        ok("bfxil", &[reg("x0"), reg("x1"), n(0), n(64)]);
        err(
            "ubfx",
            &[reg("w0"), reg("w1"), n(32), n(1)],
            Some(2),
            "expected integer in range [0, 31]",
        );
        err(
            "ubfx",
            &[reg("x0"), reg("x1"), n(64), n(1)],
            Some(2),
            "expected integer in range [0, 63]",
        );
        err(
            "sbfx",
            &[reg("x0"), reg("x1"), n(-1), n(8)],
            Some(2),
            "expected integer in range",
        );
        err(
            "ubfx",
            &[reg("w0"), reg("w1"), n(0), n(0)],
            Some(3),
            "expected integer in range [1, 32]",
        );
        err(
            "ubfx",
            &[reg("w0"), reg("w1"), n(0), n(33)],
            Some(3),
            "expected integer in range [1, 32]",
        );
        err(
            "ubfx",
            &[reg("x0"), reg("x1"), n(0), n(65)],
            Some(3),
            "expected integer in range [1, 64]",
        );
        err(
            "ubfx",
            &[reg("w0"), reg("w1"), n(30), n(4)],
            Some(3),
            "requested extract overflows",
        );
        err(
            "ubfx",
            &[reg("x0"), reg("x1"), n(60), n(8)],
            Some(3),
            "requested extract overflows",
        );
        err(
            "bfxil",
            &[reg("w0"), reg("w1"), n(31), n(2)],
            Some(3),
            "requested extract overflows",
        );
        // Mixed widths and sp fail in the rewritten form, at the register.
        err(
            "ubfx",
            &[reg("x0"), reg("w1"), n(4), n(8)],
            Some(1),
            "invalid operand",
        );
        err(
            "ubfx",
            &[reg("w0"), reg("x1"), n(4), n(8)],
            Some(1),
            "invalid operand",
        );
        err(
            "ubfx",
            &[reg("sp"), reg("x1"), n(4), n(8)],
            Some(0),
            "invalid operand",
        );
    }

    #[test]
    fn bitfield_inserts() {
        ok("ubfiz", &[reg("w0"), reg("w1"), n(4), n(28)]);
        ok("ubfiz", &[reg("w0"), reg("w1"), n(0), n(32)]);
        ok("sbfiz", &[reg("w0"), reg("w1"), n(31), n(1)]);
        ok("bfi", &[reg("w0"), reg("w1"), n(31), n(1)]);
        ok("bfi", &[reg("x0"), reg("xzr"), n(1), n(2)]);
        err(
            "ubfiz",
            &[reg("w0"), reg("w1"), n(4), n(29)],
            Some(3),
            "requested insert overflows",
        );
        err(
            "ubfiz",
            &[reg("x0"), reg("x1"), n(63), n(2)],
            Some(3),
            "requested insert overflows",
        );
        err(
            "ubfiz",
            &[reg("x0"), reg("x1"), n(1), n(64)],
            Some(3),
            "requested insert overflows",
        );
        err(
            "sbfiz",
            &[reg("x0"), reg("x1"), n(64), n(1)],
            Some(2),
            "expected integer in range [0, 63]",
        );
        err(
            "bfi",
            &[reg("w0"), reg("w1"), n(31), n(2)],
            Some(3),
            "requested insert overflows",
        );
        err(
            "bfi",
            &[reg("w0"), reg("wsp"), n(1), n(2)],
            Some(1),
            "invalid operand",
        );
    }

    #[test]
    fn bitfield_clear() {
        ok("bfc", &[reg("w0"), n(4), n(8)]);
        ok("bfc", &[reg("x0"), n(0), n(64)]);
        err(
            "bfc",
            &[reg("w0"), n(32), n(1)],
            Some(1),
            "expected integer in range [0, 31]",
        );
        err(
            "bfc",
            &[reg("w0"), n(30), n(4)],
            Some(2),
            "requested insert overflows",
        );
        err(
            "bfc",
            &[reg("x0"), n(63), n(2)],
            Some(2),
            "requested insert overflows",
        );
        err("bfc", &[reg("wsp"), n(1), n(2)], Some(0), "invalid operand");
    }

    #[test]
    fn bitfield_shapes_clang_has_no_reading_for() {
        // Wrong operand counts and immediates that aren't constant yet:
        // clang says "unrecognized instruction mnemonic".
        err(
            "ubfx",
            &[reg("x0"), reg("x1"), n(4)],
            None,
            "unrecognized instruction mnemonic",
        );
        err(
            "ubfx",
            &[reg("x0"), reg("x1"), n(4), n(8), n(1)],
            None,
            "unrecognized",
        );
        err("bfc", &[reg("x0"), reg("x1"), n(1)], None, "unrecognized");
        err("bfc", &[reg("x0"), n(1)], None, "unrecognized");
        err(
            "ubfx",
            &[reg("x0"), reg("x1"), sym("later"), n(8)],
            None,
            "unrecognized",
        );
        // Expressions we can't fold aren't judged.
        let other = Operand::Imm(Imm::Other);
        ok("ubfx", &[reg("x0"), reg("x1"), other, n(8)]);
    }

    #[test]
    fn lsl_by_an_immediate() {
        ok("lsl", &[reg("w0"), reg("w1"), n(31)]);
        ok("lsl", &[reg("w0"), reg("w1"), n(0)]);
        ok("lsl", &[reg("x0"), reg("x1"), n(63)]);
        ok("lsl", &[reg("w0"), reg("w1"), reg("w2")]);
        err(
            "lsl",
            &[reg("w0"), reg("w1"), n(32)],
            Some(2),
            "immediate must be an integer in range [0, 31]",
        );
        err(
            "lsl",
            &[reg("x0"), reg("x1"), n(64)],
            Some(2),
            "immediate must be an integer in range [0, 63]",
        );
        err(
            "lsl",
            &[reg("x0"), reg("x1"), n(-1)],
            Some(2),
            "immediate must be an integer in range [0, 63]",
        );
        err(
            "lsl",
            &[reg("x0"), reg("w1"), n(3)],
            Some(1),
            "invalid operand",
        );
        // A symbol isn't constant: the table's lsl (by a register) rejects it.
        err(
            "lsl",
            &[reg("x0"), reg("x1"), sym("lbl")],
            Some(2),
            "invalid operand",
        );
    }

    #[test]
    fn sys_aliases() {
        ok("dc", &[id("civac"), reg("x0")]);
        ok("dc", &[id("zva"), reg("x0")]);
        ok("dc", &[id("civac"), reg("xzr")]);
        ok("ic", &[id("ivau"), reg("x0")]);
        ok("ic", &[id("iallu")]);
        ok("at", &[id("s1e1r"), reg("x0")]);
        ok("tlbi", &[id("vmalle1")]);
        ok("tlbi", &[id("vae1is"), reg("x0")]);
        ok("cfp", &[id("rctx"), reg("x0")]);
        err(
            "dc",
            &[id("civac")],
            None,
            "specified dc op requires a register",
        );
        err(
            "ic",
            &[id("iallu"), reg("x0")],
            Some(1),
            "specified ic op does not use a register",
        );
        err(
            "ic",
            &[id("ivau")],
            None,
            "specified ic op requires a register",
        );
        err(
            "at",
            &[id("s1e1r")],
            None,
            "specified at op requires a register",
        );
        err(
            "tlbi",
            &[id("vmalle1"), reg("x0")],
            Some(1),
            "specified tlbi op does not use a register",
        );
        err(
            "tlbi",
            &[id("vae1")],
            None,
            "specified tlbi op requires a register",
        );
        err(
            "dc",
            &[id("civac"), reg("w0")],
            Some(1),
            "invalid operand for instruction",
        );
        err(
            "dc",
            &[id("civac"), reg("sp")],
            Some(1),
            "invalid operand for instruction",
        );
        err(
            "dc",
            &[id("civac"), reg("x0"), reg("x1")],
            Some(2),
            "unexpected token",
        );
        err(
            "dc",
            &[n(5), reg("x0")],
            Some(0),
            "invalid operand for DC instruction",
        );
        // Names we don't know (MTE, range TLBI, typos) aren't judged.
        ok("dc", &[id("gva"), reg("x0")]);
        ok("tlbi", &[id("frob")]);
        assert!(sys_operations("dc").iter().all(|o| o.register));
        assert_eq!(alias_signature("ubfx"), Some("ubfx Rd, Rn, #lsb, #width"));
        assert_eq!(alias_signature("cfp"), Some("cfp rctx, Xt"));
        assert_eq!(alias_signature("add"), None);
    }
}
