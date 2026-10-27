// Linux system calls with no C library: the number goes in x8, svc #0 calls
// the kernel, and errors come back as -errno in x0. Darwin uses x16 and
// svc #0x80 instead; see ../hello/hello.s.
        .text
        .globl  _start
        .p2align 2
        .type   _start, %function
_start:
        mov     x0, #1                  // stdout
        adrp    x1, message             // the page; ELF needs no @PAGE
        add     x1, x1, :lo12:message   // and the offset in it
        mov     x2, #(message_end - message)
        mov     x8, #64                 // write
        svc     #0
        mov     x0, #0
        mov     x8, #93                 // exit
        svc     #0
        .size   _start, .-_start

        .section .rodata
message:
        .ascii  "Hello from Linux on Arm\n"
message_end:

        .section .note.GNU-stack,"",%progbits
