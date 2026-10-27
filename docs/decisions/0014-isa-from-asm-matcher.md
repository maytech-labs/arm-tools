# 0014. gen-isa reads clang's assembler matcher, not TableGen's JSON records

- **Status:** Accepted
- **Date:** 2026-10-26

## Context

[0013](0013-isa-from-llvm-tablegen.md) chose LLVM TableGen as the source, and the spike read `--dump-json` records. Building the generator showed a better input: `llvm-tblgen --gen-asm-matcher` writes the matcher clang's assembler runs. Instruction records describe encodings; the matcher describes what the assembler accepts, which is what diagnostics need.

The matcher output for LLVM 21.1.8 has:

- 20,864 match entries: mnemonic, opcode, required feature set, and the operand class sequence, for the generic syntax (`MatchTable0`) and Apple's legacy NEON syntax (`MatchTable1`). Aliases are already expanded, and memory operands are flattened to tokens (`[`, `GPR64sp`, `SImm7s8`, `]`, `!`).
- 1,025 operand classes: tokens, register classes, and custom classes with a predicate (`isImmInRange<0,63>`) and a diagnostic kind (`InvalidImm0_63`).
- The subclass relation between classes, as a bit table.
- Feature sets and their `-march` names.
- The match class of every register.

What it lacks is in `AArch64AsmParser.cpp`: the operand error messages (`showMatchError`), and the register names clang matches in C++ before the generated matcher (`v0`, `z0`, `p0`, `pn8`, `za0h.s`, `zt0`, and the aliases `fp`, `lr`, `x31`, `w31`). Those functions are regular `case` and `.Case("v0", AArch64::Q0)` lists.

## Decision

- `cargo xtask gen-isa --llvm <checkout> --tblgen <llvm-tblgen>` runs `--gen-asm-matcher` and `--gen-register-info` (for register numbers), reads `AArch64AsmParser.cpp` for messages and register names, and writes `crates/isa/src/generated.rs`. No JSON, so no `serde_json`.
- The parsing is strict: an unexpected line is an error, so a layout change in a new LLVM fails the generator instead of producing wrong tables.
- Output is deterministic, so a regeneration with no LLVM change is an empty diff.
- Register names carry a kind (scalar, NEON vector, SVE data, SVE predicate, predicate as counter, matrix, tile list, lookup table), because clang decides the kind before matching and an operand slot accepts only some kinds. A name in an earlier C++ table shadows its scalar reading, as in `matchRegisterNameAlias`: `z0` is never a scalar register.
- How clang splits instruction names before matching (`bne` to `b.ne`, `b.eq` to `b` `.` `eq`, `fadd.4s` to `fadd` `.4s`, SYS and bitfield aliases handled in C++) is ported by hand in `arm_isa::Mnemonic`, with tests.
- `arm-isa` is licensed `MIT AND Apache-2.0 WITH LLVM-exception`, with LLVM's license in `crates/isa/LICENSE-LLVM.txt`, because its tables and the predicates ported next are derived from LLVM.

## Consequences

- Operand matching can follow clang exactly: the same classes, in the same order, with the same subclass rules.
- The predicates (83 families, like `isImmInRange` and `isSImmScaled`) and clang's operand model (how `v0.16b` or `[x0, #8]!` become operands) must be ported from C++. That's the next step.
- The generated file is 2.6 MB of Rust and compiles in about a second.
- Binaries that include `arm-isa` carry LLVM-derived code. The LLVM exception waives Apache-2.0's attribution requirement for compiled binaries, so `armls` and `armtool` can ship as usual.

## Alternatives considered

- **TableGen JSON records** (0013's spike). Aliases, operand classes, and syntax variants would all have to be reinterpreted, repeating what the matcher generator already does.
- **Parse `AArch64AsmParser.cpp` more broadly.** Its logic isn't data; only the regular tables are read, and the rest is ported by hand.
