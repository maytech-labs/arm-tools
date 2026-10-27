//! Lexer and parser for AArch64 assembly, Apple's and Linux's. See docs/design.md#syntax.

pub mod ast;
mod format;
mod kinds;
mod lexer;
mod parser;
mod platform;

pub use format::{FormatOptions, format};
pub use kinds::{ArmLanguage, SyntaxElement, SyntaxKind, SyntaxNode, SyntaxToken};
pub use lexer::{LexError, Lexed, Token, lex, lex_for};
pub use parser::{Parse, SyntaxError, parse, parse_for};
pub use platform::Platform;
pub use rowan::{NodeOrToken, TextRange, WalkEvent};
