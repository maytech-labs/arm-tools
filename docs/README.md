# arm-tools docs

| Where | What |
|---|---|
| [design.md](design.md) | How the suite is built: syntax, ISA, sema, lint, language server, command line |
| [roadmap.md](roadmap.md) | Milestones, their "done when" bars, and what's done |
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
