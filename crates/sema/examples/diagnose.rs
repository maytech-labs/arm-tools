//! Prints diagnostics for a file or stdin, like a compiler:
//! `cargo run -p arm-sema --example diagnose -- a.s`.

use std::io::Read;

use arm_sema::{Analysis, Encoding};

fn main() {
    let path = std::env::args().nth(1);
    let mut text = String::new();
    let read = match &path {
        Some(path) => std::fs::read_to_string(path).map(|t| text = t),
        None => std::io::stdin().read_to_string(&mut text).map(|_| ()),
    };
    if let Err(error) = read {
        eprintln!("can't read input: {error}");
        std::process::exit(2);
    }
    let name = path.as_deref().unwrap_or("<stdin>");
    let analysis = Analysis::new(&text);
    for d in analysis.diagnostics() {
        let at = analysis
            .line_index()
            .line_col(&text, d.range.start, Encoding::Utf8);
        println!(
            "{name}:{}:{}: {:?} [{}] {}",
            at.line + 1,
            at.col + 1,
            d.severity,
            d.code,
            d.message
        );
    }
}
