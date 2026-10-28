# 0048. Calls that never return, unseen code, jump tables, and conflicting C

- **Status:** Accepted
- **Date:** 2026-10-28

## Context

Linting the 703 programs of [arm-lessons](https://github.com/maytech-labs/arm-lessons) turned up about 70 findings on correct code, from five causes:

- `bl exit` at the end of a function, or a call to a function like `die` that ends that way, was reported as falling through.
- A macro call after the last instruction, like an `EPILOGUE` that ends in `ret`, was reported as falling through. Register types flowed across a macro call that overwrote them, so `LOAD_ADDR x0, msg` left x0 typed as the caller's `i32`.
- `br x11` through a jump table, with the frame still up, counted as leaving the function, so the stack looked unbalanced.
- `ldrb w0, [x19]` from a `char *` typed the register as `i8`, though `ldrb` zero-extends. Passing it to `putchar(int)` was reported.
- Every C file under the project root fed signatures, and the first declaration of a name won. A tree of separate programs declared `sum_array` two ways, and one program's call was checked against the other's prototype.

## Decision

- A `bl` to a C library function that never returns (`exit`, `_exit`, `_Exit`, `quick_exit`, `abort`, the `longjmp`s, `pthread_exit`, the `err`s, and the assert and stack-check failures) has no successor. A function in the file whose every path ends in such a call, or a trap, and never returns or branches out, never returns either; that's found by iterating to a fixed point. `fallthrough` accepts a function that ends in one.
- `fallthrough` doesn't judge a function whose last instruction is followed by a macro call. The type checker forgets every register type, address, and flag after code it can't see.
- `unbalanced-stack` doesn't report at `br`, `braa`, `brab`, `braaz`, or `brabz`. A tail call through a register with the frame still up is rare; a jump table is common.
- `ldrb` and `ldrh` give an unsigned value of the pointee's width; `ldrsb` and `ldrsh` give a signed value as wide as the register.
- C files in the assembly file's own directory win over the rest of the project. A name the rest of the project declares in two different ways isn't checked against either.

## Alternatives considered

- **A `noreturn` annotation only.** Precise, but the common case is `bl exit`, and every user would have to write it.
- **Read `_Noreturn` and `__attribute__((noreturn))` from C.** Worth adding later for project functions; it doesn't help with the C library, whose headers we don't read.
- **Expand macros into the control-flow graph.** The right fix in the long run, but sema's expansion isn't mapped to instruction ranges yet. Treating the macro as unseen code matches what the other flow rules already do.
- **Pick the C file whose path is closest.** Directory distance doesn't tell you what's linked together. Same directory, then unambiguous project-wide, is predictable.

## Consequences

- A function that really does fall into the next one after `bl exit` isn't caught. That code is unreachable anyway.
- A real unbalanced stack before a `br` tail call isn't caught.
- Two programs in one tree no longer trip over each other's prototypes, but a name they share loses its check unless it's declared beside the file.
