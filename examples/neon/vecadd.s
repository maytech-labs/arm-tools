// Adds two float arrays four lanes at a time with NEON, then prints the
// results. Hover over v0 or fadd to see the vector registers and forms.

        .text
        .globl  _main
        .p2align 2
_main:
        stp     x29, x30, [sp, #-32]!
        mov     x29, sp
        stp     x19, x20, [sp, #16]

        adrp    x0, a@PAGE
        add     x0, x0, a@PAGEOFF
        adrp    x1, b@PAGE
        add     x1, x1, b@PAGEOFF
        adrp    x2, result@PAGE
        add     x2, x2, result@PAGEOFF
        ld1     {v0.4s, v1.4s}, [x0]        ; eight floats from a
        ld1     {v2.4s, v3.4s}, [x1]        ; eight from b
        fadd    v0.4s, v0.4s, v2.4s         ; four lanes at a time
        fadd    v1.4s, v1.4s, v3.4s
        st1     {v0.4s, v1.4s}, [x2]

        adrp    x19, result@PAGE
        add     x19, x19, result@PAGEOFF
        mov     x20, #0
1:      ldr     s0, [x19, x20, lsl #2]
        fcvt    d0, s0                      ; printf takes doubles
        sub     sp, sp, #16
        str     d0, [sp]
        adrp    x0, format@PAGE
        add     x0, x0, format@PAGEOFF
        bl      _printf
        add     sp, sp, #16
        add     x20, x20, #1
        cmp     x20, #8
        b.lt    1b

        mov     x0, #0
        ldp     x19, x20, [sp, #16]
        ldp     x29, x30, [sp], #32
        ret

        .section __DATA,__data
        .p2align 4
a:      .float  1.0, 2.0, 3.0, 4.0, 5.0, 6.0, 7.0, 8.0
b:      .float  0.5, 0.5, 0.5, 0.5, 10.0, 20.0, 30.0, 40.0
result: .space  32

        .section __TEXT,__cstring,cstring_literals
format:
        .asciz  "%.1f\n"
