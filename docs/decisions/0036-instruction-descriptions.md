# 0036. Instruction descriptions as a Rust table

- **Status:** Accepted
- **Date:** 2026-10-27

## Context

Hover listed an instruction's forms but never said what it does. LLVM's tables have no prose, and Arm's openly licensed spec has none either ([0013](0013-isa-from-llvm-tablegen.md)). The roadmap planned a `crates/isa/data/descriptions.toml`.

## Decision

- Descriptions are a sorted `&[(&str, &str)]` in `crates/isa/src/descriptions.rs`, read with `arm_isa::instruction_description(head)`. A Rust table needs no TOML parser in arm-isa, and the compiler checks it.
- They cover what people write by hand: integer, logic, bitfield, branch, load/store, atomics, barriers, system, floating-point, common NEON, CRC, AES, and pointer authentication, about 270 mnemonics. One or two sentences each, with Darwin's conventions where they matter (`svc #0x80`, `@PAGE`, key B on arm64e).
- Keyed by the mnemonic's head, so `b.eq` gets `b`'s text, which covers `b.cond`.
- A test checks the table is sorted and every name is a mnemonic clang knows.
- Hover shows the description between the name and the forms; mnemonic completions carry it as documentation.

## Consequences

- SVE, SME, and rarer NEON instructions show forms only, until someone writes theirs.
