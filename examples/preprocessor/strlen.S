// A .S file (capital S) goes through the C preprocessor first: #include,
// #define, and #if work. armls runs the preprocessor too, so errors point at
// the lines you wrote.

#include "config.h"

// No `#` before the immediate here: inside a function-like macro, `#number`
// would be the preprocessor's stringize operator. Clang accepts a bare 4.
#define SYSCALL(number) mov x16, number %% svc #0x80
#define VERBOSE 1

        .text
        .globl  _main
        .p2align 2
_main:
        stp     x29, x30, [sp, #-16]!
        mov     x29, sp
        adrp    x0, message@PAGE
        add     x0, x0, message@PAGEOFF
        bl      length                      ; x0 = length of the string
        mov     x2, x0
        mov     x0, #STDOUT
        adrp    x1, message@PAGE
        add     x1, x1, message@PAGEOFF
        SYSCALL(SYS_WRITE)
#if VERBOSE
        mov     x0, #0
#else
        this line isn't assembled, so it isn't checked
#endif
        ldp     x29, x30, [sp], #16
        ret

// size_t length(const char *s): like strlen.
        .p2align 2
length:
        mov     x1, x0
1:      ldrb    w2, [x1], #1
        cbnz    w2, 1b
        sub     x0, x1, x0
        sub     x0, x0, #1
        ret

        .section __TEXT,__cstring,cstring_literals
message:
        .asciz  "Counted with a hand-written strlen\n"
