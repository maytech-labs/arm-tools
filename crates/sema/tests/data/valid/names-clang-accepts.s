// SVE condition names, labels named like shifts, and ELF specifiers on
// absolute constants: all valid on Darwin (found by the Linux survey).
limit = 0x1234
        // Clang before LLVM 21 (Xcode 16) wants SVE on for these names.
        .arch_extension sve
        b.none  target
        b.tstop target
        csel    x0, x1, x2, none
        bl      lsl
        b       lsr
        movz    x0, #:abs_g0:limit
target: nop
lsl:    ret
lsr:    ret
