# 0008. A lossless, error-tolerant syntax tree on rowan

- **Status:** Accepted
- **Date:** 2026-10-26

## Context

The language server sees half-typed code on every keystroke, and the formatter must reproduce comments and spacing it doesn't change. A parser that stops at the first error, or a tree that drops trivia, can't serve either.

## Decision

- The lexer produces tokens covering every byte, including whitespace, newlines, and comments.
- The parser never fails. Unexpected input becomes an `ERROR` node, the parser recovers at the next statement boundary (newline or `%%`), and diagnostics are returned beside the tree.
- The tree uses `rowan` (rust-analyzer's green/red tree library): immutable, cheap to clone, with parent pointers and text ranges. A typed AST layer wraps it.
- The lexer is hand-written, with no dependencies.

## Consequences

- One error doesn't hide the rest of the file's symbols, hover, or diagnostics.
- The formatter and code actions edit the tree and print it back exactly.
- `rowan` is a real dependency, and its API is lower-level than a typed AST; the typed layer is our code to maintain.

## Alternatives considered

- **A typed AST without trivia.** Simpler, but no formatter and poor recovery.
- **tree-sitter.** Great for highlighting and error recovery, but it brings a C runtime and a grammar in another language, and we'd still build a semantic layer in Rust.
- **A parser generator (lalrpop, pest).** Weak error recovery and no lossless output.
