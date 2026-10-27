# Roadmap

Milestones in order. Each has a "done when" bar. Check items off in the change that finishes them.

## M0: scaffold

Done when the workspace builds, `cargo xtask check` passes, and the docs and decisions are in place.

- [x] Cargo workspace with the six crates and `xtask`
- [x] AGENTS.md, README, CHANGELOG, design, roadmap
- [x] ADRs 0001 to 0010
- [x] CI on macOS and Linux
- [ ] GitHub repo and first push

## M1: syntax

**Done 2026-10-26.** Done when every `.s` file in the fixture corpus round-trips byte for byte, and the parser agrees with clang on which lines have syntax errors.

- [x] Lexer: tokens for every byte, comments, separators, numbers, strings, local label references
- [x] Lossless tree on `rowan`
- [x] Parser: labels, directives, instructions, macro calls, operand expressions
- [x] Error recovery at statement boundaries
- [x] Fixture corpus: hand-written cases and compiler output from `clang -S`
- [x] Hand-written code in the corpus: HelloSilicon samples and macro, alias, and repeat cases
- [x] `cargo xtask survey`: zero disagreements with clang on LLVM's AArch64 MC tests
- [x] Oracle tests against `clang -c -arch arm64`

## M2: ISA

Done when every instruction clang accepts in the corpus matches exactly one form, and a sample of invalid operands gets a message naming the expected form.

- [x] Decide the source of the tables ([0013](decisions/0013-isa-from-llvm-tablegen.md)): spike measured 97.7% mnemonic coverage before aliases
- [x] `cargo xtask gen-isa` and checked-in tables ([0014](decisions/0014-isa-from-asm-matcher.md))
- [x] Mnemonic splitting as clang does it (`bne`, `b.eq`, `fadd.4s`), register names with kinds
- [x] Every corpus mnemonic is known
- [x] Operand model: convert parsed operands to clang's (`v0.16b`, `[x0, #8]!`, lists, shifts)
- [x] Predicates: 378 of 385 ported from AArch64AsmParser.cpp, the rest permissive ([0017](decisions/0017-operand-matching.md))
- [x] Matcher: forms in clang's order and clang's near-miss messages; 0 disagreements with clang on 2,352 files
- [x] CPU feature checks (`instruction requires: sve2`), following `.arch`, `.cpu`, `.arch_extension`, and `[target]` ([0022](decisions/0022-cpu-features.md))
- [x] The aliases clang handles in C++: bitfield aliases, `lsl #imm`, and SYS aliases, each tested against clang ([0037](decisions/0037-cpp-aliases.md))
- [x] Hover text: our descriptions of about 270 common instructions ([0036](decisions/0036-instruction-descriptions.md))

## M3: sema

Done when go to definition would work for every symbol in the corpus, macros expand, and conditional blocks resolve.

- [x] Symbol table and references
- [x] Local numeric labels
- [x] Block structure, duplicate symbols, unknown instructions and directives, with suggestions ([0015](decisions/0015-sema-file-model.md))
- [x] Macros, `.rept`, `.irp`: expansions checked at the call ([0024](decisions/0024-macro-expansion.md))
- [x] Conditional assembly: conditions evaluated, skipped branches greyed out ([0023](decisions/0023-conditional-assembly.md))
- [x] `.include`, with `[include] paths` in armtool.toml ([0019](decisions/0019-includes.md))
- [x] Operand checking against `arm-isa`
- [x] `.S` files: diagnostics through clang's preprocessor, mapped back ([0027](decisions/0027-preprocessed-files.md))

## M4: language server v1

Shipped early, before operand checking ([0016](decisions/0016-armls-v1.md)).

Done when Neovim and VS Code show diagnostics, hover, definition, references, completion, symbols, and semantic tokens on the corpus, with no panics under a fuzzed edit session.

- [x] A fuzzed edit session with no panics (`crates/lsp/src/fuzz.rs`), and a panic guard for what it misses
- [x] Performance measured and fixed on large files and projects ([0035](decisions/0035-performance.md))

