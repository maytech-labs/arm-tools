# 0023. Conditional assembly: evaluate what clang would, skip what it skips

- **Status:** Accepted
- **Date:** 2026-10-26

## Context

Until now, both branches of `.if`/`.else` were checked, so code in a branch clang skips could get errors clang never gives. Probed with clang 21:

- Clang doesn't parse skipped branches at all: `.if 0` around junk is fine, and labels there aren't defined.
- `.if expr` needs a value known at that line: a constant defined later, or an undefined name, is "expected absolute expression".
- `.ifdef sym` asks whether `sym` is defined so far; a label later in the file doesn't count.
- `.ifc a, b` compares text, `.ifeqs` compares strings, `.ifb` tests for nothing after the directive, and `.ifeq`, `.ifne`, `.iflt`, `.ifle`, `.ifgt`, `.ifge` compare with zero.

## Decision

- **Evaluate each condition** with what's defined above it: constants with known values (the same evaluator operands use), symbols for `.ifdef`, and the directive's text for the string forms. `.elseif` and `.else` follow the branches taken before.
- **Skipped branches are skipped**: no symbols, references, items, or diagnostics, parse errors included. Only nested `.if`/`.else`/`.endif` are followed, to keep the nesting.
- **Undecided branches** (a condition we can't evaluate, like one using `.` or an expression we don't fold) are analyzed for symbols and navigation, but their diagnostics aren't reported: we can't know if clang assembles them.
- **A condition clang can't evaluate** because a name isn't a constant defined above is an error, `non-constant-condition`, in clang's words.
- Inside macro bodies, conditions depend on arguments and aren't evaluated.
- `Analysis::inactive()` lists skipped ranges; armls publishes them as hints tagged `unnecessary`, which editors grey out.

## Consequences

- A `valid/` oracle set in arm-sema (files clang accepts that must produce no diagnostics) now covers skipped junk, else and elseif chains, `.ifdef` before a label, string and comparison conditions, nesting, and labels in skipped branches. Invalid cases cover errors in the active branch and a constant defined too late.
- The survey still shows 0 disagreements.

## Alternatives considered

- **Check both branches** (as before). False errors in configuration-dependent code.
- **Skip both branches when unsure.** Loses navigation in code we can see; dropping only the diagnostics keeps it.
