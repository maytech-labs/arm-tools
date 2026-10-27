// A program in two files, assembled separately and linked together. gd on
// _print_line or _count_down jumps to text.s; grr on one lists its uses in
// both files.

        .text
        .globl  _main
        .p2align 2
_main:
        stp     x29, x30, [sp, #-16]!
        mov     x29, sp
        adrp    x0, title@PAGE
        add     x0, x0, title@PAGEOFF
        bl      _print_line                 ; defined in text.s
        mov     x0, #3
        bl      _count_down                 ; also in text.s, which calls back here
        mov     x0, #0
        ldp     x29, x30, [sp], #16
        ret

// Called from text.s: prints "liftoff". A global label, so other files can
// link to it.
        .globl  _liftoff
        .p2align 2
_liftoff:
        stp     x29, x30, [sp, #-16]!
        mov     x29, sp
        adrp    x0, done@PAGE
        add     x0, x0, done@PAGEOFF
        bl      _print_line
        ldp     x29, x30, [sp], #16
        ret

        .section __TEXT,__cstring,cstring_literals
title:  .asciz  "Counting down, across two files:"
done:   .asciz  "liftoff"
