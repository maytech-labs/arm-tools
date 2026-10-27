//! `armtool`, the command line. Not built yet; see docs/roadmap.md (M5).

use std::process::ExitCode;

fn main() -> ExitCode {
    eprintln!("armtool: not built yet; see docs/roadmap.md");
    ExitCode::from(3)
}
