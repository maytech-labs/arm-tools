# 0019. Following `.include`: import definitions, search like clang but forgive the working directory

- **Status:** Accepted
- **Date:** 2026-10-26

## Context

Real projects keep macros and constants in shared files. Until now, `.include` wasn't followed: calls to included macros were "unknown instructions" (as warnings), constants couldn't be checked as values, and go to definition stopped at the file. Probed with clang 21:

- Clang resolves `.include` relative to the directory it runs in, then each `-I` directory. Not relative to the including file: `.include "m.s"` inside `lib/nested.s` fails unless `lib` is the working directory or on `-I`.
- A missing file is an error: "Could not find include file 'x.s'".
- An include cycle makes clang hang forever.

An editor doesn't know the build's working directory or `-I` flags.

## Decision

- **`Analysis::with_includes(text, path, resolver)`** follows each `.include` outside macro bodies, analyzes the included file the same way (recursively, at most 16 deep), and imports what it defines up to that point: macros, constants with their values, register aliases still in scope, and labels. Imported symbols keep an `Origin` (the file's path and text), so hover shows their line and go to definition opens their file.
- **Search order:** the `[include] paths` in `armtool.toml` (relative to that file), the project root (the directory of `armtool.toml`, or the nearest one with `.git`), then the including file's directory. The last is more forgiving than clang, on purpose: the editor can't know the working directory, and a false "not found" is worse than accepting a file clang would find with the right `-I`.
- **Not found is a warning**, not an error, saying where we looked and to add the directory to `[include] paths`. After an include we couldn't follow, unknown instructions stay warnings.
- **Include cycles are errors** ("Clang loops forever on this"). Paths are compared after resolving symlinks, so a cycle through a symlink is still caught.
- **Paths keep the editor's spelling.** Results use the directories' spelling with `.` and `..` removed, never symlink-resolved: macOS's `/var` is a symlink to `/private/var`, and returning the resolved path would open a second buffer for the same file.
- **armls uses unsaved buffers** for included files, and reanalyzes open files that include a file when it changes.
- **Not imported:** the included file's own diagnostics (they show when you open it), local numeric labels, and code (lint rules see only this file's instructions).
- **Rename** refuses symbols from included files, saying which file to rename them in.

## Consequences

- Calls to included macros, included constants in operands (`add x0, x0, #big` is checked against big's value), and cross-file go to definition work, in armls and armtool.
- Checked by an oracle test that builds a small project on disk and agrees with `clang -I`, plus a contract test over LSP covering cross-file definition and unsaved edits.
- A project that relies on `-I` paths without an `armtool.toml` gets "not found" warnings until the paths are configured.

## Alternatives considered

- **Exactly clang's rule (working directory and `-I` only).** Correct for one build command, wrong for every editor that opened a different directory.
- **Merge the included file's code into this one.** Lint rules would then report problems in other files here, at positions that don't exist in this file.
