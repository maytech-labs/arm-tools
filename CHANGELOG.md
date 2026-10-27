# Changelog

All notable changes are listed here. The format follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and versions follow [Semantic Versioning](https://semver.org/).

## Unreleased

## 0.1.0 - 2026-10-27

The first release: a language server, linter, and formatter for AArch64 assembly on macOS and Linux, checked against clang.

### Checking

- Errors as clang reports them, without assembling: syntax, symbols, and every instruction's operands matched against clang's forms, with clang's messages for out-of-range immediates, misaligned offsets, wrong registers, and operand counts. The instruction tables are generated from LLVM 21.1.8's assembler.
- CPU features: "instruction requires: sve2" and friends, following clang's default target, `.arch`, `.cpu`, `.arch_extension`, and `[target]` in `armtool.toml`.
- `.include` is followed, conditional assembly (`.if` and its relatives) is evaluated, and macros, `.rept`, `.irp`, and `.irpc` are expanded and checked, as clang does. `.S` files go through clang's C preprocessor, with problems mapped back to the lines you wrote.
- The aliases clang handles in C++ (`ubfx`, `bfi`, `lsl #imm`, `dc`, `tlbi`), SME array operands, system register names, and the instruction after `movprfx` are checked too.
- On LLVM's AArch64 assembler tests, armtool reports no error clang doesn't, and catches 92.5% of clang's errors on Darwin and 93.7% on Linux.

### Platforms

- Apple's assembly first (`clang -arch arm64`, Mach-O, Apple's ABI), and AArch64 Linux (`clang --target=aarch64-linux-gnu`, ELF): `;` separators, GNU operator precedence, ELF directives and relocations, the generic CPU, Linux system calls, and the AAPCS64. Set `[target] platform`, or let a file's syntax decide.

### Linting

- 25 rules: Apple's ABI (x18, stack alignment, frame records, the link register, callee-saved registers saved and restored), `@PAGE` and `:lo12:` addressing, control flow (unreachable code, fallthrough, unbalanced stacks), dataflow (undefined registers, dead stores, flags clobbered by calls, syscall numbers), ELF bookkeeping on Linux, and types. Levels in `armtool.toml`, and `armtool: allow(rule)` comments.
- Optional types: `armtool:` comments declare function signatures, register types, and structs, and the linter checks calls, signedness, memory access, pointer width, and struct field offsets. Calls into C are checked against the project's C prototypes and structs and common C library functions, and printf calls against their format strings.
- Quick fixes for many findings, in the editor and with `--fix`.

### Language server

- `armls`: diagnostics; hover that says what an instruction does, its forms, and each register's role in the ABI; completion of only what fits each operand; signature help; snippets; go to definition, references, rename, workspace symbols, and call hierarchy across the project's assembly and C files; inlay hints for constants and system calls; formatting; quick fixes; folding; document links; and semantic highlighting. UTF-8 or UTF-16 positions.
- Survives a fuzzed edit session, answers its own bugs with errors instead of exiting, and analyzes a 10,000-line file in about 50 ms.
- Setup for Neovim, Helix, and VS Code in `editors/`.

### Command line

- `armtool check`, `lint`, `fmt`, `rules`, and `explain`, with `--json` and `--fix`. Exit codes: 0 clean, 1 findings, 2 usage or I/O problems.
- `armtool fmt` aligns mnemonics, operands, and comments, changing only whitespace.

### Project

- Example programs for macOS and Linux that build and run, and a file of labeled mistakes.
- A docs site built from `docs/` with mdBook, with the design and 44 decision records.
- CI on macOS, Linux, and Arm Linux; prebuilt binaries attached to each release.
