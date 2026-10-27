// Copies stdin to stdout with the read and write system calls, 64 bytes at
// a time, until read returns 0 (end of input). No C library.
// Try: echo hello | ./build/echo

        bufsize = 64

        .text
        .globl  _main
        .p2align 2
_main:
        sub     sp, sp, #bufsize            ; the buffer, on the stack
1:      mov     x0, #0                      ; fd 0: stdin
        mov     x1, sp
        mov     x2, #bufsize
        mov     x16, #3                     ; read
        svc     #0x80
        b.cs    2f                          ; Darwin sets carry on error
        cbz     x0, 2f                      ; 0 bytes: end of input
        mov     x2, x0                      ; write what was read
        mov     x0, #1                      ; fd 1: stdout
        mov     x1, sp
        mov     x16, #4                     ; write
        svc     #0x80
        b       1b
2:      add     sp, sp, #bufsize
        mov     x0, #0
        mov     x16, #1                     ; exit
        svc     #0x80
