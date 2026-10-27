//! Lints files with the default config: `cargo run -p arm-lint --example lint -- a.s`.

use arm_sema::{Analysis, Encoding};

fn main() {
    for path in std::env::args().skip(1) {
        let Ok(text) = std::fs::read_to_string(&path) else {
            eprintln!("{path}: can't read");
            continue;
        };
        let analysis = Analysis::new(&text);
        for d in arm_lint::lint(&analysis, &arm_lint::Config::default()) {
            let at = analysis
                .line_index()
                .line_col(&text, d.range.start, Encoding::Utf8);
            println!(
                "{path}:{}: {:?} [{}] {}",
                at.line + 1,
                d.severity,
                d.code,
                d.message
            );
        }
    }
}
