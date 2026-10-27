// Functions main.s calls. Each is .globl, so the linker can connect calls
// from other object files to it.

        .text
        .globl  _print_line
        .p2align 2
// void print_line(const char *s): puts, which adds the newline.
_print_line:
        b       _puts                       ; a tail call: puts returns to our caller

        .globl  _count_down
        .p2align 2
// void count_down(long n): prints n, n-1, ..., 1, then calls back into main.s.
_count_down:
        stp     x29, x30, [sp, #-32]!
        mov     x29, sp
        str     x19, [sp, #16]
        mov     x19, x0
1:      cbz     x19, 2f
        sub     sp, sp, #16
        str     x19, [sp]                   ; printf's variadic argument, on the stack
        adrp    x0, number@PAGE
        add     x0, x0, number@PAGEOFF
        bl      _printf
        add     sp, sp, #16
        sub     x19, x19, #1
        b       1b
2:      bl      _liftoff                    ; defined in main.s
        ldr     x19, [sp, #16]
        ldp     x29, x30, [sp], #32
        ret

        .section __TEXT,__cstring,cstring_literals
number: .asciz  "  %ld\n"
