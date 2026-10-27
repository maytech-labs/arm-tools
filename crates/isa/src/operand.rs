//! Clang's operand model (`AArch64Operand` in LLVM's AArch64AsmParser.cpp),
//! which operand classes are checked against. `arm-sema` builds these from
//! the syntax tree; the matcher checks them against each form.

use crate::Register;

/// One operand, as clang's parser produces it. Memory operands are
/// flattened into tokens: `[x0, #8]!` is `[`, `x0`, `#8`, `]`, `!`.
#[derive(Debug, Clone, PartialEq)]
pub enum Operand {
    /// Literal text, lowercase: `[`, `]`, `!`, `{`, `.`, or a NEON
    /// arrangement suffix like `.4s` that follows a vector register.
    Token(String),
    Register(RegisterOperand),
    /// An immediate or expression: `#4`, `4`, `_sym@PAGEOFF`, `-(1 << 3)`.
    Imm(Imm),
    /// An immediate with a shift, `#1, lsl #12`, which clang reads as one
    /// operand where an add/sub or SVE immediate is expected.
    ShiftedImm {
        value: Imm,
        shift: u32,
    },
    /// A floating-point immediate: `#1.5`, `#-0.25`.
    FpImm(f64),
    /// A condition code where one is expected: `eq`, `hs`. Lowercase.
    CondCode(String),
    /// `lsl #3`, `sxtw`, `uxtw #2`, `mul vl`. Lowercase kind.
    ShiftExtend {
        kind: String,
        amount: Option<i64>,
        /// Written with an explicit amount, even `#0`.
        explicit: bool,
    },
    /// `{v0.4s, v1.4s}`, `{v0.4s-v3.4s}`, `{z0.d, z8.d}` (strided).
    VectorList {
        first: Register,
        count: u32,
        /// Distance between registers: 1 for consecutive.
        stride: u32,
        /// The arrangement or element suffix, lowercase, without the dot:
        /// `4s`, `d`. None if the list didn't give one.
        suffix: Option<String>,
    },
    /// The `[1]` after a vector register or list.
    VectorIndex(i64),
    /// A bare name that isn't a register: a symbol, or a keyword some
    /// instructions take, like a barrier option (`ish`), prefetch operation
    /// (`pldl1keep`), system register (`tpidr_el0`), or SVE pattern (`vl8`).
    /// Lowercase is not applied: symbols are case-sensitive.
    Ident(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RegisterOperand {
    pub register: Register,
    /// The element or arrangement suffix, lowercase, without the dot:
    /// `4s` for `v0.4s`, `d` for `z0.d`. For NEON registers the matcher also
    /// sees it as a separate [`Operand::Token`].
    pub suffix: Option<String>,
    /// A shift or extend clang parses as part of this register in some
    /// classes, like `x1, lsl #3` in an SVE address.
    pub shift: Option<(String, Option<i64>)>,
}

/// An immediate value as far as the assembler can know it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Imm {
    /// A constant expression's value, with Apple's semantics: 64-bit,
    /// wrapping, true is -1.
    Constant(i64),
    /// A symbol, with its `@` variant and constant addend: `_g@PAGEOFF + 8`.
    Symbol {
        name: String,
        /// Lowercase: `pageoff`, `page`, `gotpageoff`, `tlvppageoff`.
        variant: Option<String>,
        /// ELF relocation specifier, lowercase: `lo12` for `:lo12:sym`.
        specifier: Option<String>,
        addend: i64,
    },
    /// An expression clang folds later, or one we can't evaluate.
    Other,
}

impl Imm {
    pub fn constant(&self) -> Option<i64> {
        match self {
            Imm::Constant(value) => Some(*value),
            _ => None,
        }
    }
}

impl Operand {
    pub fn constant(&self) -> Option<i64> {
        match self {
            Operand::Imm(imm) => imm.constant(),
            _ => None,
        }
    }
}

/// What a predicate says about an operand, as LLVM's `DiagnosticPredicate`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Check {
    Match,
    /// The right kind of operand, but wrong: out of range, misaligned. The
    /// class's message explains it.
    NearMiss,
    NoMatch,
    /// The predicate isn't ported yet. The matcher treats it as a match, so a
    /// missing port can't cause a false error.
    Unknown,
}
