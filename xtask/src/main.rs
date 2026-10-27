//! Repo automation, run with `cargo xtask <task>`. Tasks are Rust so the repo
//! needs nothing but Cargo (docs/decisions/0002-rust-and-cargo.md).

mod gen_features;
mod gen_isa;
mod notices;
mod survey;

use std::process::{Command, ExitCode};

const USAGE: &str = "\
cargo xtask <task>

Tasks:
  check            Everything CI runs: fmt, clippy, tests, third-party notices
  gen-isa          Regenerate crates/isa/src/generated.rs from LLVM
                   (run with no arguments for usage)
  nvim-smoke       Run armls inside headless Neovim and check what a user sees
  notices          Regenerate THIRD-PARTY-NOTICES.md from Cargo.lock
  survey <path>... Compare arm-syntax's errors with clang's on every .s file
                   under the paths (macOS only; see AGENTS.md)
";

fn main() -> ExitCode {
    let task = std::env::args().nth(1);
    match task.as_deref() {
        Some("check") => check(),
        Some("gen-isa") => gen_isa::run(std::env::args().skip(2).collect()),
        Some("nvim-smoke") => nvim_smoke(),
        Some("notices") => notices::run(std::env::args().skip(2).collect()),
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
        &["xtask", "notices", "--check"],
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

/// Builds armls and runs editors/neovim/smoke.lua in headless Neovim.
fn nvim_smoke() -> ExitCode {
    let built = Command::new(env!("CARGO"))
        .args(["build", "--quiet", "-p", "arm-lsp"])
        .status()
        .is_ok_and(|s| s.success());
    if !built {
        eprintln!("cargo build -p arm-lsp failed");
        return ExitCode::FAILURE;
    }
    let dir = std::env::temp_dir().join(format!("arm-tools-nvim-{}", std::process::id()));
    let file = dir.join("smoke.s");
    // A small project: smoke.s includes defs.s and calls into util.s. The
    // .git directory makes it the workspace root. Line numbers matter to
    // editors/neovim/smoke.lua.
    let files = [
        (
            "smoke.s",
            ".include \"defs.s\"\n.globl _main\n_main:\n    stp x29, x30, [sp, #-16]!\n    frob x0\n    mov x0, #LIMIT\n    bl _helper\n    csel x0, x1, x2, eq\n    ldp x29, x30, [sp], #16\n    ret   \n\n; end\n",
        ),
        ("defs.s", "LIMIT = 4\n"),
        ("util.s", ".globl _helper\n_helper:\n    ret\n"),
    ];
    let written = std::fs::create_dir_all(dir.join(".git")).and_then(|()| {
        files
            .iter()
            .try_for_each(|(name, text)| std::fs::write(dir.join(name), text))
    });
    if let Err(error) = written {
        eprintln!("can't write {}: {error}", dir.display());
        return ExitCode::FAILURE;
    }
    let armls = std::env::current_dir()
        .unwrap_or_default()
        .join("target/debug/armls");
    let status = Command::new("nvim")
        .args([
            "--headless",
            "--clean",
            "-c",
            "luafile editors/neovim/smoke.lua",
        ])
        .env("ARMLS", armls)
        .env("SMOKE_FILE", &file)
        .status();
    let _ = std::fs::remove_dir_all(&dir);
    match status {
        Ok(s) if s.success() => ExitCode::SUCCESS,
        Ok(_) => ExitCode::FAILURE,
        Err(error) => {
            eprintln!("can't run nvim: {error}. Install Neovim 0.11 or later.");
            ExitCode::FAILURE
        }
    }
}
