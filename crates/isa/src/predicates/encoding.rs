//! Immediate encoding checks from LLVM's `AArch64AddressingModes.h`
//! (`AArch64_AM::isLogicalImmediate` and friends), ported as written.

/// `isMask_64`: ones from bit 0 up, at least one.
fn is_mask(value: u64) -> bool {
    value != 0 && (value.wrapping_add(1) & value) == 0
}

/// `isShiftedMask_64`: one run of ones, anywhere.
fn is_shifted_mask(value: u64) -> bool {
    value != 0 && is_mask((value - 1) | value)
}

/// `processLogicalImmediate` without the encoding: whether `imm` is a
/// bitmask immediate for a logical instruction of `reg_size` bits (8, 16,
/// 32, or 64). The value must already fit in `reg_size` bits.
pub(super) fn is_logical_immediate(imm: u64, reg_size: u32) -> bool {
    if !matches!(reg_size, 8 | 16 | 32 | 64) {
        return false;
    }
    if imm == 0
        || imm == u64::MAX
        || (reg_size != 64 && (imm >> reg_size != 0 || imm == (u64::MAX >> (64 - reg_size))))
    {
        return false;
    }

    // Find the element size: the smallest power of two the value repeats in.
    let mut size = reg_size;
    loop {
        size /= 2;
        let mask = (1u64 << size) - 1;
        if (imm & mask) != ((imm >> size) & mask) {
            size *= 2;
            break;
        }
        if size <= 2 {
            break;
        }
    }

    // The element must be a rotated run of ones: 0^m 1^n.
    let mask = u64::MAX >> (64 - size);
    let imm = imm & mask;
    if is_shifted_mask(imm) {
        return true;
    }
    is_shifted_mask(!(imm | !mask))
}

/// `getFPImmFloat`: the value of an 8-bit encoded floating-point immediate.
pub(super) fn fp_imm_float(imm: u32) -> f32 {
    let sign = (imm >> 7) & 1;
    let exp = (imm >> 4) & 7;
    let mantissa = imm & 0xf;
    let mut bits = sign << 31;
    bits |= if exp & 4 != 0 { 0 } else { 1 } << 30;
    bits |= if exp & 4 != 0 { 0x1f } else { 0 } << 25;
    bits |= (exp & 3) << 23;
    bits |= mantissa << 19;
    f32::from_bits(bits)
}

/// `getFP64Imm`: the 8-bit encoding of a double, or -1 if it has none.
pub(super) fn fp64_imm(bits: u64) -> i32 {
    let sign = ((bits >> 63) & 1) as i32;
    let exp = ((bits >> 52) & 0x7ff) as i64 - 1023;
    let mantissa = bits & 0x000f_ffff_ffff_ffff;

    // Four bits of mantissa.
    if mantissa & 0x0000_ffff_ffff_ffff != 0 {
        return -1;
    }
    let mantissa = (mantissa >> 48) as i32;

    // Three bits of exponent.
    if !(-3..=4).contains(&exp) {
        return -1;
    }
    let exp = (((exp + 3) & 7) ^ 4) as i32;
    (sign << 7) | (exp << 4) | mantissa
}

/// `isAdvSIMDModImmType10`: every byte is 0x00 or 0xff.
pub(super) fn is_adv_simd_mod_imm_type10(imm: u64) -> bool {
    imm.to_le_bytes().iter().all(|&b| b == 0 || b == 0xff)
}

/// Truncates `value` to `bits` and sign-extends it back, like a C++ cast to
/// `int8_t`, `int16_t`, `int32_t`, or `int64_t`.
fn trunc(value: i64, bits: u32) -> i64 {
    match bits {
        8 => i64::from(value as i8),
        16 => i64::from(value as i16),
        32 => i64::from(value as i32),
        _ => value,
    }
}

/// `isSVECpyImm<T>` for a `bits`-wide `T`: a signed 8-bit value, or a signed
/// 16-bit multiple of 256, that fits in `T`.
pub(super) fn is_sve_cpy_imm(imm: i64, bits: u32) -> bool {
    let umax = if bits >= 64 {
        u64::MAX
    } else {
        (1u64 << bits) - 1
    };
    let mask = !(umax as i64);
    if (imm & mask) != 0 && (imm & mask) != mask {
        return false;
    }
    if imm & 0xff != 0 {
        return i64::from(imm as i8) == trunc(imm, bits);
    }
    if imm & 0xff00 != 0 {
        return i64::from(imm as i16) == trunc(imm, bits);
    }
    imm == 0
}

/// `isSVEAddSubImm<T>` for a `bits`-wide `T`: 0 to 255, or for wider elements
/// a multiple of 256 up to 65280.
pub(super) fn is_sve_add_sub_imm(imm: i64, bits: u32) -> bool {
    i64::from(imm as u8) == imm || (bits != 8 && i64::from((imm & !0xff) as u16) == imm)
}

/// `isAnyMOVZMovAlias`.
fn is_any_movz_mov_alias(value: u64, reg_width: u32) -> bool {
    (0..=reg_width.saturating_sub(16))
        .step_by(16)
        .any(|shift| value & !(0xffffu64 << shift) == 0)
}

