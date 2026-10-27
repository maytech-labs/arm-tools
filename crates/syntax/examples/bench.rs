//! Times a full parse of a file: `cargo run --release -p arm-syntax --example bench -- a.s`.

use std::time::Instant;

fn main() {
    let Some(path) = std::env::args().nth(1) else {
        eprintln!("usage: bench <file.s>");
        std::process::exit(2);
    };
    let Ok(text) = std::fs::read_to_string(&path) else {
        eprintln!("{path}: can't read");
        std::process::exit(2);
    };
    let runs = 50;
    let start = Instant::now();
    for _ in 0..runs {
        std::hint::black_box(arm_syntax::parse(&text));
    }
    let per_parse = start.elapsed() / runs;
    let lines = text.lines().count();
    println!("{lines} lines: {per_parse:?} per parse");
}
