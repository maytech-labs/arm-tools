# 0015. The semantic model is per file, and reports only what clang rejects

- **Status:** Accepted
- **Date:** 2026-10-26

## Context

The language server needs symbols, references, and errors beyond syntax. Each could be modeled in many ways, and an error clang doesn't report is a false alarm the user can't fix. Every behavior below was probed with clang 21.

## Decision

- **One file at a time.** `arm_sema::Analysis::new(text)` builds symbols, references, and diagnostics for a file. `.include` isn't followed yet; after an `.include`, an unknown instruction is a warning, not an error, because the included file may define it as a macro.
- **Symbols:** labels, local numeric labels (`1:`), constants (`.set`, `.equ`, `.equiv`, `name = expr`), register aliases (`.req`, scoped to `.unreq`, case-insensitive), and macros. `.globl`/`.global` mark a symbol exported.
- **References** resolve after the whole file is read, because a label can be used before it's defined. `1f` resolves to the next `1:`, `1b` to the previous. Names that resolve to nothing, like `_printf`, are external on Darwin and aren't errors.
- **Macro and repeat bodies** aren't checked, and labels and `1f`/`1b` in them don't define or resolve, because clang checks them per expansion.
- **Errors, each matching a clang error:** duplicate labels, a label redefined by `.set`, `.equiv` twice, `1f`/`1b` with no target, unknown instructions (with a suggestion), invalid condition codes, unknown directives (with a suggestion, or "ELF only" for `.type` and friends), macros defined twice, a macro called before its definition, and unbalanced `.macro`/`.rept`/`.if` blocks.
- **The directive list is clang's.** `gen-isa` collects every `".name"` literal in LLVM's generic, Darwin, and AArch64 parsers, plus `.loh` (named by a function), assembles each alone, and keeps those clang doesn't call unknown: 254. Harvesting directive names from LLVM's AArch64 MC tests found no others.
- **`cargo xtask survey` checks semantic errors too.** Clang reports some errors, like duplicate symbols, while writing the object file, at a line in a temporary file; in a file with N of those, up to N unmatched lines count as agreeing.

## Consequences

- On LLVM 21's 2,352 AArch64 MC test files plus our corpus: 991 lines flagged, every one rejected by clang, 0 disagreements. All valid corpus files produce no diagnostics.
- Clang's messages for late errors have no useful location; ours point at the second definition and say what to do.
- Includes, conditional assembly evaluation, and macro expansion are later work; until then, the model is conservative inside bodies and after includes.

## Alternatives considered

- **Report undefined symbols.** They're normal on Darwin: the linker resolves them.
- **Check macro bodies as written.** Clang doesn't, and parameters make most bodies invalid until substituted.
