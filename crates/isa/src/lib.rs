//! AArch64 instruction tables, generated from LLVM's assembler matcher
//! (docs/decisions/0013-isa-from-llvm-tablegen.md,
//! docs/decisions/0014-isa-from-asm-matcher.md).
//!
//! A [`Form`] is one way to write an instruction: a mnemonic and a sequence of
//! [`OperandClass`]es, as clang's matcher sees them. Memory operands are
//! flattened into tokens, so `ldp x0, x1, [sp, #16]!` is
//! `GPR64, GPR64, [, GPR64sp, SImm7s8, ], !`.

mod aliases;
mod descriptions;
mod docs;
mod features;
#[rustfmt::skip]
mod generated;
mod matcher;
mod mnemonic;
mod operand;
mod predicates;
mod tables;

pub use aliases::{SysOp, alias_signature, sys_operations};
pub use descriptions::instruction_description;
pub use docs::{Signature, directive_description, syscall_name};
pub use features::FeatureSet;
use generated::*;
pub use matcher::{MatchResult, Mismatch, match_for, match_instruction, match_prefix};
pub use mnemonic::{CONDITION_CODES, Mnemonic, SVE_CONDITION_ALIASES, Special, is_condition};
pub use operand::{Check, Imm, Operand, RegisterOperand};
pub use predicates::{BARRIER_OPTIONS, SVE_PATTERNS, check, prefetch_operations};
pub use tables::{ClassKind, RegisterKind};

/// The LLVM release the tables come from.
pub fn llvm_version() -> &'static str {
    LLVM_VERSION
}

/// Every mnemonic in the match table, sorted. Doesn't include the
/// [`Special`] names clang handles in C++.
pub fn mnemonics() -> impl Iterator<Item = &'static str> {
    MNEMONICS.iter().map(|(name, _, _)| *name)
}

/// Whether clang knows `name` as an instruction, after splitting it the way
/// clang does (`b.eq`, `fadd.4s`, `bne`).
pub fn is_instruction(name: &str) -> bool {
    let mnemonic = Mnemonic::parse(name);
    mnemonic.special().is_some() || find(&mnemonic.head).is_some()
}

/// The forms of a mnemonic, in the order clang tries them. `head` is the
/// lowercase name without suffixes; see [`Mnemonic::parse`].
pub fn forms(head: &str) -> impl Iterator<Item = Form> + use<> {
    let range = find(head).map_or(0..0, |(_, start, len)| {
        let start = *start as usize;
        start..start + usize::from(*len)
    });
    range.map(Form)
}

fn find(head: &str) -> Option<&'static (&'static str, u32, u16)> {
    MNEMONICS
        .binary_search_by(|(name, _, _)| name.cmp(&head))
        .ok()
        .map(|i| &MNEMONICS[i])
}

pub use arm_syntax::Platform;

/// Directive names clang accepts on macOS, sorted, with their leading `.`.
pub fn directives() -> impl Iterator<Item = &'static str> {
    DIRECTIVES.iter().copied()
}

/// Directive names clang accepts for a platform, sorted.
pub fn directives_for(platform: Platform) -> impl Iterator<Item = &'static str> {
    directive_table(platform).iter().copied()
}

fn directive_table(platform: Platform) -> &'static [&'static str] {
    match platform {
        Platform::Darwin => DIRECTIVES,
        Platform::Linux => ELF_DIRECTIVES,
    }
}

/// Whether clang accepts `name` as a directive on macOS, ignoring case.
pub fn is_directive(name: &str) -> bool {
    is_directive_for(name, Platform::Darwin)
}

/// Whether clang accepts `name` as a directive for a platform, ignoring case.
pub fn is_directive_for(name: &str, platform: Platform) -> bool {
    let name = name.to_ascii_lowercase();
    directive_table(platform)
        .binary_search(&name.as_str())
        .is_ok()
}

/// Which syntax a form is written in.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Syntax {
    /// `fadd v0.4s, v1.4s, v2.4s`.
    Generic,
    /// Apple's legacy NEON syntax: `fadd.4s v0, v1, v2`.
    AppleNeon,
}

/// One way to write an instruction.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Form(usize);

impl Form {
    fn info(self) -> &'static tables::FormInfo {
        &FORMS[self.0]
    }

    pub fn mnemonic(self) -> &'static str {
        let i = MNEMONICS.partition_point(|(_, start, _)| *start as usize <= self.0);
        MNEMONICS[i.saturating_sub(1)].0
    }

    /// LLVM's opcode name, like `ADDXri`. Stable within an LLVM release.
    pub fn opcode(self) -> &'static str {
        self.info().opcode
    }

    pub fn syntax(self) -> Syntax {
        match self.info().variant {
            0 => Syntax::Generic,
            _ => Syntax::AppleNeon,
        }
    }

    /// The CPU features the form needs, all of them.
    pub fn features(self) -> impl Iterator<Item = Feature> + use<> {
        FEATURE_SETS[usize::from(self.info().features)]
            .iter()
            .map(|&i| Feature(usize::from(i)))
    }

    pub fn operands(self) -> impl Iterator<Item = OperandClass> + use<> {
        let (start, len) = self.info().operands;
        let start = start as usize;
        OPERANDS[start..start + usize::from(len)]
            .iter()
            .map(|&c| OperandClass(usize::from(c)))
    }
}

