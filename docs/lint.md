# Lint rules

`armtool lint` and `armls` run these rules. Each finds code clang accepts that breaks at run time or under Apple's arm64 ABI. See [0018](decisions/0018-lint.md) for the design.

## Configuring

Put an `armtool.toml` at your project root. `armtool` and `armls` use the nearest one above each file.

```toml
[rules]
"abi/x18-reserved" = "error"     # error, warning, info, hint, or off
"style/unused-label" = "off"
```

The same file tells `armtool` and `armls` where to find `.include` files, searched in this order before the project root and the including file's directory ([0019](decisions/0019-includes.md)):

```toml
[include]
paths = ["include", "../shared"]   # relative to armtool.toml
```

The platform and CPU your build targets, so syntax and feature errors match it ([0022](decisions/0022-cpu-features.md), [0038](decisions/0038-linux.md)). Without a platform, each file's own syntax decides (`@PAGE` means Darwin, `:lo12:` means Linux), then Darwin; a file can also say `// armtool: platform=linux`. Without a CPU, it's what clang targets by default: apple-m1 on Darwin, generic Armv8-A on Linux.

```toml
[target]
platform = "linux"             # or "darwin"
cpu = "apple-m4"               # like -mcpu
arch = "armv8.6-a"             # like -march: added to the CPU's features
extensions = ["sve", "nolse"]  # like +sve and +nolse
```

And how `armtool fmt` and the editor lay code out ([0020](decisions/0020-formatter.md)):

```toml
[format]
indent = 4           # column for mnemonics and directives
mnemonic_width = 8   # operands start after this
comment_column = 40  # for trailing comments
max_blank_lines = 1
```

Silence one finding with a comment on its line, or on the line above:

```asm
    mov x18, #1    ; armtool: allow(abi/x18-reserved)
```

`armtool explain <rule>` prints a rule's page in the terminal. `armtool lint --fix` applies the fixes rules suggest, and the editor offers them as quick fixes ([0021](decisions/0021-quick-fixes.md)).

## Rules

### `abi/x18-reserved`

Uses x18, which Darwin reserves for the platform

Apple's arm64 ABI reserves x18 for the OS. The kernel may change it at any time, such as on a context switch, so a value kept there can vanish. Use another scratch register, like x9 to x15. Darwin only: Linux leaves x18 to programs.

Default level: warning.

### `abi/stack-alignment`

Moves sp by an amount that isn't a multiple of 16

sp must stay 16-byte aligned whenever it's used to access memory, or the CPU raises an alignment fault. Round stack adjustments up to a multiple of 16.

Default level: warning.

### `abi/frame-pointer`

A function that calls others doesn't set up x29

Darwin requires a valid chain of frame records for backtraces, profilers, and crash reports. A function that calls others should start with `stp x29, x30, [sp, #-16]!` and `mov x29, sp`. Darwin only: on Linux a frame pointer is optional.

Default level: warning.

### `abi/link-register`

A function calls another without saving x30

bl and blr write the return address to x30, replacing the caller's. A function that calls others must save x30 first, usually with `stp x29, x30, [sp, #-16]!`, and restore it before ret.

Default level: warning.

### `abi/callee-saved`

Writes x19 to x28 or d8 to d15 without saving them

Callers expect x19 to x28 and the low 64 bits of v8 to v15 to survive a call. A function that writes one must store it in its prologue and restore it before returning.

Default level: warning.

### `abi/callee-saved-restore`

Returns without restoring a callee-saved register it saved

The function saves x19 to x28 or d8 to d15, changes it, and then returns along some path without loading the saved value back. Its caller gets the changed value. Restore every saved register before every ret, usually with the ldp that mirrors the prologue's stp.

```asm
_f:
    stp x19, x20, [sp, #-16]!
    mov x19, x0
    cbz x0, 1f
    ldp x19, x20, [sp], #16
    ret
1:  ret              ; x19 is still changed on this path
```

Default level: warning.

### `addr/adrp-pageoff`

adrp loads a page that nothing adds the offset to

adrp gives only the 4 KB page of a symbol. The full address needs the offset within the page too: `add x0, x0, _sym@PAGEOFF`, or `ldr x0, [x0, _sym@PAGEOFF]`. On Linux: `add x0, x0, :lo12:sym`.

Default level: warning.

### `addr/variant-mismatch`

Pairs a page relocation with the wrong offset, like @GOTPAGE with @PAGEOFF

Each page variant pairs with one offset variant: @PAGE with @PAGEOFF, @GOTPAGE with @GOTPAGEOFF, and @TLVPPAGE with @TLVPPAGEOFF. On Linux, `adrp x0, sym` pairs with :lo12:, :got: with :got_lo12:, :gottprel: with :gottprel_lo12:, and :tlsdesc: with :tlsdesc_lo12:. Mixing them computes an address in the wrong place.

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

