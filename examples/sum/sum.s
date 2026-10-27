// Sums an array of longs and prints the total with printf.
//
// Calling printf on Apple silicon has a twist: variadic arguments (everything
// after the format string) go on the stack, not in x1, x2, ... as on Linux.

        .text
        .globl  _main
        .p2align 2
_main:
        stp     x29, x30, [sp, #-16]!       ; save the frame record
        mov     x29, sp
        sub     sp, sp, #16                 ; room for printf's one variadic argument

        adrp    x0, numbers@PAGE
        add     x0, x0, numbers@PAGEOFF
        mov     x1, #count
        bl      sum

        str     x0, [sp]                    ; the total, as printf's first variadic argument
        adrp    x0, format@PAGE
        add     x0, x0, format@PAGEOFF
        bl      _printf

        mov     x0, #0
        add     sp, sp, #16
        ldp     x29, x30, [sp], #16
        ret

// long sum(const long *numbers, long count): a leaf function, so it needs no
// frame record, and it only uses scratch registers.
        .p2align 2
sum:
        mov     x9, #0                      ; the running total
1:      cbz     x1, 2f                      ; nothing left?
        ldr     x10, [x0], #8               ; load and step to the next one
        add     x9, x9, x10
        sub     x1, x1, #1
        b       1b
2:      mov     x0, x9
        ret

        .section __DATA,__data
        .p2align 3
numbers:
        .quad   3, 1, 4, 1, 5, 9, 2, 6
        count = (. - numbers) / 8

        .section __TEXT,__cstring,cstring_literals
format:
        .asciz  "sum = %ld\n"
