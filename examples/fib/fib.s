// Prints the first Fibonacci numbers, computed recursively. Shows the parts
// of Apple's arm64 ABI that recursion needs: frame records, and callee-saved
// registers (x19 to x28) that survive calls.

        limit = 15                          ; defined above its use, so clang
                                            ; folds it into the cmp below

        .text
        .globl  _main
        .p2align 2
_main:
        stp     x29, x30, [sp, #-32]!
        mov     x29, sp
        stp     x19, x20, [sp, #16]         ; x19 and x20 must survive our calls,
                                            ; so we save the caller's values
        mov     x19, #0                     ; n
1:      cmp     x19, #limit
        b.gt    2f
        mov     x0, x19
        bl      fib
        mov     x20, x0
        sub     sp, sp, #16                 ; printf's variadic arguments go on the stack
        stp     x19, x20, [sp]
        adrp    x0, format@PAGE
        add     x0, x0, format@PAGEOFF
        bl      _printf
        add     sp, sp, #16
        add     x19, x19, #1
        b       1b
2:      mov     x0, #0
        ldp     x19, x20, [sp, #16]
        ldp     x29, x30, [sp], #32
        ret

// long fib(long n): fib(n - 1) + fib(n - 2), with fib(0) = 0 and fib(1) = 1.
        .p2align 2
fib:
        cmp     x0, #2
        b.lt    3f                          ; fib(0) = 0, fib(1) = 1
        stp     x29, x30, [sp, #-32]!
        mov     x29, sp
        stp     x19, x20, [sp, #16]
        mov     x19, x0                     ; keep n across the first call
        sub     x0, x19, #1
        bl      fib
        mov     x20, x0                     ; keep fib(n - 1) across the second
        sub     x0, x19, #2
        bl      fib
        add     x0, x0, x20
        ldp     x19, x20, [sp, #16]
        ldp     x29, x30, [sp], #32
3:      ret

        .section __TEXT,__cstring,cstring_literals
format:
        .asciz  "fib(%ld) = %ld\n"
