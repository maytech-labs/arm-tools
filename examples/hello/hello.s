// Hello, world with raw Darwin system calls: no C library calls at all.
// Build and run: make hello (from examples/)

        .text
        .globl  _main
        .p2align 2
_main:
        mov     x0, #1                      ; fd 1: stdout
        adrp    x1, message@PAGE            ; the 4 KB page of the string...
        add     x1, x1, message@PAGEOFF     ; ...plus its offset in the page
        mov     x2, #message_len
        mov     x16, #4                     ; syscall 4: write
        svc     #0x80

        mov     x0, #0                      ; exit code 0
        mov     x16, #1                     ; syscall 1: exit
        svc     #0x80

        // Not __cstring: the linker assumes strings there end in a zero byte and
        // may merge them. A string with a length and no zero goes in __const.
        .section __TEXT,__const
message:
        .ascii  "Hello from AArch64 assembly!\n"
        message_len = . - message
