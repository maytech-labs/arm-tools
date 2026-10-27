# 0032. Completion snippets for Apple's conventions

- **Status:** Accepted
- **Date:** 2026-10-27

## Context

Some code is written the same way every time on macOS: the frame record push and pop, `adrp` with `@PAGE` and `@PAGEOFF`, and system calls with the number in x16 and `svc #0x80`. Getting one detail wrong (`@PAGE` on the `add`, x8 instead of x16) assembles and fails at run time.

## Decision

- armls offers a fixed set of snippets as completions where a statement starts: `func`, `main`, `prologue`, `epilogue`, `addr`, `load`, `got`, `syscall`, `write`, `exit`, `loop`, and `cstring`. They live in `ide::SNIPPETS`.
- Only clients that declare `snippetSupport` get them; others would insert the placeholders as text.
- No snippet is named like an instruction, so typing a mnemonic never picks a template.
- A test expands every snippet with its defaults and checks that arm-sema reports nothing and clang assembles it.

## Consequences

- The set is ours, not configurable. Editors' own snippet engines cover personal templates.

## Alternatives considered

- **Ship snippets as editor-specific files.** One per editor, and they'd drift from the server's conventions.
