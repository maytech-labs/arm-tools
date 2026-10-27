//! The `armtool` contract: commands, output, and exit codes, from the real
//! binary on real files (docs/decisions/0007-testing.md).

// Test setup may panic on a broken environment (AGENTS.md: expect is fine in tests).
#![allow(clippy::expect_used)]

use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use serde_json::Value;

const CLEAN: &str = ".globl _f\n_f:\n    mov x0, #1\n    ret\n";
const LINTY: &str = ".globl _f\n_f:\n    mov x18, #1\n    ret\n";
const BROKEN: &str = ".globl _f\n_f:\n    add x0, x0, #5000\n    ret\n";

struct Dir(PathBuf);

impl Dir {
    fn new(name: &str) -> Dir {
        let path = std::env::temp_dir().join(format!("armtool-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&path);
        std::fs::create_dir_all(&path).expect("temp dir");
        Dir(path)
    }

    fn file(&self, name: &str, text: &str) -> PathBuf {
        let path = self.0.join(name);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).expect("dirs");
        }
        std::fs::write(&path, text).expect("write");
        path
    }
}

impl Drop for Dir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn armtool(args: &[&str], dir: &Path) -> Output {
    Command::new(env!("CARGO_BIN_EXE_armtool"))
        .args(args)
        .current_dir(dir)
        .output()
        .expect("armtool runs")
}

fn stdout(output: &Output) -> String {
    String::from_utf8_lossy(&output.stdout).into_owned()
}

#[test]
fn clean_files_exit_0() {
    let dir = Dir::new("clean");
    dir.file("a.s", CLEAN);
    let output = armtool(&["lint", "."], &dir.0);
    assert_eq!(output.status.code(), Some(0), "{}", stdout(&output));
    assert!(String::from_utf8_lossy(&output.stderr).contains("1 file checked"));
}

#[test]
fn errors_exit_1_and_name_the_place() {
    let dir = Dir::new("errors");
    dir.file("a.s", BROKEN);
    let output = armtool(&["check", "a.s"], &dir.0);
    assert_eq!(output.status.code(), Some(1));
    let text = stdout(&output);
    assert!(
        text.starts_with("a.s:3:17: error[invalid-operand]: "),
        "{text}"
    );
    assert!(text.contains("[0, 4095]"), "{text}");
}

#[test]
fn check_runs_no_lint_rules_and_lint_does() {
    let dir = Dir::new("lint");
    dir.file("a.s", LINTY);
    assert_eq!(armtool(&["check", "a.s"], &dir.0).status.code(), Some(0));
    let output = armtool(&["lint", "a.s"], &dir.0);
    assert_eq!(output.status.code(), Some(1));
    assert!(stdout(&output).contains("warning[abi/x18-reserved]"));
}

#[test]
fn json_output() {
    let dir = Dir::new("json");
    dir.file("a.s", LINTY);
    let output = armtool(&["lint", "--json", "a.s"], &dir.0);
    assert_eq!(output.status.code(), Some(1));
    let records: Value = serde_json::from_slice(&output.stdout).expect("JSON");
    let record = &records[0];
    assert_eq!(record["path"], "a.s");
    assert_eq!(record["line"], 3);
    assert_eq!(record["column"], 9);
    assert_eq!(record["severity"], "warning");
    assert_eq!(record["code"], "abi/x18-reserved");
    assert!(
        record["message"]
            .as_str()
            .is_some_and(|m| m.contains("Darwin"))
    );
    // Clean is an empty array, still exit 0.
    dir.file("a.s", CLEAN);
    let output = armtool(&["lint", "--json", "a.s"], &dir.0);
    assert_eq!(output.status.code(), Some(0));
    assert_eq!(stdout(&output).trim(), "[]");
}

#[test]
fn hints_dont_fail_the_run() {
    let dir = Dir::new("hints");
    dir.file("a.s", ".globl _f\n_f:\n    ret\nunused:\n    ret\n");
    let output = armtool(&["lint", "a.s"], &dir.0);
    assert_eq!(output.status.code(), Some(0));
    assert!(stdout(&output).contains("hint[style/unused-label]"));
}

#[test]
fn directories_are_searched_for_s_files() {
    let dir = Dir::new("dirs");
    dir.file("src/a.s", BROKEN);
    dir.file("src/deep/b.S", BROKEN);
    dir.file("src/notes.txt", "add x0, x0, #5000");
    dir.file(".hidden/c.s", BROKEN);
    let output = armtool(&["check", "src"], &dir.0);
    let text = stdout(&output);
    assert_eq!(text.lines().count(), 2, "{text}");
}

#[test]
fn nearest_config_applies() {
    let dir = Dir::new("config");
    dir.file("armtool.toml", "[rules]\n\"abi/x18-reserved\" = \"off\"\n");
    dir.file("src/a.s", LINTY);
    assert_eq!(armtool(&["lint", "src"], &dir.0).status.code(), Some(0));
    // --config overrides discovery.
    dir.file("strict.toml", "[rules]\n\"abi/x18-reserved\" = \"error\"\n");
    let output = armtool(&["lint", "--config", "strict.toml", "src"], &dir.0);
    assert!(stdout(&output).contains("error[abi/x18-reserved]"));
}

