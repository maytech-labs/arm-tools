# 0022. CPU features: clang's default target, `.arch` directives, and `[target]`

- **Status:** Accepted
- **Date:** 2026-10-26

## Context

Clang rejects instructions the target doesn't support: `abs x0, x1` is "instruction requires: cssc" on a default macOS build. Which features are on depends on the target and on directives in the file. Probed with clang 21:

- `clang -arch arm64` targets the apple-m1 CPU and adds `+v8.5a` and two dozen extensions (`clang -###` shows them). SVE, BF16, MOPS, CSSC, and MTE are off; LSE, SHA3, and FP16 are on.
- `-march=armv9.4-a+sve2` on Darwin keeps the apple-m1 CPU's features and adds the architecture's: aes and sha2 stay on.
- The `.arch armv8-a` directive replaces the set: LSE is off after it, though apple-m1 has it. `.arch_extension nolse` turns LSE off; `.arch_extension sve` turns SVE on; `.cpu apple-m4` selects that CPU's set.

## Decision

- **Feature data comes from LLVM**: `gen-isa` now also runs `llvm-tblgen --dump-json` (read with `serde_json`, an xtask-only dependency) for the 319 subtarget features and what each implies, each architecture's default extensions, 92 processors and their aliases (apple-m1 is apple-a14), and each assembler predicate's condition as clauses of alternatives (`HasSVE_or_SME` is one clause, sve or sme). Four predicates whose conditions don't fit that shape are treated as always met. Clang's default target is captured from `clang -###` when generating.
- **`arm_isa::FeatureSet`** is closed under implication; turning a feature off turns off everything that implies it, as LLVM does. `for_arch` and `for_cpu` replace (the directives); `add_arch` unions (the `-march` flag on Darwin); `modify` takes `.arch_extension` names, including `no` prefixes and user spellings like `fp16`.
- **The matcher** skips forms whose features are off and, if no other form fits, reports `MissingFeatures`. Sema reports it as `missing-feature`, in clang's words ("instruction requires: sve or sme"), with how to enable it.
- **Sema follows `.arch`, `.cpu`, and `.arch_extension`** through the file, and into included files. A spec it can't read turns feature checks off from there on, rather than guessing.
- **`[target]` in armtool.toml** sets the starting point the way clang's flags do: `cpu` (default apple-m1), `arch` added on top, then `extensions`. Projects built with `-mcpu=apple-m4` set `cpu = "apple-m4"`.

## Consequences

- The survey uses the same target as its clang command (apple-m1 plus `armv9.4-a+sve2+sme2`): 14,218 lines flagged across 2,362 files, 0 disagreements. Recall on LLVM 21's MC tests rose from 56.8% to 70.4%.
- A project whose build flags differ from the config gets feature errors clang wouldn't give, or misses some. The message says to set `[target]`.

## Alternatives considered

- **Ignore features** (0017). Simple, and silent about the most confusing error on Apple silicon, where SVE and CSSC look like valid AArch64.
- **Hand-written feature tables.** 319 features with implications change every LLVM release.
