# 0029. A workspace index for navigation between linked files

- **Status:** Accepted
- **Date:** 2026-10-26

## Context

`.include` brings definitions into a file ([0019](0019-includes.md)), but most multi-file projects don't include each other: each file is assembled separately and the linker connects `bl _helper` in one object to `_helper` in another. Go to definition stopped at the file edge for those.

## Decision

- **armls indexes the project's assembly files** at startup: every `.s` and `.S` under the client's workspace folders (or its root), skipping hidden directories and `target`, `build`, `node_modules`, `DerivedData`, and `.build`, up to 10,000 files of up to 4 MB.
- **Only global labels are definitions**: `.globl`, `.global`, and `.private_extern` (which now also marks a label exported). On Mach-O, other object files can't link to anything else. Each file's uses of names it doesn't define, and of its own global labels, are indexed for references.
- **The index follows edits**: open buffers are reindexed on every change, so an unsaved rename moves navigation at once; closed files are reread on save, on close, and on `workspace/didChangeWatchedFiles`.
- **Where it's used**: go to definition on a name this file doesn't define falls back to the index (one location, or several if the name is defined more than once); references on a global label or external name add uses from other files; hover on an external name shows the file and line it's defined on; `workspace/symbol` searches global labels.
- Names defined only outside the project, like `_printf`, still have no definition.

## Consequences

- `gd` across separately assembled files works, checked over stdio with an unopened target file and an unsaved rename, and in the owner's Neovim config. `examples/linked` is a two-file program to try it on.
- Startup reads the project's assembly once; a large tree takes a moment before the first cross-file jump.
- Symbols defined in C files aren't indexed. A call from assembly into C (`bl _my_c_function`) has no definition yet.

## Alternatives considered

- **Ask the user to `.include` related files.** That's not how separately assembled projects are built.
- **Index on demand at the first lookup.** Simpler, but references need every file anyway.