### `flow/undefined-register`

Reads a scratch register that nothing has set

x9 to x17 and v16 to v31 hold nothing useful when a function starts, and every call (bl, blr) may overwrite them. Reading one before setting it on every path gets garbage. Set it first, or keep the value in a callee-saved register (x19 to x28) across calls.

Default level: warning.

### `flow/flags-clobbered`

Reads the condition flags after a call that may have changed them

A call doesn't preserve the condition flags (NZCV): `cmp x0, #0`, then `bl _f`, then `b.eq` branches on whatever _f left behind. Compare after the call, or save the result before it with cset and test the register afterward. Calls to helpers in the same file are trusted, since they may return a result in the flags.

```asm
    cmp     x0, #0
    bl      _log
    b.eq    1f          ; tests _log's flags, not the cmp
```

Default level: warning.

### `types/call`

An argument or return value that doesn't match the annotated signature

With `; armtool: fn _f(x0: ptr<u8>, x1: i64) -> i32`, each call to _f is checked: x0 must hold a pointer and x1 a 64-bit integer where their types are known, and _f's own returns must leave an i32 in x0. A 32-bit signed value where 64 bits are wanted needs sign-extending first (sxtw). See docs/types.md.

Default level: warning.

### `types/signedness`

A signed test on unsigned values, or the reverse

After comparing unsigned values, b.gt, b.ge, b.lt, and b.le test the wrong flags; use b.hi, b.hs, b.lo, and b.ls. Signed values want the first set. sdiv and udiv must match their operands too. Types come from annotations; see docs/types.md.

Default level: warning.

### `types/memory`

A load or store that doesn't fit the pointer's type

Through a `ptr<i32>`, loads and stores move 4 bytes, offsets are multiples of 4, and pointer arithmetic moves by whole elements. Loading 8 bytes through it reads past the value; an offset of 6 lands mid-element. Using a non-pointer as an address is a finding too. Types come from annotations; see docs/types.md.

Default level: warning.

### `types/format`

printf arguments that don't match the format string

At a call to printf, fprintf, sprintf, snprintf, or dprintf, the format string is read from the data it points to, and each conversion is checked against its argument: `%d` wants an integer, `%f` a double, `%s` a pointer. On Darwin variadic arguments go on the stack, 8 bytes each, not in x1 and up; on Linux they go in registers. See docs/types.md.

```asm
    adrp    x0, fmt@PAGE
    add     x0, x0, fmt@PAGEOFF
    mov     x1, #42         ; on Darwin, this goes at [sp]
    bl      _printf
fmt: .asciz "%ld\n"
```

Default level: warning.

### `types/pointer-width`

A pointer read through a 32-bit W register

Pointers are 64 bits. Reading one as w0 instead of x0 keeps only the low half. On macOS every user-space address is above 4 GB, so the result is always wrong there; on Linux, heap and stack addresses usually are too. Use the X register.

Default level: warning.

### `types/annotation`

A type annotation comment that can't be read

`armtool:` comments declare types: `fn name(x0: T, ...) -> T` for a function, or `x9: T` for a register here. Types are i8 to i64, u8 to u64, f32, f64, ptr, and ptr<T>. See docs/types.md.

Default level: warning.

### `abi/syscall-number`

svc without the syscall number in x16 (Darwin) or x8 (Linux)

On Darwin, svc #0x80 takes the system call number in x16, not x8 as on Linux. Set x16 before the svc, for example mov x16, #4 for write. On Linux, svc #0 takes it in x8: mov x8, #64 for write.

Default level: warning.

### `elf/gnu-stack`

A Linux file with code but no .note.GNU-stack section

Without `.section .note.GNU-stack,"",%progbits`, the GNU linker assumes the code needs an executable stack and makes the whole program's stack executable, which turns stack overflows into code execution. Recent linkers warn about it. Add the line at the end of every assembly file. Linux only.

Quick fix: adds the section at the end of the file.

Default level: warning.

### `elf/function-symbol`

An exported Linux function without .type or .size

On ELF, `.type name, %function` marks a symbol as code and `.size name, .-name` gives its length. Without them, debuggers, profilers, and crash reports can't place the function, and the dynamic linker treats it as data. Add `.type` before the label and `.size` after the last instruction. Linux only.

```asm
        .globl  main
        .type   main, %function
main:
        mov     w0, #0
        ret
        .size   main, .-main
```

Quick fix: adds whichever is missing.

Default level: warning.

### `style/unused-label`

A label nothing refers to

The label isn't exported and nothing in the file branches to or loads it. Remove it, or export it with .globl if other files need it.

Default level: hint.
