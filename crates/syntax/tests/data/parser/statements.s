_start: _alias:          ; two labels, one line
1:  b 1b
"quoted name": nop
    .globl _start
    .section __TEXT,__text,regular,pure_instructions
counter = 4
. = . + 8
    nop %% ret
