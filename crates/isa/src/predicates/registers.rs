//! LLVM register class membership (`AArch64MCRegisterClasses[...].contains`),
//! from the class definitions in `AArch64RegisterInfo.td`.

use crate::Register;

/// A physical register, decoded from its LLVM name.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Phys {
    /// X0 to X30. LLVM names X29 and X30 `FP` and `LR`.
    X(u32),
    Xzr,
    Sp,
    W(u32),
    Wzr,
    Wsp,
    B(u32),
    H(u32),
    S(u32),
    D(u32),
    Q(u32),
    Z(u32),
    P(u32),
    Pn(u32),
    /// The whole ZA array.
    Za,
    /// A ZA tile: element letter (`B`, `H`, `S`, `D`, `Q`) and number.
    ZaTile(char, u32),
    Other,
}

fn number(digits: &str) -> Option<u32> {
    if digits.is_empty() || !digits.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    digits.parse().ok()
}

fn decode(llvm: &str) -> Phys {
    match llvm {
        "FP" => return Phys::X(29),
        "LR" => return Phys::X(30),
        "XZR" => return Phys::Xzr,
        "SP" => return Phys::Sp,
        "WZR" => return Phys::Wzr,
        "WSP" => return Phys::Wsp,
        "ZA" => return Phys::Za,
        _ => {}
    }
    if let Some(rest) = llvm.strip_prefix("ZA") {
        let mut chars = rest.chars();
        if let Some(letter) = chars.next()
            && let Some(n) = number(chars.as_str())
        {
            return Phys::ZaTile(letter, n);
        }
        return Phys::Other;
    }
    if let Some(n) = llvm.strip_prefix("PN").and_then(number) {
        return Phys::Pn(n);
    }
    let mut chars = llvm.chars();
    let (Some(letter), Some(n)) = (chars.next(), number(chars.as_str())) else {
        return Phys::Other;
    };
    match letter {
        'X' if n <= 30 => Phys::X(n),
        'W' if n <= 30 => Phys::W(n),
        'B' if n < 32 => Phys::B(n),
        'H' if n < 32 => Phys::H(n),
        'S' if n < 32 => Phys::S(n),
        'D' if n < 32 => Phys::D(n),
        'Q' if n < 32 => Phys::Q(n),
        'Z' if n < 32 => Phys::Z(n),
        'P' if n < 16 => Phys::P(n),
        _ => Phys::Other,
    }
}

/// The number of a Z register, for strided list checks.
pub(super) fn z_number(register: Register) -> Option<u32> {
    match decode(register.llvm_name()) {
        Phys::Z(n) => Some(n),
        _ => None,
    }
}

