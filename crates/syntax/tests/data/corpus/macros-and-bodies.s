// Hand-written: macros, repeats, register aliases, and constructs from newer
// extensions. Must parse cleanly and assemble with clang.

        .macro  push2 a, b
        stp     \a, \b, [sp, #-16]!
        .endm
        .macro  pop2 a, b
        ldp     \a, \b, [sp], #16
        .endm
        .macro  zero reg
        mov     \reg, #0
        .endm

counter .req    x19
limit   .req    x20

        .text
        .globl  _count
        .p2align 2
_count:
        push2   x29, x30
        push2   counter, limit
        mov     x29, sp
        zero    counter
        mov     limit, x0
1:      cmp     counter, limit
        b.hs    2f
        add     counter, counter, #1
        b       1b
2:      mov     x0, counter
        pop2    counter, limit
        pop2    x29, x30
        ret

        .unreq  counter
        .unreq  limit

        .p2align 2
_table_code:
        .rept   4
        nop
        .endr
        .irp    r, x0, x1, x2
        mov     \r, xzr
        .endr
        ret

        .section __DATA,__data
        .p2align 3
_table:
        .quad   _count, (_table_code + 4), _table_code - _count
        .byte   1 << 2 + 1, 6 & 3 == 3, 10 % 4 * 2
        .if     1 + 1 == 2
        .long   0x7fffffff
        .else
        .long   0
        .endif
