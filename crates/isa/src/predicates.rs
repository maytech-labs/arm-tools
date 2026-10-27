//! LLVM's operand class predicates (`isImmInRange<0, 63>` and friends, the
//! `is*` methods of `AArch64Operand` in AArch64AsmParser.cpp), ported to Rust.
//!
//! LLVM's predicates return either a `bool` or a `DiagnosticPredicate`. A
//! `DiagnosticPredicate` says `NearMatch` itself; this port returns
//! [`Check::NearMiss`] in exactly those places. LLVM turns a `false` from a
//! `bool` predicate into a near match too, and reports the class's
//! diagnostic if it has one. Here a `bool` predicate returns
//! [`Check::NearMiss`] when the operand is the right kind but the wrong
//! value, and [`Check::NoMatch`] when it's the wrong kind. The matcher
//! should treat `NearMiss` as `NoMatch` for a class without a diagnostic.
//!
//! Anything this port can't answer with confidence is [`Check::Unknown`].

mod encoding;
mod names;
mod registers;

pub use names::{BARRIER_OPTIONS, SVE_PATTERNS, prefetch_operations};

use crate::operand::{Check, Imm, Operand, RegisterOperand};
use crate::{Register, RegisterKind};

/// Checks an operand against a class predicate, as LLVM's string names it:
/// `isImmInRange<0,63>`, `isSImmScaled<7, 8>`.
pub fn check(predicate: &str, operand: &Operand) -> Check {
    let Some(pred) = Pred::parse(predicate) else {
        return Check::Unknown;
    };
    dispatch(&pred, operand).unwrap_or(Check::Unknown)
}

/// A predicate name and its template arguments.
struct Pred<'a> {
    name: &'a str,
    args: Vec<String>,
}

