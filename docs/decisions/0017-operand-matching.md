# 0017. Operand checking: clang's operand model, ported predicates, and permissive gaps

- **Status:** Accepted
- **Date:** 2026-10-26

## Context

Operand errors (`add x0, x0, #5000`, `ldp x0, x1, [sp, #3]`) are the most common mistakes in hand-written assembly. Clang's matcher checks each operand against an operand class with a predicate, but its parser decides how to read some operands from the class it expects: `#1, lsl #12` is one shifted immediate where an add/sub immediate goes, `x0, x1` is one register pair for `casp`, `x0, #0` is one base register for `ldar`, `#0.0` after `fcmp` is the tokens `#0` and `.0`, and a bare name is a symbol, condition code, or keyword. Some instructions are rewritten in C++ before matching: `sxtw x0, w1` widens its source, `uxtb x0, w1` narrows its destination.

## Decision

- **Clang's operand model** lives in `arm_isa::Operand`, mirroring LLVM's `AArch64Operand`. `arm-sema` flattens the syntax tree into it: memory to `[`, base, offset, `]`, `!`; NEON registers to a register plus its arrangement token; lists to one list operand; `p0/z` to a register, `/`, `z`; `.req` aliases resolved; `.set` constants defined above the use folded to values, as clang folds them.
- **Constant expressions** are evaluated with Apple's semantics, probed with clang: 64-bit wrapping, comparisons give -1 for true, `!x` gives 1 or 0, `>>` is logical, a leading 0 means octal, division by zero is unknown.
- **The matcher** (`arm_isa::match_instruction`) tries every form in clang's order, with backtracking over the parser-dependent readings above and the C++ rewrites, and reports the attempt that got furthest. A near miss uses clang's message for that class ("index must be a multiple of 8 in range [-512, 504]"); a generic mismatch names the closest form.
- **Predicates** (`arm_isa::check`) are ported from LLVM 21's `AArch64AsmParser.cpp` and `AArch64AddressingModes.h`: 378 of the 385 predicate strings in the tables. The rest, and cases the port can't decide (system register names, ranges like `0:1`), return `Check::Unknown`, which the matcher treats as a match. A gap can only miss an error, never invent one.
- **Not checked yet:** CPU features (`instruction requires: sve2`), since the enabled set depends on `-march` and `.arch` directives; SYS aliases, bitfield aliases, `lsl` by an immediate, and `ldr x0, =value`, which clang handles in C++.
- Done in parallel by a subagent with a written interface (`operand.rs`), then measured end to end with the survey.

## Consequences

- `cargo xtask survey` on LLVM 21's 2,352 AArch64 MC test files and our corpus: 11,480 lines flagged, every one rejected by clang, 0 disagreements. With `--recall`, arm-sema flags 56.8% of the lines clang rejects; the gap is mostly feature requirements, system registers, and directive arguments.
- Errors appear in the editor at the operand, in clang's words, and are pinned by oracle cases in `crates/sema/tests/data/invalid/operand-*.s`.

## Alternatives considered

- **Write a simpler checker by hand** (register widths, immediate ranges for common instructions). Faster to start, but it would drift from clang and never cover SIMD, SVE, or aliases.
- **Strict unknowns** (treat an unported predicate as a mismatch). More errors caught, and some of them false: the wrong tradeoff for an editor.
