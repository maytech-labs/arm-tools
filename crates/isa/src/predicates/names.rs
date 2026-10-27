//! Named operand values from LLVM's searchable tables in
//! `AArch64SystemOperands.td`, and the condition code names from
//! `AArch64AsmParser::parseCondCodeString`. LLVM looks names up ignoring
//! case.

fn listed(names: &[&str], name: &str) -> bool {
    names.iter().any(|n| n.eq_ignore_ascii_case(name))
}

/// `DB`: barrier options for `dmb`, `dsb`, and `isb`, most used first.
pub const BARRIER_OPTIONS: &[&str] = &[
    "sy", "ish", "ishld", "ishst", "osh", "oshld", "oshst", "nsh", "nshld", "nshst", "ld", "st",
];

/// `DB`: barrier options for `dmb`, `dsb`, and `isb`.
pub(super) fn is_barrier(name: &str) -> bool {
    listed(BARRIER_OPTIONS, name)
}

/// `TSB`: the only trace barrier option.
pub(super) fn is_tsb(name: &str) -> bool {
    name.eq_ignore_ascii_case("csync")
}

/// `DBnXS`: barrier options for `dsb ... nXS`.
pub(super) fn is_barrier_nxs(name: &str) -> bool {
    listed(&["oshnxs", "nshnxs", "ishnxs", "synxs"], name)
}

/// `PRFM` prefetch operations: type, target, and policy, like `pldl1keep`.
pub fn prefetch_operations() -> Vec<String> {
    let mut out = Vec::new();
    for ty in ["pld", "pli", "pst"] {
        for target in ["l1", "l2", "l3", "slc"] {
            for policy in ["keep", "strm"] {
                out.push(format!("{ty}{target}{policy}"));
            }
        }
    }
    out
}

/// `PRFM`, `SVEPRFM`, and `RPRFM`: prefetch operations. `isPrefetch` serves
/// all three, so a name from any of them is accepted.
pub(super) fn is_prefetch(name: &str) -> bool {
    let lower = name.to_ascii_lowercase();
    // PRFM: type, target, policy, like `pldl1keep` and `pstslcstrm`.
    let prfm = ["pld", "pli", "pst"].iter().any(|ty| {
        lower.strip_prefix(ty).is_some_and(|rest| {
            ["l1", "l2", "l3", "slc"].iter().any(|target| {
                rest.strip_prefix(target)
                    .is_some_and(|policy| policy == "keep" || policy == "strm")
            })
        })
    });
    // SVEPRFM is a subset of PRFM. RPRFM:
    prfm || listed(&["pldkeep", "pstkeep", "pldstrm", "pststrm"], name)
}

/// `SVEPREDPAT`: SVE predicate patterns, most used first.
pub const SVE_PATTERNS: &[&str] = &[
    "all", "pow2", "vl1", "vl2", "vl3", "vl4", "vl5", "vl6", "vl7", "vl8", "vl16", "vl32", "vl64",
    "vl128", "vl256", "mul4", "mul3",
];

/// `SVEPREDPAT`: SVE predicate patterns.
pub(super) fn is_sve_pattern(name: &str) -> bool {
    listed(SVE_PATTERNS, name)
}

/// `SVEVECLENSPECIFIER`.
pub(super) fn is_sve_vec_len_specifier(name: &str) -> bool {
    listed(&["vlx2", "vlx4"], name)
}

/// `PSB`.
pub(super) fn is_psb_hint(name: &str) -> bool {
    name.eq_ignore_ascii_case("csync")
}

/// `BTI`.
pub(super) fn is_bti_hint(name: &str) -> bool {
    listed(&["c", "j", "jc"], name)
}

/// `PHint`.
pub(super) fn is_phint(name: &str) -> bool {
    listed(&["keep", "strm"], name)
}

/// `SVCR`.
pub(super) fn is_svcr(name: &str) -> bool {
    listed(&["svcrsm", "svcrza", "svcrsmza"], name)
}

