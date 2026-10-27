# 0024. Macro and repeat expansion: check what clang assembles, report it at the call

- **Status:** Accepted
- **Date:** 2026-10-26

## Context

Macro bodies weren't checked (0012), which matched clang for a definition alone, but missed errors clang reports once a macro is called: `m x0` with a body that adds `#5000` is an error clang reports "while in macro instantiation". `.rept`, `.irp`, and `.irpc` bodies are expanded where they're written and were also unchecked. Probed with clang 21:

- Arguments are split at commas outside parentheses; brackets don't group, so `m x0, [x1, #8]` is three arguments.
- `name=value` sets a parameter by name; defaults fill the rest; `:req` must be given ("missing value for required parameter"); `:vararg` takes the rest. Too many positional arguments is an error at the call.
- `\param`, `\()` (a separator that disappears), and `\@` (a counter) are substituted; `.exitm` ends the expansion.

## Decision

- At `.endm`, a top-level macro's definition is recorded: its parameters and body text. Definitions from included files come along with them.
- At each call outside a body, the arguments are bound as clang does; binding errors are `macro-arguments` errors at the call. The expansion is then analyzed as ordinary code, seeded with the macros, constants, and register aliases in scope at the call and the current CPU features. Its problems are reported at the call, with "(in the expansion of 'm')".
- At `.endr`, a top-level repeat is expanded: once for `.rept` (the same lines each time), once per value for `.irp`, once per character for `.irpc`. A problem that every value has is reported once.
- Expansions nest up to 8 deep. Labels inside an expansion aren't added to the file's symbols, and lint rules still treat macro calls as code they can't see.
- A colon in a directive's arguments starts an expression only as an ELF specifier (`:lo12:x`), so `.macro m a:req` parses cleanly.

## Consequences

- Operand errors, unknown instructions, and feature requirements inside macros and repeats are caught where they're used, in clang's words. Oracle cases cover a bad operand through a macro, too many arguments, a bad operand in `.irp`, and a valid file using defaults, keyword arguments, varargs, and `\@`.
- The survey still shows 0 disagreements across 2,382 files.
- An expansion's error points at the call, not the line in the body; the message says which macro.

## Alternatives considered

- **Check bodies at the definition.** Parameters make most bodies invalid until substituted; clang doesn't.
- **Expand into the file's own items**, so lint rules see macro code. More useful, but positions inside an expansion don't exist in the file; a later change can map them to the call.
