# 0039. Gradual types from comment annotations

- **Status:** Accepted
- **Date:** 2026-10-27

## Context

The bugs that survive clang in hand-written assembly are mostly type bugs: a 32-bit register used as a pointer, a signed branch after an unsigned compare, an 8-byte load through a pointer to 4-byte values, the wrong thing in x1 at a call. Assembly can't express the types that would catch them.

## Decision

- **Annotations live in comments** containing `armtool:`: `fn name(x0: ptr<i32>, x1: u64) -> i64` for signatures, and `x9: ptr<u8>` for a register at a point. Clang never sees them, so annotated files assemble unchanged, and nothing needs a preprocessor. Signatures name registers rather than positions, so helpers with their own conventions can be described.
- **Types are small:** signed and unsigned integers of 8 to 64 bits, f32, f64, `ptr`, and `ptr<T>`. Structs come later.
- **Gradual:** a register's type is known only from an annotation or what flows from one. Inference runs on the control-flow graph with a meet that keeps a type only where every path agrees. Nothing is reported about an unknown type, so unannotated code has no findings and annotations can be added one function at a time.
- **Flow rules** follow the instructions that preserve meaning: register moves (W reads give the low half), loads through typed pointers, pointer arithmetic, extends, conversions, and calls to annotated functions, which set x0 or v0. Calls forget the types of caller-saved registers. Everything else makes the written register unknown.
- **Rules:** `types/call`, `types/signedness`, `types/memory`, `types/pointer-width`, and `types/annotation`, each a warning. An unsigned integer fits where a wider integer is wanted (W writes zero-extend); a signed one needs sxtw.
- **Hover** shows a register's type at the cursor.

## Consequences

- Types only help where someone writes them; the payoff grows with the annotations.
- Signatures are per file for now. A function annotated in one file isn't known in another until it's declared there too; C prototypes from headers are the next step.

## Alternatives considered

- **Infer everything, no annotations.** Without declared intent, inference can't tell a bug from a convention, and findings would be guesses.
- **A sidecar file of signatures.** Keeps sources clean, but drifts from the code and needs its own discovery.
- **Macros that expand to nothing.** Work in `.S` files only, and editors and clang would see them as code.