/// `PStateImm0_15`.
pub(super) fn is_pstate_imm0_15(name: &str) -> bool {
    listed(
        &[
            "spsel", "daifset", "daifclr", "pan", "uao", "dit", "ssbs", "tco",
        ],
        name,
    )
}

/// A system register by name, ignoring case: (name, readable, writable,
/// gated by a CPU feature).
pub(super) fn system_register(name: &str) -> Option<&'static (&'static str, bool, bool, bool)> {
    let lower = name.to_ascii_lowercase();
    let table = crate::generated::SYSTEM_REGISTERS;
    table
        .binary_search_by(|(n, ..)| n.cmp(&lower.as_str()))
        .ok()
        .map(|i| &table[i])
}

/// `S<op0>_<op1>_C<n>_C<m>_<op2>`, any system register by its encoding
/// (`AArch64SysReg::parseGenericRegister`).
pub(super) fn is_generic_system_register(name: &str) -> bool {
    let upper = name.to_ascii_uppercase();
    let parts: Vec<&str> = upper.split('_').collect();
    let number = |text: &str, max: u32| {
        !text.is_empty()
            && text.len() <= 2
            && !(text.len() == 2 && text.starts_with('0'))
            && text.parse::<u32>().is_ok_and(|n| n <= max)
    };
    match parts.as_slice() {
        [op0, op1, crn, crm, op2] => {
            op0.strip_prefix('S')
                .is_some_and(|n| n.len() == 1 && number(n, 3))
                && op1.len() == 1
                && number(op1, 7)
                && crn.strip_prefix('C').is_some_and(|n| number(n, 15))
                && crm.strip_prefix('C').is_some_and(|n| number(n, 15))
                && op2.len() == 1
                && number(op2, 7)
        }
        _ => false,
    }
}

/// `PStateImm0_1`.
pub(super) fn is_pstate_imm0_1(name: &str) -> bool {
    listed(&["allint", "pm"], name)
}

/// `parseCondCodeString`, including the SVE aliases.
pub(super) fn is_cond_code(name: &str) -> bool {
    listed(
        &[
            "eq", "ne", "cs", "hs", "cc", "lo", "mi", "pl", "vs", "vc", "hi", "ls", "ge", "lt",
            "gt", "le", "al", "nv", "none", "any", "nlast", "last", "first", "nfrst", "pmore",
            "plast", "tcont", "tstop",
        ],
        name,
    )
}

/// `tryParseSysCROperand`: `c0` to `c15`.
pub(super) fn is_sys_cr(name: &str) -> bool {
    let mut chars = name.chars();
    matches!(chars.next(), Some('c' | 'C'))
        && !chars.as_str().is_empty()
        && chars.as_str().bytes().all(|b| b.is_ascii_digit())
        && chars.as_str().parse::<u32>().is_ok_and(|n| n <= 15)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tables() {
        assert!(is_barrier("ISH") && is_barrier("sy") && !is_barrier("synxs"));
        assert!(is_barrier_nxs("synxs") && !is_barrier_nxs("sy"));
        assert!(is_prefetch("pldl1keep") && is_prefetch("PSTSLCSTRM") && is_prefetch("pststrm"));
        assert!(!is_prefetch("pldl4keep") && !is_prefetch("pld"));
        assert!(is_sve_pattern("vl256") && is_sve_pattern("ALL") && !is_sve_pattern("vl9"));
        assert!(is_cond_code("HS") && is_cond_code("tstop") && !is_cond_code("nfirst"));
        assert!(is_sys_cr("c15") && is_sys_cr("C05") && !is_sys_cr("c16") && !is_sys_cr("c"));
        assert!(is_pstate_imm0_15("DAIFSet") && !is_pstate_imm0_15("allint"));
        assert!(is_pstate_imm0_1("allint"));
    }
}
