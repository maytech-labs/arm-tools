# arm-tools docs

| Where | What |
|---|---|
| [design.md](design.md) | How the suite is built: syntax, ISA, sema, lint, language server, command line |
| [roadmap.md](roadmap.md) | Milestones, their "done when" bars, and what's done |
| [lint.md](lint.md) | Every lint rule, and how to configure them |
| [decisions/](decisions/) | Every decision, as an ADR, never rewritten. Copy [0000-template.md](decisions/0000-template.md) to add one. |

## Decisions

- [0001 One workspace for the whole suite, on a shared core](decisions/0001-one-workspace.md)
- [0002 Rust, built with Cargo](decisions/0002-rust-and-cargo.md)
- [0003 Names: arm-tools, armls, and armtool](decisions/0003-names.md)
- [0004 Apple AArch64 first, with clang as the source of truth](decisions/0004-apple-aarch64-first.md)
- [0005 MIT license](decisions/0005-mit-license.md)
- [0006 Terse conventional commits, no tool attribution](decisions/0006-commit-messages.md)
- [0007 Unit, integration, and oracle tests](decisions/0007-testing.md)
- [0008 A lossless, error-tolerant syntax tree on rowan](decisions/0008-lossless-syntax-tree.md)
- [0009 lsp-server and lsp-types, synchronous](decisions/0009-lsp-library.md)
- [0010 Where the instruction tables come from](decisions/0010-isa-source.md) (superseded by 0013)
- [0011 Parser grammar: what's checked at parse time and what waits for sema](decisions/0011-parser-grammar.md) (amended)
- [0012 Macros, bodies, and ELF syntax, from surveying real assembly against clang](decisions/0012-parser-survey-fixes.md)
- [0013 Instruction tables from LLVM 21 TableGen, with our own hover text](decisions/0013-isa-from-llvm-tablegen.md)
- [0014 gen-isa reads clang's assembler matcher, not TableGen's JSON records](decisions/0014-isa-from-asm-matcher.md)
- [0015 The semantic model is per file, and reports only what clang rejects](decisions/0015-sema-file-model.md)
- [0016 armls v1: features before operand checking, full sync, and a fixed token legend](decisions/0016-armls-v1.md)
- [0017 Operand checking: clang's operand model, ported predicates, and permissive gaps](decisions/0017-operand-matching.md)
- [0018 The linter: rules over the semantic model, a control-flow graph, and quiet when unsure](decisions/0018-lint.md)
- [0019 Following `.include`: import definitions, search like clang but forgive the working directory](decisions/0019-includes.md)
- [0020 The formatter changes only whitespace, and proves it](decisions/0020-formatter.md)
- [0021 Quick fixes travel with diagnostics](decisions/0021-quick-fixes.md)
- [0022 CPU features: clang's default target, `.arch` directives, and `[target]`](decisions/0022-cpu-features.md)
- [0023 Conditional assembly: evaluate what clang would, skip what it skips](decisions/0023-conditional-assembly.md)
- [0024 Macro and repeat expansion: check what clang assembles, report it at the call](decisions/0024-macro-expansion.md)
- [0025 Distribution: a Homebrew formula in this repo, and a thin VS Code extension](decisions/0025-distribution.md)
- [0026 Dataflow rules report only what holds on every path](decisions/0026-dataflow-rules.md)
- [0027 `.S` files: diagnostics from clang's preprocessor output, editing on the text as written](decisions/0027-preprocessed-files.md)
- [0028 Inlay hints: constant values and syscall names](decisions/0028-inlay-hints.md)
- [0029 A workspace index for navigation between linked files](decisions/0029-workspace-index.md)
