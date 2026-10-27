# 0011. Parser grammar: what's checked at parse time and what waits for sema

- **Status:** Accepted; macro handling superseded by [0012](0012-parser-survey-fixes.md)
- **Date:** 2026-10-26

## Context

The parser has to decide how much it knows about AArch64. Too little and the tree is a flat list of tokens that every later layer re-parses. Too much and it rejects valid code, or duplicates the instruction tables that belong in `arm-isa`. Each choice below was probed against Apple clang 21.

## Decision

- **One `SyntaxKind` enum** for tokens and nodes, as rowan needs. Token kinds that clash with Rust names are `StringLit`, `CharLit`, and `Unknown` (a bad character); `Error` is the node for input the parser couldn't read.
- **Statements:** labels first (any number per line), then one directive, instruction, or assignment. A name starting with `.` is a directive, except `.` itself, the location counter. `name = expr` is an assignment.
- **Macro calls parse as instructions.** Clang requires comma-separated macro arguments, the same as operands, and macro names are case-sensitive while mnemonics aren't. Sema decides which an `Instruction` node is, so the parser doesn't track `.macro` definitions.
- **Directive arguments are lenient:** expressions where they fit, raw tokens where they don't (`.macro m b=1`, `.build_version macos, 14, 0 sdk_version 14, 4`). The parser reports no errors in them; sema checks each directive's form.
- **Operands have structure:** immediates (`#expr`), memory (`[...]` with optional `!`), register lists with ranges and an index, indexed elements (`v0.s[1]`), literal pool loads (`=expr`), shifts and extends with an amount (`lsl #3`, `mul vl`), and expressions. A shift is recognized only for the names in `SHIFTS` followed by an amount, so `mov x0 x1` stays an error instead of parsing as a shift.
- **Expressions use LLVM's Darwin precedence**, confirmed by evaluating expressions with clang: `* / %` (6), `+ -` (5), `<< >>` (4), comparisons (3), `| ^ &` (2), `&& ||` (1), left to right within a level. `<>` is a synonym for `!=`. Unary `- + ~ !` bind tightest.
- **Recovery:** at the statement level, everything left becomes one `Error` node with one message. Inside brackets and braces, junk is skipped to the closer. A bad character the lexer already reported doesn't get a second error.
- **Syntax errors must be clang errors.** `tests/data/invalid.s` lists lines we report; the oracle test checks clang rejects each one.

## Consequences

- The tree is useful to sema, the linter, and the formatter without re-parsing operands.
- Some invalid code parses cleanly, for example a shift on the wrong instruction, or a directive with the wrong arguments. That's by design: those errors come from sema with better messages.
- A full parse of a 10,296-line file takes 2.6 ms in release (M-series Mac, `cargo run --release -p arm-syntax --example bench`), so the server can reparse on every change.

## Alternatives considered

- **Track `.macro` definitions in the parser** so calls get their own node. Unneeded once clang showed macro arguments use commas, and it would miss macros from `.include`.
- **Per-directive grammars in the parser.** There are over a hundred directives; sema owns their forms with a table.
- **GNU precedence.** Wrong for Apple's assembler: `1 << 2 + 1` is 8 with clang, 5 under GNU rules.
