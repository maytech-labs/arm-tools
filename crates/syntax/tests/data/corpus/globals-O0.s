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
	adrp	x9, _counter@PAGE
	ldr	w8, [x9, _counter@PAGEOFF]
	add	w8, w8, #1
	str	w8, [x9, _counter@PAGEOFF]
	adrp	x9, _shared@PAGE
	ldr	w9, [x9, _shared@PAGEOFF]
	add	w8, w8, w9
	adrp	x0, _tls_value@TLVPPAGE
	ldr	x0, [x0, _tls_value@TLVPPAGEOFF]
	ldr	x9, [x0]
	blr	x9
	ldr	w9, [x0]
	add	w8, w8, w9
	adrp	x9, _external@GOTPAGE
	ldr	x9, [x9, _external@GOTPAGEOFF]
	ldr	w9, [x9]
	add	w0, w8, w9
	ldp	x29, x30, [sp], #16             ; 16-byte Folded Reload
	ret
	.cfi_endproc
                                        ; -- End function
	.globl	_sum                            ; -- Begin function sum
	.p2align	2
_sum:                                   ; @sum
	.cfi_startproc
; %bb.0:
	sub	sp, sp, #32
	.cfi_def_cfa_offset 32
	str	w0, [sp, #28]
	add	x9, sp, #16
	add	x8, sp, #32
	str	x8, [x9]
	str	wzr, [sp, #12]
	str	wzr, [sp, #8]
	b	LBB1_1
LBB1_1:                                 ; =>This Inner Loop Header: Depth=1
	ldr	w8, [sp, #8]
	ldr	w9, [sp, #28]
	subs	w8, w8, w9
	b.ge	LBB1_4
	b	LBB1_2
LBB1_2:                                 ;   in Loop: Header=BB1_1 Depth=1
	ldr	x9, [sp, #16]
	ldr	w8, [x9]
	add	x9, x9, #8
	str	x9, [sp, #16]
	str	w8, [sp, #4]
	ldr	w9, [sp, #4]
	ldr	w8, [sp, #12]
	add	w8, w8, w9
	str	w8, [sp, #12]
	b	LBB1_3
LBB1_3:                                 ;   in Loop: Header=BB1_1 Depth=1
	ldr	w8, [sp, #8]
	add	w8, w8, #1
	str	w8, [sp, #8]
	b	LBB1_1
LBB1_4:
	ldr	w0, [sp, #12]
	add	sp, sp, #32
	ret
	.cfi_endproc
                                        ; -- End function
	.globl	_classify                       ; -- Begin function classify
	.p2align	2
_classify:                              ; @classify
	.cfi_startproc
; %bb.0:
	sub	sp, sp, #16
	.cfi_def_cfa_offset 16
	str	w0, [sp, #8]
	ldr	w8, [sp, #8]
	str	w8, [sp, #4]                    ; 4-byte Folded Spill
	cbz	w8, LBB2_6
	b	LBB2_1
LBB2_1:
	ldr	w8, [sp, #4]                    ; 4-byte Folded Reload
	subs	w8, w8, #1
	b.eq	LBB2_7
	b	LBB2_2
LBB2_2:
	ldr	w8, [sp, #4]                    ; 4-byte Folded Reload
	subs	w8, w8, #2
	b.eq	LBB2_8
	b	LBB2_3
LBB2_3:
	ldr	w8, [sp, #4]                    ; 4-byte Folded Reload
	subs	w8, w8, #3
	b.eq	LBB2_9
	b	LBB2_4
LBB2_4:
	ldr	w8, [sp, #4]                    ; 4-byte Folded Reload
	subs	w8, w8, #4
	b.eq	LBB2_10
	b	LBB2_5
LBB2_5:
	ldr	w8, [sp, #4]                    ; 4-byte Folded Reload
	subs	w8, w8, #5
	b.eq	LBB2_11
	b	LBB2_12
LBB2_6:
	mov	w8, #10                         ; =0xa
	str	w8, [sp, #12]
	b	LBB2_13
LBB2_7:
	mov	w8, #22                         ; =0x16
	str	w8, [sp, #12]
	b	LBB2_13
LBB2_8:
	mov	w8, #31                         ; =0x1f
	str	w8, [sp, #12]
	b	LBB2_13
LBB2_9:
	mov	w8, #47                         ; =0x2f
	str	w8, [sp, #12]
	b	LBB2_13
LBB2_10:
	mov	w8, #5                          ; =0x5
	str	w8, [sp, #12]
	b	LBB2_13
LBB2_11:
	mov	w8, #69                         ; =0x45
	str	w8, [sp, #12]
	b	LBB2_13
LBB2_12:
	mov	w8, #-1                         ; =0xffffffff
	str	w8, [sp, #12]
	b	LBB2_13
LBB2_13:
	ldr	w0, [sp, #12]
	add	sp, sp, #16
	ret
	.cfi_endproc
                                        ; -- End function
	.globl	_scale                          ; -- Begin function scale
	.p2align	2
_scale:                                 ; @scale
	.cfi_startproc
; %bb.0:
	sub	sp, sp, #16
	.cfi_def_cfa_offset 16
	str	d0, [sp, #8]
	str	s1, [sp, #4]
	ldr	d0, [sp, #8]
	ldr	s1, [sp, #4]
	fcvt	d1, s1
	fmov	d2, #3.25000000
	fdiv	d2, d1, d2
	fmov	d1, #1.50000000
	fmadd	d0, d0, d1, d2
	add	sp, sp, #16
	ret
	.cfi_endproc
                                        ; -- End function
	.globl	_swap                           ; -- Begin function swap
	.p2align	2
_swap:                                  ; @swap
	.cfi_startproc
; %bb.0:
	sub	sp, sp, #32
	.cfi_def_cfa_offset 32
	str	x0, [sp]
	str	x1, [sp, #8]
	ldr	x8, [sp, #8]
	str	x8, [sp, #16]
	ldr	x8, [sp]
	str	x8, [sp, #24]
	ldr	x0, [sp, #16]
	ldr	x1, [sp, #24]
	add	sp, sp, #32
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

.subsections_via_symbols
