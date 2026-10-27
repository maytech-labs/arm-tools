MODE = 2
.if MODE == 1
frob
.elseif MODE == 2
nop
.else
frob
.endif
