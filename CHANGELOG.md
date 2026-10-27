# Changelog

All notable changes are listed here. The format follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and versions follow [Semantic Versioning](https://semver.org/).

## Unreleased

### Added

- Cargo workspace: `arm-syntax`, `arm-isa`, `arm-sema`, `arm-lint`, `arm-lsp` (`armls`), `arm-cli` (`armtool`), and `xtask`.
- `armls` and `armtool` exit with code 3 and say they aren't built yet.
- `arm-syntax` lexer for Apple AArch64 assembly: lossless tokens, comments, `%%` separators, numbers, local label references, strings, and errors with ranges.
- `arm-syntax` parser: a lossless rowan tree with labels, directives, instructions, assignments, structured operands, and expressions with Apple's precedence. It never fails; errors say what was expected.
- `arm-syntax` typed AST: statements, labels, instructions, directives, assignments, and names with `@` variants.
- Corpus and oracle tests: compiler output and hand-written code must parse cleanly and assemble with clang; known-bad lines must be reported and rejected by clang.
- Examples: `tree`, `check`, and `bench` in `crates/syntax/examples/`.
- Parser: macro calls (for macros defined earlier in the file), unchecked `.macro`/`.rept`/`.irp`/`.irpc` bodies, `.req` register aliases, register writeback (`x2!`), SME index ranges (`za.s[w8, 0:1]`), ELF relocation specifiers, and `@` variants with arguments or after parentheses and quoted names.
- `cargo xtask survey <path>...`: checks that every line arm-syntax reports is one clang rejects.
- HelloSilicon samples (MIT) in the parser corpus.
- `arm-isa`: instruction tables generated from LLVM 21.1.8's assembler matcher: 20,864 forms, 1,659 mnemonics, operand classes with clang's messages, features, and 635 register names with their kinds. Mnemonic splitting as clang does it.
- `cargo xtask gen-isa` to regenerate the tables from an LLVM checkout.
- `arm-isa`: the 254 directives clang accepts on macOS, probed with clang.
- `arm-sema`: per-file symbols, references (`1f`/`1b`, register aliases, macros), and clang's semantic errors with suggestions. A line index for UTF-8 and UTF-16 positions.
- `cargo xtask survey` checks semantic errors as well as syntax.
- `armls`, the language server: diagnostics, hover (instruction forms with required features, register roles in Apple's ABI, symbols, directives), completion, go to definition, references, document highlight, rename, document symbols, and semantic tokens. UTF-8 or UTF-16 positions.
- `make install`, and editor setup for Neovim and Helix in `editors/`.
- `cargo xtask nvim-smoke`: runs armls in headless Neovim.
- Operand checking: every instruction is matched against clang's forms, with clang's messages for out-of-range immediates, misaligned offsets, wrong registers, and operand counts. Hover shows instruction signatures.
- `cargo xtask survey --recall`: how many of clang's errors arm-sema catches.
- `arm-lint`: 11 rules for Apple's ABI, address materialization, and control flow, with `armtool.toml` levels and `; armtool: allow(rule)` comments. Rule reference in docs/lint.md.
- `armtool check`, `lint` (with `--json`), `rules`, and `explain`. Exit codes 0 clean, 1 findings, 2 usage or config problems.
- armls publishes lint findings, using the nearest `armtool.toml`.
- `.include` is followed: included macros, constants, aliases, and labels are known, with go to definition across files and unsaved buffers in armls. `[include] paths` in armtool.toml. Include cycles are errors; missing includes are warnings.
- `armtool fmt` and formatting in armls: aligned mnemonics, operands, and comments, changing only whitespace (proved by byte-identical objects from clang). `[format]` in armtool.toml.
- Quick fixes: suggested instructions, directives, and conditions, ELF directive removal, stack rounding, page-offset variants, and the missing `add` after `adrp`. Code actions in armls; `armtool check --fix` and `lint --fix`.
- Folding ranges in armls: macro, repeat, and conditional blocks, label sections, and comment runs.
- CPU feature checks: "instruction requires: cssc" and friends, following clang's default target (apple-m1), `.arch`, `.cpu`, `.arch_extension`, and `[target]` in armtool.toml. Feature data generated from LLVM.
- Conditional assembly: `.if` and its relatives are evaluated as clang would; skipped branches aren't checked and are greyed out in the editor.
- Macro calls, `.rept`, `.irp`, and `.irpc` are expanded and checked, with clang's argument rules; problems are reported at the call.
- Lint rules `flow/undefined-register` (scratch registers read before they're set, at entry or after a call) and `abi/syscall-number` (svc without x16), by must-dataflow over the control-flow graph.
- `examples/`: six programs that build and run (syscalls, printf, recursion, NEON, macros and includes, the C preprocessor), and a file of labeled mistakes to try armls on.
- Operand errors explain a constant used above its definition, which clang only folds when it's defined first.
- Inlay hints in armls: constant values where they're used, and the syscall a `mov x16, #n` sets up.
- `.S` files go through clang's C preprocessor for diagnostics, mapped back to the lines you wrote.
- A Homebrew formula (`brew install --HEAD arm-tools` from this repo's tap) and a VS Code extension in `editors/vscode`.
- Design doc, roadmap, and ADRs 0001 to 0028.
- `cargo xtask check` and CI on macOS and Linux.
