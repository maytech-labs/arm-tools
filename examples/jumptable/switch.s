// A switch statement as a jump table: a table of offsets from a base label,
// one per case, and an indirect branch (br) to the one selected.

        .text
        .globl  _main
        .p2align 2
_main:
        stp     x29, x30, [sp, #-32]!
        mov     x29, sp
        str     x19, [sp, #16]
        mov     x19, #0
1:      mov     x0, x19
        bl      name                        ; x0 = the name of day x19
        bl      _puts
        add     x19, x19, #1
        cmp     x19, #8                     ; 0 to 6, then one out of range
        b.lt    1b
        mov     x0, #0
        ldr     x19, [sp, #16]
        ldp     x29, x30, [sp], #32
        ret

// const char *name(long day): the day's name, or "?" outside 0 to 6.
        .p2align 2
name:
        cmp     x0, #6
        b.hi    other                       ; unsigned: negatives are out of range too
        adr     x1, table
        ldrsw   x2, [x1, x0, lsl #2]        ; the case's offset from the table
        add     x1, x1, x2
        br      x1

        .p2align 2
table:
        .long   sunday - table
        .long   monday - table
        .long   tuesday - table
        .long   wednesday - table
        .long   thursday - table
        .long   friday - table
        .long   saturday - table

sunday:
        adr     x0, s0
        ret
monday:
        adr     x0, s1
        ret
tuesday:
        adr     x0, s2
        ret
wednesday:
        adr     x0, s3
        ret
thursday:
        adr     x0, s4
        ret
friday:
        adr     x0, s5
        ret
saturday:
        adr     x0, s6
        ret
other:
        adr     x0, unknown
        ret

        // Kept next to the code so adr (within 1 MB) can reach them.
s0:     .asciz  "Sunday"
s1:     .asciz  "Monday"
s2:     .asciz  "Tuesday"
s3:     .asciz  "Wednesday"
s4:     .asciz  "Thursday"
s5:     .asciz  "Friday"
s6:     .asciz  "Saturday"
unknown:
        .asciz  "?"
