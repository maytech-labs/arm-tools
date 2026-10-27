// Sorts an array of signed longs with insertion sort, then prints it.
// Shows nested loops, signed comparisons (b.le), and indexed addressing
// with a shift: [x0, x2, lsl #3] is x0 + x2 * 8.

        count = 10

        .text
        .globl  _main
        .p2align 2
_main:
        stp     x29, x30, [sp, #-32]!
        mov     x29, sp
        stp     x19, x20, [sp, #16]
        adrp    x0, numbers@PAGE
        add     x0, x0, numbers@PAGEOFF
        mov     x1, #count
        bl      sort

        adrp    x19, numbers@PAGE
        add     x19, x19, numbers@PAGEOFF
        mov     x20, #0
1:      ldr     x1, [x19, x20, lsl #3]
        sub     sp, sp, #16
        str     x1, [sp]                    ; printf's variadic argument, on the stack
        adrp    x0, format@PAGE
        add     x0, x0, format@PAGEOFF
        bl      _printf
        add     sp, sp, #16
        add     x20, x20, #1
        cmp     x20, #count
        b.lt    1b
        adrp    x0, newline@PAGE
        add     x0, x0, newline@PAGEOFF
        bl      _puts

        mov     x0, #0
        ldp     x19, x20, [sp, #16]
        ldp     x29, x30, [sp], #32
        ret

// void sort(long *a, long n): insertion sort, ascending.
        .p2align 2
sort:
        mov     x2, #1                      ; i: the next element to place
1:      cmp     x2, x1
        b.ge    4f
        ldr     x3, [x0, x2, lsl #3]        ; key = a[i]
        sub     x4, x2, #1                  ; j = i - 1
2:      tbnz    x4, #63, 3f                 ; j < 0: stop
        ldr     x5, [x0, x4, lsl #3]
        cmp     x5, x3
        b.le    3f                          ; a[j] <= key: found the spot
        add     x6, x4, #1
        str     x5, [x0, x6, lsl #3]        ; shift a[j] right
        sub     x4, x4, #1
        b       2b
3:      add     x6, x4, #1
        str     x3, [x0, x6, lsl #3]        ; a[j + 1] = key
        add     x2, x2, #1
        b       1b
4:      ret

        .section __DATA,__data
        .p2align 3
numbers:
        .quad   42, -7, 19, 0, 88, -100, 3, 3, 64, -1

        .section __TEXT,__cstring,cstring_literals
format: .asciz  "%ld "
newline:
        .asciz  ""
