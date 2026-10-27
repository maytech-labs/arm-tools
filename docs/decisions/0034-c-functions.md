# 0034. Index C function definitions by scanning

- **Status:** Accepted
- **Date:** 2026-10-27

## Context

Assembly on macOS is often half of a program whose other half is C. `bl _report` should lead to `report` in a `.c` file, as it does to a global label in another `.s` file ([0029](0029-workspace-index.md)).

## Decision

- The workspace index also reads `.c` and `.m` files and records each non-`static` function definition under its Mach-O symbol name: `report` is `_report`. Headers aren't read: they declare, they don't define.
- Definitions are found by a scan, not a parser: comments, strings, and preprocessor lines are skipped, and at the top level, the identifier before the last parenthesized group ahead of a `{` is a function's name. `__attribute__((...))` groups don't count, and a statement with `static` or `=` (an initializer) isn't a function.
- Definition, hover (with the C line), completion, workspace symbols, and outgoing calls in call hierarchy use them.
- Rename refuses names a C file defines: renaming from assembly would have to rewrite C, without the leading underscore, and its callers we don't index.

## Consequences

- No dependency on clang's libclang or a C parser, and it works on code that doesn't compile.
- Functions defined by macros, and C++ (mangled names), aren't found.
- Calls from C into assembly aren't indexed, so references and incoming calls don't show C callers.

## Alternatives considered

- **libclang or tree-sitter-c.** Exact, but a large native dependency for one feature.
- **Ask clangd.** Editors can run both servers; armls would still need the names to link `bl _foo`.
