# 0001. One workspace for the whole suite, on a shared core

- **Status:** Accepted
- **Date:** 2026-10-26

## Context

The goal is a suite of AArch64 tools: a language server first, then a linter, and later a formatter. Each needs to parse assembly, resolve symbols and macros, and know the instruction set.

## Decision

- One repository, `arm-tools`, holding one Cargo workspace.
- The shared work lives in library crates: `arm-syntax`, `arm-isa`, `arm-sema`, `arm-lint`. The binaries `armls` and `armtool` are thin layers over them.
- Lint rules run inside the language server as diagnostics, and from `armtool lint` in CI. A rule is written once.
- The language server ships first. The linter follows on the same core.

## Consequences

- The LSP work builds the parser and semantic model the linter needs, so the linter starts with most of its foundation done.
- The crate boundaries are a contract between layers; changes to the core can break both tools, so integration tests cover both.

## Alternatives considered

- **Build the LSP alone, extract a core later.** Faster first demo, but the extraction is a rewrite of the parts that matter most.
- **A repo per tool.** They'd share code through published crates, which slows every change to the core.
