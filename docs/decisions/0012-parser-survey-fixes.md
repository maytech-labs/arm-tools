# 0012. Macros, bodies, and ELF syntax, from surveying real assembly against clang

- **Status:** Accepted
- **Date:** 2026-10-26

## Context

[0011](0011-parser-grammar.md) was built from hand-made probes. To test it on real code, `cargo xtask survey` parses every `.s` file under a path and checks that each line we report as an error is one clang rejects. The first run, on LLVM's 2,383 AArch64 MC test files and the HelloSilicon book code, found 6,775 lines we flagged that clang accepts. They came from five gaps, each confirmed with clang:

- **Macro calls and bodies.** `do_add 8h` pastes `8h` into `v0.\sz`. Clang doesn't parse a macro or `.rept`/`.irp`/`.irpc` body until it expands it, so `mov x0 x1 x2` inside an unused macro is fine. A macro must be defined before its first call, and names are case-sensitive.
- **SME matrix indexes:** `za.s[w8, 0:1, vgx2]`.
- **Register writeback:** `cpyfp [x0]!, [x1]!, x2!`.
- **Variants with arguments, and after parentheses or quoted names:** `_g@AUTH(ia,42)`, `(_foo + 1)@PAGE`, `"+[T m].v"@PAGEOFF`.
- **ELF relocation specifiers:** `#:lo12:sym`, `:got:sym`, `%pltpcrel(sym)`. Clang parses them on macOS and only fails when writing Mach-O ("unknown AArch64 fixup kind!"), with no useful location. [0004](0004-apple-aarch64-first.md) is right that clang rejects them, but it's not a syntax error.
- **Register aliases:** `fred .req x5`.

## Decision

This supersedes 0011's "Macro calls parse as instructions".

- **The parser tracks `.macro` definitions in order.** A call to a macro defined earlier in the file is a `MacroCall` node with lenient arguments. A call before the definition is an `Instruction`, as clang sees it. Macros from `.include` aren't known to the parser; sema handles them.
- **Bodies of `.macro`, `.rept`, `.irp`, and `.irpc` are not checked.** Their statements still get nodes (labels, directives, `Instruction` with an `ArgList` instead of an `OperandList`), so the outline and navigation work, but no syntax errors are reported until a later layer expands them.
- **A number followed by letters is two tokens,** as in LLVM's lexer, not an "invalid number" error. `mov x0, 12abc` is still an error, at `abc`.
- New nodes: `Writeback`, `Specifier`, `RegisterAlias`, `MacroCall`. `Indexed` takes a list with `a:b` ranges. `Variant` takes parenthesized arguments.
- **ELF specifiers parse cleanly.** Sema will report them with a message that names the Mach-O form to use (`@PAGEOFF` for `:lo12:`), which is better than clang's.
- **`cargo xtask survey` is part of the workflow** for parser changes. The bar is zero disagreements.

## Consequences

- After the fixes: 2,446 files, 79 lines flagged, all rejected by clang (69 in the file, 10 more on their own, where clang had stopped reporting partway through the file); 0 disagreements.
- Errors inside unexpanded macros wait for sema. That matches clang, which reports them only at the call site.
- The survey corpus isn't checked in (LLVM's tests are large and Apache-2.0); AGENTS.md says how to fetch it. Five HelloSilicon files (MIT) are checked in with their license.

## Alternatives considered

- **Check macro bodies as if they were plain code.** Reports errors in valid files.
- **Reject ELF specifiers in the parser.** Clang doesn't reject them at parse time either, and sema can give a better message.
- **Check in LLVM's MC tests.** Thousands of files, many deliberately invalid, under a different license. Surveying them on demand gives the same signal.
