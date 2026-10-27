	.build_version macos, 26, 0	sdk_version 27, 0
	.section	__TEXT,__text,regular,pure_instructions
	.globl	_add4                           ; -- Begin function add4
	.p2align	2
_add4:                                  ; @add4
	.cfi_startproc
; %bb.0:
	sub	sp, sp, #192
	.cfi_def_cfa_offset 192
	str	x0, [sp, #136]
	str	x1, [sp, #128]
	str	x2, [sp, #120]
	str	w3, [sp, #116]
	str	wzr, [sp, #112]
	b	LBB0_1
LBB0_1:                                 ; =>This Inner Loop Header: Depth=1
	ldr	w8, [sp, #112]
	add	w8, w8, #4
	ldr	w9, [sp, #116]
	subs	w8, w8, w9
	b.gt	LBB0_4
	b	LBB0_2
LBB0_2:                                 ;   in Loop: Header=BB0_1 Depth=1
	ldr	x8, [sp, #128]
	ldrsw	x9, [sp, #112]
	lsl	x9, x9, #2
	ldr	q0, [x8, x9]
	str	q0, [sp, #64]
	ldr	q0, [sp, #64]
	str	q0, [sp, #80]
	ldr	q0, [sp, #80]
	str	q0, [sp, #48]
	ldr	q1, [sp, #48]
	ldr	x8, [sp, #120]
	ldrsw	x9, [sp, #112]
	lsl	x9, x9, #2
	ldr	q0, [x8, x9]
	str	q0, [sp, #16]
	ldr	q0, [sp, #16]
	str	q0, [sp, #32]
	ldr	q0, [sp, #32]
	str	q0, [sp]
	ldr	q0, [sp]
	str	q1, [sp, #176]
	str	q0, [sp, #160]
	ldr	q0, [sp, #176]
	ldr	q1, [sp, #160]
	fadd.4s	v0, v0, v1
	str	q0, [sp, #144]
	ldr	q0, [sp, #144]
	str	q0, [sp, #96]
	ldr	x8, [sp, #136]
	ldrsw	x9, [sp, #112]
	lsl	x9, x9, #2
	ldr	q0, [sp, #96]
	str	q0, [x8, x9]
	b	LBB0_3
LBB0_3:                                 ;   in Loop: Header=BB0_1 Depth=1
	ldr	w8, [sp, #112]
	add	w8, w8, #4
	str	w8, [sp, #112]
	b	LBB0_1
LBB0_4:
	add	sp, sp, #192
	ret
	.cfi_endproc
                                        ; -- End function
	.globl	_dot                            ; -- Begin function dot
	.p2align	2
_dot:                                   ; @dot
	.cfi_startproc
; %bb.0:
	sub	sp, sp, #336
	stp	x28, x27, [sp, #320]            ; 16-byte Folded Spill
	.cfi_def_cfa_offset 336
	.cfi_offset w27, -8
	.cfi_offset w28, -16
	str	x0, [sp, #152]
	str	x1, [sp, #144]
	str	w2, [sp, #140]
	add	x8, sp, #204
	movi.2d	v0, #0000000000000000
	str	s0, [sp, #204]
	ldr	s0, [sp, #204]
                                        ; kill: def $q0 killed $s0
	ld1.s	{ v0 }[1], [x8]
	ld1.s	{ v0 }[2], [x8]
	ld1.s	{ v0 }[3], [x8]
	str	q0, [sp, #160]
	ldr	q0, [sp, #160]
	str	q0, [sp, #176]
	ldr	q0, [sp, #176]
	str	q0, [sp, #112]
	str	wzr, [sp, #108]
	b	LBB1_1
LBB1_1:                                 ; =>This Inner Loop Header: Depth=1
	ldr	w8, [sp, #108]
	add	w8, w8, #4
	ldr	w9, [sp, #140]
	subs	w8, w8, w9
	b.gt	LBB1_4
	b	LBB1_2
LBB1_2:                                 ;   in Loop: Header=BB1_1 Depth=1
	ldr	q2, [sp, #112]
	ldr	x8, [sp, #152]
	ldrsw	x9, [sp, #108]
	lsl	x9, x9, #2
	ldr	q0, [x8, x9]
	str	q0, [sp, #64]
	ldr	q0, [sp, #64]
	str	q0, [sp, #80]
	ldr	q0, [sp, #80]
	str	q0, [sp, #48]
	ldr	q1, [sp, #48]
	ldr	x8, [sp, #144]
	ldrsw	x9, [sp, #108]
	lsl	x9, x9, #2
	ldr	q0, [x8, x9]
	str	q0, [sp, #16]
	ldr	q0, [sp, #16]
	str	q0, [sp, #32]
	ldr	q0, [sp, #32]
	str	q0, [sp]
	ldr	q0, [sp]
	str	q2, [sp, #272]
	str	q1, [sp, #256]
	str	q0, [sp, #240]
	ldr	q0, [sp, #272]
	ldr	q2, [sp, #256]
	ldr	q1, [sp, #240]
	fmla.4s	v0, v1, v2
	str	q0, [sp, #208]
	ldr	q0, [sp, #208]
	str	q0, [sp, #224]
	ldr	q0, [sp, #224]
	str	q0, [sp, #112]
	b	LBB1_3
LBB1_3:                                 ;   in Loop: Header=BB1_1 Depth=1
	ldr	w8, [sp, #108]
	add	w8, w8, #4
	str	w8, [sp, #108]
	b	LBB1_1
LBB1_4:
	ldr	q0, [sp, #112]
	str	q0, [sp, #304]
	ldr	q0, [sp, #304]
	faddp.4s	v0, v0, v0
                                        ; kill: def $d0 killed $d0 killed $q0
	faddp.2s	s0, v0
	str	s0, [sp, #296]
	ldr	s0, [sp, #296]
	str	s0, [sp, #300]
	ldr	s0, [sp, #300]
	ldp	x28, x27, [sp, #320]            ; 16-byte Folded Reload
	add	sp, sp, #336
	ret
	.cfi_endproc
                                        ; -- End function
	.globl	_lookup                         ; -- Begin function lookup
	.p2align	2
_lookup:                                ; @lookup
	.cfi_startproc
; %bb.0:
	sub	sp, sp, #96
	.cfi_def_cfa_offset 96
	str	q0, [sp, #16]
	str	q1, [sp]
	ldr	q1, [sp, #16]
	ldr	q0, [sp]
	str	q1, [sp, #80]
	str	q0, [sp, #64]
	ldr	q0, [sp, #80]
	ldr	q1, [sp, #64]
	tbl.16b	v0, { v0 }, v1
	str	q0, [sp, #32]
	ldr	q0, [sp, #32]
	str	q0, [sp, #48]
	ldr	q0, [sp, #48]
	add	sp, sp, #96
	ret
	.cfi_endproc
                                        ; -- End function
	.globl	_atomic_inc                     ; -- Begin function atomic_inc
	.p2align	2
_atomic_inc:                            ; @atomic_inc
	.cfi_startproc
; %bb.0:
	sub	sp, sp, #32
	.cfi_def_cfa_offset 32
	str	x0, [sp, #24]
	ldr	x9, [sp, #24]
	mov	x8, #1                          ; =0x1
	str	x8, [sp, #16]
	ldr	x8, [sp, #16]
	ldaddal	x8, x8, [x9]
	str	x8, [sp, #8]
	ldr	x0, [sp, #8]
	add	sp, sp, #32
	ret
	.cfi_endproc
                                        ; -- End function
.subsections_via_symbols