impl<'a> Pred<'a> {
    fn parse(text: &'a str) -> Option<Pred<'a>> {
        let text = text.trim();
        let Some(open) = text.find('<') else {
            return Some(Pred {
                name: text,
                args: Vec::new(),
            });
        };
        let inner = text[open + 1..].strip_suffix('>')?;
        let args = strip_comments(inner)
            .split(',')
            .map(|a| a.trim().to_string())
            .collect();
        Some(Pred {
            name: text[..open].trim(),
            args,
        })
    }

    fn arg(&self, i: usize) -> Option<&str> {
        self.args.get(i).map(String::as_str)
    }

    fn int(&self, i: usize) -> Option<i64> {
        self.arg(i)?.parse().ok()
    }

    fn bool(&self, i: usize) -> Option<bool> {
        match self.arg(i)? {
            "true" => Some(true),
            "false" => Some(false),
            _ => None,
        }
    }

    /// `AArch64::GPR64spRegClassID` as `GPR64sp`.
    fn class(&self, i: usize) -> Option<&str> {
        let arg = self.arg(i)?;
        let arg = arg.strip_prefix("AArch64::").unwrap_or(arg);
        arg.strip_suffix("RegClassID")
    }

    /// The part after the last `::`: `RegKind::NeonVector` as `NeonVector`.
    fn tail(&self, i: usize) -> Option<&str> {
        let arg = self.arg(i)?;
        Some(arg.rsplit("::").next().unwrap_or(arg))
    }

    /// `int8_t` and friends as a bit width.
    fn int_type(&self, i: usize) -> Option<u32> {
        match self.arg(i)? {
            "int8_t" => Some(8),
            "int16_t" => Some(16),
            "int32_t" => Some(32),
            "int64_t" => Some(64),
            _ => None,
        }
    }

    fn reg_kind(&self, i: usize) -> Option<RegisterKind> {
        match self.tail(i)? {
            "NeonVector" => Some(RegisterKind::NeonVector),
            "SVEDataVector" => Some(RegisterKind::SveData),
            "SVEPredicateVector" => Some(RegisterKind::SvePredicate),
            _ => None,
        }
    }
}

/// Drops `/* ... */` comments, like `/*IsRange=*/true`.
fn strip_comments(text: &str) -> String {
    let mut out = String::new();
    let mut rest = text;
    while let Some(start) = rest.find("/*") {
        out.push_str(&rest[..start]);
        match rest[start..].find("*/") {
            Some(end) => rest = &rest[start + end + 2..],
            None => return out,
        }
    }
    out.push_str(rest);
    out
}

fn dispatch(p: &Pred, op: &Operand) -> Option<Check> {
    use Check::*;
    let check = match p.name {
        // Integer immediates.
        "isImm" => match value(op) {
            Value::Imm(_) => Match,
            _ => NoMatch,
        },
        "isUImm6" => const_bool(op, |v| (0..64).contains(&v)),
        "isImmInRange" => imm_in_range(op, p.int(0)?, p.int(1)?),
        "isSImm" => {
            let bits = bits_arg(p.int(0)?)?;
            const_bool(op, |v| imm_scaled_ok(v, bits, 1, true))
        }
        "isSImmScaled" => imm_scaled(op, bits_arg(p.int(0)?)?, scale_arg(p.int(1)?)?, true),
        // `isUImmScaled<Bits, Scale, Offset, IsRange>`: a range (`0:3`) fits
        // only a range class, and its last value must be its first plus
        // Offset; then the first is checked like an immediate.
        "isUImmScaled" => {
            // Range classes use 0 bits for a single offset, 0.
            let bits = u32::try_from(p.int(0)?).ok().filter(|b| *b <= 32)?;
            let scale = scale_arg(p.int(1)?)?;
            let is_range = p.args.len() > 3 && p.bool(3)?;
            let offset = if p.args.len() > 2 { p.int(2)? } else { 0 };
            match op {
                Operand::ImmRange { first, last } => {
                    if !is_range || *last != first + offset {
                        NoMatch
                    } else {
                        from_bool(true, imm_scaled_ok(*first, bits, scale, false))
                    }
                }
                _ if is_range => NoMatch,
                _ => return Some(imm_scaled(op, bits, scale, false)),
            }
        }
        "isSImm9OffsetFB" => {
            let scale = scale_arg(p.int(0)? / 8)?;
            const_bool(op, |v| {
                imm_scaled_ok(v, 9, 1, true) && !uimm12_offset_const(v, scale)
            })
        }
        "isUImm12Offset" => {
            let scale = scale_arg(p.int(0)?)?;
            imm_bool(op, |e| match e {
                Expr::Const(v) => uimm12_offset_const(*v, scale),
                _ => symbolic_uimm12_offset(e),
            })
        }
        "isLogicalImm" => {
            let bits = p.int_type(0)?;
            const_bool(op, |v| logical_imm(v, bits))
        }
        "isAddSubImm" => add_sub_imm(op),
        "isAddSubImmNeg" => match value(op) {
            Value::None => NoMatch,
            v => from_bool(
                true,
                shifted_val(&v, 12)
                    .is_some_and(|(n, _)| n < 0 && n.checked_neg().is_some_and(|m| m <= 0xfff)),
            ),
        },
        "isSVECpyImm" => sve_cpy_imm(op, p.int_type(0)?),
        "isSVEAddSubImm" => sve_add_sub_imm(op, p.int_type(0)?),
        "isSVEPreferredLogicalImm" => {
            let bits = p.int_type(0)?;
            let logical = const_bool(op, |v| logical_imm(v, bits)) == Match;
            if logical && sve_cpy_imm(op, bits) != Match {
                Match
            } else {
                NoMatch
            }
        }
        "isSIMDImmType10" => const_bool(op, |v| encoding::is_adv_simd_mod_imm_type10(v as u64)),
        "isMOVZMovAlias" => {
            let (width, shift) = mov_args(p)?;
            imm_bool(op, |e| match e {
                Expr::Const(v) => encoding::is_movz_mov_alias(*v as u64, shift, width),
                // Only `lsl #0` takes an expression.
                _ => shift == 0,
            })
        }
        "isMOVNMovAlias" => {
            let (width, shift) = mov_args(p)?;
            const_bool(op, |v| encoding::is_movn_mov_alias(v as u64, shift, width))
        }
        "isMovWSymbolG0" => mov_w_symbol(op, MOVW_G0),
        "isMovWSymbolG1" => mov_w_symbol(op, MOVW_G1),
        "isMovWSymbolG2" => mov_w_symbol(op, MOVW_G2),
        "isMovWSymbolG3" => mov_w_symbol(op, MOVW_G3),
        "isBranchTarget" => {
            let n = p.int(0)?;
            if !(1..=32).contains(&n) {
                return None;
            }
            let lo = -((1i64 << (n - 1)) << 2);
            let hi = ((1i64 << (n - 1)) - 1) << 2;
            imm_bool(op, |e| match e {
                Expr::Const(v) => v & 3 == 0 && (lo..=hi).contains(v),
                _ => true,
            })
        }
        "isPAuthPCRelLabel16Operand" => imm_bool(op, |e| match e {
            Expr::Const(v) => v & 3 == 0 && *v <= 0 && *v > -(1 << 18),
            _ => true,
        }),
        "isAdrpLabel" => adrp_label(op),
        "isAdrLabel" => adr_label(op),
        "isComplexRotation" => {
            let angle = p.int(0)?;
            let remainder = p.int(1)?;
            if angle <= 0 || remainder < 0 {
                return None;
            }
            match value(op) {
                Value::Imm(Expr::Const(v)) => {
                    let v = v as u64;
                    from_bool(true, v % angle as u64 == remainder as u64 && v <= 270)
                }
                _ => NoMatch,
            }
        }
        "isSVEPattern" => named_or_const(op, names::is_sve_pattern, 0..=31),
        "isSVEVecLenSpecifier" => named_or_const(op, names::is_sve_vec_len_specifier, 0..=1),

        // Floating-point immediates.
        "isFPImm" => fp_imm(op)?,
        "isExactFPImm" => exact_fp_imm(op, exact_fp(p.tail(0)?)?, exact_fp(p.tail(1)?)?),

        // Condition codes and named operands.
        "isCondCode" => match op {
            Operand::CondCode(name) | Operand::Ident(name) => {
                from_bool(true, names::is_cond_code(name))
            }
            _ => NoMatch,
        },
        "isBarrier" => match (keyword(op), value(op)) {
            (Some(name), _) => yes(names::is_barrier(name) || names::is_tsb(name)),
            (None, Value::Imm(Expr::Const(v))) => yes((0..=15).contains(&v)),
            _ => NoMatch,
        },
        "isBarriernXS" => match (keyword(op), value(op)) {
            (Some(name), _) => yes(names::is_barrier_nxs(name)),
            (None, Value::Imm(Expr::Const(v))) => yes(matches!(v, 16 | 20 | 24 | 28)),
            _ => NoMatch,
        },
        "isPrefetch" => match (keyword(op), value(op)) {
            (Some(name), _) => yes(names::is_prefetch(name)),
            (None, Value::Imm(Expr::Const(v))) => match v {
                0..=31 => Match,
                // Only the range prefetch (`rprfm`) takes 32 to 63.
                32..=63 => return None,
                _ => NoMatch,
            },
            _ => NoMatch,
        },
        "isPSBHint" => yes(keyword(op).is_some_and(names::is_psb_hint)),
        "isBTIHint" => yes(keyword(op).is_some_and(names::is_bti_hint)),
        "isPHint" => yes(keyword(op).is_some_and(names::is_phint)),
        "isSysCR" => yes(keyword(op).is_some_and(names::is_sys_cr)),
        "isSVCR" => match keyword(op) {
            Some(name) => from_bool(true, names::is_svcr(name)),
            None => NoMatch,
        },
        "isSystemPStateFieldWithImm0_15" => yes(keyword(op).is_some_and(names::is_pstate_imm0_15)),
        "isSystemPStateFieldWithImm0_1" => yes(keyword(op).is_some_and(names::is_pstate_imm0_1)),
        // `tryParseSysReg`: a name from LLVM's SysReg table that the target
        // has, readable for mrs and writable for msr, or a generic
        // `s3_0_c15_c2_0`. A name gated by a CPU feature isn't judged: the
        // feature set isn't known here.
        "isMRSSystemRegister" | "isMSRSystemRegister" => {
            let Some(name) = keyword(op) else {
                if matches!(op, Operand::Register(_)) {
                    return None;
                }
                return Some(NoMatch);
            };
            if names::is_svcr(name) {
                return Some(NoMatch);
            }
            let read = p.name == "isMRSSystemRegister";
            match names::system_register(name) {
                // Gated: with the feature, the direction decides; without
                // it, clang doesn't know the name. Either way, a read-only
                // register can't be written, nor a write-only one read.
                Some(&(_, readable, writable, true)) => {
                    if (read && readable) || (!read && writable) {
                        return None;
                    }
                    NearMiss
                }
                Some(&(_, readable, writable, false)) => {
                    from_bool(true, if read { readable } else { writable })
                }
                None => from_bool(true, names::is_generic_system_register(name)),
            }
        }

        // Shifts and extends.
        "isShifter" => shifter(op, |_| true),
        "isArithmeticShifter" => {
            let width = p.int(0)?;
            shifter(op, |s| {
                matches!(s.ty, Some(Sh::Lsl | Sh::Lsr | Sh::Asr)) && i64::from(s.amount) < width
            })
        }
        "isLogicalShifter" => {
            let width = p.int(0)?;
            shifter(op, |s| {
                matches!(s.ty, Some(Sh::Lsl | Sh::Lsr | Sh::Asr | Sh::Ror))
                    && i64::from(s.amount) < width
            })
        }
        "isMovImm32Shifter" => shifter(op, |s| s.ty == Some(Sh::Lsl) && matches!(s.amount, 0 | 16)),
        "isMovImm64Shifter" => shifter(op, |s| {
            s.ty == Some(Sh::Lsl) && matches!(s.amount, 0 | 16 | 32 | 48)
        }),
        "isLogicalVecShifter" => shifter(op, |s| {
            s.ty == Some(Sh::Lsl) && matches!(s.amount, 0 | 8 | 16 | 24)
        }),
        "isLogicalVecHalfWordShifter" => {
            shifter(op, |s| s.ty == Some(Sh::Lsl) && matches!(s.amount, 0 | 8))
        }
        "isMoveVecShifter" => {
            shift_extend(op, |s| s.ty == Some(Sh::Msl) && matches!(s.amount, 8 | 16))
        }
        "isLSLImm3Shift" => shift_extend(op, |s| s.ty == Some(Sh::Lsl) && s.amount <= 7),
        "isExtend" => shift_extend(op, is_extend),
        "isExtend64" => shift_extend(op, |s| {
            is_extend(s)
                && matches!(
                    s.ty,
                    Some(Sh::Uxtb | Sh::Sxtb | Sh::Uxth | Sh::Sxth | Sh::Uxtw | Sh::Sxtw)
                )
        }),
        "isExtendLSL64" => shift_extend(op, |s| {
            is_extend(s) && matches!(s.ty, Some(Sh::Uxtx | Sh::Sxtx | Sh::Lsl)) && s.amount <= 4
        }),
        "isMemXExtend" => {
            let amount = log2_bytes(p.int(0)?)?;
            shift_extend(op, |s| {
                is_extend(s)
                    && matches!(s.ty, Some(Sh::Lsl | Sh::Sxtx))
                    && (s.amount == amount || s.amount == 0)
            })
        }
        "isMemWExtend" => {
            let amount = log2_bytes(p.int(0)?)?;
            shift_extend(op, |s| {
                is_extend(s)
                    && matches!(s.ty, Some(Sh::Uxtw | Sh::Sxtw))
                    && (s.amount == amount || s.amount == 0)
            })
        }
        "isGPR64WithShiftExtend" => {
            let class = p.class(0)?;
            let amount = log2_bytes(p.int(1)?)?;
            let Some(reg) = register(op, RegisterKind::Scalar) else {
                return Some(NoMatch);
            };
            let shift = register_shift(reg);
            from_bool(
                true,
                registers::contains(class, reg.register)?
                    && shift.ty == Some(Sh::Lsl)
                    && shift.amount == amount,
            )
        }
        "isSVEDataVectorRegWithShiftExtend" => sve_data_with_shift(p, op)?,

        // Scalar registers.
        "isGPR64" | "isFPRasZPR" => scalar_in(op, p.class(0)?)?,
        "isGPR32as64" => scalar_in(op, "GPR64")?,
        "isGPR64as32" => scalar_in(op, "GPR32")?,
        "isGPR64x8" => scalar_in(op, "GPR64x8Class")?,
        "isWSeqPair" => scalar_in(op, "WSeqPairsClass")?,
        "isXSeqPair" => scalar_in(op, "XSeqPairsClass")?,
        "isSyspXzrPair" => match register(op, RegisterKind::Scalar) {
            Some(reg) => from_bool(true, reg.register.llvm_name() == "XZR"),
            None => NoMatch,
        },

        // Vector registers.
        "isNeonVectorReg" => yes(register(op, RegisterKind::NeonVector).is_some()),
        "isNeonVectorRegLo" => match register(op, RegisterKind::NeonVector) {
            Some(reg) => from_bool(
                true,
                registers::contains("FPR128_lo", reg.register)?
                    || registers::contains("FPR64_lo", reg.register)?,
            ),
            None => NoMatch,
        },
        "isNeonVectorReg0to7" => match register(op, RegisterKind::NeonVector) {
            Some(reg) => from_bool(true, registers::contains("FPR128_0to7", reg.register)?),
            None => NoMatch,
        },
        "isSVEDataVectorRegOfWidth" => {
            sve_reg_of_width(op, &[RegisterKind::SveData], p.int(0)?, p.class(1)?)?
        }
        "isSVEPredicateVectorRegOfWidth" => {
            sve_reg_of_width(op, &[RegisterKind::SvePredicate], p.int(0)?, p.class(1)?)?
        }
        "isSVEPredicateAsCounterRegOfWidth" => sve_reg_of_width(
            op,
            &[RegisterKind::SvePredicateAsCounter],
            p.int(0)?,
            p.class(1)?,
        )?,
        "isSVEPredicateOrPredicateAsCounterRegOfWidth" => sve_reg_of_width(
            op,
            &[
                RegisterKind::SvePredicate,
                RegisterKind::SvePredicateAsCounter,
            ],
            p.int(0)?,
            p.class(1)?,
        )?,
        "isMatrixRegOperand" => matrix_reg(op, p.tail(0)?, p.int(1)?, p.class(2)?)?,
        "isMatrixTileList" => match op {
            Operand::VectorList { first, .. } => match first.kind() {
                RegisterKind::MatrixTileList => Match,
                RegisterKind::Matrix => return None,
                _ => NoMatch,
            },
            Operand::Register(reg)
                if matches!(
                    reg.register.kind(),
                    RegisterKind::Matrix | RegisterKind::MatrixTileList
                ) =>
            {
                return None;
            }
            Operand::Token(_) => return None,
            _ => NoMatch,
        },

        // Vector lists and indexes.
        "isImplicitlyTypedVectorList" => {
            let kind = p.reg_kind(0)?;
            let count = p.int(1)?;
            let consecutive = if p.args.len() > 2 { p.bool(2)? } else { false };
            match list(op) {
                List::Not => NoMatch,
                List::Bad => return None,
                List::Is(l) if l.kind != kind => NoMatch,
                List::Is(l) => from_bool(
                    true,
                    i64::from(l.count) == count
                        && l.elements == 0
                        && (!consecutive || l.stride == 1),
                ),
            }
        }
        "isTypedVectorList" => {
            let want = typed_args(p, 1)?;
            match list(op) {
                List::Not => NoMatch,
                List::Bad => return None,
                List::Is(l) if l.kind != want.kind => NoMatch,
                List::Is(l) => from_bool(true, l.is_typed(&want)),
            }
        }
        "isTypedVectorListMultiple" => {
            let want = typed_args(p, 1)?;
            let class = p.class(4)?;
            match list(op) {
                List::Bad => return None,
                List::Is(l) if l.is_typed(&want) => {
                    from_bool(true, registers::contains(class, l.first)?)
                }
                _ => NoMatch,
            }
        }
        "isTypedVectorListStrided" => {
            let kind = p.reg_kind(0)?;
            let count = p.int(1)?;
            let stride = p.int(2)?;
            let width = p.int(3)?;
            let want = Typed {
                kind,
                count,
                elements: 0,
                width,
                stride,
            };
            match list(op) {
                List::Bad => return None,
                List::Is(l) if l.is_typed(&want) => {
                    let z = i64::from(registers::z_number(l.first)?);
                    yes(z < stride || (16..16 + stride).contains(&z))
                }
                _ => NoMatch,
            }
        }
        "isVectorIndex" => {
            let (min, max) = (p.int(0)?, p.int(1)?);
            match op {
                // LLVM keeps the index in an `int`.
                Operand::VectorIndex(v) => {
                    from_bool(true, (min..=max).contains(&i64::from(*v as i32)))
                }
                _ => NoMatch,
            }
        }
        _ => return None,
    };
    Some(check)
}

/// LLVM's `DiagnosticPredicate(bool)`, split by operand kind.
fn from_bool(right_kind: bool, ok: bool) -> Check {
    match (right_kind, ok) {
        (false, _) => Check::NoMatch,
        (true, true) => Check::Match,
        (true, false) => Check::NearMiss,
    }
}

/// For predicates that only ever match or don't: kind checks and names.
fn yes(ok: bool) -> Check {
    if ok { Check::Match } else { Check::NoMatch }
}

fn bits_arg(bits: i64) -> Option<u32> {
    u32::try_from(bits).ok().filter(|b| (1..=32).contains(b))
}

fn scale_arg(scale: i64) -> Option<i64> {
    Some(scale).filter(|s| (1..=1 << 20).contains(s))
}

/// `Log2_32(Width / 8)`.
fn log2_bytes(width: i64) -> Option<u32> {
    let bytes = u64::try_from(width / 8)
        .ok()
        .filter(|b| b.is_power_of_two())?;
    Some(bytes.trailing_zeros())
}

// Immediates.

/// An immediate as LLVM's predicates see its `MCExpr`.
enum Expr<'a> {
    Const(i64),
    Symbol {
        variant: Option<&'a str>,
        specifier: Option<&'a str>,
        addend: i64,
    },
    Other,
}

