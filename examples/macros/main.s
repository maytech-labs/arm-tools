// Macros, .include, .rept, and .irp. Build from examples/: make macros

        .include "syscalls.s"

        .text
        .globl  _main
        .p2align 2
_main:
        print   greeting
        .irp    line, first, second
        print   \line
        .endr
        exit    0

        // Strings with lengths and no zero byte go in __const, not __cstring,
        // where the linker would merge them as C strings.
        .section __TEXT,__const
greeting:
        .ascii  "Macros expand where they're called:\n"
        greeting_len = . - greeting
first:
        .ascii  "  first, from .irp\n"
        first_len = . - first
second:
        .ascii  "  second, from .irp\n"
        second_len = . - second

        // .rept repeats lines: four zero quads.
        .section __DATA,__data
        .p2align 3
        .rept   4
        .quad   0
        .endr
