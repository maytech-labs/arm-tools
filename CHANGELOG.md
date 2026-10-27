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
- Design doc, roadmap, and ADRs 0001 to 0016.
- `cargo xtask check` and CI on macOS and Linux.
