# 0037. Checking the aliases clang handles in C++

- **Status:** Accepted
- **Date:** 2026-10-27

## Context

Clang checks a few instructions in C++ before its match table, so the tables don't describe them, and operand matching skipped them ([0017](0017-operand-matching.md)): the bitfield aliases (`ubfx`, `sbfx`, `bfxil`, `ubfiz`, `sbfiz`, `bfi`, `bfc`), `lsl` by an immediate, and the SYS aliases (`dc`, `ic`, `at`, `tlbi`, `cfp`, `dvp`, `cpp`, `cosp`).

## Decision

- **Bitfield aliases and `lsl #imm`** follow `AArch64AsmParser::MatchAndEmitInstruction`: the same range checks in the same order, then the rewrite to `ubfm`, `sbfm`, or `bfm`, which goes through the matcher like any instruction, so register errors land where clang puts them. Wrong operand counts, and immediates clang can't fold while parsing (a symbol, a constant set below the line), are errors, as clang reports "unrecognized instruction mnemonic" for them. Expressions we can't evaluate aren't judged.
- **Messages give the real range.** Clang says "expected integer in range [0, 31]" and "[1, 32]" even for 64-bit registers; armls says [0, 63] and [1, 64] there.
- **SYS aliases** check what's certain: the operation is a name, the register is an X register or xzr, there's at most one, and for operations in our hand-written list (`arm_isa::sys_operations`: cache, address translation, and common TLB operations), whether a register is required or not allowed. Clang knows many more operations (MTE, RME, range TLBI, nXS); names not in our list aren't judged, so a missing entry never produces a false error.
- `tlbip` stays unchecked.
- Every case is a fixture checked against clang: valid lines in `crates/sema/tests/data/valid/aliases.s`, each error in its own file under `invalid/`.
- Editors get a signature for each alias (`arm_isa::alias_signature`) and complete SYS operation names, registers, and constants for their operands.

## Consequences

- Unknown SYS operation names, and MTE operations used without the feature, aren't reported.
