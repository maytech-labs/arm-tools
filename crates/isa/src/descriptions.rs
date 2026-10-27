//! What instructions do, in a sentence or two, for hover. Written by us, for
//! the instructions people write by hand; the tables have none
//! (docs/decisions/0036-instruction-descriptions.md).

/// What an instruction does, by its lowercase mnemonic head: `ldp`, `b` (for
/// `b` and `b.cond` alike), `fadd`.
pub fn instruction_description(head: &str) -> Option<&'static str> {
    DESCRIPTIONS
        .binary_search_by(|(name, _)| name.cmp(&head))
        .ok()
        .map(|i| DESCRIPTIONS[i].1)
}

/// Sorted by mnemonic.
const DESCRIPTIONS: &[(&str, &str)] = &[
    (
        "abs",
        "Absolute value of each vector element, or of a register with CSSC.",
    ),
    (
        "adc",
        "Add with carry: Rd = Rn + Rm + C. Chains multi-word additions.",
    ),
    ("adcs", "Add with carry, and set the flags."),
    (
        "add",
        "Add: Rd = Rn + an immediate, or a register optionally shifted or extended. With `@PAGEOFF` (`:lo12:` on Linux), the low 12 bits of an address after `adrp`.",
    ),
    ("addp", "Add adjacent pairs of vector elements."),
    ("adds", "Add and set the flags (N, Z, C, V)."),
    ("addv", "Add every element of a vector into one scalar."),
    ("adr", "Form a PC-relative address within 1 MB."),
    (
        "adrp",
        "Form the address of a symbol's 4 KB page, PC-relative within 4 GB: `adrp x0, sym@PAGE` on Darwin, `adrp x0, sym` on Linux. Add the low bits with `add` or a load's `@PAGEOFF` (`:lo12:` on Linux).",
    ),
    (
        "aesd",
        "One round of AES decryption: AddRoundKey, InvSubBytes, InvShiftRows.",
    ),
    (
        "aese",
        "One round of AES encryption: AddRoundKey, SubBytes, ShiftRows.",
    ),
    ("aesimc", "AES inverse MixColumns."),
    ("aesmc", "AES MixColumns."),
    ("and", "Bitwise AND with a register or a bitmask immediate."),
    (
        "ands",
        "Bitwise AND, and set N and Z. `tst` is `ands` into the zero register.",
    ),
    (
        "asr",
        "Arithmetic shift right: the sign bit fills in from the left.",
    ),
    ("asrv", "Arithmetic shift right by a register amount."),
    (
        "autiasp",
        "Authenticate the return address in x30 against sp with key A, before `ret`. Pairs with `paciasp`.",
    ),
    (
        "autibsp",
        "Authenticate the return address in x30 against sp with key B, before `ret`. Pairs with `pacibsp`.",
    ),
    (
        "b",
        "Branch to a label, PC-relative within 128 MB. `b.cond` branches only if the condition holds on the flags, within 1 MB.",
    ),
    ("bfc", "Clear a field of bits: `bfc Rd, #lsb, #width`."),
    (
        "bfi",
        "Insert the low bits of Rn into a field of Rd: `bfi Rd, Rn, #lsb, #width`. Other bits of Rd are kept.",
    ),
    (
        "bfm",
        "Bitfield move, the instruction behind `bfi`, `bfxil`, and `bfc`.",
    ),
    (
        "bfxil",
        "Copy a field from Rn into the low bits of Rd: `bfxil Rd, Rn, #lsb, #width`. Other bits of Rd are kept.",
    ),
    ("bic", "Bit clear: Rd = Rn AND NOT Rm."),
    ("bics", "Bit clear, and set N and Z."),
    (
        "bif",
        "Insert each bit of the first source where the second source's bit is 0.",
    ),
    (
        "bit",
        "Insert each bit of the first source where the second source's bit is 1.",
    ),
    (
        "bl",
        "Call: branch to a label and put the return address in x30 (lr). Save x30 first if this function returns.",
    ),
    (
        "blr",
        "Call the address in a register; the return address goes in x30 (lr).",
    ),
    ("br", "Branch to the address in a register."),
    (
        "brk",
        "Breakpoint: stop in the debugger. `brk #1` is what `__builtin_trap` emits.",
    ),
    (
        "bsl",
        "Bitwise select: take each bit from the first source where the destination's bit is 1, else from the second.",
    ),
    (
        "bti",
        "Branch target identification: marks a valid target for indirect branches.",
    ),
    (
        "cas",
        "Compare and swap: if memory equals Rs, store Rt there. Rs gets the old value. Variants with `a` and `l` add acquire and release ordering.",
    ),
    ("casp", "Compare and swap a pair of registers, atomically."),
    (
        "cbnz",
        "Branch if a register isn't zero, within 1 MB. Doesn't read or set the flags.",
    ),
    (
        "cbz",
        "Branch if a register is zero, within 1 MB. Doesn't read or set the flags.",
    ),
    (
        "ccmn",
        "Conditional compare negative: if the condition holds, set the flags from Rn + operand, else set them to an immediate.",
    ),
    (
        "ccmp",
        "Conditional compare: if the condition holds, set the flags from Rn - operand, else set them to an immediate. Chains comparisons like `a == 1 && b == 2`.",
    ),
    (
        "cinc",
        "Conditional increment: Rd = Rn + 1 if the condition holds, else Rn.",
    ),
    (
        "cinv",
        "Conditional invert: Rd = NOT Rn if the condition holds, else Rn.",
    ),
    (
        "clrex",
        "Clear this core's exclusive monitor, cancelling an `ldxr` reservation.",
    ),
    (
        "cls",
        "Count leading sign bits: how many bits after the top one equal it.",
    ),
    ("clz", "Count leading zero bits."),
    (
        "cmeq",
        "Compare vector elements for equality: all ones where equal, else zero.",
    ),
    (
        "cmge",
        "Compare signed vector elements, greater than or equal.",
    ),
    ("cmgt", "Compare signed vector elements, greater than."),
    ("cmhi", "Compare unsigned vector elements, higher."),
    ("cmhs", "Compare unsigned vector elements, higher or same."),
    (
        "cmn",
        "Compare negative: set the flags from Rn + operand. An alias of `adds` into the zero register.",
    ),
    (
        "cmp",
        "Compare: set the flags from Rn - operand. An alias of `subs` into the zero register.",
    ),
    (
        "cmtst",
        "Test vector elements: all ones where the AND of the two is nonzero.",
    ),
    (
        "cneg",
        "Conditional negate: Rd = -Rn if the condition holds, else Rn.",
    ),
    ("cnt", "Count the set bits in each byte of a vector."),
    ("crc32b", "Update a CRC-32 checksum with a byte."),
    (
        "crc32cb",
        "Update a CRC-32C (Castagnoli) checksum with a byte.",
    ),
    (
        "crc32ch",
        "Update a CRC-32C (Castagnoli) checksum with a halfword.",
    ),
    (
        "crc32cw",
        "Update a CRC-32C (Castagnoli) checksum with a word.",
    ),
    (
        "crc32cx",
        "Update a CRC-32C (Castagnoli) checksum with a doubleword.",
    ),
    ("crc32h", "Update a CRC-32 checksum with a halfword."),
    ("crc32w", "Update a CRC-32 checksum with a word."),
    ("crc32x", "Update a CRC-32 checksum with a doubleword."),
    (
        "csel",
        "Conditional select: Rd = Rn if the condition holds, else Rm.",
    ),
    (
        "cset",
        "Set a register to 1 if the condition holds, else 0.",
    ),
    (
        "csetm",
        "Set a register to all ones if the condition holds, else 0.",
    ),
    (
        "csinc",
        "Conditional select increment: Rd = Rn if the condition holds, else Rm + 1.",
    ),
    (
        "csinv",
        "Conditional select invert: Rd = Rn if the condition holds, else NOT Rm.",
    ),
    (
        "csneg",
        "Conditional select negate: Rd = Rn if the condition holds, else -Rm.",
    ),
    (
        "dc",
        "Data cache operation on an address, like `dc civac, x0` to clean and invalidate a line. An alias of `sys`.",
    ),
    (
        "dmb",
        "Data memory barrier: memory accesses before it are observed before those after it. `dmb ish` orders between cores.",
    ),
    (
        "dsb",
        "Data synchronization barrier: wait until memory accesses before it complete.",
    ),
    (
        "dup",
        "Duplicate a scalar or one vector element into every element.",
    ),
    ("eon", "Bitwise exclusive OR NOT: Rd = Rn XOR NOT Rm."),
    (
        "eor",
        "Bitwise exclusive OR with a register or a bitmask immediate.",
    ),
    (
        "ext",
        "Extract a vector from a pair of vectors, starting at a byte index.",
    ),
    (
        "extr",
        "Extract a register from a pair of registers, starting at a bit. `ror` by an immediate is `extr` with Rn twice.",
    ),
    ("fabs", "Floating-point absolute value."),
    ("fadd", "Floating-point add."),
    ("faddp", "Floating-point add of adjacent pairs."),
    (
        "fccmp",
        "Floating-point conditional compare: if the condition holds, compare, else set the flags to an immediate.",
    ),
    (
        "fcmp",
        "Floating-point compare, setting the flags. Unordered (NaN) sets C and V.",
    ),
    (
        "fcmpe",
        "Floating-point compare that raises Invalid Operation on any NaN.",
    ),
    ("fcsel", "Floating-point conditional select."),
    (
        "fcvt",
        "Convert between floating-point precisions: half, single, double.",
    ),
    (
        "fcvtas",
        "Convert floating-point to a signed integer, rounding to nearest with ties away from zero.",
    ),
    (
        "fcvtms",
        "Convert floating-point to a signed integer, rounding toward minus infinity (floor).",
    ),
    (
        "fcvtns",
        "Convert floating-point to a signed integer, rounding to nearest with ties to even.",
    ),
    (
        "fcvtps",
        "Convert floating-point to a signed integer, rounding toward plus infinity (ceil).",
    ),
    (
        "fcvtzs",
        "Convert floating-point to a signed integer, rounding toward zero, as C's `(long)x` does.",
    ),
    (
        "fcvtzu",
        "Convert floating-point to an unsigned integer, rounding toward zero.",
    ),
    ("fdiv", "Floating-point divide."),
    (
        "fmadd",
        "Floating-point fused multiply-add: Rd = Ra + Rn * Rm, rounded once.",
    ),
    ("fmax", "Floating-point maximum; NaN if either is NaN."),
    (
        "fmaxnm",
        "Floating-point maximum, preferring a number over a quiet NaN.",
    ),
    ("fmin", "Floating-point minimum; NaN if either is NaN."),
    (
        "fminnm",
        "Floating-point minimum, preferring a number over a quiet NaN.",
    ),
    (
        "fmla",
        "Floating-point fused multiply-add into vector elements.",
    ),
    (
        "fmls",
        "Floating-point fused multiply-subtract from vector elements.",
    ),
    (
        "fmov",
        "Move a floating-point value: between FP registers, between FP and general registers bit for bit, or an immediate.",
    ),
    (
        "fmsub",
        "Floating-point fused multiply-subtract: Rd = Ra - Rn * Rm.",
    ),
    ("fmul", "Floating-point multiply."),
    ("fneg", "Floating-point negate."),
    (
        "fnmadd",
        "Floating-point negated fused multiply-add: Rd = -Ra - Rn * Rm.",
    ),
    ("fnmul", "Floating-point multiply and negate."),
    (
        "frinta",
        "Round to an integral value, ties away from zero (C's `round`).",
    ),
    (
        "frinti",
        "Round to an integral value in the current rounding mode.",
    ),
    (
        "frintm",
        "Round to an integral value toward minus infinity (C's `floor`).",
    ),
    ("frintn", "Round to an integral value, ties to even."),
    (
        "frintp",
        "Round to an integral value toward plus infinity (C's `ceil`).",
    ),
    (
        "frintx",
        "Round to an integral value in the current mode, signalling Inexact.",
    ),
    (
        "frintz",
        "Round to an integral value toward zero (C's `trunc`).",
    ),
    ("fsqrt", "Floating-point square root."),
    ("fsub", "Floating-point subtract."),
    (
        "hint",
        "A hint instruction by number; unknown hints run as `nop`.",
    ),
    ("hlt", "Halt for an external debugger."),
    (
        "ic",
        "Instruction cache operation, like `ic ivau, x0` after writing code. An alias of `sys`.",
    ),
    (
        "ins",
        "Insert a general register or a vector element into one element. `mov v0.s[1], w0` is `ins`.",
    ),
    (
        "isb",
        "Instruction synchronization barrier: refetch instructions after it, as after changing system registers or writing code.",
    ),
    (
        "ld1",
        "Load one to four vector registers from consecutive memory, or one element (`ld1 {v0.s}[1], [x0]`).",
    ),
    (
        "ld1r",
        "Load one element and replicate it into every element of a vector.",
    ),
    (
        "ld2",
        "Load interleaved pairs, de-interleaving them into two vector registers.",
    ),
    (
        "ld3",
        "Load interleaved triples, like RGB pixels, into three vector registers.",
    ),
    (
        "ld4",
        "Load interleaved quadruples, like RGBA pixels, into four vector registers.",
    ),
    (
        "ldadd",
        "Atomically add to memory and return the old value. `a` and `l` variants add acquire and release ordering.",
    ),
    (
        "ldapr",
        "Load-acquire with RCpc ordering: weaker than `ldar`, enough for C++'s `memory_order_acquire`.",
    ),
    (
        "ldar",
        "Load-acquire: no later memory access is observed before this load.",
    ),
    ("ldarb", "Load-acquire a byte."),
    ("ldarh", "Load-acquire a halfword."),
    (
        "ldaxp",
        "Load-acquire exclusive pair, starting an atomic read-modify-write with `stlxp`.",
    ),
    (
        "ldaxr",
        "Load-acquire exclusive, starting an atomic read-modify-write with `stlxr`.",
    ),
    (
        "ldclr",
        "Atomically clear bits in memory (AND NOT) and return the old value.",
    ),
    (
        "ldeor",
        "Atomically XOR into memory and return the old value.",
    ),
    (
        "ldnp",
        "Load a pair of registers with a hint that the data won't be reused soon.",
    ),
    (
        "ldp",
        "Load a pair of registers from two consecutive memory locations. `ldp x29, x30, [sp], #16` restores the frame record.",
    ),
    (
        "ldpsw",
        "Load a pair of signed words, sign-extended to 64 bits.",
    ),
    (
        "ldr",
        "Load a register from memory. Addressing: `[Xn, #imm]`, `[Xn, Xm, lsl #3]`, pre-index `[Xn, #imm]!`, post-index `[Xn], #imm`, a literal label, or `=value` from the literal pool.",
    ),
    ("ldrb", "Load a byte, zero-extended."),
    ("ldrh", "Load a halfword, zero-extended."),
    ("ldrsb", "Load a byte, sign-extended."),
    ("ldrsh", "Load a halfword, sign-extended."),
    ("ldrsw", "Load a word, sign-extended to 64 bits."),
    (
        "ldset",
        "Atomically set bits in memory (OR) and return the old value.",
    ),
    (
        "ldsmax",
        "Atomically store the signed maximum in memory and return the old value.",
    ),
    (
        "ldsmin",
        "Atomically store the signed minimum in memory and return the old value.",
    ),
    (
        "ldumax",
        "Atomically store the unsigned maximum in memory and return the old value.",
    ),
    (
        "ldumin",
        "Atomically store the unsigned minimum in memory and return the old value.",
    ),
    (
        "ldur",
        "Load a register with an unscaled offset from -256 to 255, for offsets `ldr` can't encode.",
    ),
    ("ldurb", "Load a byte with an unscaled offset."),
    ("ldurh", "Load a halfword with an unscaled offset."),
    (
        "ldxp",
        "Load exclusive pair, starting an atomic read-modify-write with `stxp`.",
    ),
    (
        "ldxr",
        "Load exclusive, starting an atomic read-modify-write with `stxr`.",
    ),
    ("lsl", "Logical shift left."),
    ("lslv", "Logical shift left by a register amount."),
    ("lsr", "Logical shift right: zeros fill in from the left."),
    ("lsrv", "Logical shift right by a register amount."),
    ("madd", "Multiply-add: Rd = Ra + Rn * Rm."),
    ("mla", "Multiply-add into vector elements."),
    ("mls", "Multiply-subtract from vector elements."),
    ("mneg", "Multiply and negate: Rd = -(Rn * Rm)."),
    (
        "mov",
        "Copy a register, or set it to an immediate. An alias of `orr`, `add`, `movz`, `movn`, `ins`, or `umov`, depending on the operands.",
    ),
    ("movi", "Move an immediate into every element of a vector."),
    (
        "movk",
        "Move a 16-bit immediate into one halfword of a register, keeping the rest. Builds 64-bit constants after `movz`.",
    ),
    ("movn", "Move the inverse of a shifted 16-bit immediate."),
    (
        "movz",
        "Move a shifted 16-bit immediate, zeroing the rest of the register.",
    ),
    ("mrs", "Read a system register, like `mrs x0, tpidrro_el0`."),
    ("msr", "Write a system register or a PSTATE field."),
    ("msub", "Multiply-subtract: Rd = Ra - Rn * Rm."),
    ("mul", "Multiply, keeping the low half of the product."),
    ("mvn", "Bitwise NOT."),
    (
        "mvni",
        "Move the inverse of an immediate into every element of a vector.",
    ),
    ("neg", "Negate: Rd = -operand."),
    ("negs", "Negate, and set the flags."),
    ("ngc", "Negate with carry: Rd = -Rm - NOT C."),
    ("nop", "No operation."),
    ("not", "Bitwise NOT of each vector element."),
    ("orn", "Bitwise OR NOT: Rd = Rn OR NOT Rm."),
    (
        "orr",
        "Bitwise OR with a register or a bitmask immediate. `mov x0, x1` is `orr` with the zero register.",
    ),
    (
        "paciasp",
        "Sign the return address in x30 with key A and sp, at function entry. Pairs with `autiasp` or `retaa`.",
    ),
    (
        "pacibsp",
        "Sign the return address in x30 with key B and sp, at function entry. Pairs with `autibsp` or `retab`; Apple's arm64e uses key B.",
    ),
    (
        "pmull",
        "Polynomial (carry-less) multiply of vector elements, as in GHASH and CRC folding.",
    ),
    (
        "prfm",
        "Prefetch memory into a cache level: `prfm pldl1keep, [x0]`. A hint; it never faults.",
    ),
    ("rbit", "Reverse the order of the bits."),
    (
        "ret",
        "Return: branch to the address in x30 (lr), or another register if given.",
    ),
    (
        "retaa",
        "Authenticate the return address with key A and sp, then return.",
    ),
    (
        "retab",
        "Authenticate the return address with key B and sp, then return.",
    ),
    ("rev", "Reverse the bytes: swap endianness."),
    ("rev16", "Reverse the bytes in each halfword."),
    ("rev32", "Reverse the bytes in each word."),
    (
        "rev64",
        "Reverse the elements in each doubleword of a vector.",
    ),
    ("ror", "Rotate right."),
    ("rorv", "Rotate right by a register amount."),
    (
        "sbc",
        "Subtract with carry: Rd = Rn - Rm - NOT C. Chains multi-word subtractions.",
    ),
    ("sbcs", "Subtract with carry, and set the flags."),
    (
        "sbfiz",
        "Signed bitfield insert in zero: move the low bits of Rn to a position in Rd, sign-extended, zeroing below.",
    ),
    (
        "sbfm",
        "Signed bitfield move, the instruction behind `sbfx`, `sbfiz`, `asr`, and `sxtb`.",
    ),
    (
        "sbfx",
        "Signed bitfield extract: copy a field of Rn to the low bits of Rd, sign-extended.",
    ),
    ("scvtf", "Convert a signed integer to floating-point."),
    (
        "sdiv",
        "Signed divide, rounding toward zero. Dividing by zero gives 0, without a trap.",
    ),
    ("sev", "Send an event to all cores, waking any in `wfe`."),
    ("sevl", "Send an event to this core only."),
    ("sha256h", "SHA-256 hash update, first part."),
    ("shl", "Shift each vector element left by an immediate."),
    (
        "shrn",
        "Shift each element right and narrow it to half width.",
    ),
    (
        "smaddl",
        "Signed multiply-add long: Xd = Xa + Wn * Wm, with a 64-bit product.",
    ),
    ("smaxv", "Signed maximum across a vector."),
    ("sminv", "Signed minimum across a vector."),
    (
        "smov",
        "Move a vector element to a general register, sign-extended.",
    ),
    (
        "smsubl",
        "Signed multiply-subtract long: Xd = Xa - Wn * Wm.",
    ),
    (
        "smulh",
        "Signed multiply high: the top 64 bits of the 128-bit product.",
    ),
    (
        "smull",
        "Signed multiply long: Xd = Wn * Wm, a 64-bit product.",
    ),
    (
        "sqadd",
        "Signed saturating add: results clamp to the type's range.",
    ),
    ("sqsub", "Signed saturating subtract."),
    (
        "sri",
        "Shift each element right and insert, keeping the destination's top bits.",
    ),
    (
        "sshll",
        "Signed shift left long: widen each element and shift it.",
    ),
    ("sshr", "Signed shift right of each vector element."),
    (
        "st1",
        "Store one to four vector registers to consecutive memory, or one element.",
    ),
    (
        "st2",
        "Store two vector registers, interleaving their elements.",
    ),
    (
        "st3",
        "Store three vector registers, interleaving their elements.",
    ),
    (
        "st4",
        "Store four vector registers, interleaving their elements.",
    ),
    (
        "stadd",
        "Atomically add to memory, without returning the old value.",
    ),
    (
        "stlr",
        "Store-release: every earlier memory access is observed before this store.",
    ),
    ("stlrb", "Store-release a byte."),
    ("stlrh", "Store-release a halfword."),
    (
        "stlxp",
        "Store-release exclusive pair: completes `ldaxp`. The status register is 0 on success, 1 if the reservation was lost.",
    ),
    (
        "stlxr",
        "Store-release exclusive: completes `ldaxr`. The status register is 0 on success, 1 if the reservation was lost.",
    ),
    (
        "stnp",
        "Store a pair of registers with a hint that the data won't be reused soon.",
    ),
    (
        "stp",
        "Store a pair of registers to two consecutive memory locations. `stp x29, x30, [sp, #-16]!` pushes the frame record.",
    ),
    (
        "str",
        "Store a register to memory, with the same addressing modes as `ldr`.",
    ),
    ("strb", "Store the low byte of a register."),
    ("strh", "Store the low halfword of a register."),
    (
        "stur",
        "Store a register with an unscaled offset from -256 to 255.",
    ),
    ("sturb", "Store a byte with an unscaled offset."),
    ("sturh", "Store a halfword with an unscaled offset."),
    (
        "stxp",
        "Store exclusive pair: completes `ldxp`. The status register is 0 on success.",
    ),
    (
        "stxr",
        "Store exclusive: completes `ldxr`. The status register is 0 on success, 1 if another core got there first; loop and retry.",
    ),
    (
        "sub",
        "Subtract: Rd = Rn - an immediate, or a register optionally shifted or extended.",
    ),
    (
        "subs",
        "Subtract and set the flags. `cmp` is `subs` into the zero register.",
    ),
    (
        "svc",
        "Supervisor call: enter the kernel. On Darwin, `svc #0x80` with the syscall number in x16; errors set the carry flag. On Linux, `svc #0` with the number in x8; errors come back as -errno in x0.",
    ),
    (
        "swp",
        "Atomically swap a register with memory, returning the old value.",
    ),
    ("sxtb", "Sign-extend the low byte."),
    ("sxth", "Sign-extend the low halfword."),
    ("sxtl", "Sign-extend each element to twice its width."),
    ("sxtw", "Sign-extend the low word to 64 bits."),
    (
        "sys",
        "A system instruction by its encoding fields: what `dc`, `ic`, `at`, and `tlbi` are written as.",
    ),
    (
        "tbl",
        "Table lookup: each byte of the index vector picks a byte from one to four table registers, 0 if out of range.",
    ),
    (
        "tbnz",
        "Test a bit and branch if it's 1, within 32 KB: `tbnz w0, #3, label`.",
    ),
    (
        "tbx",
        "Table lookup that keeps the destination byte when an index is out of range.",
    ),
    ("tbz", "Test a bit and branch if it's 0, within 32 KB."),
    ("tlbi", "Invalidate TLB entries. An alias of `sys`."),
    (
        "trn1",
        "Transpose: interleave the even elements of two vectors.",
    ),
    (
        "trn2",
        "Transpose: interleave the odd elements of two vectors.",
    ),
    (
        "tst",
        "Test bits: set N and Z from Rn AND operand. An alias of `ands` into the zero register.",
    ),
    ("uabd", "Unsigned absolute difference of vector elements."),
    (
        "uaddlv",
        "Unsigned add across a vector into a wider scalar.",
    ),
    (
        "ubfiz",
        "Unsigned bitfield insert in zero: move the low bits of Rn to a position in Rd, zeroing the rest.",
    ),
    (
        "ubfm",
        "Unsigned bitfield move, the instruction behind `ubfx`, `ubfiz`, `lsl`, `lsr`, and `uxtb`.",
    ),
    (
        "ubfx",
        "Unsigned bitfield extract: copy a field of Rn to the low bits of Rd, zero-extended: `ubfx Rd, Rn, #lsb, #width`.",
    ),
    ("ucvtf", "Convert an unsigned integer to floating-point."),
    (
        "udf",
        "Permanently undefined: always traps. Marks code that must never run.",
    ),
    (
        "udiv",
        "Unsigned divide. Dividing by zero gives 0, without a trap.",
    ),
    (
        "umaddl",
        "Unsigned multiply-add long: Xd = Xa + Wn * Wm, with a 64-bit product.",
    ),
    ("umaxv", "Unsigned maximum across a vector."),
    ("uminv", "Unsigned minimum across a vector."),
    (
        "umlal",
        "Unsigned multiply-add long into wider vector elements.",
    ),
    (
        "umov",
        "Move a vector element to a general register, zero-extended. `mov w0, v0.s[1]` is `umov`.",
    ),
    (
        "umsubl",
        "Unsigned multiply-subtract long: Xd = Xa - Wn * Wm.",
    ),
    (
        "umulh",
        "Unsigned multiply high: the top 64 bits of the 128-bit product.",
    ),
    (
        "umull",
        "Unsigned multiply long: Xd = Wn * Wm, a 64-bit product.",
    ),
    (
        "uqadd",
        "Unsigned saturating add: results clamp to the type's range.",
    ),
    (
        "uqsub",
        "Unsigned saturating subtract: results clamp at zero.",
    ),
    (
        "urhadd",
        "Unsigned rounding halving add: (a + b + 1) / 2 per element, without overflow.",
    ),
    (
        "ushll",
        "Unsigned shift left long: widen each element and shift it.",
    ),
    ("ushr", "Unsigned shift right of each vector element."),
    ("uxtb", "Zero-extend the low byte."),
    ("uxth", "Zero-extend the low halfword."),
    ("uxtl", "Zero-extend each element to twice its width."),
    (
        "uzp1",
        "Unzip: the even elements of two vectors, concatenated.",
    ),
    (
        "uzp2",
        "Unzip: the odd elements of two vectors, concatenated.",
    ),
    (
        "wfe",
        "Wait for an event: sleep until `sev` or an interrupt. Used in spin locks.",
    ),
    ("wfi", "Wait for an interrupt."),
    (
        "xpaclri",
        "Strip the pointer authentication code from x30, without checking it.",
    ),
    (
        "xtn",
        "Narrow each element to half width, keeping the low half.",
    ),
    (
        "yield",
        "Hint that this thread is spinning and another may run.",
    ),
    ("zip1", "Zip: interleave the low halves of two vectors."),
    ("zip2", "Zip: interleave the high halves of two vectors."),
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn descriptions_are_sorted_and_name_real_instructions() {
        for pair in DESCRIPTIONS.windows(2) {
            assert!(pair[0].0 < pair[1].0, "{} before {}", pair[0].0, pair[1].0);
        }
        for (name, text) in DESCRIPTIONS {
            assert!(crate::is_instruction(name), "{name} isn't an instruction");
            assert!(text.ends_with('.') || text.ends_with(')'), "{name}: {text}");
            assert!(!text.contains('\u{2014}'), "{name}: no em dashes");
        }
        assert!(DESCRIPTIONS.len() >= 250, "{}", DESCRIPTIONS.len());
        assert!(instruction_description("ldp").is_some_and(|d| d.contains("pair")));
        assert!(instruction_description("frob").is_none());
    }
}