- [x] Server loop, document store, position encoding
- [x] Diagnostics, hover, definition, references, symbols
- [x] Completion, semantic tokens
- [x] Rename, document highlight
- [x] Navigation between separately assembled files: definition, references, hover, workspace symbols ([0029](decisions/0029-workspace-index.md))
- [x] Index C function definitions, for calls from assembly into C ([0034](decisions/0034-c-functions.md))
- [x] Folding ranges and quick-fix code actions ([0021](decisions/0021-quick-fixes.md))
- [x] Inlay hints: constant values and syscall names ([0028](decisions/0028-inlay-hints.md))
- [x] Signature help, and completion of what each operand accepts ([0030](decisions/0030-operand-aware-editing.md))
- [x] Rename global labels across files ([0031](decisions/0031-cross-file-rename.md))
- [x] Document links for `.include` and, in `.S` files, `#include`
- [x] Snippets: functions, frame records, `@PAGE` addressing, system calls ([0032](decisions/0032-snippets.md))
- [x] Call hierarchy, across files ([0033](decisions/0033-call-hierarchy.md))
- [x] `editors/` setup for Neovim and Helix, `make install`, and `cargo xtask nvim-smoke`

## M4b: AArch64 Linux

Done when Linux assembly gets the same checks, lint, and editor features as Darwin's, in its own syntax, with zero survey disagreements against `clang --target=aarch64-linux-gnu` ([0038](decisions/0038-linux.md)).

- [x] `Platform`: `;` separators, GNU precedence, ELF directives, the generic CPU, `.S` preprocessing for Linux
- [x] `[target] platform`, a file comment, or the file's own syntax picks it; Darwin otherwise
- [x] Relocation and directive errors that name the other platform
- [x] Lint per platform: x18 and frame pointers are Darwin's; `:lo12:` pairs; syscalls in x8
- [x] Editor: register roles, syscall names, snippets, completion, and C names for Linux
- [x] Linux examples built and run on an Arm Linux CI runner
- [x] Linux-specific lint rules: `elf/gnu-stack` and `elf/function-symbol`, with quick fixes

## M5: linter

Done when the first rules in the design run in `armls` and `armtool lint`, each with tests and a doc page, and none fires falsely on the corpus.

- [x] Rule engine, IDs, severities, `armtool.toml`, inline allows ([0018](decisions/0018-lint.md))
- [x] File-wide control-flow graph, functions by reachability, sp dataflow
- [x] The first rules: [lint.md](lint.md)
- [x] `armtool check`, `lint --json`, `rules`, `explain`; lint in armls
- [x] Code actions for fixes, and `armtool lint --fix`
- [ ] More rules: syscall conventions, x16/x17 across calls, reads of undefined registers

## M6: formatter

- [x] `armtool fmt` and LSP formatting: column alignment for mnemonics, operands, and comments, whitespace only, proved by identical objects ([0020](decisions/0020-formatter.md))

## M7: deeper analysis

- [x] Dataflow lints: undefined scratch registers, syscall number in x16 ([0026](decisions/0026-dataflow-rules.md))
- [x] Gradual types from comment annotations: calls, signedness, memory access, pointer width ([0039](decisions/0039-types.md), [types.md](types.md))
- [x] Types from C prototypes and the C library, and printf format strings ([0040](decisions/0040-c-signatures.md))
- [x] Struct layouts and field offsets ([0041](decisions/0041-structs.md))
- [x] Struct definitions from C headers ([0042](decisions/0042-c-structs.md))
- [x] Values lost across calls: scratch and argument registers after a call, and callee-saved registers not restored on every path
- [x] Register liveness: dead stores (`flow/dead-store`)
- [ ] Darwin syscall argument checks by syscall number

## M8: distribution

- [x] `make install` and a HEAD-only Homebrew formula ([0025](decisions/0025-distribution.md))
- [x] GitHub releases with prebuilt binaries for macOS and Linux ([0044](decisions/0044-releases.md))
- [ ] A stable Homebrew formula, from the first release's source archive
- [x] VS Code extension (`.vsix`), Helix config
- [ ] VS Code Marketplace listing, Zed extension
