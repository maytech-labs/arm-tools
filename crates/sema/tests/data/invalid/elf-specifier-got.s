adrp x0, :got:msg
ldr x0, [x0, :got_lo12:msg]
msg: nop
