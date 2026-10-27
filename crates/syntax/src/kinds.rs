//! Every kind of token and node in the tree, as one enum so rowan can store
//! both (docs/decisions/0008-lossless-syntax-tree.md).

macro_rules! syntax_kinds {
    ($($(#[$doc:meta])* $name:ident,)*) => {
        #[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
        #[repr(u16)]
        pub enum SyntaxKind {
            $($(#[$doc])* $name,)*
        }

        /// Every kind, indexed by its `u16` value.
        const ALL: &[SyntaxKind] = &[$(SyntaxKind::$name,)*];
    };
}

syntax_kinds! {
    // Tokens.

    /// Spaces, tabs, vertical tabs, and form feeds.
    Whitespace,
    /// `\n` or `\r\n`. Ends a statement.
    Newline,
    /// `; ...`, `// ...`, `/* ... */`, or `# ...` as the first non-blank on a line.
    Comment,
    /// `%%`. Ends a statement, like a newline.
    Separator,
    /// Names, directives, mnemonics, and registers: `_main`, `.globl`, `mov`, `x0`.
    Ident,
    /// Decimal, hex (`0x`), binary (`0b`), or floating point.
    Number,
    /// A reference to a local numeric label: `1f` (next `1:`) or `1b` (previous).
    LocalLabelRef,
    /// `"..."`, with backslash escapes.
    StringLit,
    /// `'a'`, a character constant.
    CharLit,
    /// `#`, the immediate prefix.
    Hash,
    Comma,
    Colon,
    LBracket,
    RBracket,
    LBrace,
    RBrace,
    LParen,
    RParen,
    /// `!`, pre-index writeback or logical not.
    Bang,
    Plus,
    Minus,
    Star,
    Slash,
    Percent,
    Amp,
    Pipe,
    Caret,
    Tilde,
    Lt,
    Gt,
    Shl,
    Shr,
    Eq,
    EqEq,
    /// `!=` or `<>`.
    NotEq,
    LtEq,
    GtEq,
    AmpAmp,
    PipePipe,
    /// `@`, the symbol variant marker in `_g@PAGEOFF`.
    At,
    /// `\`, which starts a macro argument reference: `\arg`.
    Backslash,
    /// A character the assembler doesn't accept here.
    Unknown,
    /// End of input. The lexer never produces it; the parser uses it as a sentinel.
    Eof,

    // Nodes.

    /// The whole file: statements, with newlines and separators between them.
    SourceFile,
    /// `name:`, `"quoted name":`, or a local numeric label `1:`.
    Label,
    /// A mnemonic and its operands: `add x0, x0, #1`. Inside a macro or repeat
    /// body, its operands are an `ArgList`: they aren't assembly until expanded.
    Instruction,
    /// A call to a macro defined earlier in the file. Arguments are an `ArgList`.
    MacroCall,
    /// A directive and its arguments: `.globl _main`.
    Directive,
    /// `name = expr`.
    Assignment,
    /// `name .req register`, a register alias.
    RegisterAlias,
    /// An instruction's operands, separated by commas.
    OperandList,
    /// A directive's arguments. Parsed leniently: sema checks each directive's form.
    ArgList,
    /// `#expr`.
    Immediate,
    /// `[base, offset]`, optionally followed by `!`.
    Memory,
    /// `{v0.4s, v1.4s}`, optionally followed by an index: `{v0.s}[1]`.
    RegisterList,
    /// `v0.4s-v3.4s` inside a register list.
    RegisterRange,
    /// A vector element: `v0.s[1]`.
    Indexed,
    /// A shift, extend, or SVE multiplier with its amount: `lsl #3`, `mul vl`.
    Shift,
    /// `=expr`, a literal pool load: `ldr x0, =sym`.
    LiteralPool,
    /// A register with writeback: `x2!` in `cpyfp [x0]!, [x1]!, x2!`.
    Writeback,
    /// A relocation specifier: `:lo12:sym`, `:got:sym`, `%pltpcrel(sym)`. ELF
    /// syntax: clang parses it on macOS but rejects it when writing Mach-O, so
    /// sema reports it.
    Specifier,
    /// A name: a register, symbol, mnemonic operand like `eq`, or `.`.
    NameRef,
    /// `@PAGE` and friends after a symbol, with optional arguments: `@AUTH(ia,42)`.
    Variant,
    /// A number, character, string, or local label reference.
    Literal,
    /// `-x`, `~x`, `!x`, `+x`.
    PrefixExpr,
    /// `a + b`, with Apple's precedence.
    BinExpr,
    /// `(expr)`.
    ParenExpr,
    /// `\name`, `\()`, or `\@` inside a macro body.
    MacroArg,
    /// Input the parser couldn't make sense of. A diagnostic says why, except
    /// inside macro and repeat bodies, which aren't checked until expanded.
    Error,
}

impl SyntaxKind {
    /// Whitespace and comments: tokens the parser skips but the tree keeps.
    pub fn is_trivia(self) -> bool {
        matches!(self, SyntaxKind::Whitespace | SyntaxKind::Comment)
    }

    /// Newlines, `%%`, and the end of input.
    pub fn ends_statement(self) -> bool {
        matches!(
            self,
            SyntaxKind::Newline | SyntaxKind::Separator | SyntaxKind::Eof
        )
    }

    pub(crate) fn from_raw(raw: u16) -> SyntaxKind {
        ALL.get(usize::from(raw))
            .copied()
            .unwrap_or(SyntaxKind::Error)
    }
}

/// The rowan language tag for AArch64 assembly.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum ArmLanguage {}

impl rowan::Language for ArmLanguage {
    type Kind = SyntaxKind;

    fn kind_from_raw(raw: rowan::SyntaxKind) -> SyntaxKind {
        SyntaxKind::from_raw(raw.0)
    }

    fn kind_to_raw(kind: SyntaxKind) -> rowan::SyntaxKind {
        rowan::SyntaxKind(kind as u16)
    }
}

impl From<SyntaxKind> for rowan::SyntaxKind {
    fn from(kind: SyntaxKind) -> Self {
        rowan::SyntaxKind(kind as u16)
    }
}

pub type SyntaxNode = rowan::SyntaxNode<ArmLanguage>;
pub type SyntaxToken = rowan::SyntaxToken<ArmLanguage>;
pub type SyntaxElement = rowan::SyntaxElement<ArmLanguage>;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn raw_round_trips_for_every_kind() {
        for &kind in ALL {
            assert_eq!(SyntaxKind::from_raw(kind as u16), kind);
        }
    }

    #[test]
    fn out_of_range_raw_is_error() {
        assert_eq!(SyntaxKind::from_raw(u16::MAX), SyntaxKind::Error);
    }
}
