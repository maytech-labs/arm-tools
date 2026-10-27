// Each line is a separate case: arm-syntax must report a syntax error, and on
// macOS, clang must reject the line. Lines starting with // are ignored.
mov x0, x1,
mov x0 x1
ldr x0, [x1
ldr x0, [x1 x2]
, nop
mov x0, #
mov x0, #(1
ld1 {v0.4s, v1.4s, [x0]
mov x0, `
.ascii "unterminated
mov x0, 0x
mov x0, 12abc
add x0, x0, #1 +
add x0, x0, #-
