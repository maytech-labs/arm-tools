// Macros shared by main.s through .include. Go to definition on a macro call
// in main.s jumps here.

        sys_exit = 1
        sys_write = 4
        stdout = 1

// write(stdout, label, label_len)
        .macro  print label
        mov     x0, #stdout
        adrp    x1, \label@PAGE
        add     x1, x1, \label@PAGEOFF
        mov     x2, #\label\()_len
        mov     x16, #sys_write
        svc     #0x80
        .endm

        .macro  exit code=0
        mov     x0, #\code
        mov     x16, #sys_exit
        svc     #0x80
        .endm
