# 0028. Inlay hints: constant values and syscall names

- **Status:** Accepted
- **Date:** 2026-10-26

## Context

Two things are hard to see in assembly: what a named constant is worth where it's used (`#LIMIT`), and which system call a `mov x16, #4` sets up.

## Decision

- **Constant values:** after each use of a constant defined exactly once (in the file or what it includes), a hint like `= 4095 (0xfff)`, with hex for values of 16 and up. Constants set more than once (`.set a, 1` then `.set a, 2`) get no hint: they have no single value.
- **Syscall names:** after `mov x16, #n` or `movz x16, #n`, the name of Darwin system call `n` (`syscall write`), when an `svc` follows within eight instructions, since x16 is also an ordinary scratch register. The names come from a short table of common BSD system calls in `arm_isa::syscall_name`, numbered as in XNU's `syscalls.master`; these numbers are a stable public interface.
- armls serves them through `textDocument/inlayHint`.

## Consequences

- Reading syscall code and constant-heavy code doesn't need a lookup.
- The syscall table covers the common calls only; others get no hint.

## Alternatives considered

- **Generate the full syscall table from XNU's source.** Its license (APSL) and size aren't worth it for hints; the numbers of common calls are facts that don't change.