/// A CPU feature an instruction can need, like `neon` or `sve2`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Feature(usize);

impl Feature {
    /// The name clang prints and `-march` takes: `neon`, `sve2`, `armv8.1a`.
    pub fn name(self) -> &'static str {
        FEATURES[self.0].name
    }

    /// LLVM's predicate name: `HasNEON`.
    pub fn ident(self) -> &'static str {
        FEATURES[self.0].ident
    }
}

/// What one operand position accepts.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct OperandClass(usize);

impl OperandClass {
    fn info(self) -> &'static tables::ClassInfo {
        &CLASSES[self.0]
    }

    /// `GPR64`, `AddSubImm`, or a token's text like `[` or `.16b`.
    pub fn name(self) -> &'static str {
        self.info().name
    }

    pub fn kind(self) -> ClassKind {
        self.info().kind
    }

    /// LLVM's predicate for a custom class, like `isImmInRange<0, 63>`. Empty
    /// for tokens and registers.
    pub fn predicate(self) -> &'static str {
        self.info().predicate
    }

    /// LLVM's diagnostic kind when an operand nearly fits, like
    /// `InvalidMemoryIndexed8`. Empty if there's none.
    pub fn diagnostic(self) -> &'static str {
        self.info().diagnostic
    }

    /// Clang's message when an operand nearly fits this class, if it has one.
    pub fn message(self) -> Option<&'static str> {
        Some(self.info().message).filter(|m| !m.is_empty())
    }

    /// Whether everything in `self` is also in `other`, as clang's matcher
    /// decides: a `GPR64common` register fits a `GPR64` slot.
    pub fn is_subclass_of(self, other: OperandClass) -> bool {
        if self == other {
            return true;
        }
        let (offset, start, len) = SUBCLASS_TABLE[self.0];
        let Some(index) = other.0.checked_sub(usize::from(start)) else {
            return false;
        };
        if index >= usize::from(len) {
            return false;
        }
        let bit = offset as usize + index;
        SUBCLASS_DATA
            .get(bit / 8)
            .is_some_and(|byte| (byte >> (bit % 8)) & 1 == 1)
    }
}

/// A register name clang accepts, with its kind.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Register(usize);

impl Register {
    /// Every register name clang accepts, sorted by name.
    pub fn all() -> impl Iterator<Item = Register> {
        (0..REGISTERS.len()).map(Register)
    }

    /// Every reading of a name, ignoring case. Most names have one; a few, like
    /// `za0.d`, are both a [`RegisterKind::Matrix`] and a tile list entry.
    pub fn lookup(name: &str) -> impl Iterator<Item = Register> + use<> {
        let name = name.to_ascii_lowercase();
        let start = REGISTERS.partition_point(|r| r.name < name.as_str());
        (start..REGISTERS.len())
            .take_while(move |&i| REGISTERS[i].name == name)
            .map(Register)
    }

    /// The reading of `name` as a register of `kind`, as clang's parser asks
    /// for it.
    pub fn lookup_kind(name: &str, kind: RegisterKind) -> Option<Register> {
        Register::lookup(name).find(|r| r.kind() == kind)
    }

    /// The name as written in the table, lowercase.
    pub fn name(self) -> &'static str {
        REGISTERS[self.0].name
    }

    pub fn kind(self) -> RegisterKind {
        REGISTERS[self.0].kind
    }

    /// LLVM's register: `X0`. Names like `v0` and `q0` share one.
    pub fn llvm_name(self) -> &'static str {
        REGISTERS[self.0].llvm
    }

    /// The most specific operand class the register belongs to.
    pub fn class(self) -> Option<OperandClass> {
        Some(OperandClass(usize::from(REGISTERS[self.0].class))).filter(|c| c.0 != 0)
    }

    /// Whether the register fits an operand of class `class`.
    pub fn fits(self, class: OperandClass) -> bool {
        self.class().is_some_and(|c| c.is_subclass_of(class))
    }
}

