# 0027. `.S` files: diagnostics from clang's preprocessor output, editing on the text as written

- **Status:** Accepted
- **Date:** 2026-10-26

## Context

Clang runs `.S` files (capital S) through the C preprocessor before assembling them; `.s` files it doesn't. Apple's own assembly and most projects use `#include`, `#define`, and `#if`. Read as plain assembly, a function-like macro such as `CALL(_g)` is an unknown instruction, `#N` is an undefined symbol, and junk under `#if 0` gets checked.

## Decision

- **For `.S` files, diagnostics come from the preprocessed text.** `arm_sema::preprocess` runs `xcrun clang -E -x assembler-with-cpp -arch arm64 -`, feeding the text on stdin (so unsaved edits count), from the file's directory with `-I` for each include directory. Its output is analyzed and linted, and each diagnostic is mapped back through the `# 12 "<stdin>"` line markers: same columns when the line came through unchanged, the whole line otherwise. Problems in included headers are dropped here; they show when the header is open. Fixes are kept only on unchanged lines.
- **Editor features use the text as written**, where preprocessor lines are comments, so hover, definition, rename, and highlighting work on what you see.
- **Without clang** (off macOS, or if preprocessing fails), only syntax errors are shown for the file, with a warning saying why.
- `arm_lint::check` is the one place both armls and armtool check a file: includes, preprocessing, semantic errors, and lint rules.

## Consequences

- `.S` files get the same checks as `.s` files, with clang's own macro expansion and `#if` evaluation. Tested with real clang: a `#define`, a function-like macro, an included header, and `#if 0` junk produce no diagnostics, and an out-of-range immediate maps back to its exact column.
- Each check of a `.S` file runs clang once, about 20 ms, which armls does on every change.

## Alternatives considered

- **Implement the C preprocessor.** A large, well-specified program clang already provides on every Mac this tool targets.
- **Treat preprocessor lines as comments and nothing more.** False errors on every function-like macro.