/// `isMOVZMovAlias`: `mov` with this value is `movz` with this shift.
pub(super) fn is_movz_mov_alias(value: u64, shift: u32, reg_width: u32) -> bool {
    if shift >= 64 {
        return false;
    }
    let value = if reg_width == 32 {
        value & 0xffff_ffff
    } else {
        value
    };
    // "lsl #0" takes precedence: in practice this only affects "#0, lsl #0".
    if value == 0 && shift != 0 {
        return false;
    }
    value & !(0xffffu64 << shift) == 0
}

/// `isMOVNMovAlias`: `mov` with this value is `movn` with this shift.
pub(super) fn is_movn_mov_alias(value: u64, shift: u32, reg_width: u32) -> bool {
    // MOVZ takes precedence over MOVN.
    if is_any_movz_mov_alias(value, reg_width) {
        return false;
    }
    let value = if reg_width == 32 {
        !value & 0xffff_ffff
    } else {
        !value
    };
    is_movz_mov_alias(value, shift, reg_width)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn logical_immediates() {
        let cases: &[(u64, u32, bool)] = &[
            (0x5555_5555_5555_5555, 64, true),
            (0x00ff_00ff_00ff_00ff, 64, true),
            (0x1, 64, true),
            (0x8000_0000_0000_0001, 64, true),
            (0x5, 64, false),
            (0, 64, false),
            (u64::MAX, 64, false),
            (0xffff_0000, 32, true),
            (0xffff_fffe, 32, true),
            (0xffff_ffff, 32, false),
            (0x1_0000_0000, 32, false),
            (0x5555, 16, true),
            (0x0f, 8, true),
            (0x81, 8, true),
            (0x05, 8, false),
            (0xff, 8, false),
            (0x100, 8, false),
        ];
        for &(imm, size, want) in cases {
            assert_eq!(is_logical_immediate(imm, size), want, "{imm:#x}/{size}");
        }
    }

    #[test]
    fn fp_immediates() {
        let cases: &[(f64, i32)] = &[
            (1.0, 0x70),
            (2.0, 0x00),
            (0.5, 0x60),
            (-2.0, 0x80),
            (31.0, 0x3f),
            (0.125, 0x40),
            (1.9375, 0x7f),
            (32.0, -1),
            (0.1, -1),
            (0.0, -1),
            (0.0625, -1),
        ];
        for &(value, want) in cases {
            assert_eq!(fp64_imm(value.to_bits()), want, "{value}");
        }
        assert_eq!(fp_imm_float(0x70), 1.0);
        assert_eq!(fp_imm_float(0x00), 2.0);
        assert_eq!(fp_imm_float(0x80), -2.0);
        for imm in 0..=255 {
            let value = f64::from(fp_imm_float(imm));
            assert_eq!(fp64_imm(value.to_bits()), imm as i32, "{imm:#x}");
        }
    }

    #[test]
    fn adv_simd_type10() {
        assert!(is_adv_simd_mod_imm_type10(0xff00_ff00_ff00_ff00));
        assert!(is_adv_simd_mod_imm_type10(0));
        assert!(is_adv_simd_mod_imm_type10(u64::MAX));
        assert!(!is_adv_simd_mod_imm_type10(0xff00_ff00_ff00_ff01));
    }

    #[test]
    fn sve_cpy_immediates() {
        let cases: &[(i64, u32, bool)] = &[
            (127, 8, true),
            (-128, 8, true),
            (255, 8, true),
            (256, 8, false),
            (256, 16, true),
            (257, 16, false),
            (-32768, 16, true),
            (32512, 16, true),
            (65536, 32, false),
            (-1, 64, true),
            (-256, 64, true),
            (0, 32, true),
        ];
        for &(imm, bits, want) in cases {
            assert_eq!(is_sve_cpy_imm(imm, bits), want, "{imm}/{bits}");
        }
    }

    #[test]
    fn sve_add_sub_immediates() {
        let cases: &[(i64, u32, bool)] = &[
            (255, 8, true),
            (256, 8, false),
            (256, 16, true),
            (0xff00, 32, true),
            (0x1_0000, 64, false),
            (257, 16, false),
            (-1, 16, false),
        ];
        for &(imm, bits, want) in cases {
            assert_eq!(is_sve_add_sub_imm(imm, bits), want, "{imm}/{bits}");
        }
    }

    #[test]
    fn mov_aliases() {
        assert!(is_movz_mov_alias(0x1_0000, 16, 64));
        assert!(!is_movz_mov_alias(0x1_0000, 0, 64));
        assert!(is_movz_mov_alias(0, 0, 64));
        assert!(!is_movz_mov_alias(0, 16, 64));
        assert!(is_movz_mov_alias(0xffff_0000_0000_0000, 48, 64));
        // `mov w0, #-65536`: the top half is dropped for 32-bit registers.
        assert!(is_movz_mov_alias(0xffff_ffff_ffff_0000, 16, 32));

        assert!(is_movn_mov_alias(!0x1_0000, 16, 64));
        assert!(is_movn_mov_alias(0xffff_ffff_ffff_0000, 0, 64));
        assert!(!is_movn_mov_alias(0x1_0000, 16, 64));
        assert!(is_movn_mov_alias(0xfffe_ffff, 16, 32));
        // MOVZ wins: `mov x0, #0xffff` is never a MOVN.
        assert!(!is_movn_mov_alias(0xffff, 0, 64));
    }
}