/// Looks up an operand class by name, for tests and tools: `GPR64`, `[`.
pub fn operand_class(name: &str) -> Option<OperandClass> {
    CLASSES
        .iter()
        .position(|c| c.name == name)
        .map(OperandClass)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn class(name: &str) -> OperandClass {
        operand_class(name).unwrap_or_else(|| panic!("no class {name}"))
    }

    #[test]
    fn tables_come_from_llvm_21() {
        assert_eq!(llvm_version(), "21.1.8");
    }

    #[test]
    fn mnemonics_are_sorted_and_unique() {
        let all: Vec<_> = mnemonics().collect();
        assert!(all.windows(2).all(|w| w[0] < w[1]));
        assert!(all.len() > 1500, "{}", all.len());
    }

    #[test]
    fn add_immediate_form() {
        let form = forms("add")
            .find(|f| f.opcode() == "ADDXri")
            .expect("ADDXri");
        let operands: Vec<_> = form.operands().map(OperandClass::name).collect();
        assert_eq!(operands, ["GPR64sp", "GPR64sp", "AddSubImm"]);
        assert_eq!(form.mnemonic(), "add");
        assert_eq!(form.syntax(), Syntax::Generic);
        assert_eq!(form.features().count(), 0);
    }

    #[test]
    fn memory_operands_are_flattened() {
        let form = forms("ldp")
            .find(|f| f.opcode() == "LDPXpre")
            .expect("LDPXpre");
        let operands: Vec<_> = form.operands().map(OperandClass::name).collect();
        assert_eq!(
            operands,
            ["GPR64", "GPR64", "[", "GPR64sp", "SImm7s8", "]", "!"]
        );
    }

    #[test]
    fn features() {
        let abs = forms("abs").find(|f| f.opcode() == "ABSWr").expect("ABSWr");
        let names: Vec<_> = abs.features().map(Feature::name).collect();
        assert_eq!(names, ["cssc"]);
    }

    #[test]
    fn apple_neon_syntax_forms() {
        assert!(forms("fadd").any(|f| f.syntax() == Syntax::AppleNeon));
    }

    #[test]
    fn messages() {
        let message = class("AddSubImm").message().expect("a message");
        assert!(message.contains("[0, 4095]"), "{message}");
    }

    fn scalar(name: &str) -> Register {
        Register::lookup_kind(name, RegisterKind::Scalar).unwrap_or_else(|| panic!("no {name}"))
    }

    #[test]
    fn registers() {
        let x0 = scalar("X0");
        assert_eq!(x0.llvm_name(), "X0");
        assert!(x0.fits(class("GPR64")));
        assert!(x0.fits(class("GPR64sp")));
        assert!(!x0.fits(class("GPR32")));

        let sp = scalar("sp");
        assert!(sp.fits(class("GPR64sp")));
        assert!(!sp.fits(class("GPR64")));

        let xzr = scalar("xzr");
        assert!(xzr.fits(class("GPR64")));
        assert!(!xzr.fits(class("GPR64sp")));
    }

    #[test]
    fn register_aliases() {
        assert_eq!(scalar("x31").llvm_name(), "XZR");
        assert_eq!(scalar("w31").llvm_name(), "WZR");
        assert_eq!(scalar("fp").llvm_name(), "FP");
        assert_eq!(scalar("lr").llvm_name(), "LR");
        assert!(Register::lookup("x32").next().is_none());
    }

    #[test]
    fn register_kinds() {
        let kind = |name| Register::lookup(name).next().map(Register::kind);
        assert_eq!(kind("v31"), Some(RegisterKind::NeonVector));
        assert_eq!(kind("q31"), Some(RegisterKind::Scalar));
        assert_eq!(kind("z0"), Some(RegisterKind::SveData));
        assert_eq!(kind("p15"), Some(RegisterKind::SvePredicate));
        assert_eq!(kind("pn8"), Some(RegisterKind::SvePredicateAsCounter));
        assert_eq!(kind("zt0"), Some(RegisterKind::LookupTable));
        let za0d: Vec<_> = Register::lookup("ZA0.D").map(Register::kind).collect();
        assert!(
            za0d.contains(&RegisterKind::Matrix) && za0d.contains(&RegisterKind::MatrixTileList)
        );
        assert_eq!(
            Register::lookup_kind("v0", RegisterKind::NeonVector).map(Register::llvm_name),
            Some("Q0")
        );
    }

    #[test]
    fn directive_names() {
        for name in [
            ".text",
            ".globl",
            ".p2align",
            ".cfi_startproc",
            ".macro",
            ".SECTION",
        ] {
            assert!(is_directive(name), "{name}");
        }
        // ELF-only, so clang on macOS rejects them.
        for name in [".type", ".size", ".frob"] {
            assert!(!is_directive(name), "{name}");
        }
    }

    #[test]
    fn instruction_names() {
        for name in ["add", "B.EQ", "bne", "fadd.4s", "tlbi", "ubfx", "ldaddal"] {
            assert!(is_instruction(name), "{name}");
        }
        assert!(!is_instruction("frobnicate"));
    }
}
