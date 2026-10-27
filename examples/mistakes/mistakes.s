// Deliberately broken: open this in your editor to see what armls catches.
// Each line says what it should show. It doesn't build; that's the point.
// Try `gra` (code actions) on the ones marked "quick fix".

        .text
        .globl  _main
        .p2align 2
_main:
        stp     x29, x30, [sp, #-16]!
        mov     x29, sp
        movv    x0, x1                      ; unknown instruction, quick fix: mov
        add     x0, x0, #5000               ; immediate out of range for add
        ldp     x0, x1, [sp, #3]            ; offset must be a multiple of 8
        add     x0, w1, x2                  ; wrong register width
        b.eqq   _main                       ; invalid condition, quick fix: b.eq
        abs     x0, x1                      ; needs the cssc extension (off on apple-m1)
        cmp     x0, #later                  ; a constant defined below its use
        ldp     x29, x30, [sp], #16
        ret
        later = 3

// Lint rules that follow the code need a function they can read in full, so
// these are in functions with no errors.
        .globl  _lint
        .p2align 2
_lint:
        stp     x29, x30, [sp, #-16]!
        mov     x29, sp
        mov     x18, #1                     ; x18 is reserved by Darwin
        sub     sp, sp, #24                 ; misaligns sp, quick fix: 32
        adrp    x0, value@PAGE              ; no @PAGEOFF added, quick fix
        mov     x9, #1
        bl      helper
        add     x0, x9, #1                  ; x9 was clobbered by the call
        add     sp, sp, #32                 ; pops 8 more than it pushed: unbalanced stack
        ldp     x29, x30, [sp], #16
        ret
        mov     x0, #1                      ; unreachable after ret

helper:
        mov     x19, #7                     ; callee-saved x19 isn't saved
        ret

        .globl  _syscall
        .p2align 2
_syscall:
        mov     x0, #0
        mov     x8, #93                     ; Linux habit: the number goes in x16 on macOS
        svc     #0x80                       ; svc without x16 set
        ret

        .globl  _clamp
        .p2align 2
_clamp:
        stp     x19, x20, [sp, #-16]!
        mov     x19, x0
        cmp     x19, #100
        b.gt    1f
        ldp     x19, x20, [sp], #16
        ret
1:      add     sp, sp, #16                 ; balanced, but x19 isn't restored
        mov     x0, #100
        ret                                 ; returns with the caller's x19 changed

        .globl  _report
        .p2align 2
_report:
        stp     x29, x30, [sp, #-16]!
        mov     x29, sp
        cmp     x0, #0
        bl      _puts
        b.eq    1f                          ; tests _puts's flags, not the cmp
        mov     x0, #1
1:      ldp     x29, x30, [sp], #16
        ret

        .type   _main, @function            ; ELF only, quick fix: remove the line

        .section __DATA,__data
value:  .quad   42
