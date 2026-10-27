//! Lexer and parser for Apple AArch64 assembly. See docs/design.md#syntax.

pub mod ast;
mod kinds;
mod lexer;
mod parser;

pub use kinds::{ArmLanguage, SyntaxElement, SyntaxKind, SyntaxNode, SyntaxToken};
pub use lexer::{LexError, Lexed, Token, lex};
pub use parser::{Parse, SyntaxError, parse};
pub use rowan::{NodeOrToken, TextRange, WalkEvent};
