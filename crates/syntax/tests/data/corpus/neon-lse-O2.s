	.build_version macos, 26, 0	sdk_version 27, 0
	.section	__TEXT,__text,regular,pure_instructions
	.globl	_add4                           ; -- Begin function add4
	.p2align	2
_add4:                                  ; @add4
	.cfi_startproc
; %bb.0:
	cmp	w3, #4
	b.lt	LBB0_3
; %bb.1:
	mov	w8, w3
	mov	w9, #4                          ; =0x4
LBB0_2:                                 ; =>This Inner Loop Header: Depth=1
	ldr	q0, [x1], #16
	ldr	q1, [x2], #16
	fadd.4s	v0, v0, v1
	str	q0, [x0], #16
	add	x9, x9, #4
	cmp	x9, x8
	b.ls	LBB0_2
LBB0_3:
	ret
	.cfi_endproc
                                        ; -- End function
	.globl	_dot                            ; -- Begin function dot
	.p2align	2
_dot:                                   ; @dot
	.cfi_startproc
; %bb.0:
	movi.2d	v0, #0000000000000000
	cmp	w2, #4
	b.lt	LBB1_3
; %bb.1:
	mov	w8, w2
	mov	w9, #4                          ; =0x4
LBB1_2:                                 ; =>This Inner Loop Header: Depth=1
	ldr	q1, [x0], #16
	ldr	q2, [x1], #16
	fmla.4s	v0, v2, v1
	add	x9, x9, #4
	cmp	x9, x8
	b.ls	LBB1_2
LBB1_3:
	faddp.4s	v0, v0, v0
	faddp.2s	s0, v0
	ret
	.cfi_endproc
                                        ; -- End function
	.globl	_lookup                         ; -- Begin function lookup
	.p2align	2
_lookup:                                ; @lookup
	.cfi_startproc
; %bb.0:
	tbl.16b	v0, { v0 }, v1
	ret
	.cfi_endproc
                                        ; -- End function
	.globl	_atomic_inc                     ; -- Begin function atomic_inc
	.p2align	2
_atomic_inc:                            ; @atomic_inc
	.cfi_startproc
; %bb.0:
	mov	w8, #1                          ; =0x1
	ldaddal	x8, x0, [x0]
	ret
	.cfi_endproc
                                        ; -- End function
.subsections_via_symbols
