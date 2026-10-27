	.build_version macos, 26, 0	sdk_version 27, 0
	.section	__TEXT,__text,regular,pure_instructions
	.globl	_bump                           ; -- Begin function bump
	.p2align	2
_bump:                                  ; @bump
	.cfi_startproc
; %bb.0:
	stp	x29, x30, [sp, #-16]!           ; 16-byte Folded Spill
	mov	x29, sp
	.cfi_def_cfa w29, 16
	.cfi_offset w30, -8
	.cfi_offset w29, -16
Lloh0:
	adrp	x8, _counter@PAGE
	ldr	w9, [x8, _counter@PAGEOFF]
	add	w9, w9, #1
	str	w9, [x8, _counter@PAGEOFF]
Lloh1:
	adrp	x8, _shared@PAGE
Lloh2:
	ldr	w8, [x8, _shared@PAGEOFF]
	add	w8, w8, w9
Lloh3:
	adrp	x0, _tls_value@TLVPPAGE
Lloh4:
	ldr	x0, [x0, _tls_value@TLVPPAGEOFF]
	ldr	x9, [x0]
	blr	x9
	ldr	w9, [x0]
Lloh5:
	adrp	x10, _external@GOTPAGE
Lloh6:
	ldr	x10, [x10, _external@GOTPAGEOFF]
Lloh7:
	ldr	w10, [x10]
	add	w8, w8, w10
	add	w0, w8, w9
	ldp	x29, x30, [sp], #16             ; 16-byte Folded Reload
	ret
	.loh AdrpLdrGotLdr	Lloh5, Lloh6, Lloh7
	.loh AdrpLdr	Lloh3, Lloh4
	.loh AdrpLdr	Lloh1, Lloh2
	.loh AdrpAdrp	Lloh0, Lloh1
	.cfi_endproc
                                        ; -- End function
	.globl	_sum                            ; -- Begin function sum
	.p2align	2
_sum:                                   ; @sum
	.cfi_startproc
; %bb.0:
	sub	sp, sp, #16
	.cfi_def_cfa_offset 16
	add	x8, sp, #16
	str	x8, [sp, #8]
	cmp	w0, #1
	b.lt	LBB1_3
; %bb.1:
	mov	w8, #0                          ; =0x0
LBB1_2:                                 ; =>This Inner Loop Header: Depth=1
	ldr	x9, [sp, #8]
	add	x10, x9, #8
	str	x10, [sp, #8]
	ldr	w9, [x9]
	add	w8, w9, w8
	subs	w0, w0, #1
	b.ne	LBB1_2
	b	LBB1_4
LBB1_3:
	mov	w8, #0                          ; =0x0
LBB1_4:
	mov	x0, x8
	add	sp, sp, #16
	ret
	.cfi_endproc
                                        ; -- End function
	.globl	_classify                       ; -- Begin function classify
	.p2align	2
_classify:                              ; @classify
	.cfi_startproc
; %bb.0:
	cmp	w0, #5
	b.hi	LBB2_2
; %bb.1:
Lloh8:
	adrp	x8, l_switch.table.classify@PAGE
Lloh9:
	add	x8, x8, l_switch.table.classify@PAGEOFF
	ldr	w0, [x8, w0, uxtw #2]
	ret
LBB2_2:
	mov	w0, #-1                         ; =0xffffffff
	ret
	.loh AdrpAdd	Lloh8, Lloh9
	.cfi_endproc
                                        ; -- End function
	.globl	_scale                          ; -- Begin function scale
	.p2align	2
_scale:                                 ; @scale
	.cfi_startproc
; %bb.0:
	fcvt	d1, s1
	fmov	d2, #3.25000000
	fdiv	d1, d1, d2
	fmov	d2, #1.50000000
	fmadd	d0, d0, d2, d1
	ret
	.cfi_endproc
                                        ; -- End function
	.globl	_swap                           ; -- Begin function swap
	.p2align	2
_swap:                                  ; @swap
	.cfi_startproc
; %bb.0:
	mov	x8, x0
	mov	x0, x1
	mov	x1, x8
	ret
	.cfi_endproc
                                        ; -- End function
	.section	__DATA,__data
	.globl	_shared                         ; @shared
	.p2align	2, 0x0
_shared:
	.long	7                               ; 0x7

	.section	__TEXT,__cstring,cstring_literals
l_.str:                                 ; @.str
	.asciz	"a"

l_.str.1:                               ; @.str.1
	.asciz	"b"

l_.str.2:                               ; @.str.2
	.asciz	"c"

	.section	__DATA,__data
	.globl	_names                          ; @names
	.p2align	3, 0x0
_names:
	.quad	l_.str
	.quad	l_.str.1
	.quad	l_.str.2

.zerofill __DATA,__bss,_counter,4,2     ; @counter
.tbss _tls_value$tlv$init, 4, 2         ; @tls_value

	.section	__DATA,__thread_vars,thread_local_variables
	.globl	_tls_value
_tls_value:
	.quad	__tlv_bootstrap
	.quad	0
	.quad	_tls_value$tlv$init

	.section	__TEXT,__const
	.p2align	2, 0x0                          ; @switch.table.classify
l_switch.table.classify:
	.long	10                              ; 0xa
	.long	22                              ; 0x16
	.long	31                              ; 0x1f
	.long	47                              ; 0x2f
	.long	5                               ; 0x5
	.long	69                              ; 0x45

.subsections_via_symbols
