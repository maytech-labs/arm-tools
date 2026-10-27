// Bit tricks: count leading zeros (clz), reverse bits (rbit), and count set
// bits with NEON (cnt counts per byte, addv adds the bytes up).

        .text
        .globl  _main
        .p2align 2
_main:
        stp     x29, x30, [sp, #-16]!
        mov     x29, sp
        movz    x0, #0xbeef                 ; build 0x00f0deadbeef in pieces
        movk    x0, #0xdead, lsl #16
        movk    x0, #0x00f0, lsl #32

        clz     x1, x0                      ; leading zeros
        rbit    x2, x0                      ; bits reversed
        fmov    d0, x0                      ; popcount, through a vector register
        cnt     v0.8b, v0.8b
        addv    b0, v0.8b
        fmov    w3, s0

        sub     sp, sp, #32                 ; four variadic arguments for printf
        stp     x0, x1, [sp]
        stp     x2, x3, [sp, #16]
        adrp    x0, format@PAGE
        add     x0, x0, format@PAGEOFF
        bl      _printf
        add     sp, sp, #32

        mov     x0, #0
        ldp     x29, x30, [sp], #16
        ret

        .section __TEXT,__cstring,cstring_literals
format: .asciz  "value    %#llx\nclz      %llu\nrbit     %#llx\npopcount %llu\n"
