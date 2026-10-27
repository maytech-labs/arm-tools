# 0007. Unit, integration, and oracle tests

- **Status:** Accepted
- **Date:** 2026-10-26

## Context

A parser for someone else's language is only right if it agrees with the real implementation. Hand-written expectations encode our understanding of Apple's assembler, which can be wrong.

## Decision

- **Unit tests** beside the code drive it test-first.
- **Integration tests** in `crates/<crate>/tests/` pin each crate's public contract, and for the binaries, run the real `armls` over stdio and `armtool` on the command line.
- **Oracle tests** assemble fixtures with `clang -c -arch arm64` and check that our syntax and operand diagnostics agree with clang about which lines are valid. They run where `xcrun clang` exists and skip with a message elsewhere.
- **Snapshot fixtures** for the parser live in `crates/syntax/tests/data/` as `.s` input next to the expected tree, so a reviewer can read them.

## Consequences

- Disagreements with clang show up as test failures, not user bug reports.
- CI runs on macOS to get the oracle, and on Linux to keep the core portable.
- Oracle tests depend on the installed Xcode version; a clang upgrade can change results, and that's a finding worth knowing.

## Alternatives considered

- **Hand-written expectations only.** Cheaper, but encodes our guesses.
- **Oracle only.** Clang tells us valid or invalid, not the tree shape or message we want.
