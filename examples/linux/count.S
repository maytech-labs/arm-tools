// A .S file goes through the C preprocessor first. armls runs it for Linux,
// so __linux__ is defined, as it is in your build.
#ifndef __linux__
#error "count.S is for Linux"
#endif

#define SYS_WRITE 64
#define SYS_EXIT 93
#define COUNT 5

        .text
        .globl  _start
        .p2align 2
        .type   _start, %function
_start:
        mov     x19, #COUNT
1:      mov     x0, #1
        adrp    x1, star
        add     x1, x1, :lo12:star
        mov     x2, #1
        mov     x8, #SYS_WRITE
        svc     #0
        subs    x19, x19, #1
        b.ne    1b
        mov     x0, #1
        adrp    x1, newline
        add     x1, x1, :lo12:newline
        mov     x2, #1
        mov     x8, #SYS_WRITE
        svc     #0
        mov     x0, #0
        mov     x8, #SYS_EXIT
        svc     #0
        .size   _start, .-_start

        .section .rodata
star:
        .ascii  "*"
newline:
        .ascii  "\n"

        .section .note.GNU-stack,"",%progbits
