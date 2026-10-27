// The aliases clang handles in C++ (docs/decisions/0037-cpp-aliases.md).
.set n, 4
ubfx x0, x1, #4, #8
ubfx w0, w1, #4, #8
ubfx xzr, x1, #4, #8
ubfx w0, w1, #0, #32
ubfx x0, x1, #0, #64
ubfx x0, x1, #n, #8
ubfx x0, x1, #4, #(2*4)
ubfx x0, x1, 4, 8
sbfx x0, x1, #1, #63
bfxil x0, x1, #0, #64
ubfiz w0, w1, #4, #28
ubfiz w0, w1, #0, #32
sbfiz w0, w1, #31, #1
bfi w0, w1, #31, #1
bfi x0, xzr, #1, #2
bfc w0, #4, #8
bfc x0, #0, #64
lsl w0, w1, #31
lsl w0, w1, #0
lsl x0, x1, #63
lsl x0, x1, #n
lsl w0, w1, w2
dc civac, x0
dc zva, x0
dc civac, xzr
dc cvau, x9
ic ivau, x0
ic iallu
ic ialluis
tlbi vmalle1
tlbi vae1is, x0
tlbi aside1is, x0
tlbi vale1os, x0
