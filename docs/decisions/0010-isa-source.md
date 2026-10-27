# 0010. Where the instruction tables come from

- **Status:** Superseded by [0013](0013-isa-from-llvm-tablegen.md)
- **Date:** 2026-10-26

## Context

`arm-isa` needs every A64 instruction's operand forms, constraints, and a description. Writing them by hand is error-prone: there are over a thousand forms with SIMD, SVE, and SME. Two machine-readable sources exist:

- **Arm's A64 ISA XML** (the Exploration Tools release): the architecture spec itself, with encodings, assembler syntax, and description text. Its license needs checking before we redistribute derived tables or description text.
- **LLVM's TableGen records** for AArch64 (`AArch64InstrInfo.td` and friends), dumped with `llvm-tblgen --dump-json`: exactly what clang's assembler accepts, under Apache-2.0 with LLVM exceptions. No prose descriptions. Needs `llvm-tblgen`, which Xcode doesn't ship (Homebrew `llvm` does).

## Decision

Proposed: generate operand forms from LLVM TableGen, since agreeing with clang is the goal ([0004](0004-apple-aarch64-first.md)), and take hover text from Arm's XML if its license allows redistribution, else write short descriptions ourselves. Decide after a spike in M2 that checks coverage of the fixture corpus and the Arm license terms.

The generator runs in `cargo xtask gen-isa`. The output is checked in, so normal builds need no LLVM.

## Consequences

To be filled in when accepted.

## Alternatives considered

- **Hand-written tables.** Fine for the base integer set, unworkable for SIMD and SVE.
- **Arm XML only.** Correct per the spec, but clang accepts aliases and rejects some spec forms; we'd disagree with the user's assembler.
