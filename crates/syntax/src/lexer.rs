//! Tokens for Apple AArch64 assembly.
//!
//! Every byte of the input is in exactly one token, so the tokens concatenate
//! back to the input (docs/decisions/0008-lossless-syntax-tree.md). Lexing never
//! fails: bad input becomes a token plus a [`LexError`]. The rules follow what
//! `clang -c -arch arm64` accepts (docs/decisions/0004-apple-aarch64-first.md).

use std::ops::Range;

use crate::SyntaxKind;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Token {
    pub kind: SyntaxKind,
    /// Length in bytes.
    pub len: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LexError {
    /// Byte range in the input.
    pub range: Range<usize>,
    pub message: &'static str,
}

#[derive(Debug, Default)]
pub struct Lexed {
    pub tokens: Vec<Token>,
    pub errors: Vec<LexError>,
}

impl Lexed {
    /// Each token with its byte range in the input.
    pub fn ranges(&self) -> impl Iterator<Item = (SyntaxKind, Range<usize>)> + '_ {
        let mut start = 0;
        self.tokens.iter().map(move |t| {
            let range = start..start + t.len;
            start += t.len;
            (t.kind, range)
        })
    }
}

/// Splits `text` into tokens. Never fails; see the module docs.
pub fn lex(text: &str) -> Lexed {
    let mut lexer = Lexer {
        text,
        pos: 0,
        line_start: true,
        out: Lexed::default(),
    };
    while lexer.pos < text.len() {
        lexer.token();
    }
    lexer.out
}

struct Lexer<'a> {
    text: &'a str,
    pos: usize,
    /// True when only whitespace has been seen since the last newline, where
    /// `#` starts a comment instead of an immediate.
    line_start: bool,
    out: Lexed,
}