/// What an operand is, to an immediate predicate.
enum Value<'a> {
    /// LLVM's `k_Immediate`.
    Imm(Expr<'a>),
    /// LLVM's `k_ShiftedImm`, with a shift other than 0.
    Shifted(Expr<'a>, u32),
    None,
}

fn expr(imm: &Imm) -> Expr<'_> {
    match imm {
        Imm::Constant(v) => Expr::Const(*v),
        Imm::Symbol {
            variant,
            specifier,
            addend,
            ..
        } => Expr::Symbol {
            variant: variant.as_deref(),
            specifier: specifier.as_deref(),
            addend: *addend,
        },
        Imm::Other => Expr::Other,
    }
}

fn value(op: &Operand) -> Value<'_> {
    match op {
        Operand::Imm(imm) => Value::Imm(expr(imm)),
        // A bare name where LLVM parses an expression is a symbol reference.
        Operand::Ident(_) | Operand::CondCode(_) => Value::Imm(Expr::Symbol {
            variant: None,
            specifier: None,
            addend: 0,
        }),
        // `tryParseImmWithOptionalShift` turns `#1, lsl #0` into a plain
        // immediate.
        Operand::ShiftedImm { value, shift: 0 } => Value::Imm(expr(value)),
        Operand::ShiftedImm { value, shift } => Value::Shifted(expr(value), *shift),
        _ => Value::None,
    }
}

/// A `bool` predicate on any immediate.
fn imm_bool(op: &Operand, ok: impl Fn(&Expr) -> bool) -> Check {
    match value(op) {
        Value::Imm(e) => from_bool(true, ok(&e)),
        _ => Check::NoMatch,
    }
}

/// A `bool` predicate that needs a constant.
fn const_bool(op: &Operand, ok: impl Fn(i64) -> bool) -> Check {
    imm_bool(op, |e| match e {
        Expr::Const(v) => ok(*v),
        _ => false,
    })
}

fn imm_in_range(op: &Operand, min: i64, max: i64) -> Check {
    // `UImm6P1` and `UImm6M1` share `isImmInRange<0,63>` but parse with
    // `tryParseAdjImm0_63`, which adds or subtracts 1 first. The predicate
    // can't tell them apart, so the two values only they accept are
    // unknown.
    if (min, max) == (0, 63) && matches!(value(op), Value::Imm(Expr::Const(-1 | 64))) {
        return Check::Unknown;
    }
    const_bool(op, |v| (min..=max).contains(&v))
}

/// The range test of `isImmScaled`.
fn imm_scaled_ok(v: i64, bits: u32, scale: i64, signed: bool) -> bool {
    let (min, max) = if signed {
        let half = 1i64 << (bits - 1);
        (half * -scale, (half - 1) * scale)
    } else {
        (0, ((1i64 << bits) - 1) * scale)
    };
    (min..=max).contains(&v) && v % scale == 0
}

/// `isImmScaled`, a `DiagnosticPredicate`: non-constants don't match.
fn imm_scaled(op: &Operand, bits: u32, scale: i64, signed: bool) -> Check {
    match value(op) {
        Value::Imm(Expr::Const(v)) => from_bool(true, imm_scaled_ok(v, bits, scale, signed)),
        _ => Check::NoMatch,
    }
}

fn uimm12_offset_const(v: i64, scale: i64) -> bool {
    v % scale == 0 && v >= 0 && v / scale < 0x1000
}

/// A symbol reference, as `classifySymbolRef` splits it.
struct SymbolRef<'a> {
    darwin: Option<&'a str>,
    elf: Option<&'a str>,
    addend: i64,
}

/// `classifySymbolRef`: None where LLVM returns false.
fn classify<'a>(e: &Expr<'a>) -> Option<SymbolRef<'a>> {
    match e {
        // Shouldn't use both Darwin and ELF syntax.
        Expr::Symbol {
            variant: Some(_),
            specifier: Some(_),
            ..
        } => None,
        Expr::Symbol {
            variant,
            specifier,
            addend,
        } => Some(SymbolRef {
            darwin: *variant,
            elf: *specifier,
            addend: *addend,
        }),
        Expr::Const(_) | Expr::Other => None,
    }
}

fn is(spec: Option<&str>, name: &str) -> bool {
    spec.is_some_and(|s| s.eq_ignore_ascii_case(name))
}

fn is_any(spec: Option<&str>, names: &[&str]) -> bool {
    names.iter().any(|n| is(spec, n))
}

// ELF relocation specifiers, as written: `:lo12:sym`.
const UIMM12_ELF: &[&str] = &[
    "lo12",
    "got_lo12",
    "got_auth_lo12",
    "dtprel_lo12",
    "dtprel_lo12_nc",
    "tprel_lo12",
    "tprel_lo12_nc",
    "gottprel_lo12",
    "tlsdesc_lo12",
    "tlsdesc_auth_lo12",
    "secrel_lo12",
    "secrel_hi12",
    "gotpage_lo15",
];
const ADD_SUB_ELF: &[&str] = &[
    "lo12",
    "got_auth_lo12",
    "dtprel_hi12",
    "dtprel_lo12",
    "dtprel_lo12_nc",
    "tprel_hi12",
    "tprel_lo12",
    "tprel_lo12_nc",
    "tlsdesc_lo12",
    "tlsdesc_auth_lo12",
    "secrel_hi12",
    "secrel_lo12",
];
const MOVW_G3: &[&str] = &["abs_g3", "prel_g3"];
const MOVW_G2: &[&str] = &[
    "abs_g2",
    "abs_g2_s",
    "abs_g2_nc",
    "prel_g2",
    "prel_g2_nc",
    "tprel_g2",
    "dtprel_g2",
];
const MOVW_G1: &[&str] = &[
    "abs_g1",
    "abs_g1_s",
    "abs_g1_nc",
    "prel_g1",
    "prel_g1_nc",
    "gottprel_g1",
    "tprel_g1",
    "tprel_g1_nc",
    "dtprel_g1",
    "dtprel_g1_nc",
];
const MOVW_G0: &[&str] = &[
    "abs_g0",
    "abs_g0_s",
    "abs_g0_nc",
    "prel_g0",
    "prel_g0_nc",
    "gottprel_g0_nc",
    "tprel_g0",
    "tprel_g0_nc",
    "dtprel_g0",
    "dtprel_g0_nc",
];
const ADRP_ELF: &[&str] = &[
    "pg_hi21_nc",
    "got",
    "got_auth",
    "gotpage_lo15",
    "gottprel",
    "tlsdesc",
    "tlsdesc_auth",
];

/// `isSymbolicUImm12Offset`.
fn symbolic_uimm12_offset(e: &Expr) -> bool {
    // If LLVM doesn't understand the expression, it hopes for the best.
    let Some(s) = classify(e) else {
        return true;
    };
    if is(s.darwin, "pageoff") || is_any(s.elf, UIMM12_ELF) {
        return true;
    }
    if is(s.darwin, "gotpageoff") || is(s.darwin, "tlvppageoff") {
        return s.addend == 0;
    }
    false
}

/// `getShiftedVal<Width>`.
fn shifted_val(v: &Value, width: u32) -> Option<(i64, u32)> {
    match v {
        Value::Shifted(Expr::Const(n), shift) if *shift == width => Some((*n, width)),
        Value::Imm(Expr::Const(n)) => {
            let n = *n;
            if n != 0 && ((n >> width) as u64) << width == n as u64 {
                Some((n >> width, width))
            } else {
                Some((n, 0))
            }
        }
        _ => None,
    }
}

/// `isAddSubImm`.
fn add_sub_imm(op: &Operand) -> Check {
    let v = value(op);
    let e = match &v {
        Value::Imm(e) => e,
        Value::Shifted(e, shift) => {
            if *shift != 12 {
                return Check::NearMiss;
            }
            e
        }
        Value::None => return Check::NoMatch,
    };
    if let Some(s) = classify(e) {
        return from_bool(
            true,
            is(s.darwin, "pageoff")
                || is(s.darwin, "tlvppageoff")
                || (is(s.darwin, "gotpageoff") && s.addend == 0)
                || is_any(s.elf, ADD_SUB_ELF),
        );
    }
    match shifted_val(&v, 12) {
        Some((n, _)) => from_bool(true, (0..=0xfff).contains(&n)),
        // An expression: LLVM hopes for the best.
        None => Check::Match,
    }
}

/// `isSVECpyImm<T>`.
fn sve_cpy_imm(op: &Operand, bits: u32) -> Check {
    let v = value(op);
    if !matches!(v, Value::Shifted(..) | Value::Imm(Expr::Const(_))) {
        return Check::NoMatch;
    }
    if let Some((n, shift)) = shifted_val(&v, 8)
        && !(bits == 8 && shift != 0)
        && encoding::is_sve_cpy_imm((n as u64).wrapping_shl(shift) as i64, bits)
    {
        return Check::Match;
    }
    Check::NearMiss
}

/// `isSVEAddSubImm<T>`.
fn sve_add_sub_imm(op: &Operand, bits: u32) -> Check {
    let v = value(op);
    if !matches!(v, Value::Shifted(..) | Value::Imm(Expr::Const(_))) {
        return Check::NoMatch;
    }
    if let Some((n, shift)) = shifted_val(&v, 8)
        && !(bits == 8 && shift != 0)
        && encoding::is_sve_add_sub_imm(n.wrapping_shl(shift), bits)
    {
        return Check::Match;
    }
    Check::NearMiss
}

/// The value test of `isLogicalImm<T>`. The bits above `T` must be all zeros
/// or all ones, so `#~imm` works.
fn logical_imm(v: i64, bits: u32) -> bool {
    let upper = if bits >= 64 { 0 } else { u64::MAX << bits };
    let v = v as u64;
    if v & upper != 0 && v & upper != upper {
        return false;
    }
    encoding::is_logical_immediate(v & !upper, bits)
}

