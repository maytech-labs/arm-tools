stp x29, x30, [sp, #-16]!
ldr x0, [x1, x2, lsl #3]
ldr x0, [x1], #8
adrp x0, _msg@PAGE
add x0, x0, _msg@PAGEOFF
ld1 {v0.4s-v3.4s}, [x0]
ld1 {v0.s}[1], [x0]
mov v0.s[1], w0
ldr x0, =0x12345678
csel x0, x1, x2, eq
fmov d0, #1.5
