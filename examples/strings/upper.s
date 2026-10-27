// Uppercases a string in place, one byte at a time, then prints it with puts.
// Shows byte loads and stores (ldrb, strb) and post-indexed addressing.

        .text
        .globl  _main
        .p2align 2
_main:
        stp     x29, x30, [sp, #-16]!
        mov     x29, sp
        adrp    x0, text@PAGE
        add     x0, x0, text@PAGEOFF
        bl      upper
        adrp    x0, text@PAGE
        add     x0, x0, text@PAGEOFF
        bl      _puts
        mov     x0, #0
        ldp     x29, x30, [sp], #16
        ret

// void upper(char *s): 'a' to 'z' become 'A' to 'Z'; everything else stays.
        .p2align 2
upper:
1:      ldrb    w1, [x0]                    ; the next byte
        cbz     w1, 3f                      ; the zero at the end?
        sub     w2, w1, #'a'                ; 0 to 25 for lowercase letters...
        cmp     w2, #25                     ; ...and unsigned-bigger otherwise
        b.hi    2f
        sub     w1, w1, #('a' - 'A')
        strb    w1, [x0]
2:      add     x0, x0, #1
        b       1b
3:      ret

        // Writable, so it lives in __DATA, not __TEXT.
        .section __DATA,__data
text:   .asciz  "shout this, please (but not the 123s)"
