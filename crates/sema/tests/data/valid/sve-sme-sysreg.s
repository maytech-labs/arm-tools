// SVE movprfx pairs, SME array indexes and ranges, and system registers
// by name and by encoding (docs/decisions/0043-recall.md).
        .arch_extension sve
        .arch_extension sme2
        .arch_extension sme-i16i64
        movprfx z0, z1
        add     z0.s, p0/m, z0.s, z2.s
        movprfx z3.s, p1/m, z4.s
        fadd    z3.s, p1/m, z3.s, z5.s
        add     za.d[w8, 0, vgx2], {z0.d, z1.d}
        add     za.s[w9, 7], {z0.s - z3.s}
        smlall  za.s[w8, 0:3], z0.b, z1.b
        mrs     x0, tpidr_el0
        msr     tpidr_el0, x0
        mrs     x1, midr_el1
        mrs     x2, s3_0_c15_c2_0
        msr     S3_7_C15_C15_7, x2
