//! Row types for the tables in `generated.rs`.

pub(crate) struct FeatureInfo {
    /// LLVM's predicate name without decoration: `HasNEON`.
    pub ident: &'static str,
    /// The name clang prints and `-march` takes: `neon`.
    pub name: &'static str,
}

/// What an operand class matches.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ClassKind {
    Invalid,
    /// An operand that may be left out.
    Optional,
    /// Literal text: `[`, `!`, `.16b`, `#0`.
    Token,
    /// A register from a set of registers.
    Register,
    /// Anything else, checked by a predicate: immediates, shifts, labels, lists.
    Custom,
}

pub(crate) struct ClassInfo {
    /// `GPR64`, `AddSubImm`, or a token's text.
    pub name: &'static str,
    pub kind: ClassKind,
    /// LLVM's predicate method for custom classes: `isAddSubImm`, `isImmInRange<0, 63>`.
    pub predicate: &'static str,
    /// LLVM's diagnostic kind on a near miss: `AddSubSecondSource`.
    pub diagnostic: &'static str,
    /// Clang's message for that diagnostic.
    pub message: &'static str,
}

/// Which kind of register a name is. Clang's parser tells them apart before
/// matching, and an operand slot accepts only some kinds.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum RegisterKind {
    /// General purpose, floating point scalar, and system: `x0`, `w0`, `sp`, `d0`.
    Scalar,
    /// NEON vector: `v0`.
    NeonVector,
    /// SVE data vector: `z0`.
    SveData,
    /// SVE predicate: `p0`.
    SvePredicate,
    /// SVE predicate as counter: `pn8`.
    SvePredicateAsCounter,
    /// SME ZA array and tiles: `za`, `za0.s`, `za1h.d`.
    Matrix,
    /// SME tile in a tile list: `{za0.d, za1.d}`.
    MatrixTileList,
    /// SME2 lookup table: `zt0`.
    LookupTable,
}

pub(crate) struct RegisterInfo {
    /// Lowercase: `x0`, `v0`, `za0h.s`.
    pub name: &'static str,
    pub kind: RegisterKind,
    /// LLVM's register: `X0`. `v0` and `q0` are both `Q0`.
    pub llvm: &'static str,
    /// Index into `CLASSES` of the register's most specific match class, or 0.
    pub class: u16,
}

pub(crate) struct FormInfo {
    /// LLVM's opcode: `ADDXri`.
    pub opcode: &'static str,
    /// 0 for the generic syntax, 1 for Apple's legacy NEON syntax.
    pub variant: u8,
    /// Index into `FEATURE_SETS`.
    pub features: u16,
    /// Start and length in `OPERANDS`.
    pub operands: (u32, u16),
}