/// Whether LLVM's register class contains `register`. `class` is the name
/// without `AArch64::` and `RegClassID`, like `GPR64sp`. None for a class
/// this port doesn't know.
///
/// For the tuple classes (`GPR64x8Class`, `WSeqPairsClass`,
/// `XSeqPairsClass`) LLVM's operand is the tuple; this checks the register
/// that starts it.
pub(super) fn contains(class: &str, register: Register) -> Option<bool> {
    use Phys::*;
    let r = decode(register.llvm_name());
    let yes = match class {
        "GPR64" => matches!(r, X(_) | Xzr),
        "GPR64sp" => matches!(r, X(_) | Sp),
        "GPR64common" => matches!(r, X(_)),
        "GPR32" => matches!(r, W(_) | Wzr),
        "GPR32sp" => matches!(r, W(_) | Wsp),
        "GPR64x8Class" => matches!(r, X(n) if n % 2 == 0 && n <= 22),
        "WSeqPairsClass" => matches!(r, W(n) if n % 2 == 0),
        "XSeqPairsClass" => matches!(r, X(n) if n % 2 == 0),
        "FPR8" => matches!(r, B(_)),
        "FPR16" => matches!(r, H(_)),
        "FPR16_lo" => matches!(r, H(n) if n < 16),
        "FPR32" => matches!(r, S(_)),
        "FPR64" => matches!(r, D(_)),
        "FPR64_lo" => matches!(r, D(n) if n < 16),
        "FPR128" => matches!(r, Q(_)),
        "FPR128_lo" => matches!(r, Q(n) if n < 16),
        "FPR128_0to7" => matches!(r, Q(n) if n < 8),
        "ZPR" => matches!(r, Z(_)),
        "ZPR_4b" => matches!(r, Z(n) if n < 16),
        "ZPR_3b" => matches!(r, Z(n) if n < 8),
        "ZPRMul2" => matches!(r, Z(n) if n % 2 == 0),
        "ZPRMul4" => matches!(r, Z(n) if n % 4 == 0),
        "ZPRMul2_Lo" => matches!(r, Z(n) if n % 2 == 0 && n < 16),
        "ZPRMul2_Hi" => matches!(r, Z(n) if n % 2 == 0 && n >= 16),
        "ZPR_K" => matches!(r, Z(20..=23 | 28..=31)),
        "PPR" => matches!(r, P(_)),
        "PPR_3b" => matches!(r, P(n) if n < 8),
        "PPR_p8to15" => matches!(r, P(n) if n >= 8),
        "PPRMul2" => matches!(r, P(n) if n % 2 == 0),
        "PNR" => matches!(r, Pn(n) if n < 16),
        "PNR_p8to15" => matches!(r, Pn(8..=15)),
        "PPRorPNR" => matches!(r, P(_)) || matches!(r, Pn(n) if n < 16),
        "MPR" => r == Za,
        "MPR8" => matches!(r, ZaTile('B', 0)),
        "MPR16" => matches!(r, ZaTile('H', n) if n < 2),
        "MPR32" => matches!(r, ZaTile('S', n) if n < 4),
        "MPR64" => matches!(r, ZaTile('D', n) if n < 8),
        "MPR128" => matches!(r, ZaTile('Q', n) if n < 16),
        _ => return None,
    };
    Some(yes)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::RegisterKind;

    fn reg(name: &str, kind: RegisterKind) -> Register {
        Register::lookup_kind(name, kind).unwrap_or_else(|| panic!("no {name}"))
    }

    #[test]
    fn membership() {
        use RegisterKind::*;
        let cases: &[(&str, RegisterKind, &str, bool)] = &[
            ("x0", Scalar, "GPR64", true),
            ("xzr", Scalar, "GPR64", true),
            ("sp", Scalar, "GPR64", false),
            ("sp", Scalar, "GPR64sp", true),
            ("xzr", Scalar, "GPR64sp", false),
            ("fp", Scalar, "GPR64common", true),
            ("lr", Scalar, "GPR64common", true),
            ("x30", Scalar, "GPR64common", true),
            ("w0", Scalar, "GPR64", false),
            ("wzr", Scalar, "GPR32", true),
            ("wsp", Scalar, "GPR32", false),
            ("x22", Scalar, "GPR64x8Class", true),
            ("x24", Scalar, "GPR64x8Class", false),
            ("x1", Scalar, "GPR64x8Class", false),
            ("x30", Scalar, "XSeqPairsClass", true),
            ("x29", Scalar, "XSeqPairsClass", false),
            ("w30", Scalar, "WSeqPairsClass", true),
            ("h15", Scalar, "FPR16_lo", true),
            ("h16", Scalar, "FPR16_lo", false),
            ("b15_hi", Scalar, "FPR8", false),
            ("q7", Scalar, "FPR128_0to7", true),
            ("v8", NeonVector, "FPR128_0to7", false),
            ("v15", NeonVector, "FPR128_lo", true),
            ("z7", SveData, "ZPR_3b", true),
            ("z8", SveData, "ZPR_3b", false),
            ("z20", SveData, "ZPR_K", true),
            ("z24", SveData, "ZPR_K", false),
            ("z16", SveData, "ZPRMul2_Hi", true),
            ("z14", SveData, "ZPRMul2_Hi", false),
            ("z28", SveData, "ZPRMul4", true),
            ("z30", SveData, "ZPRMul4", false),
            ("p7", SvePredicate, "PPR_3b", true),
            ("p8", SvePredicate, "PPR_3b", false),
            ("pn8", SvePredicateAsCounter, "PNR_p8to15", true),
            ("pn7", SvePredicateAsCounter, "PNR_p8to15", false),
            ("pn7", SvePredicateAsCounter, "PPRorPNR", true),
            ("p7", SvePredicate, "PNR", false),
            ("za", Matrix, "MPR", true),
            ("za1.s", Matrix, "MPR32", true),
            ("za1h.d", Matrix, "MPR64", true),
            ("za1.s", Matrix, "MPR64", false),
        ];
        for &(name, kind, class, want) in cases {
            assert_eq!(
                contains(class, reg(name, kind)),
                Some(want),
                "{name} in {class}"
            );
        }
        assert_eq!(contains("NoSuchClass", reg("x0", Scalar)), None);
    }
}
