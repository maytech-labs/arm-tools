# 0013. Instruction tables from LLVM 21 TableGen, with our own hover text

- **Status:** Accepted
- **Date:** 2026-10-26

## Context

[0010](0010-isa-source.md) proposed LLVM TableGen for operand forms and Arm's spec for descriptions, pending a spike. The spike, on 2026-10-26:

- `llvm-tblgen --dump-json` (Homebrew LLVM 21.1.8) on `llvm/lib/Target/AArch64/AArch64.td` at tag `llvmorg-21.1.8` produces 89 MB of records. LLVM main's `.td` files don't load with the 21.1.8 tool, so the tag must match the tool.
- 9,000 `Instruction` records: 7,407 assembler-visible, 1,593 codegen-only or pseudo. 3,025 `InstAlias` records. 1,533 distinct mnemonics, and 306 more that exist only as aliases.
- Each record has the assembly string, typed operands (`GPR64sp:$Rn`, `addsub_shifted_imm64:$imm`), and the features it needs. Assembly strings carry two variants: the generic syntax and Apple's legacy NEON syntax (`fadd.4s v0, v1, v2`).
- System operand tables are records too: 1,254 system registers, plus the DC, AT, IC, PRFM, barrier, BTI, and SVE predicate pattern names.
- **Coverage:** 3,669 mnemonic uses in LLVM 21's own valid MC tests and our corpus. 97.7% are found as a first-variant mnemonic, `b.<cond>`, or a SYS alias (`tlbi`, `dc`, `at`, `ic`). The rest are Apple's legacy NEON syntax (in the second variant), about ten aliases LLVM's C++ parser builds (`ubfx`, `ubfiz`, `sbfx`, `sbfiz`, `bfi`, `bfxil`, `bfc`, `bne`-style branches), and macros from our test files. That's effectively full coverage.
- **Descriptions:** Arm's open machine-readable spec (AARCHMRS, BSD) omits all descriptive content. The prose is only in the XML under Arm's proprietary license, which we can't redistribute in an MIT project.

## Decision

- Operand forms, aliases, features, and system operand names come from LLVM TableGen, pinned to the LLVM tag that matches Apple's clang (21.1.8 today). Both assembly variants are kept.
- The aliases LLVM's C++ parser builds, and the SYS alias syntax, are a short hand-written table in `arm-isa`, each with a test against clang.
- `cargo xtask gen-isa --llvm <checkout> --tblgen <path>` reads the JSON and writes compact Rust tables into `crates/isa/src/generated/`, checked in. Normal builds need no LLVM. JSON parsing (`serde_json`) is an xtask dependency only.
- Hover text is ours: one or two lines per mnemonic, written in `crates/isa/data/descriptions.toml`, plus the operand forms and required features from the tables, and a link to the instruction's page on developer.arm.com. Mnemonics without a written description still show forms and features.
- When Apple's clang moves to a new LLVM, regenerate from the matching tag. The clang oracle tests catch where Apple's fork differs.

## Consequences

- Operand checking matches clang's assembler, aliases included.
- Writing descriptions is ongoing work. Start with the base integer, load/store, branch, and common NEON instructions, which cover most real code.
- The generator depends on LLVM's internal record layout, which can change between versions. It's xtask code, so breakage shows up when regenerating, not in users' builds.

## Alternatives considered

- **Arm's proprietary XML for descriptions.** Best text, but its license doesn't allow redistribution here.
- **Arm's open AARCHMRS for forms.** Spec-accurate, but no prose either, and it doesn't know clang's aliases and quirks.
- **Generate at build time from LLVM.** Every build would need LLVM and its sources.
