// A Linux program: ELF sections and symbol types, :lo12: addressing, the
// syscall number in x8, and ; as a separator.
        .text
        .globl  main
        .p2align 2
        .type   main, %function
main:
        .cfi_startproc
        stp     x29, x30, [sp, #-16]!
        mov     x29, sp
        adrp    x0, greeting
        add     x0, x0, :lo12:greeting
        bl      puts
        adrp    x1, :got:counter
        ldr     x1, [x1, :got_lo12:counter]
        ldr     w2, [x1]
        add     w2, w2, #1 ; str w2, [x1]
        mov     x0, #1
        adrp    x1, message
        add     x1, x1, :lo12:message
        mov     x2, #(message_end - message)
        mov     x8, #64                 // write
        svc     #0
        mov     x18, #0                 // free to use on Linux
        mov     w0, #(1 << 2 + 1)       // GNU precedence: (1 << 2) + 1
        ldp     x29, x30, [sp], #16
        ret
        .cfi_endproc
        .size   main, .-main

        .weak   helper
        .hidden helper
        .type   helper, @function
helper:
        cbz     x0, .Ldone
        sub     x0, x0, #1
        b       helper
.Ldone:
        ret
        .size   helper, .-helper

        .section .text.atomics,"ax",@progbits
        .arch   armv8.1-a
        .globl  bump
bump:
        mov     w1, #1
        ldaddal w1, w0, [x0]
        ret

        .section .rodata
greeting:
        .asciz  "hi"
message:
        .ascii  "hello\n"
message_end:

        .data
        .globl  counter
counter:
        .word   0

        .section .bss
        .balign 8
buffer:
        .zero   64

        .section .note.GNU-stack,"",%progbits
