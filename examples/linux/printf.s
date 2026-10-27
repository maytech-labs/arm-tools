// Calling C. On Linux, printf's variadic arguments go in registers from x1
// on; on Darwin they go on the stack (see ../sum/sum.s).
        .text
        .globl  main
        .p2align 2
        .type   main, %function
main:
        stp     x29, x30, [sp, #-16]!
        mov     x29, sp
        adrp    x0, format
        add     x0, x0, :lo12:format
        mov     x1, #6
        mov     x2, #7
        mul     x3, x1, x2
        bl      printf
        mov     w0, #0
        ldp     x29, x30, [sp], #16
        ret
        .size   main, .-main

        .section .rodata
format:
        .asciz  "%ld x %ld = %ld\n"

        .section .note.GNU-stack,"",%progbits