fn mov_args(p: &Pred) -> Option<(u32, u32)> {
    let width = u32::try_from(p.int(0)?).ok()?;
    let shift = u32::try_from(p.int(1)?).ok().filter(|s| *s < 64)?;
    Some((width, shift))
}

/// `isMovWSymbol`.
fn mov_w_symbol(op: &Operand, allowed: &[&str]) -> Check {
    imm_bool(op, |e| {
        classify(e).is_some_and(|s| s.darwin.is_none() && is_any(s.elf, allowed))
    })
}

/// `isAdrpLabel`, with the checks `tryParseAdrpLabel` makes on symbols.
fn adrp_label(op: &Operand) -> Check {
    let min = -(4096 * (1i64 << 20));
    let max = 4096 * ((1i64 << 20) - 1);
    imm_bool(op, |e| match e {
        Expr::Const(v) => v % 4096 == 0 && (min..=max).contains(v),
        _ => match classify(e) {
            Some(s) if s.darwin.is_none() && s.elf.is_none() => true,
            Some(s) if is_any(s.darwin, &["gotpage", "tlvppage"]) && s.addend != 0 => false,
            Some(s) => {
                is_any(s.darwin, &["page", "gotpage", "tlvppage"]) || is_any(s.elf, ADRP_ELF)
            }
            None => true,
        },
    })
}

/// `isAdrLabel`, with the checks `tryParseAdrLabel` makes on symbols.
fn adr_label(op: &Operand) -> Check {
    let min = -(1i64 << 20);
    let max = (1i64 << 20) - 1;
    imm_bool(op, |e| match e {
        Expr::Const(v) => (min..=max).contains(v),
        _ => match classify(e) {
            Some(s) if s.darwin.is_none() && s.elf.is_none() => true,
            Some(s) => is(s.elf, "got_auth"),
            None => true,
        },
    })
}

/// A bare name: an identifier, or a plain symbol with nothing attached.
fn keyword(op: &Operand) -> Option<&str> {
    match op {
        Operand::Ident(name) => Some(name),
        Operand::Imm(Imm::Symbol {
            name,
            variant: None,
            specifier: None,
            addend: 0,
        }) => Some(name),
        _ => None,
    }
}

/// `isSVEPattern` and `isSVEVecLenSpecifier`: the parser turns a known name
/// into its encoding; anything else goes through the normal immediate path.
fn named_or_const(
    op: &Operand,
    known: fn(&str) -> bool,
    range: std::ops::RangeInclusive<i64>,
) -> Check {
    if keyword(op).is_some_and(known) {
        return Check::Match;
    }
    match value(op) {
        Value::Imm(Expr::Const(v)) => from_bool(true, range.contains(&v)),
        _ => Check::NoMatch,
    }
}

// Floating-point immediates.

/// `isFPImm`. `tryParseFPImm` reads a decimal integer as that value and a
/// hex integer up to 0xff as an 8-bit encoding. `Imm::Constant` doesn't
/// say which was written, so every value up to 255 is accepted.
fn fp_imm(op: &Operand) -> Option<Check> {
    Some(match op {
        Operand::FpImm(v) => {
            // The `FPImm` parser turns `#0.0` into the tokens `#0` and `.0`,
            // so LLVM's predicate never sees it.
            if v.to_bits() == 0 {
                return None;
            }
            from_bool(true, encoding::fp64_imm(v.to_bits()) != -1)
        }
        Operand::Imm(Imm::Constant(c)) => from_bool(
            true,
            (0..=255).contains(c) || encoding::fp64_imm((*c as f64).to_bits()) != -1,
        ),
        _ => Check::NoMatch,
    })
}

/// `AArch64ExactFPImm` values.
fn exact_fp(name: &str) -> Option<f64> {
    match name {
        "zero" => Some(0.0),
        "half" => Some(0.5),
        "one" => Some(1.0),
        "two" => Some(2.0),
        _ => None,
    }
}

/// `isExactFPImm<A, B>`. LLVM also needs the written value to be exact,
/// which `Operand::FpImm` doesn't record; this assumes it was.
fn exact_fp_imm(op: &Operand, a: f64, b: f64) -> Check {
    let equal = |v: f64| v.to_bits() == a.to_bits() || v.to_bits() == b.to_bits();
    match op {
        Operand::FpImm(v) => from_bool(true, equal(*v)),
        // An integer, read as decimal or as a hex encoding.
        Operand::Imm(Imm::Constant(c)) => from_bool(
            true,
            equal(*c as f64)
                || ((0..=255).contains(c) && equal(f64::from(encoding::fp_imm_float(*c as u32)))),
        ),
        _ => Check::NoMatch,
    }
}

// Shifts and extends.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Sh {
    Lsl,
    Lsr,
    Asr,
    Ror,
    Msl,
    Uxtb,
    Uxth,
    Uxtw,
    Uxtx,
    Sxtb,
    Sxth,
    Sxtw,
    Sxtx,
}

fn sh(name: &str) -> Option<Sh> {
    Some(match name.to_ascii_lowercase().as_str() {
        "lsl" => Sh::Lsl,
        "lsr" => Sh::Lsr,
        "asr" => Sh::Asr,
        "ror" => Sh::Ror,
        "msl" => Sh::Msl,
        "uxtb" => Sh::Uxtb,
        "uxth" => Sh::Uxth,
        "uxtw" => Sh::Uxtw,
        "uxtx" => Sh::Uxtx,
        "sxtb" => Sh::Sxtb,
        "sxth" => Sh::Sxth,
        "sxtw" => Sh::Sxtw,
        "sxtx" => Sh::Sxtx,
        _ => return None,
    })
}

/// A shift or extend as LLVM stores it: the amount is `unsigned`.
struct Shift {
    /// None for a kind LLVM doesn't know.
    ty: Option<Sh>,
    amount: u32,
    explicit: bool,
}

/// LLVM's `k_ShiftExtend`.
fn shift_operand(op: &Operand) -> Option<Shift> {
    match op {
        Operand::ShiftExtend {
            kind,
            amount,
            explicit,
        } => Some(Shift {
            ty: Some(sh(kind)?),
            amount: amount.unwrap_or(0) as u32,
            explicit: *explicit,
        }),
        _ => None,
    }
}

/// The shift parsed with a register; `lsl #0`, not explicit, by default.
fn register_shift(reg: &RegisterOperand) -> Shift {
    match &reg.shift {
        None => Shift {
            ty: Some(Sh::Lsl),
            amount: 0,
            explicit: false,
        },
        Some((kind, amount)) => Shift {
            ty: sh(kind),
            amount: amount.unwrap_or(0) as u32,
            explicit: amount.is_some(),
        },
    }
}

/// A predicate that first asks `isShiftExtend`.
fn shift_extend(op: &Operand, ok: impl Fn(&Shift) -> bool) -> Check {
    match shift_operand(op) {
        Some(s) => from_bool(true, ok(&s)),
        None => Check::NoMatch,
    }
}

/// A predicate that first asks `isShifter`: an LSL, LSR, ASR, ROR, or MSL.
fn shifter(op: &Operand, ok: impl Fn(&Shift) -> bool) -> Check {
    match shift_operand(op) {
        Some(s) if matches!(s.ty, Some(Sh::Lsl | Sh::Lsr | Sh::Asr | Sh::Ror | Sh::Msl)) => {
            from_bool(true, ok(&s))
        }
        Some(_) => Check::NearMiss,
        None => Check::NoMatch,
    }
}

/// `isExtend` past the kind check.
fn is_extend(s: &Shift) -> bool {
    matches!(
        s.ty,
        Some(
            Sh::Uxtb
                | Sh::Sxtb
                | Sh::Uxth
                | Sh::Sxth
                | Sh::Uxtw
                | Sh::Sxtw
                | Sh::Uxtx
                | Sh::Sxtx
                | Sh::Lsl
        )
    ) && s.amount <= 4
}

// Registers.

fn register(op: &Operand, kind: RegisterKind) -> Option<&RegisterOperand> {
    match op {
        Operand::Register(reg) if reg.register.kind() == kind => Some(reg),
        _ => None,
    }
}

/// `Kind == k_Register && Reg.Kind == RegKind::Scalar` and in `class`.
fn scalar_in(op: &Operand, class: &str) -> Option<Check> {
    Some(match register(op, RegisterKind::Scalar) {
        Some(reg) => from_bool(true, registers::contains(class, reg.register)?),
        None => Check::NoMatch,
    })
}

/// The element width of an SVE or SME suffix: `b` is 8, none is 0.
fn element_width(suffix: Option<&str>) -> Option<i64> {
    match suffix.map(str::to_ascii_lowercase).as_deref() {
        None | Some("") => Some(0),
        Some("b") => Some(8),
        Some("h") => Some(16),
        Some("s") => Some(32),
        Some("d") => Some(64),
        Some("q") => Some(128),
        _ => None,
    }
}

/// `isSVEDataVectorRegOfWidth` and the predicate versions: right kind, then
/// class and element width.
fn sve_reg_of_width(
    op: &Operand,
    kinds: &[RegisterKind],
    width: i64,
    class: &str,
) -> Option<Check> {
    let reg = match op {
        Operand::Register(reg) if kinds.contains(&reg.register.kind()) => reg,
        _ => return Some(Check::NoMatch),
    };
    let actual = element_width(reg.suffix.as_deref())?;
    let in_class = registers::contains(class, reg.register)?;
    Some(from_bool(true, in_class && actual == width))
}

/// `isSVEDataVectorRegWithShiftExtend<Width, Class, Type, ShiftWidth, Same>`.
fn sve_data_with_shift(p: &Pred, op: &Operand) -> Option<Check> {
    let width = p.int(0)?;
    let class = p.class(1)?;
    let ty = sh(p.tail(2)?)?;
    let shift_width = p.int(3)?;
    let always_same = p.bool(4)?;
    let amount = log2_bytes(shift_width)?;

    if sve_reg_of_width(op, &[RegisterKind::SveData], width, class)? != Check::Match {
        return Some(Check::NoMatch);
    }
    let Operand::Register(reg) = op else {
        return Some(Check::NoMatch);
    };
    let shift = register_shift(reg);
    let match_shift = shift.amount == amount;
    // A more specific diagnostic comes from the unscaled form.
    if !match_shift
        && matches!(ty, Sh::Uxtw | Sh::Sxtw)
        && !always_same
        && shift.explicit
        && shift_width == 8
    {
        return Some(Check::NoMatch);
    }
    Some(from_bool(true, match_shift && shift.ty == Some(ty)))
}

