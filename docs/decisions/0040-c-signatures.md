# 0040. Signatures from C, and printf format strings

- **Status:** Accepted
- **Date:** 2026-10-27
- **Extends:** [0039](0039-types.md)

## Context

Type annotations ([0039](0039-types.md)) only help where someone writes them. Most calls out of assembly go to C: the C library, or the project's own C code, whose prototypes already say what each argument is. And printf is the classic place for argument bugs, made worse on Darwin, where variadic arguments go on the stack instead of in x1 and up.

## Decision

- **C prototypes become register signatures** by the AAPCS64 rules: integers and pointers in x0 to x7, floats in v0 to v7, counted separately; the result in x0, or v0 for a float. Names take Darwin's leading `_`. C types map to ours (`int` to i32, `size_t` to u64, `char *` to `ptr<i8>`, `void *` to `ptr`); a parameter of a type we can't place in a register, like a struct by value, drops the whole prototype rather than guess.
- **Where they come from,** in order of precedence: the file's own annotations; prototypes and definitions in the project's `.h`, `.c`, and `.m` files (the project being the directory of `armtool.toml`, or the nearest one with `.git`), found by a scan like [0034](0034-c-functions.md)'s and cached for two seconds so an editor can ask on every edit; then a built-in table of about 40 common C library functions. `static` functions are skipped.
- **Pointers to integers of one width fit each other** (`char *` and `unsigned char *`), as C's own warnings are lax there.
- **Format strings:** the checker tracks which label's address a register holds (`adr`; `adrp` and its `@PAGEOFF` or `:lo12:`). At a call to printf, fprintf, sprintf, snprintf, or dprintf whose format register holds a label with `.asciz`, `.string`, or `.ascii` data, each conversion is matched to its argument (`%d` an integer, `%f` a double, `%s` a pointer, `*` an extra integer):
  - On Linux, arguments are in the next x registers and floats in v registers; known types are checked.
  - On Darwin, arguments are in 8-byte stack slots. The checker follows sp through the straight-line code before the call, so `str w1, [sp, #-32]!` counts as the first slot. With conversions and no stores, but argument registers written, it reports the Darwin-specific bug of passing them in registers; with stores, a missing slot or a mistyped value. Stores of x29 and x30 are the frame record, not arguments.
- New rule: `types/format`, a warning.

## Consequences

- C library calls are checked in every file, with no annotations: `bl _strlen` with an integer in x0 is a finding.
- Names can collide: an assembly function named like a C function in the same project gets that prototype. That's usually intended (assembly implementing a C declaration).
- Format strings built at run time, or stored in other files, aren't checked.
