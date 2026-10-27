//! Prints the tree for a file or for stdin: `cargo run -p arm-syntax --example tree -- a.s`.

use std::io::Read;

fn main() {
    let mut text = String::new();
    let read = match std::env::args().nth(1) {
        Some(path) => std::fs::read_to_string(path).map(|t| text = t),
        None => std::io::stdin().read_to_string(&mut text).map(|_| ()),
    };
    if let Err(error) = read {
        eprintln!("can't read input: {error}");
        std::process::exit(2);
    }
    print!("{}", arm_syntax::parse(&text).debug_tree());
}