/// `isMatrixRegOperand<Kind, EltSize, RegClass>`.
fn matrix_reg(op: &Operand, kind: &str, width: i64, class: &str) -> Option<Check> {
    let reg = match op {
        Operand::Register(reg)
            if matches!(
                reg.register.kind(),
                RegisterKind::Matrix | RegisterKind::MatrixTileList
            ) =>
        {
            reg
        }
        _ => return Some(Check::NoMatch),
    };
    let name = reg.register.name();
    let (actual_kind, actual_width) = if name.eq_ignore_ascii_case("za") {
        ("Array", element_width(reg.suffix.as_deref())?)
    } else {
        let (head, tail) = name.split_once('.')?;
        let kind = match head.chars().last() {
            Some('h' | 'H') => "Row",
            Some('v' | 'V') => "Col",
            _ => "Tile",
        };
        (kind, element_width(Some(tail))?)
    };
    let in_class = registers::contains(class, reg.register)?;
    Some(from_bool(
        true,
        actual_kind == kind && in_class && actual_width == width,
    ))
}

// Vector lists.

/// What `isTypedVectorList` asks for.
struct Typed {
    kind: RegisterKind,
    count: i64,
    elements: i64,
    width: i64,
    stride: i64,
}

fn typed_args(p: &Pred, stride: i64) -> Option<Typed> {
    Some(Typed {
        kind: p.reg_kind(0)?,
        count: p.int(1)?,
        elements: p.int(2)?,
        width: p.int(3)?,
        stride,
    })
}

/// A vector list as LLVM's `VectorListOp` holds it.
struct ListInfo {
    kind: RegisterKind,
    first: Register,
    count: u32,
    stride: u32,
    elements: i64,
    width: i64,
}

impl ListInfo {
    fn is_typed(&self, want: &Typed) -> bool {
        self.kind == want.kind
            && i64::from(self.count) == want.count
            && self.width == want.width
            && i64::from(self.stride) == want.stride
            && self.elements == want.elements
    }
}

enum List {
    Not,
    /// A list whose suffix LLVM's `parseVectorKind` wouldn't accept.
    Bad,
    Is(ListInfo),
}

/// `parseVectorKind`: the element count and width of a suffix.
fn vector_kind(suffix: Option<&str>, kind: RegisterKind) -> Option<(i64, i64)> {
    let suffix = suffix.map(str::to_ascii_lowercase);
    let suffix = suffix.as_deref().unwrap_or("");
    if kind != RegisterKind::NeonVector {
        return element_width(Some(suffix)).map(|w| (0, w));
    }
    Some(match suffix {
        "" => (0, 0),
        "1d" => (1, 64),
        "1q" => (1, 128),
        "2h" => (2, 16),
        "2b" => (2, 8),
        "2s" => (2, 32),
        "2d" => (2, 64),
        "4b" => (4, 8),
        "4h" => (4, 16),
        "4s" => (4, 32),
        "8b" => (8, 8),
        "8h" => (8, 16),
        "16b" => (16, 8),
        "b" => (0, 8),
        "h" => (0, 16),
        "s" => (0, 32),
        "d" => (0, 64),
        _ => return None,
    })
}

