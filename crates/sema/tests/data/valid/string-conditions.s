.ifc abc, abc
nop
.else
frob
.endif
.ifeqs "a", "b"
frob
.endif
.ifb
nop
.endif
