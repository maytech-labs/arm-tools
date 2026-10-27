//! Tokens to a lossless tree (docs/decisions/0008-lossless-syntax-tree.md).
//!
//! The parser never fails. Input it can't make sense of goes into an `Error`
//! node with a diagnostic, and parsing resumes at the next statement. Trivia
//! (whitespace and comments) attaches to the innermost node open when it's
//! reached, so a statement's node starts at its first real token and trailing
//! comments sit between statements.

use std::collections::HashSet;
use std::ops::Range;

use rowan::{Checkpoint, GreenNode, GreenNodeBuilder};

use crate::SyntaxKind::{self, *};
use crate::{SyntaxNode, ast, lex};

/// A problem in the source, with its byte range.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SyntaxError {
    pub range: Range<usize>,
    pub message: String,
}

/// The result of parsing: a tree that covers every byte, and what went wrong.
#[derive(Debug, Clone)]
pub struct Parse {
    green: GreenNode,
    errors: Vec<SyntaxError>,
}

impl Parse {
    pub fn syntax(&self) -> SyntaxNode {
        SyntaxNode::new_root(self.green.clone())
    }

    pub fn tree(&self) -> ast::SourceFile {
        ast::SourceFile::new(self.syntax())
    }

    /// Lexer and parser errors, in source order.
    pub fn errors(&self) -> &[SyntaxError] {
        &self.errors
    }

    /// The tree as indented text, one node or token per line, followed by the
    /// errors. For tests and debugging.
    pub fn debug_tree(&self) -> String {
        use std::fmt::Write;
        let mut out = String::new();
        let mut depth = 0;
        for event in self.syntax().preorder_with_tokens() {
            match event {
                rowan::WalkEvent::Enter(element) => {
                    let indent = "  ".repeat(depth);
                    let range = element.text_range();
                    let _ = match &element {
                        rowan::NodeOrToken::Node(node) => {
                            depth += 1;
                            writeln!(out, "{indent}{:?}@{range:?}", node.kind())
                        }
                        rowan::NodeOrToken::Token(token) => writeln!(
                            out,
                            "{indent}{:?}@{range:?} {:?}",
                            token.kind(),
                            token.text()
                        ),
                    };
                }
                rowan::WalkEvent::Leave(rowan::NodeOrToken::Node(_)) => depth -= 1,
                rowan::WalkEvent::Leave(rowan::NodeOrToken::Token(_)) => {}
            }
        }
        for error in &self.errors {
            let _ = writeln!(out, "error {:?}: {}", error.range, error.message);
        }
        out
    }
}

/// Parses a whole file. Never fails; see the module docs.
pub fn parse(text: &str) -> Parse {
    let lexed = lex(text);
    let mut parser = Parser {
        text,
        tokens: lexed.ranges().collect(),
        pos: 0,
        builder: GreenNodeBuilder::new(),
        macros: HashSet::new(),
        bodies: Vec::new(),
        errors: lexed
            .errors
            .iter()
            .map(|e| SyntaxError {
                range: e.range.clone(),
                message: e.message.to_string(),
            })
            .collect(),
    };
    parser.source_file();
    let Parser {
        builder,
        mut errors,
        ..
    } = parser;
    errors.sort_by_key(|e| e.range.start);
    Parse {
        green: builder.finish(),
        errors,
    }
}

/// Shift, extend, and SVE multiplier names that take an amount after them.
const SHIFTS: &[&str] = &[
    "lsl", "lsr", "asr", "ror", "msl", "uxtb", "uxth", "uxtw", "uxtx", "sxtb", "sxth", "sxtw",
    "sxtx", "mul",
];

/// Binding power of a binary operator, using Apple's (LLVM's Darwin) table.
/// Probed with clang 21; see docs/design.md#syntax.
fn binary_precedence(kind: SyntaxKind) -> Option<u8> {
    Some(match kind {
        AmpAmp | PipePipe => 1,
        Pipe | Caret | Amp => 2,
        EqEq | NotEq | Lt | LtEq | Gt | GtEq => 3,
        Shl | Shr => 4,
        Plus | Minus => 5,
        Star | Slash | Percent => 6,
        _ => return None,
    })
}

