//! How clang splits an instruction name before matching it
//! (`AArch64AsmParser::parseInstruction` in LLVM 21).

/// Instruction names clang handles in C++ instead of through the match table.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Special {
    /// `ic`, `dc`, `at`, `tlbi`, `cfp`, `dvp`, `cpp`, `cosp`: written as `SYS`.
    SysAlias,
    /// `tlbip`: written as `SYSP`.
    SyspAlias,
    /// `bfc`, `bfi`, `sbfiz`, `ubfiz`, `bfxil`, `sbfx`, `ubfx`: rewritten to
    /// `BFM`, `SBFM`, or `UBFM` after their operands are checked.
    Bitfield,
}

const SYS_ALIASES: &[&str] = &["ic", "dc", "at", "tlbi", "cfp", "dvp", "cpp", "cosp"];
const BITFIELD_ALIASES: &[&str] = &["bfc", "bfi", "sbfiz", "ubfiz", "bfxil", "sbfx", "ubfx"];

/// The condition codes clang accepts after `b.` and `bc.`.
pub const CONDITION_CODES: &[&str] = &[
    "eq", "ne", "cs", "hs", "cc", "lo", "mi", "pl", "vs", "vc", "hi", "ls", "ge", "lt", "gt", "le",
    "al", "nv",
];

/// SVE's names for condition codes (`b.none` is `b.eq`). Clang accepts them
/// wherever a condition goes, with or without SVE, but not run together
/// with `b` (`bnone`).
pub const SVE_CONDITION_ALIASES: &[&str] = &[
    "none", "any", "nlast", "last", "first", "nfrst", "pmore", "plast", "tcont", "tstop",
];

/// Whether clang accepts `name` as a condition: a code or an SVE alias.
pub fn is_condition(name: &str) -> bool {
    let lower = name.to_ascii_lowercase();
    CONDITION_CODES.contains(&lower.as_str()) || SVE_CONDITION_ALIASES.contains(&lower.as_str())
}

/// An instruction name split the way clang splits it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Mnemonic {
    /// Lowercase, without suffixes: `b` for `b.eq`, `fadd` for `fadd.4s`.
    pub head: String,
    /// The condition after `b.` or `bc.`, lowercase. Set for `beq` too, which
    /// clang reads as `b.eq`.
    pub condition: Option<String>,
    /// Suffixes with their dots, lowercase: `[".4s"]` for `fadd.4s`. They match
    /// token operands in Apple's NEON syntax.
    pub suffixes: Vec<String>,
}

impl Mnemonic {
    pub fn parse(name: &str) -> Mnemonic {
        let lower = name.to_ascii_lowercase();
        let lower = match lower.strip_prefix('b') {
            Some(cond) if CONDITION_CODES.contains(&cond) => format!("b.{cond}"),
            _ => lower,
        };
        let mut parts = lower.split('.');
        let head = parts.next().unwrap_or_default().to_string();
        let mut condition = None;
        if head == "b" || head == "bc" {
            condition = parts.next().map(str::to_string);
        }
        let suffixes = parts.map(|s| format!(".{s}")).collect();
        Mnemonic {
            head,
            condition,
            suffixes,
        }
    }

    /// Whether clang handles this name outside the match table.
    pub fn special(&self) -> Option<Special> {
        let head = self.head.as_str();
        if SYS_ALIASES.contains(&head) {
            Some(Special::SysAlias)
        } else if head == "tlbip" {
            Some(Special::SyspAlias)
        } else if BITFIELD_ALIASES.contains(&head) {
            Some(Special::Bitfield)
        } else {
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn split(name: &str) -> (String, Option<String>, Vec<String>) {
        let m = Mnemonic::parse(name);
        (m.head, m.condition, m.suffixes)
    }

    #[test]
    fn plain_and_uppercase() {
        assert_eq!(split("ADD"), ("add".into(), None, vec![]));
    }

    #[test]
    fn conditional_branches() {
        assert_eq!(split("b.eq"), ("b".into(), Some("eq".into()), vec![]));
        assert_eq!(split("bc.NE"), ("bc".into(), Some("ne".into()), vec![]));
        assert_eq!(split("bne"), ("b".into(), Some("ne".into()), vec![]));
        // `bl` and `blr` aren't conditions.
        assert_eq!(split("bl"), ("bl".into(), None, vec![]));
        assert_eq!(split("blr"), ("blr".into(), None, vec![]));
    }

    #[test]
    fn apple_neon_suffix() {
        assert_eq!(split("fadd.4s"), ("fadd".into(), None, vec![".4s".into()]));
    }

    #[test]
    fn special_names() {
        assert_eq!(Mnemonic::parse("tlbi").special(), Some(Special::SysAlias));
        assert_eq!(Mnemonic::parse("TLBIP").special(), Some(Special::SyspAlias));
        assert_eq!(Mnemonic::parse("ubfx").special(), Some(Special::Bitfield));
        assert_eq!(Mnemonic::parse("add").special(), None);
    }
}
