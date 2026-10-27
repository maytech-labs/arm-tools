# 0020. The formatter changes only whitespace, and proves it

- **Status:** Accepted
- **Date:** 2026-10-26

## Context

A formatter for assembly is only trustworthy if it can't change what the file assembles to. Assembly makes that easy to break: macro bodies paste tokens (`v0.\sz`, `\name\()_x:`), so inserting a space can change meaning, and `%%` and block comments make "one statement per line" untrue.

## Decision

- **Only whitespace changes.** The formatter never adds, removes, or joins tokens. Inside operands it adds a space after each comma, removes spaces just inside brackets, braces, and parentheses, and otherwise only collapses existing whitespace to one space. Tokens that touched stay touching; the mnemonic is the run of touching tokens before the first space, so a macro paste stays one word. The one exception is `name=value`, where `=` is always its own token and spacing it is safe.
- **Layout:** labels at the left edge, sharing a line with a statement when they're shorter than the indent; mnemonics and directives at `indent` (4); operands after the mnemonic padded to `mnemonic_width` (8); trailing comments at `comment_column` (40) or one space after the code; full-line comments stay at the left edge if they were there, else at the indent; at most `max_blank_lines` (1) blank lines in a row; no trailing whitespace; one final newline. All four numbers are settable under `[format]` in `armtool.toml`.
- **Leave what can't be judged:** lines a block comment spans, lines with `%%`, and lines with syntax errors are copied, trimmed of trailing whitespace only.
- **It lives in `arm-syntax`** (`arm_syntax::format`), since it needs only tokens. `armtool fmt` rewrites files, `armtool fmt --check` lists files that need it and exits 1 without writing, and armls answers `textDocument/formatting` with one whole-document edit, or none.
- **Proved three ways:** tests on the corpus check that the token sequence is unchanged and that formatting twice changes nothing; an oracle test assembles every corpus file clang accepts before and after formatting and requires byte-identical object files; `cargo run -p arm-syntax --example fmt_check` runs the first two checks on any files (LLVM's 2,313 AArch64 MC test and HelloSilicon files: 0 problems).

## Consequences

- People can run `armtool fmt` on a whole tree, or format on save, without reviewing the diff for meaning.
- Mnemonic case and operand spelling are left alone: changing case would be a token change, and that's not this tool's call.
- Range formatting isn't offered yet; whole-document formatting is fast enough.

## Alternatives considered

- **Rebuild each line from the syntax tree.** More control over layout, and more ways to change meaning, especially in macro bodies the parser doesn't check.
- **Tabs, like compiler output.** Configurable widths in spaces are clearer in diffs and reviews; a tab option can come later.
