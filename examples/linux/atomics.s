// LSE atomics need Armv8.1-A. Clang's default CPU on Linux is generic
// Armv8-A, unlike Darwin's apple-m1, so .arch turns them on. Delete the
// .arch line to see the error.
        .arch   armv8.1-a
        .text
        .globl  main
        .p2align 2
        .type   main, %function
main:
        stp     x29, x30, [sp, #-16]!
        mov     x29, sp
        adrp    x9, counter
        add     x9, x9, :lo12:counter
        mov     x10, #1
        ldaddal x10, x11, [x9]          // add 1, get the old value in x11
        mov     x10, #41
        ldaddal x10, x11, [x9]
        ldr     x1, [x9]
        adrp    x0, format
        add     x0, x0, :lo12:format
        bl      printf
        mov     w0, #0
        ldp     x29, x30, [sp], #16
        ret
        .size   main, .-main

        .data
        .p2align 3
counter:
        .xword  0

        .section .rodata
format:
        .asciz  "counter = %ld\n"

        .section .note.GNU-stack,"",%progbits
