//! Prints syntax errors for each file given: `cargo run -p arm-syntax --example check -- a.s`.

fn main() {
    for path in std::env::args().skip(1) {
        let Ok(text) = std::fs::read_to_string(&path) else {
            eprintln!("{path}: can't read");
            continue;
        };
        let parse = arm_syntax::parse(&text);
        for error in parse.errors() {
            let line = text[..error.range.start].matches('\n').count() + 1;
            println!(
                "{path}:{line}: {} [{:?}]",
                error.message,
                &text[error.range.clone()]
            );
        }
        println!("{path}: {} errors", parse.errors().len());
    }
}
