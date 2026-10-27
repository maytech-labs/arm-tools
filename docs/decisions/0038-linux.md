# 0038. AArch64 Linux as a second platform

- **Status:** Accepted
- **Date:** 2026-10-27
- **Extends:** [0004](0004-apple-aarch64-first.md), which planned for this

## Context

Apple silicon stays the main audience, but the same instruction set is assembled for Linux (servers, Raspberry Pi, Android's userland, CI on Arm runners) with different conventions. Clang assembles both, so it can be the oracle for both: `clang --target=aarch64-linux-gnu` works with Apple's clang, no Linux machine needed.

What differs, probed with Apple clang (LLVM 21) on 2026-10-27:

- `;` separates statements on ELF; `//` and `#` at a line start are comments. `%%` is an error.
- Operator precedence is GNU's (`getGNUBinOpPrecedence`): shifts bind like `*`, and `|`, `&`, `^` bind tighter than `+` and `-`. `1 << 2 + 1` is 5 on Linux and 8 on Darwin.
- Relocations are ELF specifiers: `adrp x0, sym`, `:lo12:sym`, `:got:sym`, `:got_lo12:sym`. `@PAGE` and friends are errors. Darwin rejects ELF specifiers on symbols, but accepts them on absolute constants (`#:abs_g0:limit`).
- Directives: 211 for ELF against Darwin's 254 (`.type`, `.size`, `.hidden`, `.weak` on one side; `.subsections_via_symbols`, `.zerofill`, `.loh` on the other), probed by `cargo xtask gen-isa` like Darwin's.
- Clang's default CPU is generic Armv8-A with FP and NEON, not apple-m1: LSE, CRC, AES, and RCpc need `.arch` or `-march`.
- System calls take their number in x8 with `svc #0`; Darwin's take x16 and `svc #0x80`. x18 is free on Linux; Darwin reserves it.
- Clang's `.S` preprocessing defines `__linux__` (and `linux`) instead of `__APPLE__`.

## Decision

- `arm_syntax::Platform` is `Darwin` or `Linux`. The lexer, parser (separators, precedence), formatter, directive tables, default features, relocation checks, preprocessor target, lint rules, and editor features take it.
- Which platform a file is for, in order: `[target] platform = "linux"` in `armtool.toml`; a comment `armtool: platform=linux` in the file; what the file gives away (`@PAGE`, `__TEXT`, `.subsections_via_symbols` for Darwin; `:lo12:`, `.type`, `.section .text`, `%function` for Linux, when it shows only one); then Darwin.
- Darwin stays the default rather than the host OS, so a project lints the same on a Mac and on Linux CI.
- Using the other platform's relocation syntax or directives is an error that names the platform it belongs to and how to switch (`wrong-platform`, and `unknown-directive` with a note).
- `cargo xtask survey --linux` checks against `clang --target=aarch64-linux-gnu`. On LLVM's AArch64 MC tests, both platforms have zero disagreements; recall is 76.6% on Linux and 71.2% on Darwin. Fixtures in `crates/sema/tests/data/linux/` are checked against clang in both directions.
- 32-bit Arm, Windows on Arm, and bare-metal-specific rules are out of scope.

## Consequences

- A file with no platform clues and no config is read as Darwin; on Linux, its `;` would be a comment. Linux projects should set `[target] platform`.
- `.S` files on a Linux host are preprocessed with `clang`, else `cc`.
