// double dot(const double *a, const double *b, long n), called from main.c.
// Arguments arrive in x0, x1, x2; the double result goes back in d0. For
// each pair it calls report(i, product) in main.c.

// armtool: fn _dot(x0: ptr<f64>, x1: ptr<f64>, x2: i64) -> f64
// armtool: fn _report(x0: i64, d0: f64) -> void

        .text
        .globl  _dot
        .p2align 2
_dot:
        stp     x29, x30, [sp, #-64]!
        mov     x29, sp
        stp     x19, x20, [sp, #16]         ; pointers, kept across calls
        stp     x21, x22, [sp, #32]         ; count and index
        str     d8, [sp, #48]               ; the running sum: d8 survives calls
        mov     x19, x0
        mov     x20, x1
        mov     x21, x2
        mov     x22, #0
        fmov    d8, xzr
1:      cmp     x22, x21
        b.ge    2f
        ldr     d0, [x19, x22, lsl #3]
        ldr     d1, [x20, x22, lsl #3]
        fmul    d1, d0, d1                  ; product, the second argument...
        fadd    d8, d8, d1
        fmov    d0, d1                      ; ...goes in d0, since it's a double
        mov     x0, x22                     ; the index, the first argument
        bl      _report                     ; a C function in main.c
        add     x22, x22, #1
        b       1b
2:      fmov    d0, d8                      ; return the sum in d0
        ldr     d8, [sp, #48]
        ldp     x21, x22, [sp, #32]
        ldp     x19, x20, [sp, #16]
        ldp     x29, x30, [sp], #64
        ret