fn list(op: &Operand) -> List {
    let Operand::VectorList {
        first,
        count,
        stride,
        suffix,
    } = op
    else {
        return List::Not;
    };
    let kind = first.kind();
    match vector_kind(suffix.as_deref(), kind) {
        Some((elements, width)) => List::Is(ListInfo {
            kind,
            first: *first,
            count: *count,
            stride: *stride,
            elements,
            width,
        }),
        None => List::Bad,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::generated::CLASSES;
    use Check::*;

    fn imm(v: i64) -> Operand {
        Operand::Imm(Imm::Constant(v))
    }

    fn sym(variant: Option<&str>, specifier: Option<&str>, addend: i64) -> Operand {
        Operand::Imm(Imm::Symbol {
            name: "_foo".into(),
            variant: variant.map(Into::into),
            specifier: specifier.map(Into::into),
            addend,
        })
    }

    fn shifted(v: i64, shift: u32) -> Operand {
        Operand::ShiftedImm {
            value: Imm::Constant(v),
            shift,
        }
    }

    fn range(first: i64, last: i64) -> Operand {
        Operand::ImmRange { first, last }
    }

    fn ident(name: &str) -> Operand {
        Operand::Ident(name.into())
    }

    fn reg_of(name: &str, kind: RegisterKind) -> Register {
        Register::lookup_kind(name, kind).unwrap_or_else(|| panic!("no {name}"))
    }

    fn reg_full(
        name: &str,
        kind: RegisterKind,
        suffix: Option<&str>,
        shift: Option<(&str, Option<i64>)>,
    ) -> Operand {
        Operand::Register(RegisterOperand {
            register: reg_of(name, kind),
            suffix: suffix.map(Into::into),
            shift: shift.map(|(k, a)| (k.into(), a)),
        })
    }

    fn x(name: &str) -> Operand {
        reg_full(name, RegisterKind::Scalar, None, None)
    }

    fn z(name: &str, suffix: Option<&str>) -> Operand {
        reg_full(name, RegisterKind::SveData, suffix, None)
    }

    fn shift(kind: &str, amount: Option<i64>) -> Operand {
        Operand::ShiftExtend {
            kind: kind.into(),
            amount,
            explicit: amount.is_some(),
        }
    }

    fn vlist(
        first: &str,
        kind: RegisterKind,
        count: u32,
        stride: u32,
        suffix: Option<&str>,
    ) -> Operand {
        Operand::VectorList {
            first: reg_of(first, kind),
            count,
            stride,
            suffix: suffix.map(Into::into),
        }
    }

    fn run(cases: &[(&str, Operand, Check)]) {
        for (predicate, operand, want) in cases {
            assert_eq!(
                check(predicate, operand),
                *want,
                "{predicate} on {operand:?}"
            );
        }
    }

    #[test]
    fn parses_predicate_strings() {
        let p = Pred::parse("isUImmScaled<0, 2, 1, /*IsRange=*/true>").expect("parses");
        assert_eq!(p.name, "isUImmScaled");
        assert_eq!(p.args, ["0", "2", "1", "true"]);
        let p = Pred::parse("isTypedVectorListStrided<RegKind::SVEDataVector, 2,8,16>")
            .expect("parses");
        assert_eq!(p.reg_kind(0), Some(RegisterKind::SveData));
        assert_eq!(p.int(3), Some(16));
        let p = Pred::parse("isGPR64<AArch64::GPR64spRegClassID>").expect("parses");
        assert_eq!(p.class(0), Some("GPR64sp"));
        assert!(Pred::parse("isImm<").is_none());
        assert_eq!(check("isNoSuchThing", &imm(0)), Unknown);
        assert_eq!(check("isImmInRange<a,b>", &imm(0)), Unknown);
    }

    #[test]
    fn integer_ranges() {
        run(&[
            ("isImm", imm(5), Match),
            ("isImm", sym(None, None, 0), Match),
            ("isImm", ident("label"), Match),
            ("isImm", Operand::Imm(Imm::Other), Match),
            ("isImm", x("x0"), NoMatch),
            ("isImmInRange<0,7>", imm(7), Match),
            ("isImmInRange<0,7>", imm(8), NearMiss),
            ("isImmInRange<0,7>", imm(-1), NearMiss),
            ("isImmInRange<0,7>", sym(None, None, 0), NearMiss),
            ("isImmInRange<0,7>", Operand::Imm(Imm::Other), NearMiss),
            ("isImmInRange<0,7>", x("x0"), NoMatch),
            ("isImmInRange<1,64>", imm(64), Match),
            // UImm6P1 and UImm6M1 adjust by one before this predicate runs.
            ("isImmInRange<0,63>", imm(64), Unknown),
            ("isImmInRange<0,63>", imm(-1), Unknown),
            ("isImmInRange<0,63>", imm(65), NearMiss),
            ("isUImm6", imm(63), Match),
            ("isUImm6", imm(64), NearMiss),
            ("isSImm<9>", imm(-256), Match),
            ("isSImm<9>", imm(256), NearMiss),
        ]);
    }

    #[test]
    fn scaled_immediates() {
        run(&[
            // `ldp x0, x1, [sp, #504]`: SImm7s8 is -512 to 504 in steps of 8.
            ("isSImmScaled<7, 8>", imm(504), Match),
            ("isSImmScaled<7, 8>", imm(-512), Match),
            ("isSImmScaled<7, 8>", imm(512), NearMiss),
            ("isSImmScaled<7, 8>", imm(4), NearMiss),
            ("isSImmScaled<7, 8>", sym(None, None, 0), NoMatch),
            ("isSImmScaled<4, 16>", imm(-128), Match),
            ("isUImmScaled<6, 8>", imm(504), Match),
            ("isUImmScaled<6, 8>", imm(-8), NearMiss),
            ("isUImmScaled<0, 2, 1, /*IsRange=*/true>", imm(0), NoMatch),
            // `ldr x0, [x1, #32760]` assembles; `#32768` doesn't.
            ("isUImm12Offset<8>", imm(32760), Match),
            ("isUImm12Offset<8>", imm(32768), NearMiss),
            ("isUImm12Offset<8>", imm(3), NearMiss),
            ("isUImm12Offset<8>", imm(-8), NearMiss),
            ("isUImm12Offset<8>", sym(Some("pageoff"), None, 16), Match),
            ("isUImm12Offset<8>", sym(None, Some("lo12"), 0), Match),
            ("isUImm12Offset<8>", sym(Some("gotpageoff"), None, 0), Match),
            (
                "isUImm12Offset<8>",
                sym(Some("gotpageoff"), None, 8),
                NearMiss,
            ),
            ("isUImm12Offset<8>", sym(None, None, 0), NearMiss),
            ("isUImm12Offset<8>", Operand::Imm(Imm::Other), Match),
            // `ldr x0, [x1, #3]` falls back to `ldur`.
            ("isSImm9OffsetFB<64>", imm(3), Match),
            ("isSImm9OffsetFB<64>", imm(8), NearMiss),
            ("isSImm9OffsetFB<64>", imm(-256), Match),
        ]);
    }

    #[test]
    fn add_sub_immediates() {
        run(&[
            ("isAddSubImm", imm(4095), Match),
            // `add x0, x0, #4096` is `#1, lsl #12`; `#4097` is an error.
            ("isAddSubImm", imm(4096), Match),
            ("isAddSubImm", imm(4097), NearMiss),
            ("isAddSubImm", imm(-1), NearMiss),
            ("isAddSubImm", shifted(1, 12), Match),
            ("isAddSubImm", shifted(4096, 12), NearMiss),
            ("isAddSubImm", shifted(1, 0), Match),
            ("isAddSubImm", shifted(1, 16), NearMiss),
            ("isAddSubImm", sym(Some("pageoff"), None, 8), Match),
            ("isAddSubImm", sym(Some("tlvppageoff"), None, 0), Match),
            ("isAddSubImm", sym(Some("gotpageoff"), None, 0), Match),
            ("isAddSubImm", sym(Some("gotpageoff"), None, 8), NearMiss),
            ("isAddSubImm", sym(None, Some("lo12"), 0), Match),
            ("isAddSubImm", sym(None, None, 0), NearMiss),
            ("isAddSubImm", ident("_foo"), NearMiss),
            ("isAddSubImm", sym(Some("pageoff"), Some("lo12"), 0), Match),
            ("isAddSubImm", Operand::Imm(Imm::Other), Match),
            ("isAddSubImm", x("x1"), NoMatch),
            // `sub x0, x0, #-4096` assembles as `add`.
            ("isAddSubImmNeg", imm(-4096), Match),
            ("isAddSubImmNeg", imm(-4095), Match),
            ("isAddSubImmNeg", imm(4), NearMiss),
            ("isAddSubImmNeg", imm(i64::MIN), NearMiss),
            ("isAddSubImmNeg", x("x0"), NoMatch),
        ]);
    }

    #[test]
    fn logical_immediates() {
        run(&[
            ("isLogicalImm<int64_t>", imm(0x5555_5555_5555_5555), Match),
            ("isLogicalImm<int64_t>", imm(0), NearMiss),
            ("isLogicalImm<int64_t>", imm(-1), NearMiss),
            // `and w0, w0, #0xffffffff00000001` and `#-2` assemble.
            (
                "isLogicalImm<int32_t>",
                imm(0xffff_ffff_0000_0001_u64 as i64),
                Match,
            ),
            ("isLogicalImm<int32_t>", imm(-2), Match),
            ("isLogicalImm<int32_t>", imm(0x1_0000_0001), NearMiss),
            ("isLogicalImm<int8_t>", imm(0x0f), Match),
            ("isLogicalImm<int8_t>", imm(0x1ff), NearMiss),
            ("isLogicalImm<int64_t>", sym(None, None, 0), NearMiss),
            ("isSVEPreferredLogicalImm<int64_t>", imm(0x7f), NoMatch),
            // `mov z0.d, #0xff` assembles as `dupm`; `#0x7f` as `dup`.
            ("isSVEPreferredLogicalImm<int64_t>", imm(0xff), Match),
            (
                "isSVEPreferredLogicalImm<int64_t>",
                imm(0x5555_5555_5555_5555),
                Match,
            ),
            ("isSVEPreferredLogicalImm<int16_t>", imm(0x007f), NoMatch),
            ("isSVEPreferredLogicalImm<int16_t>", imm(0x0f0f), Match),
        ]);
    }

    #[test]
    fn sve_immediates() {
        run(&[
            ("isSVECpyImm<int8_t>", imm(-128), Match),
            ("isSVECpyImm<int8_t>", imm(255), Match),
            ("isSVECpyImm<int8_t>", imm(256), NearMiss),
            ("isSVECpyImm<int8_t>", shifted(1, 8), NearMiss),
            ("isSVECpyImm<int16_t>", imm(256), Match),
            ("isSVECpyImm<int16_t>", shifted(-128, 8), Match),
            ("isSVECpyImm<int16_t>", shifted(1, 4), NearMiss),
            ("isSVECpyImm<int32_t>", imm(257), NearMiss),
            ("isSVECpyImm<int64_t>", sym(None, None, 0), NoMatch),
            ("isSVECpyImm<int64_t>", x("x0"), NoMatch),
            ("isSVEAddSubImm<int8_t>", imm(255), Match),
            ("isSVEAddSubImm<int8_t>", imm(-1), NearMiss),
            ("isSVEAddSubImm<int16_t>", imm(0xff00), Match),
            ("isSVEAddSubImm<int32_t>", shifted(255, 8), Match),
            ("isSVEAddSubImm<int32_t>", shifted(256, 8), NearMiss),
            ("isSVEPattern", ident("vl256"), Match),
            ("isSVEPattern", ident("ALL"), Match),
            ("isSVEPattern", imm(31), Match),
            ("isSVEPattern", imm(32), NearMiss),
            ("isSVEPattern", ident("vl9"), NoMatch),
            ("isSVEVecLenSpecifier", ident("vlx4"), Match),
            ("isSVEVecLenSpecifier", imm(1), Match),
            ("isSVEVecLenSpecifier", imm(2), NearMiss),
        ]);
    }

    #[test]
    fn mov_aliases_and_symbols() {
        run(&[
            ("isMOVZMovAlias<64, 16>", imm(0x1_0000), Match),
            ("isMOVZMovAlias<64, 0>", imm(0x1_0000), NearMiss),
            ("isMOVZMovAlias<64, 0>", imm(0), Match),
            ("isMOVZMovAlias<64, 16>", imm(0), NearMiss),
            ("isMOVZMovAlias<32, 16>", imm(-65536), Match),
            ("isMOVZMovAlias<64, 0>", sym(None, Some("abs_g0"), 0), Match),
            ("isMOVZMovAlias<64, 16>", sym(None, None, 0), NearMiss),
            ("isMOVNMovAlias<64, 0>", imm(-1), Match),
            ("isMOVNMovAlias<64, 16>", imm(!0x1_0000), Match),
            ("isMOVNMovAlias<64, 0>", imm(0xffff), NearMiss),
            ("isMOVNMovAlias<32, 0>", sym(None, None, 0), NearMiss),
            ("isMovWSymbolG0", sym(None, Some("abs_g0_nc"), 0), Match),
            ("isMovWSymbolG1", sym(None, Some("abs_g0_nc"), 0), NearMiss),
            ("isMovWSymbolG3", sym(None, Some("prel_g3"), 4), Match),
            ("isMovWSymbolG2", sym(Some("page"), None, 0), NearMiss),
            ("isMovWSymbolG2", sym(None, None, 0), NearMiss),
            ("isMovWSymbolG2", imm(0), NearMiss),
            (
                "isSIMDImmType10",
                imm(0xff00_ff00_ff00_ff00_u64 as i64),
                Match,
            ),
            (
                "isSIMDImmType10",
                imm(0xff00_ff00_ff00_ff01_u64 as i64),
                NearMiss,
            ),
        ]);
    }

    #[test]
    fn labels() {
        run(&[
            // `b #134217724` assembles; `#134217728` and `#3` don't.
            ("isBranchTarget<26>", imm(134_217_724), Match),
            ("isBranchTarget<26>", imm(134_217_728), NearMiss),
            ("isBranchTarget<26>", imm(-134_217_728), Match),
            ("isBranchTarget<26>", imm(3), NearMiss),
            ("isBranchTarget<14>", imm(32764), Match),
            ("isBranchTarget<14>", imm(32768), NearMiss),
            ("isBranchTarget<19>", ident("label"), Match),
            ("isBranchTarget<19>", x("x0"), NoMatch),
            ("isAdrpLabel", imm(4096), Match),
            ("isAdrpLabel", imm(4095), NearMiss),
            ("isAdrpLabel", ident("_foo"), Match),
            ("isAdrpLabel", sym(Some("page"), None, 8), Match),
            ("isAdrpLabel", sym(Some("gotpage"), None, 0), Match),
            ("isAdrpLabel", sym(Some("gotpage"), None, 8), NearMiss),
            // `adrp x0, _foo@PAGEOFF`: page or gotpage label reference expected.
            ("isAdrpLabel", sym(Some("pageoff"), None, 0), NearMiss),
            ("isAdrpLabel", sym(None, Some("got"), 0), Match),
            ("isAdrLabel", imm((1 << 20) - 1), Match),
            ("isAdrLabel", imm(1 << 20), NearMiss),
            ("isAdrLabel", ident("_foo"), Match),
            // `adr x0, _foo@PAGE`: unexpected adr label.
            ("isAdrLabel", sym(Some("page"), None, 0), NearMiss),
            ("isPAuthPCRelLabel16Operand", imm(-4), Match),
            ("isPAuthPCRelLabel16Operand", imm(4), NearMiss),
            ("isPAuthPCRelLabel16Operand", imm(-(1 << 18)), NearMiss),
        ]);
    }

    #[test]
    fn floating_point() {
        run(&[
            ("isFPImm", Operand::FpImm(1.0), Match),
            ("isFPImm", Operand::FpImm(-2.0), Match),
            ("isFPImm", Operand::FpImm(0.1), NearMiss),
            ("isFPImm", Operand::FpImm(32.0), NearMiss),
            ("isFPImm", Operand::FpImm(0.0), Unknown),
            // `fmov d0, #1`, `#0x70`, and `#-2` assemble; `#32` doesn't.
            ("isFPImm", imm(1), Match),
            ("isFPImm", imm(0x70), Match),
            ("isFPImm", imm(-2), Match),
            ("isFPImm", imm(256), NearMiss),
            ("isFPImm", x("x0"), NoMatch),
            (
                "isExactFPImm<AArch64ExactFPImm::half, AArch64ExactFPImm::one>",
                Operand::FpImm(0.5),
                Match,
            ),
            (
                "isExactFPImm<AArch64ExactFPImm::half, AArch64ExactFPImm::one>",
                Operand::FpImm(2.0),
                NearMiss,
            ),
            (
                "isExactFPImm<AArch64ExactFPImm::zero, AArch64ExactFPImm::one>",
                Operand::FpImm(-0.0),
                NearMiss,
            ),
            (
                "isExactFPImm<AArch64ExactFPImm::zero, AArch64ExactFPImm::one>",
                imm(1),
                Match,
            ),
            (
                "isExactFPImm<AArch64ExactFPImm::half, AArch64ExactFPImm::two>",
                imm(3),
                NearMiss,
            ),
            (
                "isExactFPImm<AArch64ExactFPImm::half, AArch64ExactFPImm::two>",
                x("x0"),
                NoMatch,
            ),
        ]);
    }

    #[test]
    fn named_operands() {
        run(&[
            ("isCondCode", Operand::CondCode("eq".into()), Match),
            ("isCondCode", Operand::CondCode("tstop".into()), Match),
            ("isCondCode", Operand::CondCode("xx".into()), NearMiss),
            ("isCondCode", imm(0), NoMatch),
            ("isBarrier", ident("ish"), Match),
            ("isBarrier", ident("csync"), Match),
            ("isBarrier", ident("synxs"), NoMatch),
            ("isBarrier", imm(15), Match),
            ("isBarrier", imm(16), NoMatch),
            ("isBarriernXS", ident("synxs"), Match),
            ("isBarriernXS", imm(20), Match),
            ("isBarriernXS", imm(21), NoMatch),
            ("isPrefetch", ident("pldl1keep"), Match),
            ("isPrefetch", ident("pldl9keep"), NoMatch),
            ("isPrefetch", imm(31), Match),
            ("isPrefetch", imm(40), Unknown),
            ("isPrefetch", imm(64), NoMatch),
            ("isPSBHint", ident("csync"), Match),
            ("isBTIHint", ident("jc"), Match),
            ("isBTIHint", ident("x"), NoMatch),
            ("isPHint", ident("keep"), Match),
            ("isSysCR", ident("c7"), Match),
            ("isSysCR", ident("c16"), NoMatch),
            ("isSVCR", ident("svcrsmza"), Match),
            ("isSVCR", ident("tpidr_el0"), NearMiss),
            ("isSystemPStateFieldWithImm0_15", ident("daifset"), Match),
            ("isSystemPStateFieldWithImm0_15", ident("allint"), NoMatch),
            ("isSystemPStateFieldWithImm0_1", ident("allint"), Match),
            // SME offset ranges: `0:3` for a range of 4, scaled by 4.
            (
                "isUImmScaled<2, 4, 3, /*IsRange=*/true>",
                range(0, 3),
                Match,
            ),
            (
                "isUImmScaled<2, 4, 3, /*IsRange=*/true>",
                range(4, 7),
                Match,
            ),
            (
                "isUImmScaled<2, 4, 3, /*IsRange=*/true>",
                range(0, 2),
                NoMatch,
            ),
            (
                "isUImmScaled<2, 4, 3, /*IsRange=*/true>",
                range(2, 5),
                NearMiss,
            ),
            ("isUImmScaled<2, 4, 3, /*IsRange=*/true>", imm(0), NoMatch),
            (
                "isUImmScaled<0, 2, 1, /*IsRange=*/true>",
                range(0, 1),
                Match,
            ),
            (
                "isUImmScaled<0, 2, 1, /*IsRange=*/true>",
                range(2, 3),
                NearMiss,
            ),
            ("isUImmScaled<3, 8, 0>", range(0, 0), NoMatch),
            ("isMRSSystemRegister", ident("tpidr_el0"), Match),
            ("isMSRSystemRegister", ident("tpidr_el0"), Match),
            // Read-only, and write-only.
            ("isMRSSystemRegister", ident("midr_el1"), Match),
            ("isMSRSystemRegister", ident("midr_el1"), NearMiss),
            ("isMRSSystemRegister", ident("TPIDR_EL0"), Match),
            ("isMRSSystemRegister", ident("tpidr_el9"), NearMiss),
            // Generic names, in range or not.
            ("isMRSSystemRegister", ident("s3_0_c15_c2_0"), Match),
            ("isMSRSystemRegister", ident("S3_7_C15_C15_7"), Match),
            ("isMRSSystemRegister", ident("s4_0_c15_c2_0"), NearMiss),
            ("isMRSSystemRegister", ident("s3_0_c16_c2_0"), NearMiss),
            // SVCR names aren't system registers to mrs.
            ("isMRSSystemRegister", ident("svcrsm"), NoMatch),
            // Gated by SME: read-only either way; readable only with SME.
            ("isMSRSystemRegister", ident("smidr_el1"), NearMiss),
            ("isMRSSystemRegister", ident("smidr_el1"), Unknown),
            ("isMSRSystemRegister", imm(0), NoMatch),
        ]);
    }

    #[test]
    fn shifts_and_extends() {
        run(&[
            ("isShifter", shift("ror", Some(3)), Match),
            ("isShifter", shift("uxtw", None), NearMiss),
            ("isShifter", imm(0), NoMatch),
            ("isArithmeticShifter<32>", shift("asr", Some(31)), Match),
            ("isArithmeticShifter<32>", shift("asr", Some(32)), NearMiss),
            ("isArithmeticShifter<64>", shift("ror", Some(1)), NearMiss),
            // A negative amount wraps to a large unsigned one.
            ("isArithmeticShifter<64>", shift("lsl", Some(-1)), NearMiss),
            ("isLogicalShifter<64>", shift("ror", Some(63)), Match),
            ("isMovImm32Shifter", shift("lsl", Some(16)), Match),
            ("isMovImm32Shifter", shift("lsl", Some(32)), NearMiss),
            ("isMovImm64Shifter", shift("lsl", Some(48)), Match),
            ("isLogicalVecShifter", shift("lsl", Some(24)), Match),
            (
                "isLogicalVecHalfWordShifter",
                shift("lsl", Some(16)),
                NearMiss,
            ),
            ("isMoveVecShifter", shift("msl", Some(16)), Match),
            ("isMoveVecShifter", shift("msl", Some(0)), NearMiss),
            ("isMoveVecShifter", shift("uxtw", None), NearMiss),
            ("isLSLImm3Shift", shift("lsl", Some(7)), Match),
            ("isExtend", shift("uxtw", None), Match),
            ("isExtend", shift("sxtx", Some(4)), Match),
            ("isExtend", shift("sxtx", Some(5)), NearMiss),
            ("isExtend", shift("lsr", Some(1)), NearMiss),
            ("isExtend", shift("mul vl", None), NoMatch),
            ("isExtend64", shift("uxtx", None), NearMiss),
            ("isExtend64", shift("sxtw", Some(2)), Match),
            ("isExtendLSL64", shift("uxtx", Some(4)), Match),
            ("isMemXExtend<64>", shift("lsl", Some(3)), Match),
            ("isMemXExtend<64>", shift("sxtx", Some(0)), Match),
            ("isMemXExtend<64>", shift("lsl", Some(2)), NearMiss),
            ("isMemXExtend<64>", shift("uxtw", None), NearMiss),
            ("isMemWExtend<32>", shift("uxtw", Some(2)), Match),
            ("isMemWExtend<32>", shift("sxtw", None), Match),
            ("isMemWExtend<32>", shift("sxtx", None), NearMiss),
        ]);
    }

    #[test]
    fn scalar_registers() {
        run(&[
            ("isGPR64<AArch64::GPR64spRegClassID>", x("sp"), Match),
            ("isGPR64<AArch64::GPR64spRegClassID>", x("xzr"), NearMiss),
            ("isGPR64<AArch64::FPR16_loRegClassID>", x("h15"), Match),
            ("isGPR64<AArch64::FPR16_loRegClassID>", x("h16"), NearMiss),
            ("isGPR64<AArch64::FPR128RegClassID>", z("z0", None), NoMatch),
            ("isFPRasZPR<AArch64::FPR32RegClassID>", x("s3"), Match),
            ("isFPRasZPR<AArch64::FPR32RegClassID>", x("d3"), NearMiss),
            ("isGPR32as64", x("x3"), Match),
            ("isGPR32as64", x("w3"), NearMiss),
            ("isGPR64as32", x("wzr"), Match),
            ("isGPR64x8", x("x22"), Match),
            ("isGPR64x8", x("x23"), NearMiss),
            ("isXSeqPair", x("x4"), Match),
            ("isXSeqPair", x("x5"), NearMiss),
            ("isWSeqPair", x("w30"), Match),
            ("isSyspXzrPair", x("xzr"), Match),
            ("isSyspXzrPair", x("x0"), NearMiss),
            (
                "isGPR64WithShiftExtend<AArch64::GPR64RegClassID, 64>",
                reg_full("x1", RegisterKind::Scalar, None, Some(("lsl", Some(3)))),
                Match,
            ),
            (
                "isGPR64WithShiftExtend<AArch64::GPR64RegClassID, 64>",
                x("x1"),
                NearMiss,
            ),
            (
                "isGPR64WithShiftExtend<AArch64::GPR64RegClassID, 8>",
                x("x1"),
                Match,
            ),
            (
                "isGPR64WithShiftExtend<AArch64::GPR64commonRegClassID, 8>",
                x("xzr"),
                NearMiss,
            ),
        ]);
    }

    #[test]
    fn vector_registers() {
        use RegisterKind::*;
        run(&[
            (
                "isNeonVectorReg",
                reg_full("v31", NeonVector, Some("4s"), None),
                Match,
            ),
            ("isNeonVectorReg", x("q0"), NoMatch),
            (
                "isNeonVectorRegLo",
                reg_full("v15", NeonVector, None, None),
                Match,
            ),
            (
                "isNeonVectorRegLo",
                reg_full("v16", NeonVector, None, None),
                NearMiss,
            ),
            (
                "isNeonVectorReg0to7",
                reg_full("v8", NeonVector, None, None),
                NearMiss,
            ),
            (
                "isSVEDataVectorRegOfWidth<64, AArch64::ZPRRegClassID>",
                z("z31", Some("d")),
                Match,
            ),
            (
                "isSVEDataVectorRegOfWidth<64, AArch64::ZPRRegClassID>",
                z("z31", Some("s")),
                NearMiss,
            ),
            (
                "isSVEDataVectorRegOfWidth<0, AArch64::ZPRRegClassID>",
                z("z31", None),
                Match,
            ),
            (
                "isSVEDataVectorRegOfWidth<8, AArch64::ZPR_3bRegClassID>",
                z("z8", Some("b")),
                NearMiss,
            ),
            (
                "isSVEDataVectorRegOfWidth<0, AArch64::ZPR_KRegClassID>",
                z("z28", None),
                Match,
            ),
            (
                "isSVEDataVectorRegOfWidth<8, AArch64::ZPRRegClassID>",
                x("x0"),
                NoMatch,
            ),
            (
                "isSVEDataVectorRegOfWidth<8, AArch64::ZPRRegClassID>",
                z("z0", Some("4s")),
                Unknown,
            ),
            (
                "isSVEPredicateVectorRegOfWidth<0, AArch64::PPR_3bRegClassID>",
                reg_full("p7", SvePredicate, None, None),
                Match,
            ),
            (
                "isSVEPredicateVectorRegOfWidth<0, AArch64::PPR_3bRegClassID>",
                reg_full("p8", SvePredicate, None, None),
                NearMiss,
            ),
            (
                "isSVEPredicateVectorRegOfWidth<0, AArch64::PPRRegClassID>",
                reg_full("pn8", SvePredicateAsCounter, None, None),
                NoMatch,
            ),
            (
                "isSVEPredicateAsCounterRegOfWidth<0, AArch64::PNR_p8to15RegClassID>",
                reg_full("pn8", SvePredicateAsCounter, None, None),
                Match,
            ),
            (
                "isSVEPredicateAsCounterRegOfWidth<16, AArch64::PNRRegClassID>",
                reg_full("pn3", SvePredicateAsCounter, Some("s"), None),
                NearMiss,
            ),
            (
                "isSVEPredicateOrPredicateAsCounterRegOfWidth<8, AArch64::PPRorPNRRegClassID>",
                reg_full("p3", SvePredicate, Some("b"), None),
                Match,
            ),
            (
                "isSVEPredicateOrPredicateAsCounterRegOfWidth<0, AArch64::PPRorPNRRegClassID>",
                reg_full("pn3", SvePredicateAsCounter, None, None),
                Match,
            ),
        ]);
    }

    #[test]
    fn sve_shifted_registers() {
        use RegisterKind::SveData;
        let lsl3 = reg_full("z1", SveData, Some("d"), Some(("lsl", Some(3))));
        let sxtw = reg_full("z1", SveData, Some("s"), Some(("sxtw", None)));
        let sxtw2 = reg_full("z1", SveData, Some("s"), Some(("sxtw", Some(2))));
        let plain = z("z1", Some("d"));
        let p = |s: &str| format!("isSVEDataVectorRegWithShiftExtend<{s}>");
        run(&[
            (
                &p("64, AArch64::ZPRRegClassID, AArch64_AM::LSL, 64, false"),
                lsl3.clone(),
                Match,
            ),
            (
                &p("64, AArch64::ZPRRegClassID, AArch64_AM::LSL, 32, false"),
                lsl3.clone(),
                NearMiss,
            ),
            (
                &p("64, AArch64::ZPRRegClassID, AArch64_AM::LSL, 8, false"),
                plain.clone(),
                Match,
            ),
            (
                &p("32, AArch64::ZPRRegClassID, AArch64_AM::LSL, 8, false"),
                plain,
                NoMatch,
            ),
            (
                &p("32, AArch64::ZPRRegClassID, AArch64_AM::SXTW, 8, false"),
                sxtw.clone(),
                Match,
            ),
            (
                &p("32, AArch64::ZPRRegClassID, AArch64_AM::SXTW, 32, false"),
                sxtw2.clone(),
                Match,
            ),
            // An explicit amount on the unscaled form leaves the diagnostic
            // to the scaled form.
            (
                &p("32, AArch64::ZPRRegClassID, AArch64_AM::SXTW, 8, false"),
                sxtw2.clone(),
                NoMatch,
            ),
            (
                &p("32, AArch64::ZPRRegClassID, AArch64_AM::SXTW, 8, true"),
                sxtw2,
                NearMiss,
            ),
            (
                &p("32, AArch64::ZPRRegClassID, AArch64_AM::UXTW, 8, false"),
                sxtw,
                NearMiss,
            ),
        ]);
    }

    #[test]
    fn matrix_registers() {
        use RegisterKind::Matrix;
        run(&[
            (
                "isMatrixRegOperand<MatrixKind::Array, 0, AArch64::MPRRegClassID>",
                reg_full("za", Matrix, None, None),
                Match,
            ),
            (
                "isMatrixRegOperand<MatrixKind::Array, 64, AArch64::MPRRegClassID>",
                reg_full("za", Matrix, Some("d"), None),
                Match,
            ),
            (
                "isMatrixRegOperand<MatrixKind::Array, 64, AArch64::MPRRegClassID>",
                reg_full("za", Matrix, None, None),
                NearMiss,
            ),
            (
                "isMatrixRegOperand<MatrixKind::Row, 32, AArch64::MPR32RegClassID>",
                reg_full("za3h.s", Matrix, None, None),
                Match,
            ),
            (
                "isMatrixRegOperand<MatrixKind::Col, 32, AArch64::MPR32RegClassID>",
                reg_full("za3h.s", Matrix, None, None),
                NearMiss,
            ),
            (
                "isMatrixRegOperand<MatrixKind::Tile, 64, AArch64::MPR64RegClassID>",
                reg_full("za7.d", Matrix, None, None),
                Match,
            ),
            (
                "isMatrixRegOperand<MatrixKind::Tile, 64, AArch64::MPR64RegClassID>",
                z("z0", None),
                NoMatch,
            ),
            ("isMatrixTileList", imm(0), NoMatch),
        ]);
    }

    #[test]
    fn vector_lists() {
        use RegisterKind::*;
        run(&[
            (
                "isTypedVectorList<RegKind::NeonVector, 2, 4, 32>",
                vlist("v0", NeonVector, 2, 1, Some("4s")),
                Match,
            ),
            (
                "isTypedVectorList<RegKind::NeonVector, 2, 4, 32>",
                vlist("v0", NeonVector, 3, 1, Some("4s")),
                NearMiss,
            ),
            (
                "isTypedVectorList<RegKind::NeonVector, 2, 4, 32>",
                vlist("v0", NeonVector, 2, 2, Some("4s")),
                NearMiss,
            ),
            (
                "isTypedVectorList<RegKind::NeonVector, 1, 0, 32>",
                vlist("v0", NeonVector, 1, 1, Some("s")),
                Match,
            ),
            (
                "isTypedVectorList<RegKind::NeonVector, 1, 1, 64>",
                vlist("v0", NeonVector, 1, 1, Some("1d")),
                Match,
            ),
            (
                "isTypedVectorList<RegKind::NeonVector, 2, 4, 32>",
                vlist("z0", SveData, 2, 1, Some("s")),
                NoMatch,
            ),
            (
                "isTypedVectorList<RegKind::NeonVector, 2, 4, 32>",
                vlist("v0", NeonVector, 2, 1, Some("3s")),
                Unknown,
            ),
            (
                "isTypedVectorList<RegKind::NeonVector, 2, 4, 32>",
                x("x0"),
                NoMatch,
            ),
            (
                "isTypedVectorList<RegKind::SVEDataVector, 4, 0, 64>",
                vlist("z31", SveData, 4, 1, Some("d")),
                Match,
            ),
            (
                "isImplicitlyTypedVectorList<RegKind::NeonVector, 2>",
                vlist("v31", NeonVector, 2, 1, None),
                Match,
            ),
            (
                "isImplicitlyTypedVectorList<RegKind::NeonVector, 2>",
                vlist("v0", NeonVector, 2, 1, Some("s")),
                Match,
            ),
            (
                "isImplicitlyTypedVectorList<RegKind::NeonVector, 2>",
                vlist("v0", NeonVector, 2, 1, Some("4s")),
                NearMiss,
            ),
            (
                "isImplicitlyTypedVectorList<RegKind::NeonVector, 2, true>",
                vlist("v0", NeonVector, 2, 2, None),
                NearMiss,
            ),
            (
                "isImplicitlyTypedVectorList<RegKind::NeonVector, 2>",
                vlist("v0", NeonVector, 2, 2, None),
                Match,
            ),
            (
                "isTypedVectorListMultiple<RegKind::SVEDataVector, 2, 0, 64, AArch64::ZPRMul2RegClassID>",
                vlist("z2", SveData, 2, 1, Some("d")),
                Match,
            ),
            (
                "isTypedVectorListMultiple<RegKind::SVEDataVector, 2, 0, 64, AArch64::ZPRMul2RegClassID>",
                vlist("z3", SveData, 2, 1, Some("d")),
                NearMiss,
            ),
            (
                "isTypedVectorListMultiple<RegKind::SVEDataVector, 2, 0, 64, AArch64::ZPRMul2RegClassID>",
                vlist("z2", SveData, 2, 1, Some("s")),
                NoMatch,
            ),
            (
                "isTypedVectorListMultiple<RegKind::SVEPredicateVector, 2, 0, 8, AArch64::PPRMul2RegClassID>",
                vlist("p2", SvePredicate, 2, 1, Some("b")),
                Match,
            ),
            (
                "isTypedVectorListStrided<RegKind::SVEDataVector, 2,8,64>",
                vlist("z7", SveData, 2, 8, Some("d")),
                Match,
            ),
            (
                "isTypedVectorListStrided<RegKind::SVEDataVector, 2,8,64>",
                vlist("z23", SveData, 2, 8, Some("d")),
                Match,
            ),
            (
                "isTypedVectorListStrided<RegKind::SVEDataVector, 2,8,64>",
                vlist("z8", SveData, 2, 8, Some("d")),
                NoMatch,
            ),
            (
                "isTypedVectorListStrided<RegKind::SVEDataVector, 4,4,8>",
                vlist("z19", SveData, 4, 4, Some("b")),
                Match,
            ),
            (
                "isTypedVectorListStrided<RegKind::SVEDataVector, 4,4,8>",
                vlist("z0", SveData, 4, 1, Some("b")),
                NoMatch,
            ),
            ("isVectorIndex<0, 3>", Operand::VectorIndex(3), Match),
            ("isVectorIndex<0, 3>", Operand::VectorIndex(4), NearMiss),
            ("isVectorIndex<1, 1>", Operand::VectorIndex(0), NearMiss),
            ("isVectorIndex<0, 3>", imm(1), NoMatch),
            ("isComplexRotation<90, 0>", imm(270), Match),
            ("isComplexRotation<90, 0>", imm(360), NearMiss),
            ("isComplexRotation<180, 90>", imm(90), Match),
            ("isComplexRotation<180, 90>", imm(180), NearMiss),
            ("isComplexRotation<180, 90>", imm(-90), NearMiss),
        ]);
    }

    /// Every predicate the generated tables name is either ported (it gives a
    /// real answer for some operand) or on this list, and none panics.
    #[test]
    fn every_generated_predicate_is_handled() {
        let deliberately_unknown: [&str; 0] = [];
        let samples = [
            imm(0),
            imm(1),
            imm(-1),
            sym(Some("pageoff"), None, 0),
            ident("ish"),
            Operand::Imm(Imm::Other),
            shifted(1, 12),
            Operand::FpImm(1.5),
            Operand::CondCode("eq".into()),
            shift("lsl", Some(2)),
            x("x0"),
            z("z0", Some("d")),
            reg_full("p0", RegisterKind::SvePredicate, None, None),
            reg_full("za", RegisterKind::Matrix, None, None),
            vlist("v0", RegisterKind::NeonVector, 2, 1, Some("4s")),
            Operand::VectorIndex(0),
            Operand::Token("[".into()),
        ];
        let mut predicates: Vec<&str> = CLASSES
            .iter()
            .map(|c| c.predicate)
            .filter(|p| !p.is_empty())
            .collect();
        predicates.sort_unstable();
        predicates.dedup();
        assert_eq!(predicates.len(), 385);

        let mut unknown = Vec::new();
        for predicate in &predicates {
            let answers: Vec<Check> = samples.iter().map(|op| check(predicate, op)).collect();
            if answers.iter().all(|a| *a == Unknown) {
                unknown.push(*predicate);
            }
        }
        assert_eq!(unknown, deliberately_unknown);
    }
}
