// Atomic memory operations from Armv8.1's LSE extension, which every Apple
// silicon Mac has: ldaddal adds to memory and returns the old value; cas
// stores only if memory still holds what you expect. Hover ldaddal to see
// that it needs lse.

        .text
        .globl  _main
        .p2align 2
_main:
        stp     x29, x30, [sp, #-16]!
        mov     x29, sp
        adrp    x8, counter@PAGE
        add     x8, x8, counter@PAGEOFF

        mov     x1, #5
        ldaddal x1, x2, [x8]                ; counter += 5; x2 = what it was (10)

        mov     x3, #15                     ; expect 15 (it is), swap in 100
        mov     x4, #100
        casal   x3, x4, [x8]                ; x3 = what was there: 15, so it swapped

        mov     x5, #7                      ; expect 7 (it isn't): no swap
        mov     x6, #999
        casal   x5, x6, [x8]                ; x5 = what was there: 100

        ldr     x7, [x8]                    ; finally: 100
        sub     sp, sp, #32
        stp     x2, x3, [sp]
        stp     x5, x7, [sp, #16]
        adrp    x0, format@PAGE
        add     x0, x0, format@PAGEOFF
        bl      _printf
        add     sp, sp, #32

        mov     x0, #0
        ldp     x29, x30, [sp], #16
        ret

        .section __DATA,__data
        .p2align 3
counter:
        .quad   10

        .section __TEXT,__cstring,cstring_literals
format: .asciz  "ldaddal returned %ld\ncas saw %ld and swapped\ncas saw %ld and didn't\ncounter is %ld\n"