fn starts_expr(kind: SyntaxKind) -> bool {
    matches!(
        kind,
        Ident
            | Number
            | LocalLabelRef
            | StringLit
            | CharLit
            | LParen
            | Minus
            | Plus
            | Tilde
            | Bang
            | Backslash
            | Colon
            | Percent
    )
}

/// Directives that start a body the assembler doesn't parse until it expands
/// it, and the directives that end each.
const BODY_STARTS: &[(&str, &[&str])] = &[
    (".macro", &[".endm", ".endmacro"]),
    (".rept", &[".endr"]),
    (".irp", &[".endr"]),
    (".irpc", &[".endr"]),
];

struct Parser<'t> {
    text: &'t str,
    tokens: Vec<(SyntaxKind, Range<usize>)>,
    /// Index of the next token to emit, trivia included.
    pos: usize,
    builder: GreenNodeBuilder<'static>,
    /// Macros defined so far. Clang requires a macro to be defined before it's called.
    macros: HashSet<String>,
    /// For each open macro or repeat body, the directives that close it.
    bodies: Vec<&'static [&'static str]>,
    errors: Vec<SyntaxError>,
}

impl Parser<'_> {
    fn source_file(&mut self) {
        self.builder.start_node(SourceFile.into());
        loop {
            match self.current() {
                Eof => break,
                Newline | Separator => self.bump(),
                _ => self.statement(),
            }
        }
        self.skip_trivia();
        self.builder.finish_node();
    }

    /// Labels, then at most one directive, instruction, or assignment.
    fn statement(&mut self) {
        while matches!(self.current(), Ident | StringLit | Number) && self.nth(1) == Colon {
            self.start(Label);
            self.bump();
            self.bump();
            self.finish();
        }
        if self.at_end() {
            return;
        }
        let text = self.current_text();
        match self.current() {
            Ident if self.nth(1) == Eq => self.assignment(),
            Ident if self.nth(1) == Ident && self.nth_text(1).eq_ignore_ascii_case(".req") => {
                self.start(RegisterAlias);
                self.bump();
                self.bump();
                if self.at_end() {
                    self.error_here("expected a register after .req");
                } else {
                    self.operand();
                }
                self.end_statement();
                self.finish();
            }
            Ident if text.starts_with('.') && text != "." => self.directive(),
            Ident if self.macros.contains(text) => {
                self.start(MacroCall);
                self.bump();
                self.lenient_args();
                self.finish();
            }
            Ident if !self.bodies.is_empty() => {
                self.start(Instruction);
                self.bump();
                self.lenient_args();
                self.finish();
            }
            Ident => self.instruction(),
            _ => {
                self.error_here("expected an instruction, directive, or label");
                self.error_rest();
            }
        }
    }

    fn assignment(&mut self) {
        self.start(Assignment);
        self.bump();
        self.bump();
        self.expr_or_error("expected a value after =");
        self.end_statement();
        self.finish();
    }

    /// Directive arguments vary too much for one grammar, so they're parsed
    /// leniently. Tracks macro definitions and the bodies of macros and repeats.
    fn directive(&mut self) {
        let name = self.current_text();
        let closes = self
            .bodies
            .last()
            .is_some_and(|ends| ends.iter().any(|e| e.eq_ignore_ascii_case(name)));
        let opens = BODY_STARTS
            .iter()
            .find(|(start, _)| start.eq_ignore_ascii_case(name))
            .map(|(_, ends)| *ends);
        let defines_macro = name.eq_ignore_ascii_case(".macro");
        self.start(Directive);
        self.bump();
        if defines_macro && self.at(Ident) {
            let macro_name = self.current_text().to_string();
            self.macros.insert(macro_name);
        }
        self.lenient_args();
        self.finish();
        if closes {
            self.bodies.pop();
        }
        if let Some(ends) = opens {
            self.bodies.push(ends);
        }
    }

    /// Expressions where they fit, raw tokens where they don't, to the end of
    /// the statement.
    fn lenient_args(&mut self) {
        if self.at_end() {
            return;
        }
        self.start(ArgList);
        while !self.at_end() {
            let before = self.pos;
            if starts_expr(self.current()) {
                self.expr();
            }
            if self.pos == before {
                self.bump();
            }
        }
        self.finish();
    }

    fn instruction(&mut self) {
        self.start(Instruction);
        self.bump();
        if !self.at_end() {
            self.start(OperandList);
            self.operand();
            while self.at(Comma) {
                self.bump();
                if self.at_end() || self.at(Comma) {
                    self.error_here("expected an operand after ,");
                } else {
                    self.operand();
                }
            }
            self.finish();
        }
        self.end_statement();
        self.finish();
    }

    /// Reports and wraps anything left before the end of the statement.
    fn end_statement(&mut self) {
        if !self.at_end() {
            self.error_here("expected , or end of line");
            self.error_rest();
        }
    }

    fn operand(&mut self) {
        let shift = self.at(Ident)
            && SHIFTS
                .iter()
                .any(|s| s.eq_ignore_ascii_case(self.current_text()))
            && matches!(self.nth(1), Hash | Number | Ident);
        match self.current() {
            Hash => self.immediate(),
            LBracket => self.memory(),
            LBrace => self.register_list(),
            Eq => {
                self.start(LiteralPool);
                self.bump();
                self.expr_or_error("expected a value after =");
                self.finish();
            }
            Ident if shift => {
                self.start(Shift);
                self.bump();
                if self.at(Hash) {
                    self.immediate();
                } else {
                    self.expr();
                }
                self.finish();
            }
            Ident if self.nth(1) == LBracket => {
                self.start(Indexed);
                self.bump();
                self.bump();
                self.index_elements();
                self.close(RBracket, "expected ] to close the index");
                self.finish();
            }
            kind if starts_expr(kind) => {
                let checkpoint = self.checkpoint();
                self.expr();
                if self.at(Bang) {
                    self.start_at(checkpoint, Writeback);
                    self.bump();
                    self.finish();
                }
            }
            kind => {
                self.error_here("expected an operand");
                if !kind.ends_statement() && !matches!(kind, Comma | RBracket | RBrace) {
                    self.start(Error);
                    self.bump();
                    self.finish();
                }
            }
        }
    }

    /// `1`, or SME's `w8, 0:1, vgx2` in `za.s[w8, 0:1, vgx2]`.
    fn index_elements(&mut self) {
        loop {
            if starts_expr(self.current()) {
                self.operand();
                if self.at(Colon) {
                    self.bump();
                    self.expr_or_error("expected a value after :");
                }
            } else {
                self.error_here("expected an index");
            }
            if !self.at(Comma) {
                break;
            }
            self.bump();
        }
    }

    fn immediate(&mut self) {
        self.start(Immediate);
        self.bump();
        self.expr_or_error("expected a value after #");
        self.finish();
    }

    /// `[base]`, `[base, offset]`, `[base, index, lsl #3]`, then an optional `!`.
    fn memory(&mut self) {
        self.start(Memory);
        self.bump();
        if !self.at(RBracket) {
            self.operand();
            while self.at(Comma) {
                self.bump();
                self.operand();
            }
        }
        self.close(RBracket, "expected ] to close the memory operand");
        if self.at(Bang) {
            self.bump();
        }
        self.finish();
    }

    /// `{v0.4s, v1.4s}`, `{v0.4s-v3.4s}`, then an optional index `[1]`.
    fn register_list(&mut self) {
        self.start(RegisterList);
        self.bump();
        if !self.at(RBrace) {
            loop {
                if self.at(Ident) && self.nth(1) == Minus && self.nth(2) == Ident {
                    self.start(RegisterRange);
                    self.bump();
                    self.bump();
                    self.bump();
                    self.finish();
                } else {
                    self.operand();
                }
                if !self.at(Comma) {
                    break;
                }
                self.bump();
            }
        }
        self.close(RBrace, "expected } to close the register list");
        if self.at(LBracket) {
            self.bump();
            self.expr_or_error("expected an index");
            self.close(RBracket, "expected ] to close the index");
        }
        self.finish();
    }

    fn expr_or_error(&mut self, message: &str) {
        if starts_expr(self.current()) {
            self.expr();
        } else {
            self.error_here(message);
        }
    }

    fn expr(&mut self) {
        self.expr_bp(0);
    }

    /// Pratt parser: binary operators that bind tighter than `min`, left to right.
    fn expr_bp(&mut self, min: u8) {
        let checkpoint = self.checkpoint();
        self.unary();
        while let Some(precedence) = binary_precedence(self.current()) {
            if precedence <= min {
                break;
            }
            self.start_at(checkpoint, BinExpr);
            self.bump();
            if starts_expr(self.current()) {
                self.expr_bp(precedence);
            } else {
                self.error_here("expected a value after the operator");
            }
            self.finish();
        }
    }

    fn unary(&mut self) {
        match self.current() {
            Minus | Plus | Tilde | Bang => {
                self.start(PrefixExpr);
                self.bump();
                if starts_expr(self.current()) {
                    self.unary();
                } else {
                    self.error_here("expected a value after the operator");
                }
                self.finish();
            }
            _ => self.primary(),
        }
    }

    fn primary(&mut self) {
        match self.current() {
            Ident => {
                self.start(NameRef);
                self.bump();
                self.variant();
                self.finish();
            }
            StringLit if self.nth(1) == At && self.nth(2) == Ident => {
                self.start(NameRef);
                self.bump();
                self.variant();
                self.finish();
            }
            Colon if self.nth(1) == Ident && self.nth(2) == Colon => {
                self.start(Specifier);
                self.bump();
                self.bump();
                self.bump();
                if starts_expr(self.current()) {
                    self.unary();
                } else {
                    self.error_here("expected a symbol after the specifier");
                }
                self.finish();
            }
            Number | LocalLabelRef | StringLit | CharLit => {
                self.start(Literal);
                self.bump();
                self.finish();
            }
            LParen => {
                self.start(ParenExpr);
                self.bump();
                self.expr_or_error("expected a value after (");
                self.close(RParen, "expected ) to close (");
                self.variant();
                self.finish();
            }
            Percent if self.nth(1) == Ident && self.nth(2) == LParen => {
                self.start(Specifier);
                self.bump();
                self.bump();
                self.bump();
                self.expr_or_error("expected a symbol after (");
                self.close(RParen, "expected ) to close the specifier");
                self.finish();
            }
            Backslash => {
                self.start(MacroArg);
                self.bump();
                if self.at(Ident) || self.at(At) {
                    self.bump();
                } else if self.at(LParen) && self.nth(1) == RParen {
                    self.bump();
                    self.bump();
                } else {
                    self.error_here("expected a macro argument name after \\");
                }
                self.finish();
            }
            _ => self.error_here("expected a value"),
        }
    }

    /// `@PAGEOFF` or `@AUTH(ia,42)` after a symbol, if there is one.
    fn variant(&mut self) {
        if !(self.at(At) && self.nth(1) == Ident) {
            return;
        }
        self.start(Variant);
        self.bump();
        self.bump();
        if self.at(LParen) {
            self.bump();
            while !self.at_end() && !self.at(RParen) {
                self.bump();
            }
            self.close(RParen, "expected ) to close the variant's arguments");
        }
        self.finish();
    }

    /// Expects `closer`. If something else is in the way, reports it once and
    /// skips to the closer or the end of the statement.
    fn close(&mut self, closer: SyntaxKind, message: &str) {
        if self.at(closer) {
            self.bump();
            return;
        }
        self.error_here(message);
        if !self.at_end() {
            self.start(Error);
            while !self.at_end() && !self.at(closer) {
                self.bump();
            }
            self.finish();
            if self.at(closer) {
                self.bump();
            }
        }
    }

    /// Wraps everything up to the end of the statement in an `Error` node.
    fn error_rest(&mut self) {
        if self.at_end() {
            return;
        }
        self.start(Error);
        while !self.at_end() {
            self.bump();
        }
        self.finish();
    }

    /// Reports an error at the current token. An `Unknown` token already has a
    /// lexer error, so it doesn't get a second one.
    /// Bodies of macros and repeats aren't assembly until expanded, so errors
    /// in them aren't reported.
    fn error_here(&mut self, message: &str) {
        if self.current() == Unknown || !self.bodies.is_empty() {
            return;
        }
        let range = match self.nth_index(0) {
            Some(i) if !self.tokens[i].0.ends_statement() => self.tokens[i].1.clone(),
            Some(i) => self.tokens[i].1.start..self.tokens[i].1.start,
            None => self.text.len()..self.text.len(),
        };
        self.errors.push(SyntaxError {
            range,
            message: message.to_string(),
        });
    }

    /// Index into `tokens` of the `n`th non-trivia token from `pos`.
    fn nth_index(&self, n: usize) -> Option<usize> {
        (self.pos..self.tokens.len())
            .filter(|&i| !self.tokens[i].0.is_trivia())
            .nth(n)
    }

    fn nth(&self, n: usize) -> SyntaxKind {
        self.nth_index(n).map_or(Eof, |i| self.tokens[i].0)
    }

    fn current(&self) -> SyntaxKind {
        self.nth(0)
    }

    fn current_text(&self) -> &str {
        self.nth_text(0)
    }

    fn nth_text(&self, n: usize) -> &str {
        self.nth_index(n)
            .map_or("", |i| &self.text[self.tokens[i].1.clone()])
    }

    fn at(&self, kind: SyntaxKind) -> bool {
        self.current() == kind
    }

    fn at_end(&self) -> bool {
        self.current().ends_statement()
    }

    fn skip_trivia(&mut self) {
        while let Some((kind, range)) = self.tokens.get(self.pos) {
            if !kind.is_trivia() {
                break;
            }
            self.builder
                .token((*kind).into(), &self.text[range.clone()]);
            self.pos += 1;
        }
    }

    /// Emits the current token, and any trivia before it.
    fn bump(&mut self) {
        self.skip_trivia();
        if let Some((kind, range)) = self.tokens.get(self.pos) {
            self.builder
                .token((*kind).into(), &self.text[range.clone()]);
            self.pos += 1;
        }
    }

    fn start(&mut self, kind: SyntaxKind) {
        self.skip_trivia();
        self.builder.start_node(kind.into());
    }

    fn checkpoint(&mut self) -> Checkpoint {
        self.skip_trivia();
        self.builder.checkpoint()
    }

    fn start_at(&mut self, checkpoint: Checkpoint, kind: SyntaxKind) {
        self.builder.start_node_at(checkpoint, kind.into());
    }

    fn finish(&mut self) {
        self.builder.finish_node();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The kinds of the nodes under the first statement, in preorder, so tests
    /// can check shape without ranges.
    fn shape(text: &str) -> String {
        let parse = parse(text);
        assert_eq!(parse.errors(), [], "{text}");
        let mut out = Vec::new();
        for event in parse.syntax().preorder() {
            match event {
                rowan::WalkEvent::Enter(node) if node.kind() != SourceFile => {
                    out.push(format!("{:?}(", node.kind()));
                }
                rowan::WalkEvent::Leave(node) if node.kind() != SourceFile => out.push(")".into()),
                _ => {}
            }
        }
        out.concat()
    }

    fn errors(text: &str) -> Vec<(&str, String)> {
        parse(text)
            .errors()
            .iter()
            .map(|e| (&text[e.range.clone()], e.message.clone()))
            .collect()
    }

    #[test]
    fn tree_covers_every_byte() {
        for text in [
            "",
            "\n",
            "  ; c\n",
            "mov x0, #1 ; c",
            "[x0",
            "#",
            ")",
            "a: b: c d e",
        ] {
            assert_eq!(parse(text).syntax().to_string(), text);
        }
    }

    #[test]
    fn labels_then_instruction_on_one_line() {
        assert_eq!(shape("_f: _g: ret"), "Label()Label()Instruction()");
    }

    #[test]
    fn local_and_quoted_labels() {
        assert_eq!(shape("1: \"a b\":"), "Label()Label()");
    }

    #[test]
    fn directive_args_are_lenient() {
        assert_eq!(
            shape(".macro m a, b=1"),
            "Directive(ArgList(NameRef()NameRef()NameRef()Literal()))"
        );
    }

    #[test]
    fn assignment_and_location_counter() {
        assert_eq!(shape("a = 5"), "Assignment(Literal())");
        assert_eq!(
            shape(". = . + 4"),
            "Assignment(BinExpr(NameRef()Literal()))"
        );
    }

    #[test]
    fn operands() {
        assert_eq!(
            shape("ldr x0, [x1, x2, lsl #3]"),
            "Instruction(OperandList(NameRef()Memory(NameRef()NameRef()Shift(Immediate(Literal())))))"
        );
        assert_eq!(
            shape("stp x29, x30, [sp, #-16]!"),
            "Instruction(OperandList(NameRef()NameRef()Memory(NameRef()Immediate(PrefixExpr(Literal())))))"
        );
        assert_eq!(
            shape("ld1 {v0.4s-v3.4s}, [x0]"),
            "Instruction(OperandList(RegisterList(RegisterRange())Memory(NameRef())))"
        );
        assert_eq!(
            shape("ld1 {v0.s}[1], [x0]"),
            "Instruction(OperandList(RegisterList(NameRef()Literal())Memory(NameRef())))"
        );
        assert_eq!(
            shape("mov v0.s[1], w0"),
            "Instruction(OperandList(Indexed(Literal())NameRef()))"
        );
        assert_eq!(
            shape("ldr x0, =0x1234"),
            "Instruction(OperandList(NameRef()LiteralPool(Literal())))"
        );
        assert_eq!(
            shape("adrp x0, _g@PAGE"),
            "Instruction(OperandList(NameRef()NameRef(Variant())))"
        );
        assert_eq!(
            shape("ld1d {z0.d}, p0/z, [x0, #1, mul vl]"),
            "Instruction(OperandList(RegisterList(NameRef())BinExpr(NameRef()NameRef())Memory(NameRef()Immediate(Literal())Shift(NameRef()))))"
        );
    }

    #[test]
    fn darwin_precedence() {
        // + binds tighter than <<: 1 << (2 + 1).
        assert_eq!(
            shape("a = 1 << 2 + 1"),
            "Assignment(BinExpr(Literal()BinExpr(Literal()Literal())))"
        );
        // == binds tighter than &: 6 & (3 == 3).
        assert_eq!(
            shape("a = 6 & 3 == 3"),
            "Assignment(BinExpr(Literal()BinExpr(Literal()Literal())))"
        );
        // Same level is left to right: (10 % 4) * 2.
        assert_eq!(
            shape("a = 10 % 4 * 2"),
            "Assignment(BinExpr(BinExpr(Literal()Literal())Literal()))"
        );
        assert_eq!(
            shape("a = -~1"),
            "Assignment(PrefixExpr(PrefixExpr(Literal())))"
        );
    }

    #[test]
    fn macro_args() {
        assert_eq!(
            shape("mov x0, \\a"),
            "Instruction(OperandList(NameRef()MacroArg()))"
        );
        assert_eq!(shape(".byte \\()"), "Directive(ArgList(MacroArg()))");
    }

    #[test]
    fn macro_calls_need_an_earlier_definition() {
        assert_eq!(
            shape(".macro m a\n.endm\nm 8h"),
            "Directive(ArgList(NameRef()NameRef()))Directive()MacroCall(ArgList(Literal()NameRef()))"
        );
        // Called before it's defined: an instruction, as clang sees it.
        assert_eq!(
            errors("m 8h\n.macro m a\n.endm"),
            [("h", "expected , or end of line".into())]
        );
        // Case-sensitive, as in clang.
        assert!(!errors(".macro m\n.endm\nM 8h").is_empty());
    }

    #[test]
    fn bodies_are_not_checked_until_expanded() {
        let text = ".macro m sz\nadd v0.\\sz, v0.\\sz\nmov x0 x1 x2\n\\sz\\():\n.endm\n.rept 2\nmov x0 x1\n.endr\nmov x0 x1\n";
        assert_eq!(errors(text), [("x1", "expected , or end of line".into())]);
        let parse = parse(text);
        let last = parse.syntax().children().last().map(|n| n.to_string());
        assert_eq!(last.as_deref(), Some("mov x0 x1"));
    }

    #[test]
    fn nested_bodies_close_in_order() {
        let text = ".macro outer\n.rept 2\nmov x0 x1\n.endr\nmov x0 x1\n.endm\nmov x0 x1\n";
        assert_eq!(errors(text).len(), 1);
    }

    #[test]
    fn sme_index_with_ranges() {
        assert_eq!(
            shape("bfmlsl za.s[w8, 0:1, vgx2], z0.h, z0.h"),
            "Instruction(OperandList(Indexed(NameRef()Literal()Literal()NameRef())NameRef()NameRef()))"
        );
    }

    #[test]
    fn register_alias() {
        assert_eq!(shape("fred .req x5"), "RegisterAlias(NameRef())");
        assert_eq!(shape("fred .REQ z1.b"), "RegisterAlias(NameRef())");
        assert_eq!(
            errors("fred .req"),
            [("", "expected a register after .req".into())]
        );
    }

    #[test]
    fn variant_after_parenthesized_expression() {
        assert_eq!(
            shape("adrp x0, (_foo + 1)@PAGE"),
            "Instruction(OperandList(NameRef()ParenExpr(BinExpr(NameRef()Literal())Variant())))"
        );
    }

    #[test]
    fn percent_specifier() {
        assert_eq!(
            shape("ldr w0, [x1, %pltpcrel(g)]"),
            "Instruction(OperandList(NameRef()Memory(NameRef()Specifier(NameRef()))))"
        );
        assert_eq!(
            shape("a = 7 % 2"),
            "Assignment(BinExpr(Literal()Literal()))"
        );
    }

    #[test]
    fn register_writeback() {
        assert_eq!(
            shape("cpyfp [x0]!, [x1]!, x2!"),
            "Instruction(OperandList(Memory(NameRef())Memory(NameRef())Writeback(NameRef())))"
        );
    }

    #[test]
    fn elf_specifiers_parse() {
        assert_eq!(
            shape("add x0, x0, #:lo12:var"),
            "Instruction(OperandList(NameRef()NameRef()Immediate(Specifier(NameRef()))))"
        );
        assert_eq!(
            errors("adrp x0, :got:"),
            [("", "expected a symbol after the specifier".into())]
        );
    }

    #[test]
    fn variants_with_arguments_and_quoted_symbols() {
        assert_eq!(
            shape(".quad _g@AUTH(ia,42)"),
            "Directive(ArgList(NameRef(Variant())))"
        );
        assert_eq!(
            shape("ldrb w3, [x2, \"+[T m].v\"@PAGEOFF]"),
            "Instruction(OperandList(NameRef()Memory(NameRef()NameRef(Variant()))))"
        );
    }

    #[test]
    fn separator_splits_statements() {
        assert_eq!(shape("nop %% ret"), "Instruction()Instruction()");
    }

    #[test]
    fn trailing_comment_is_outside_the_statement() {
        let parse = parse("ret ; done");
        let statement = parse.syntax().first_child().map(|n| n.to_string());
        assert_eq!(statement.as_deref(), Some("ret"));
    }

    #[test]
    fn errors_name_what_was_expected() {
        assert_eq!(
            errors("mov x0, x1,"),
            [("", "expected an operand after ,".into())]
        );
        assert_eq!(
            errors("mov x0 x1"),
            [("x1", "expected , or end of line".into())]
        );
        assert_eq!(
            errors("ldr x0, [x1"),
            [("", "expected ] to close the memory operand".into())]
        );
        assert_eq!(
            errors(", nop"),
            [(",", "expected an instruction, directive, or label".into())]
        );
        assert_eq!(
            errors("mov x0, #"),
            [("", "expected a value after #".into())]
        );
        assert_eq!(errors("a = (1"), [("", "expected ) to close (".into())]);
    }

    #[test]
    fn recovery_resumes_at_the_next_statement() {
        let parse = parse("mov x0 x1 x2\nret\n");
        assert_eq!(parse.errors().len(), 1);
        let kinds: Vec<_> = parse.syntax().children().map(|n| n.kind()).collect();
        assert_eq!(kinds, [Instruction, Instruction]);
    }

    #[test]
    fn junk_inside_brackets_is_one_error() {
        assert_eq!(
            errors("ldr x0, [x1 x2]"),
            [("x2", "expected ] to close the memory operand".into())]
        );
    }

    #[test]
    fn lexer_errors_are_not_reported_twice() {
        assert_eq!(errors("mov x0, `"), [("`", "unexpected character".into())]);
    }
}
