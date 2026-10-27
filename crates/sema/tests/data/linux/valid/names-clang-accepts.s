// The same on Linux, plus adr and adrp to labels named like shifts.
limit = 0x1234
        // Clang before LLVM 21 (Xcode 16) wants SVE on for these names.
        .arch_extension sve
        b.none  target
        csel    x0, x1, x2, any
        bl      lsl
        b.eq    lsr
        adrp    x0, uxtw
        adr     x1, lsl
        movz    x0, #:abs_g3:limit
        movk    x0, #:abs_g2_nc:later
target: nop
lsl:    ret
lsr:    ret
uxtw:   ret
later = 0x1234567890ab
