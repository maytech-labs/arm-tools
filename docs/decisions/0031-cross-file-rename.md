# 0031. Rename global labels across the project

- **Status:** Accepted
- **Date:** 2026-10-27

## Context

Rename changed one file. Renaming a global label left every caller in other files pointing at a name that no longer exists, which the linker only reports later.

## Decision

- Renaming a global label (`.globl` or `.private_extern`) also edits every file in the workspace index ([0029](0029-workspace-index.md)) that defines or uses it, open or not, through one `WorkspaceEdit`.
- Rename works from either end: the label in its defining file, or a use in a file that doesn't define it, if another indexed file does. Names nothing in the project defines, like `_printf`, can't be renamed.
- The new name is refused if any indexed file already defines it as a global label, or this file defines it, since either would be a duplicate symbol at link time.
- File-local labels with the same name in other files are left alone: they're different symbols.

## Consequences

- Files outside the workspace roots, and C code, aren't edited.
- Uses inside macro bodies in other files are found only as the index sees them.

## Alternatives considered

- **Refuse renaming global labels.** Safe, but leaves the user to do it by hand.
