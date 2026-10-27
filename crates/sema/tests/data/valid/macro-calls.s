.macro m a, b=1, c:vararg
.quad \a + \b
.endm
m 1
m 1, 2, 3, 4
m b=4, a=2
.macro push2 a, b
stp \a, \b, [sp, #-16]!
.endm
push2 x29, x30
