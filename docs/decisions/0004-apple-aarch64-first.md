# 0004. Apple AArch64 first, with clang as the source of truth

- **Status:** Accepted
- **Date:** 2026-10-26

## Context

"ARM assembly" covers AArch64 and AArch32, and several dialects: Apple's (LLVM's integrated assembler with Darwin settings), GNU `as`, and LLVM's ELF flavor. They differ in comments, separators, relocation syntax, and directives. Supporting all of them at once multiplies the work before anything ships.

## Decision

- Phase 1 targets AArch64 as accepted by `clang -c -arch arm64` on macOS, producing Mach-O.
- Apple's clang is the source of truth. When behavior is unclear, probe clang and record the result as a test.
- The lexer and parser stay dialect-aware internally, so GNU/ELF AArch64 can be added later as an option, not a rewrite.

Probed with Apple clang 21.0.0 on 2026-10-26:

- `;`, `//`, `/* */`, and `#` at the start of a line are comments.
- `%%` separates statements. `;` does not.
- `_g@PAGE`, `_g@PAGEOFF` work; `:lo12:_g` is rejected ("unknown AArch64 fixup kind").
- `@` as a comment is rejected.
- Writing x18 is accepted by the assembler; reserving it is an ABI rule, so it's a lint, not a syntax error.
- Immediates work with or without `#`. Mnemonics and registers are case-insensitive.

## Consequences

- Diagnostics match what the user's build will say.
- Linux and GNU users wait for a later dialect option.

## Alternatives considered

- **GNU `as` syntax first.** More tutorials use it, but the target platform here is the Mac.
- **Support every dialect from day one.** Too much surface before the first useful release.