impl Lexer<'_> {
    fn token(&mut self) {
        let start = self.pos;
        let kind = self.kind();
        debug_assert!(self.pos > start, "every token consumes input");
        self.out.tokens.push(Token {
            kind,
            len: self.pos - start,
        });
        self.line_start = match kind {
            SyntaxKind::Newline => true,
            SyntaxKind::Whitespace => self.line_start,
            _ => false,
        };
    }

    fn kind(&mut self) -> SyntaxKind {
        use SyntaxKind::*;
        let start = self.pos;
        let Some(c) = self.bump() else {
            return Unknown;
        };
        let next = self.peek();
        match c {
            '\n' => Newline,
            '\r' if next == Some('\n') => {
                self.bump();
                Newline
            }
            ' ' | '\t' | '\x0b' | '\x0c' | '\r' => {
                self.eat_while(|c| matches!(c, ' ' | '\t' | '\x0b' | '\x0c'));
                Whitespace
            }
            ';' => self.line_comment(),
            '/' if next == Some('/') => self.line_comment(),
            '/' if next == Some('*') => self.block_comment(start),
            '#' if self.line_start => self.line_comment(),
            '%' if next == Some('%') => {
                self.bump();
                Separator
            }
            '"' => self.quoted(
                start,
                '"',
                StringLit,
                "unterminated string: add a closing \"",
            ),
            '\'' => self.quoted(
                start,
                '\'',
                CharLit,
                "unterminated character: add a closing '",
            ),
            '0'..='9' => self.number(start, c),
            c if is_ident_start(c) => {
                self.eat_while(is_ident_continue);
                Ident
            }
            '#' => Hash,
            ',' => Comma,
            ':' => Colon,
            '[' => LBracket,
            ']' => RBracket,
            '{' => LBrace,
            '}' => RBrace,
            '(' => LParen,
            ')' => RParen,
            '+' => Plus,
            '-' => Minus,
            '*' => Star,
            '/' => Slash,
            '%' => Percent,
            '^' => Caret,
            '~' => Tilde,
            '@' => At,
            '\\' => Backslash,
            '!' => self.pair('=', NotEq, Bang),
            '=' => self.pair('=', EqEq, Eq),
            '&' => self.pair('&', AmpAmp, Amp),
            '|' => self.pair('|', PipePipe, Pipe),
            '<' if next == Some('<') => self.pair('<', Shl, Lt),
            '<' if next == Some('>') => {
                self.bump();
                NotEq
            }
            '<' => self.pair('=', LtEq, Lt),
            '>' if next == Some('>') => self.pair('>', Shr, Gt),
            '>' => self.pair('=', GtEq, Gt),
            _ => {
                self.error(start, "unexpected character");
                Unknown
            }
        }
    }

    fn line_comment(&mut self) -> SyntaxKind {
        self.eat_while(|c| c != '\n' && c != '\r');
        // A lone `\r` not followed by `\n` stays in the comment.
        while self.rest().starts_with('\r') && !self.rest().starts_with("\r\n") {
            self.bump();
            self.eat_while(|c| c != '\n' && c != '\r');
        }
        SyntaxKind::Comment
    }

    fn block_comment(&mut self, start: usize) -> SyntaxKind {
        match self.rest()[1..].find("*/") {
            Some(end) => self.pos += 1 + end + 2,
            None => {
                self.pos = self.text.len();
                self.error(start, "unterminated comment: add a closing */");
            }
        }
        SyntaxKind::Comment
    }

    /// A string or character constant. Stops at the closing quote, or before a
    /// newline when there isn't one.
    fn quoted(
        &mut self,
        start: usize,
        quote: char,
        kind: SyntaxKind,
        unterminated: &'static str,
    ) -> SyntaxKind {
        loop {
            match self.peek() {
                None | Some('\n' | '\r') => {
                    self.error(start, unterminated);
                    return kind;
                }
                Some('\\') => {
                    self.bump();
                    if !matches!(self.peek(), None | Some('\n' | '\r')) {
                        self.bump();
                    }
                }
                Some(c) => {
                    self.bump();
                    if c == quote {
                        return kind;
                    }
                }
            }
        }
    }

    /// `first` has been consumed and is a digit.
    fn number(&mut self, start: usize, first: char) -> SyntaxKind {
        let radix_prefix =
            |p: char| first == '0' && matches!(self.peek(), Some(c) if c.eq_ignore_ascii_case(&p));
        if radix_prefix('x') {
            self.bump();
            if self.eat_while(|c| c.is_ascii_hexdigit()) == 0 {
                return self.bad_number(start, "hex number needs digits after 0x");
            }
        } else if radix_prefix('b') && matches!(self.peek_second(), Some('0' | '1')) {
            self.bump();
            self.eat_while(|c| c == '0' || c == '1');
        } else {
            self.eat_while(|c| c.is_ascii_digit());
            if matches!(self.peek(), Some('b' | 'f'))
                && !self.peek_second().is_some_and(is_ident_continue)
            {
                self.bump();
                return SyntaxKind::LocalLabelRef;
            }
            if self.peek() == Some('.') && self.peek_second().is_some_and(|c| c.is_ascii_digit()) {
                self.bump();
                self.eat_while(|c| c.is_ascii_digit());
            }
            if matches!(self.peek(), Some('e' | 'E')) {
                let rest = &self.rest()[1..];
                let digits = rest.strip_prefix(['+', '-']).unwrap_or(rest);
                if digits.starts_with(|c: char| c.is_ascii_digit()) {
                    self.pos += 1 + (rest.len() - digits.len());
                    self.eat_while(|c| c.is_ascii_digit());
                }
            }
        }
        // Letters right after a number end it, as in LLVM: `8h` in a macro call is
        // a number and a name, pasted together on expansion.
        SyntaxKind::Number
    }

    /// Consumes the rest of a malformed number so it's one token, not several.
    fn bad_number(&mut self, start: usize, message: &'static str) -> SyntaxKind {
        self.eat_while(is_ident_continue);
        self.error(start, message);
        SyntaxKind::Number
    }

    /// Consumes `second` if it's next and returns `two`, else returns `one`.
    fn pair(&mut self, second: char, two: SyntaxKind, one: SyntaxKind) -> SyntaxKind {
        if self.peek() == Some(second) {
            self.bump();
            two
        } else {
            one
        }
    }

    fn error(&mut self, start: usize, message: &'static str) {
        self.out.errors.push(LexError {
            range: start..self.pos,
            message,
        });
    }

    fn rest(&self) -> &str {
        &self.text[self.pos..]
    }

    fn peek(&self) -> Option<char> {
        self.rest().chars().next()
    }

    fn peek_second(&self) -> Option<char> {
        self.rest().chars().nth(1)
    }

    fn bump(&mut self) -> Option<char> {
        let c = self.peek()?;
        self.pos += c.len_utf8();
        Some(c)
    }

    /// Consumes characters while `pred` holds. Returns how many bytes it consumed.
    fn eat_while(&mut self, pred: impl Fn(char) -> bool) -> usize {
        let len = self.rest().find(|c| !pred(c)).unwrap_or(self.rest().len());
        self.pos += len;
        len
    }
}

