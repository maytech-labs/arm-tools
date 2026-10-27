# Lint rules

`armtool lint` and `armls` run these rules. Each finds code clang accepts that breaks at run time or under Apple's arm64 ABI. See [0018](decisions/0018-lint.md) for the design.

## Configuring

Put an `armtool.toml` at your project root. `armtool` and `armls` use the nearest one above each file.

```toml
[rules]
"abi/x18-reserved" = "error"     # error, warning, info, hint, or off
"style/unused-label" = "off"
```

Silence one finding with a comment on its line, or on the line above:

```asm
    mov x18, #1    ; armtool: allow(abi/x18-reserved)
```

`armtool explain <rule>` prints a rule's page in the terminal.

## Rules

### `abi/x18-reserved`

Uses x18, which Darwin reserves for the platform

Apple's arm64 ABI reserves x18 for the OS. The kernel may change it at any time, such as on a context switch, so a value kept there can vanish. Use another scratch register, like x9 to x15.

Default level: warning.

### `abi/stack-alignment`

Moves sp by an amount that isn't a multiple of 16

sp must stay 16-byte aligned whenever it's used to access memory, or the CPU raises an alignment fault. Round stack adjustments up to a multiple of 16.

Default level: warning.

### `abi/frame-pointer`

A function that calls others doesn't set up x29

Darwin requires a valid chain of frame records for backtraces, profilers, and crash reports. A function that calls others should start with `stp x29, x30, [sp, #-16]!` and `mov x29, sp`.

Default level: warning.

### `abi/link-register`

A function calls another without saving x30

bl and blr write the return address to x30, replacing the caller's. A function that calls others must save x30 first, usually with `stp x29, x30, [sp, #-16]!`, and restore it before ret.

Default level: warning.

### `abi/callee-saved`

Writes x19 to x28 or d8 to d15 without saving them

Callers expect x19 to x28 and the low 64 bits of v8 to v15 to survive a call. A function that writes one must store it in its prologue and restore it before returning.

Default level: warning.

### `addr/adrp-pageoff`

adrp loads a page that nothing adds the offset to

adrp gives only the 4 KB page of a symbol. The full address needs the offset within the page too: `add x0, x0, _sym@PAGEOFF`, or `ldr x0, [x0, _sym@PAGEOFF]`.

Default level: warning.

### `addr/variant-mismatch`

Pairs @PAGE, @GOTPAGE, or @TLVPPAGE with the wrong offset

Each page variant pairs with one offset variant: @PAGE with @PAGEOFF, @GOTPAGE with @GOTPAGEOFF, and @TLVPPAGE with @TLVPPAGEOFF. Mixing them computes an address in the wrong place.

Default level: warning.

### `flow/unreachable`

An instruction that nothing can reach

The instruction follows one that never falls through, like b or ret, and has no label, so no branch can reach it. Add a label, or remove the code.

Default level: warning.

### `flow/fallthrough`

A function's last instruction falls into the next function

The function doesn't end with ret or a branch, so it keeps running into whatever comes next. End it with ret, or a branch if that's intended.

Default level: warning.

### `flow/unbalanced-stack`

Returns with sp at a different place than on entry

Along some path, the function moves sp and doesn't move it back before ret or a tail call, so the caller's stack is corrupted. Every push needs a matching pop on every path.

Default level: warning.

### `style/unused-label`

A label nothing refers to

The label isn't exported and nothing in the file branches to or loads it. Remove it, or export it with .globl if other files need it.

Default level: hint.