#[test]
fn bad_config_is_exit_2_with_a_fix() {
    let dir = Dir::new("badconfig");
    dir.file("armtool.toml", "[rules]\n\"abi/x18-reserve\" = \"off\"\n");
    dir.file("a.s", CLEAN);
    let output = armtool(&["lint", "a.s"], &dir.0);
    assert_eq!(output.status.code(), Some(2));
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("Did you mean 'abi/x18-reserved'?"),
        "{stderr}"
    );
}

#[test]
fn allow_comments() {
    let dir = Dir::new("allow");
    dir.file(
        "a.s",
        ".globl _f\n_f:\n    mov x18, #1 ; armtool: allow(abi/x18-reserved)\n    ret\n",
    );
    assert_eq!(armtool(&["lint", "a.s"], &dir.0).status.code(), Some(0));
}

#[test]
fn includes_follow_the_configured_paths() {
    let dir = Dir::new("includes");
    dir.file(
        "inc/macros.s",
        ".macro zero r\n    mov \\r, #0\n.endm\nbig = 4097\n",
    );
    dir.file(
        "src/a.s",
        ".include \"macros.s\"\n.globl _f\n_f:\n    zero x0\n    ret\n",
    );
    // Without the include path, the macro is unknown and the include missing.
    let output = armtool(&["check", "src/a.s"], &dir.0);
    assert!(
        stdout(&output).contains("include-not-found"),
        "{}",
        stdout(&output)
    );
    dir.file("armtool.toml", "[include]\npaths = [\"inc\"]\n");
    let output = armtool(&["check", "src/a.s"], &dir.0);
    assert_eq!(output.status.code(), Some(0), "{}", stdout(&output));
    // Imported constants are checked as values.
    dir.file("src/b.s", ".include \"macros.s\"\n    add x0, x0, #big\n");
    let output = armtool(&["check", "src/b.s"], &dir.0);
    assert!(
        stdout(&output).contains("invalid-operand"),
        "{}",
        stdout(&output)
    );
}

#[test]
fn fmt_rewrites_and_check_only_reports() {
    let dir = Dir::new("fmt");
    let messy = "_f:  stp x29,x30,[sp,#-16]!\n  ret\n";
    let tidy = "_f: stp     x29, x30, [sp, #-16]!\n    ret\n";
    let file = dir.file("a.s", messy);
    let check = armtool(&["fmt", "--check", "."], &dir.0);
    assert_eq!(check.status.code(), Some(1));
    assert!(stdout(&check).contains("a.s"));
    assert_eq!(
        std::fs::read_to_string(&file).expect("read"),
        messy,
        "--check wrote"
    );
    assert!(armtool(&["fmt", "a.s"], &dir.0).status.success());
    assert_eq!(std::fs::read_to_string(&file).expect("read"), tidy);
    assert_eq!(
        armtool(&["fmt", "--check", "a.s"], &dir.0).status.code(),
        Some(0)
    );
    // [format] settings apply.
    dir.file("armtool.toml", "[format]\nindent = 8\nmnemonic_width = 4\n");
    assert!(armtool(&["fmt", "a.s"], &dir.0).status.success());
    assert_eq!(
        std::fs::read_to_string(&file).expect("read"),
        "_f:     stp x29, x30, [sp, #-16]!\n        ret\n"
    );
}

#[test]
fn fix_applies_suggestions_and_reports_whats_left() {
    let dir = Dir::new("fix");
    let file = dir.file(
        "a.s",
        ".globl _f\n_f:\n    movv x0, x1\n    sub sp, sp, #24\n    add sp, sp, #32\n    mov x18, #1\n    ret\n",
    );
    let output = armtool(&["lint", "--fix", "a.s"], &dir.0);
    assert_eq!(
        std::fs::read_to_string(&file).expect("read"),
        ".globl _f\n_f:\n    mov x0, x1\n    sub sp, sp, #32\n    add sp, sp, #32\n    mov x18, #1\n    ret\n"
    );
    assert!(String::from_utf8_lossy(&output.stderr).contains("fixed 2"));
    // x18 has no automatic fix, so it's still reported.
    let text = stdout(&output);
    assert_eq!(text.lines().count(), 1, "{text}");
    assert!(text.contains("abi/x18-reserved"));
}

#[test]
fn rules_and_explain() {
    let dir = Dir::new("rules");
    let output = armtool(&["rules"], &dir.0);
    assert!(output.status.success());
    let text = stdout(&output);
    for rule in [
        "abi/x18-reserved",
        "flow/unbalanced-stack",
        "style/unused-label",
    ] {
        assert!(text.contains(rule), "{rule}");
    }
    let output = armtool(&["explain", "abi/callee-saved"], &dir.0);
    assert!(output.status.success());
    assert!(stdout(&output).contains("x19 to x28"));
    assert_eq!(armtool(&["explain", "nope"], &dir.0).status.code(), Some(2));
}

#[test]
fn usage_errors_are_exit_2() {
    let dir = Dir::new("usage");
    for args in [
        &[][..],
        &["frob"],
        &["check"],
        &["check", "--frob", "a.s"],
        &["check", "missing.s"],
    ] {
        let output = armtool(args, &dir.0);
        assert_eq!(output.status.code(), Some(2), "{args:?}");
    }
    let version = armtool(&["--version"], &dir.0);
    assert!(stdout(&version).contains("LLVM 21.1.8"));
}