fn is_ident_start(c: char) -> bool {
    c.is_ascii_alphabetic() || matches!(c, '_' | '.' | '$') || (!c.is_ascii() && c.is_alphabetic())
}

fn is_ident_continue(c: char) -> bool {
    is_ident_start(c) || c.is_ascii_digit()
}

#[cfg(test)]
mod tests {
    use super::SyntaxKind::*;
    use super::*;

    /// Non-trivia tokens with their text.
    fn toks(text: &str) -> Vec<(SyntaxKind, &str)> {
        lex(text)
            .ranges()
            .filter(|(k, _)| *k != Whitespace)
            .map(|(k, r)| (k, &text[r]))
            .collect()
    }

    fn errors(text: &str) -> Vec<(&str, &'static str)> {
        lex(text)
            .errors
            .into_iter()
            .map(|e| (&text[e.range], e.message))
            .collect()
    }

    fn assert_lossless(text: &str) {
        let lexed = lex(text);
        let rebuilt: std::string::String = lexed.ranges().map(|(_, r)| &text[r]).collect();
        assert_eq!(rebuilt, text);
    }

    #[test]
    fn instruction_with_immediate() {
        assert_eq!(
            toks("mov x0, #1"),
            [
                (Ident, "mov"),
                (Ident, "x0"),
                (Comma, ","),
                (Hash, "#"),
                (Number, "1")
            ]
        );
    }

    #[test]
    fn semicolon_and_slash_comments_run_to_end_of_line() {
        assert_eq!(
            toks("ret ; done\nret // done\n"),
            [
                (Ident, "ret"),
                (Comment, "; done"),
                (Newline, "\n"),
                (Ident, "ret"),
                (Comment, "// done"),
                (Newline, "\n"),
            ]
        );
    }

    #[test]
    fn block_comment_spans_lines() {
        assert_eq!(
            toks("/* a\nb */ret"),
            [(Comment, "/* a\nb */"), (Ident, "ret")]
        );
    }

    #[test]
    fn unterminated_block_comment_is_an_error_to_end_of_file() {
        assert_eq!(toks("ret /* a\nb"), [(Ident, "ret"), (Comment, "/* a\nb")]);
        assert_eq!(
            errors("ret /* a\nb"),
            [("/* a\nb", "unterminated comment: add a closing */")]
        );
    }

    #[test]
    fn hash_at_line_start_is_a_comment_elsewhere_an_immediate() {
        assert_eq!(
            toks("  # note\nadd x0, x0, #4"),
            [
                (Comment, "# note"),
                (Newline, "\n"),
                (Ident, "add"),
                (Ident, "x0"),
                (Comma, ","),
                (Ident, "x0"),
                (Comma, ","),
                (Hash, "#"),
                (Number, "4"),
            ]
        );
    }

    #[test]
    fn hash_after_separator_is_not_a_comment() {
        assert_eq!(
            toks("nop %% #1"),
            [
                (Ident, "nop"),
                (Separator, "%%"),
                (Hash, "#"),
                (Number, "1")
            ]
        );
    }

    #[test]
    fn percent_alone_is_modulo() {
        assert_eq!(
            toks("7 % 2"),
            [(Number, "7"), (Percent, "%"), (Number, "2")]
        );
    }

    #[test]
    fn numbers() {
        for n in [
            "0", "42", "0x1F", "0XfF", "0b1010", "0B1", "1.5", "2.5e-3", "1e10", "017",
        ] {
            assert_eq!(toks(n), [(Number, n)], "{n}");
            assert!(errors(n).is_empty(), "{n}");
        }
    }

