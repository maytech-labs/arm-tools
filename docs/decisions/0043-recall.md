# 0043. Catching more of clang's errors: SME arrays, system registers, and movprfx

- **Status:** Accepted
- **Date:** 2026-10-27

## Context

`cargo xtask survey --recall` measures how many of clang's errors on LLVM's AArch64 MC tests armtool also reports. It stood at 71.2% on Darwin and 76.6% on Linux. A new `--misses` mode lists clang's message on every line we miss; grouped, three causes covered most of the gap.

## Decision

- **SME array operands** (`za.d[w8, 0, vgx2]`, `za.s[w8, 0:3]`) are flattened as clang reads them: the matrix, `[`, the index register, the offset, `]`. `vgx2` and `vgx4` are dropped, as clang drops them. An offset range is a new operand, `ImmRange`, checked by `isUImmScaled`'s range rules (the last value must be the first plus the class's offset; a range fits only range classes). A range is read only when written as two integers, as `tryParseImmRange` requires. Before this, these operands weren't matched at all, so neither were their CPU feature requirements.
- **System registers** come from LLVM's `SysReg` records, generated into `SYSTEM_REGISTERS` by `cargo xtask gen-isa` with readable, writable, and whether a feature gates them. `mrs` wants a readable name, `msr` a writable one, or the generic `s<op0>_<op1>_c<n>_c<m>_<op2>`. A gated name isn't judged in its allowed direction, since predicates don't see the feature set; the wrong direction is always an error, since without the feature clang doesn't know the name.
- **movprfx:** each instruction's matched form is kept, and `gen-isa` emits each SVE instruction's TSFlags as `PREFIX_FLAGS`: destructive or not, and element size. As clang's `validateInstruction` does, the instruction after a movprfx must be destructive and write the same register, and after a predicated movprfx, use the same governing predicate and element size. Clang's other check, the destination used as a non-tied source, needs operand tying we don't model, so it's left out.

## Consequences

- Recall is 92.5% on Darwin and 93.7% on Linux, with zero disagreements on both: every new error is one clang reports too.
- What's left is mostly SME and D128 feature errors on forms we still can't read, operand-specific diagnostics, and errors clang raises while writing the object.
