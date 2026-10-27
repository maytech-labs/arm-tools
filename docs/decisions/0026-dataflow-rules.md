# 0026. Dataflow rules report only what holds on every path

- **Status:** Accepted
- **Date:** 2026-10-26

## Context

The linter's control-flow graph ([0018](0018-lint.md)) can answer "is this register set before it's read?" Two bugs are common in hand-written Apple assembly: reading a scratch register (x9 to x17, v16 to v31) that a `bl` just clobbered, and calling `svc #0x80` with the syscall number in x8, Linux style, instead of x16.

## Decision

- **`flow/undefined-register`:** a read of a scratch register that is undefined on every path to it: since a call (`bl`, `blr`) anywhere, or since entry in an exported function. Non-exported helpers may take inputs in scratch registers by their own convention, so their entry isn't checked.
- **`abi/syscall-number`:** an `svc` where x16 is undefined on every path, by the same analysis (`svc` reads x16).
- **Must, not may:** paths meet by intersection, so a register set on any path into a read is treated as set. Paths that can't happen at run time can't cause a finding; a bug on only some paths can be missed.
- **Reads** are every register operand except destinations, plus x16 for `svc` and x30 for `ret`. A destination that's also read (`movk`) isn't counted as a read, which can only miss a finding.

## Consequences

- No findings on the compiler output or hand-written corpus code; the tests cover entry, calls, one-path definitions, helpers, and syscalls.
- Register liveness, dead stores, and checks on arguments to known functions can build on the same analysis.

## Alternatives considered

- **May-analysis (union at joins).** Finds more, including false ones on paths guarded by the same condition twice, which hand-written code does often.
- **Check every function's entry.** Flags every helper that takes its input in x9 on purpose.
