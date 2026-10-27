//! Checks the formatter's promises on any files: tokens unchanged and stable.
//! `cargo run --release -p arm-syntax --example fmt_check -- dir/*.s`

use arm_syntax::{FormatOptions, SyntaxKind, format, lex};

fn tokens(text: &str) -> Vec<(SyntaxKind, String)> {
    lex(text)
        .ranges()
        .filter(|(k, _)| !matches!(k, SyntaxKind::Whitespace | SyntaxKind::Newline))
        .map(|(k, r)| (k, text[r].trim_end().to_string()))
        .collect()
}

fn main() {
    let (mut checked, mut bad) = (0, 0);
    for path in std::env::args().skip(1) {
        let Ok(text) = std::fs::read_to_string(&path) else {
            continue;
        };
        checked += 1;
        let once = format(&text, &FormatOptions::default());
        if tokens(&once) != tokens(&text) {
            bad += 1;
            println!("{path}: tokens changed");
        } else if format(&once, &FormatOptions::default()) != once {
            bad += 1;
            println!("{path}: not stable");
        }
    }
    println!("{checked} files, {bad} problems");
    std::process::exit(i32::from(bad > 0));
}
