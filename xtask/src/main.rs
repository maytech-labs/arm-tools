//! Repo automation, run with `cargo xtask <task>`. Tasks are Rust so the repo
//! needs nothing but Cargo (docs/decisions/0002-rust-and-cargo.md).

mod gen_isa;
mod survey;

use std::process::{Command, ExitCode};

const USAGE: &str = "\
cargo xtask <task>

Tasks:
  check            Everything CI runs: fmt, clippy, tests
  gen-isa          Regenerate crates/isa/src/generated.rs from LLVM
                   (run with no arguments for usage)
  survey <path>... Compare arm-syntax's errors with clang's on every .s file
                   under the paths (macOS only; see AGENTS.md)
";

fn main() -> ExitCode {
    let task = std::env::args().nth(1);
    match task.as_deref() {
        Some("check") => check(),
        Some("gen-isa") => gen_isa::run(std::env::args().skip(2).collect()),
        Some("survey") => survey::run(std::env::args().skip(2).collect()),
        _ => {
            eprint!("{USAGE}");
            ExitCode::from(2)
        }
    }
}

/// Runs the same steps as CI, stopping at the first failure.
fn check() -> ExitCode {
    let steps: &[&[&str]] = &[
        &["fmt", "--all", "--check"],
        &[
            "clippy",
            "--workspace",
            "--all-targets",
            "--",
            "-D",
            "warnings",
        ],
        &["test", "--workspace"],
    ];
    for args in steps {
        eprintln!("==> cargo {}", args.join(" "));
        let ok = Command::new(env!("CARGO"))
            .args(*args)
            .status()
            .is_ok_and(|s| s.success());
        if !ok {
            eprintln!("cargo {} failed", args[0]);
            return ExitCode::FAILURE;
        }
    }
    ExitCode::SUCCESS
}
