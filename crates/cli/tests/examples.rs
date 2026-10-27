//! The programs in examples/ stay correct: the valid ones lint clean and
//! build and run with clang, and mistakes.s keeps showing every problem it
//! says it does.

// Test setup may panic on a broken environment (AGENTS.md: expect is fine in tests).
#![allow(clippy::expect_used)]

use std::path::{Path, PathBuf};
use std::process::Command;

fn examples() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples")
}

fn armtool(args: &[&str]) -> (Option<i32>, String) {
    let output = Command::new(env!("CARGO_BIN_EXE_armtool"))
        .args(args)
        .current_dir(examples())
        .output()
        .expect("armtool runs");
    (
        output.status.code(),
        String::from_utf8_lossy(&output.stdout).into_owned(),
    )
}

#[test]
fn valid_examples_lint_clean() {
    for dir in ["hello", "sum", "fib", "neon", "macros", "preprocessor"] {
        let (code, out) = armtool(&["lint", dir]);
        assert_eq!(code, Some(0), "{dir}: {out}");
        assert!(out.is_empty(), "{dir} has findings: {out}");
    }
}

#[test]
fn mistakes_shows_each_problem() {
    let (code, out) = armtool(&["lint", "mistakes"]);
    assert_eq!(code, Some(1));
    for expected in [
        "unknown-instruction",
        "invalid-operand",
        "invalid-condition",
        "missing-feature",
        "defined below this line",
        "abi/x18-reserved",
        "abi/stack-alignment",
        "addr/adrp-pageoff",
        "flow/undefined-register",
        "flow/unbalanced-stack",
        "flow/unreachable",
        "abi/callee-saved",
        "abi/syscall-number",
        "unknown-directive",
    ] {
        assert!(
            out.contains(expected),
            "mistakes.s no longer shows {expected}:\n{out}"
        );
    }
}

#[test]
fn oracle_examples_build_and_run() {
    if !cfg!(target_os = "macos") {
        return eprintln!("skipped: building needs Apple's clang");
    }
    let build = std::env::temp_dir().join(format!("arm-examples-{}", std::process::id()));
    let status = Command::new("make")
        .args(["-s", "run"])
        .arg(format!("BUILD={}", build.display()))
        .current_dir(examples())
        .output()
        .expect("make runs");
    let out = String::from_utf8_lossy(&status.stdout);
    let err = String::from_utf8_lossy(&status.stderr);
    let _ = std::fs::remove_dir_all(&build);
    assert!(status.status.success(), "{out}\n{err}");
    assert!(!err.contains("warning"), "the linker warned:\n{err}");
    for expected in [
        "Hello from AArch64 assembly!",
        "sum = 31",
        "fib(15) = 610",
        "48.0",
        "  second, from .irp",
        "Counted with a hand-written strlen",
    ] {
        assert!(out.contains(expected), "missing {expected:?} in:\n{out}");
    }
}
