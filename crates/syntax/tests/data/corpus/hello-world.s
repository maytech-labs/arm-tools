# Hello world for macOS on Apple silicon.
// Build: clang -c -arch arm64 hello.s && ld -o hello hello.o -lSystem -syslibroot $(xcrun --show-sdk-path) -e _main

    .section __TEXT,__text,regular,pure_instructions
    .globl _main
    .p2align 2
_main:                                  ; entry point
    stp x29, x30, [sp, #-16]!           ; save frame
    mov x29, sp
    mov x2, #0                          ; loop counter
1:  cmp x2, #3
    b.ge 2f
    mov x0, #1                          ; fd: stdout
    adrp x1, msg@PAGE
    add x1, x1, msg@PAGEOFF
    mov x16, #4 %% stp x2, xzr, [sp, #-16]!
    mov x2, len
    svc #0x80                           /* write(2) */
    ldp x2, xzr, [sp], #16
    add x2, x2, #1
    b 1b
2:  mov x0, #0
    ldp x29, x30, [sp], #16
    ret

    .section __TEXT,__cstring,cstring_literals
msg:
    .asciz "Hello, \"arm\"\n"
    .set len, . - msg - 1