    #[test]
    fn local_label_refs() {
        for r in ["1f", "1b", "10b", "0b", "0f"] {
            assert_eq!(toks(r), [(LocalLabelRef, r)], "{r}");
        }
        assert_eq!(toks("b 1f"), [(Ident, "b"), (LocalLabelRef, "1f")]);
    }

    #[test]
    fn local_label_definition_is_number_then_colon() {
        assert_eq!(toks("1:"), [(Number, "1"), (Colon, ":")]);
    }

    #[test]
    fn hex_prefix_without_digits_is_an_error() {
        assert_eq!(toks("0x"), [(Number, "0x")]);
        assert_eq!(errors("0x"), [("0x", "hex number needs digits after 0x")]);
    }

    #[test]
    fn letters_after_a_number_are_a_separate_token() {
        assert_eq!(toks("8h"), [(Number, "8"), (Ident, "h")]);
        assert_eq!(toks("1fx"), [(Number, "1"), (Ident, "fx")]);
        assert_eq!(toks("0x1fg"), [(Number, "0x1f"), (Ident, "g")]);
        assert!(errors("8h").is_empty());
    }

    #[test]
    fn identifiers_and_directives() {
        assert_eq!(
            toks(".globl _main$1"),
            [(Ident, ".globl"), (Ident, "_main$1")]
        );
        assert_eq!(toks("b ."), [(Ident, "b"), (Ident, ".")]);
    }

    #[test]
    fn symbol_variant() {
        assert_eq!(
            toks("adrp x0, _g@PAGE"),
            [
                (Ident, "adrp"),
                (Ident, "x0"),
                (Comma, ","),
                (Ident, "_g"),
                (At, "@"),
                (Ident, "PAGE")
            ]
        );
    }

    #[test]
    fn memory_operand_with_writeback() {
        assert_eq!(
            toks("[sp, #-16]!"),
            [
                (LBracket, "["),
                (Ident, "sp"),
                (Comma, ","),
                (Hash, "#"),
                (Minus, "-"),
                (Number, "16"),
                (RBracket, "]"),
                (Bang, "!"),
            ]
        );
    }

    #[test]
    fn two_character_operators() {
        assert_eq!(
            toks("<< >> <= >= == != <> && ||"),
            [
                (Shl, "<<"),
                (Shr, ">>"),
                (LtEq, "<="),
                (GtEq, ">="),
                (EqEq, "=="),
                (NotEq, "!="),
                (NotEq, "<>"),
                (AmpAmp, "&&"),
                (PipePipe, "||"),
            ]
        );
    }

    #[test]
    fn strings_and_chars() {
        assert_eq!(
            toks(r#".asciz "a\"b""#),
            [(Ident, ".asciz"), (StringLit, r#""a\"b""#)]
        );
        assert_eq!(toks(r"'\n'"), [(CharLit, r"'\n'")]);
    }

    #[test]
    fn unterminated_string_stops_before_newline() {
        assert_eq!(
            toks("\"ab\nret"),
            [(StringLit, "\"ab"), (Newline, "\n"), (Ident, "ret")]
        );
        assert_eq!(
            errors("\"ab\nret"),
            [("\"ab", "unterminated string: add a closing \"")]
        );
    }

    #[test]
    fn crlf_is_one_newline() {
        assert_eq!(
            toks("nop\r\nnop"),
            [(Ident, "nop"), (Newline, "\r\n"), (Ident, "nop")]
        );
    }

    #[test]
    fn macro_argument() {
        assert_eq!(toks("\\arg"), [(Backslash, "\\"), (Ident, "arg")]);
    }

    #[test]
    fn unexpected_character_is_an_error_token() {
        assert_eq!(toks("nop `"), [(Ident, "nop"), (Unknown, "`")]);
        assert_eq!(errors("nop `"), [("`", "unexpected character")]);
    }

    #[test]
    fn lossless_on_odd_input() {
        for text in [
            "",
            "\r",
            "\r\r\n",
            "é_label: nop",
            "\"unterminated",
            "/*",
            "0x 0b 1. 1e 1e+ ' \\",
            "🦀 ; comment 🦀\r\n#\t",
        ] {
            assert_lossless(text);
        }
    }
}
