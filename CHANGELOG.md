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
- `adr` and `adrp` read a label named like a register (`adr x0, s0`) as a label, as clang does; branches still don't.
- Navigation between files that are assembled separately and linked: go to definition, references, and hover on global labels in other files, and workspace symbol search. `.private_extern` labels count as global.
- `examples/`: fourteen programs that build and run (syscalls, printf, recursion, NEON, macros and includes, the C preprocessor), and a file of labeled mistakes to try armls on.
- Operand errors explain a constant used above its definition, which clang only folds when it's defined first.
- Inlay hints in armls: constant values where they're used, and the syscall a `mov x16, #n` sets up.
- `.S` files go through clang's C preprocessor for diagnostics, mapped back to the lines you wrote.
- A Homebrew formula (`brew install --HEAD arm-tools` from this repo's tap) and a VS Code extension in `editors/vscode`.
- Design doc, roadmap, and ADRs 0001 to 0043.
- `cargo xtask check` and CI on macOS and Linux.
- `armls` signature help: the forms an instruction's operands so far could begin, with the operand at the cursor.
- `armls` completion offers what fits the operand: registers of the right width, condition codes, barriers, prefetch operations, shifts and extends, labels (with `@PAGE` and `@PAGEOFF`, and global labels from other files), and constants. Mnemonics show a signature.
- `armls` renames global labels in every file of the project that defines or uses them.
- `armls` document links: the file names in `.include`, and `#include` in `.S` files, open the file.
- `armls` snippets: `func`, `main`, frame record push and pop, `@PAGE` addressing, and Darwin system calls.
- `armls` call hierarchy: incoming and outgoing calls, `bl` and tail calls, across the project's files.
- `armls` indexes C and Objective-C function definitions: go to definition, hover, completion, and call hierarchy reach from `bl _foo` into `foo` in a `.c` or `.m` file.
- A fuzzed edit session for armls: random edits, every request after each, no panics allowed; formatting must change only whitespace and be stable.
- armls answers a bug in a request with an error, and logs a bad or failing notification, instead of exiting.
- armls is faster on large files and projects: semantic tokens and lint no longer slow down quadratically, each edit is analyzed once, queued edits coalesce, and indexing uses every core.
- Hover and completion say what about 270 common instructions do.
- Operand checking, signature help, and completion for `ubfx`, `bfi`, `bfc` and the other bitfield aliases, `lsl` by an immediate, and `dc`, `ic`, `at`, and `tlbi`.
- `cargo xtask nvim-smoke` checks completion, signature help, call hierarchy, inlay hints, document links, formatting, and rename across files in real Neovim.
- Docs: README, design, and examples describe the finished language server.
- AArch64 Linux: `[target] platform = "linux"` (or a file that looks like Linux) gets ELF conventions: `;` separators, GNU precedence, ELF directives and relocations, the generic CPU, and `.S` preprocessing for Linux. Checked against `clang --target=aarch64-linux-gnu` with `cargo xtask survey --linux`.
- SVE condition names (`b.none`, `csel ..., any`), labels named like shifts (`bl lsl`), and ELF specifiers on constants are no longer reported.
- Lint on Linux: x18 and frame-pointer rules are Darwin only, `adrp` pairs with `:lo12:` and the GOT and TLS specifiers, and `svc #0` takes its number in x8.
- armls on Linux: register hover, syscall names (x8, `svc #0`), snippets, completion and signatures in ELF syntax (`:lo12:`, bare `adrp` labels), and C functions under their unprefixed names.
- Linux examples in examples/linux, built and run on an Arm Linux CI runner.
- Lint: LSE atomics (`ldadd`, `swp`, `cas`) read and write the right registers, and a function that never returns, like `_start`, may change callee-saved registers.
- Lint: `abi/callee-saved-restore` finds a saved register returned changed on some path, and `flow/undefined-register` now covers x2 to x7 and v4 to v7 after calls to other files' functions.
- Lint on Linux: `elf/gnu-stack` (no `.note.GNU-stack`, so an executable stack) and `elf/function-symbol` (exported functions without `.type` or `.size`), each with a quick fix.
- Lint: `flow/flags-clobbered` finds condition-flag reads (`b.eq`, `csel`, `adc`) after a call that may have changed the flags.
- Types for assembly: `armtool:` comments declare signatures and register types, and five `types/` rules check calls, signedness, memory access, and pointer width. Hover shows a register's type. See docs/types.md.
- Types from C: calls are checked against the project's C prototypes and common C library functions, and printf-family calls against their format strings (`types/format`), including Darwin's stack-passed variadic arguments.
- Structs in types: `armtool: struct name { ... }` with C's layout; loads, stores, and pointer arithmetic through a struct pointer are checked against its fields, and `types/layout` checks `struct_field` offset constants.
- Structs from C: struct and typedef definitions in the project's C files type the assembly that uses them, with no annotation.
- Quick fixes for type findings: insert `sxtw`, swap a signed condition for its unsigned counterpart (or `sdiv` and `udiv`), use X registers for a pointer, and correct an offset constant.
- Lint: `flow/dead-store` finds register writes nothing reads. The dataflow now knows that `movk`, `bfi`, accumulating NEON instructions, and lane moves read their destination, and that vector lists (`{v0.4s, v1.4s}`) are read by stores and written by loads.
- A docs site, built from docs/ with mdBook and published to GitHub Pages: getting started, the editor, the command line, configuration, platforms, lint rules, types, and the design and decisions.
- More of clang's errors caught, 92.5% on Darwin and 93.7% on Linux (from 71% and 77%): SME array operands and offset ranges, system register names for mrs and msr, and the instruction after movprfx. `cargo xtask survey --misses` lists what's still missed.
