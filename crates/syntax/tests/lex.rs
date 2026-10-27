//! The lexer's contract on a real program: lossless and error-free on valid
//! input. `parse.rs` checks with clang that the program is valid.

use arm_syntax::{SyntaxKind, lex};

const HELLO: &str = include_str!("data/corpus/hello-world.s");

#[test]
fn tokens_rebuild_the_input() {
    let lexed = lex(HELLO);
    let rebuilt: String = lexed.ranges().map(|(_, r)| &HELLO[r]).collect();
    assert_eq!(rebuilt, HELLO);
}

#[test]
fn valid_program_has_no_errors() {
    assert_eq!(lex(HELLO).errors, []);
}

#[test]
fn valid_program_uses_every_apple_construct() {
    let kinds: Vec<_> = lex(HELLO).ranges().map(|(k, _)| k).collect();
    for kind in [
        SyntaxKind::Comment,
        SyntaxKind::Separator,
        SyntaxKind::LocalLabelRef,
        SyntaxKind::At,
        SyntaxKind::StringLit,
        SyntaxKind::Bang,
    ] {
        assert!(kinds.contains(&kind), "fixture has no {kind:?}");
    }
}
