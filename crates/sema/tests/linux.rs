//! arm-sema on Linux files: ELF conventions, checked against
//! `clang --target=aarch64-linux-gnu` (docs/decisions/0038-linux.md).

// Test helpers may panic on broken fixtures (AGENTS.md: expect is fine in tests).
#![allow(clippy::expect_used)]

use std::path::{Path, PathBuf};
use std::process::Command;

use arm_sema::{Analysis, Item, Platform, Severity};

fn data(kind: &str) -> Vec<PathBuf> {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/data/linux")
        .join(kind);
    let mut out: Vec<PathBuf> = std::fs::read_dir(&dir)
        .expect("fixtures")
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|e| e == "s"))
        .collect();
    out.sort();
    out
}

fn read(path: &Path) -> String {
    std::fs::read_to_string(path).unwrap_or_else(|e| panic!("{}: {e}", path.display()))
}

fn linux(text: &str) -> Analysis {
    Analysis::for_platform(text, Platform::Linux)
}

/// Whether clang accepts the file for aarch64-linux-gnu, or None off macOS.
fn clang_accepts(path: &Path) -> Option<bool> {
    if !cfg!(target_os = "macos") {
        return None;
    }
    let name = path.file_stem().and_then(|s| s.to_str()).unwrap_or("x");
    let object =
        std::env::temp_dir().join(format!("arm-sema-linux-{}-{name}.o", std::process::id()));
    let output = Command::new("xcrun")
        .args(["clang", "-c", "--target=aarch64-linux-gnu", "-o"])
        .arg(&object)
        .arg(path)
        .output()
        .expect("xcrun runs");
    let _ = std::fs::remove_file(&object);
    Some(output.status.success())
}

#[test]
fn valid_linux_files_have_no_diagnostics() {
    for path in data("valid") {
        let analysis = linux(&read(&path));
        assert_eq!(analysis.diagnostics(), [], "{}", path.display());
    }
}

#[test]
fn oracle_accepts_the_valid_files() {
    for path in data("valid") {
        match clang_accepts(&path) {
            None => return eprintln!("skipped: the clang oracle runs only on macOS"),
            Some(accepted) => assert!(accepted, "clang rejects {}", path.display()),
        }
    }
}

#[test]
fn invalid_linux_cases_are_reported() {
    for path in data("invalid") {
        let analysis = linux(&read(&path));
        assert!(
            analysis
                .diagnostics()
                .iter()
                .any(|d| d.severity == Severity::Error),
            "no error for {}",
            path.display()
        );
    }
}

#[test]
fn oracle_rejects_the_invalid_cases() {
    for path in data("invalid") {
        match clang_accepts(&path) {
            None => return eprintln!("skipped: the clang oracle runs only on macOS"),
            Some(accepted) => assert!(!accepted, "clang accepts {}", path.display()),
        }
    }
}

#[test]
fn semicolons_separate_statements() {
    let analysis = linux("mov x0, #1 ; frob x1\n");
    let found: Vec<&str> = analysis.diagnostics().iter().map(|d| d.code).collect();
    assert_eq!(found, ["unknown-instruction"]);
    // On Darwin, the same text is an instruction and a comment.
    assert_eq!(Analysis::new("mov x0, #1 ; frob x1\n").diagnostics(), []);
}

#[test]
fn messages_name_the_other_platform() {
    let message = |text: &str| {
        linux(text)
            .diagnostics()
            .first()
            .map(|d| (d.code, d.message.clone()))
            .expect("a diagnostic")
    };
    let (code, text) = message("add x0, x0, msg@PAGEOFF\nmsg: nop\n");
    assert_eq!(code, "wrong-platform");
    assert!(text.contains("Mach-O") && text.contains(":lo12:"), "{text}");
    let (code, text) = message(".subsections_via_symbols\n");
    assert_eq!(code, "unknown-directive");
    assert!(text.contains("Mach-O only"), "{text}");
    // And the other way: ELF syntax in a Darwin file.
    let darwin = Analysis::new("add x0, x0, :lo12:msg\nmsg: nop\n");
    let d = &darwin.diagnostics()[0];
    assert_eq!(d.code, "wrong-platform");
    assert!(d.message.contains("@PAGEOFF"), "{}", d.message);
    assert_eq!(&darwin.text()[d.range.clone()], ":lo12:");
}

#[test]
fn elf_sections_and_exports() {
    let text = ".section .text.hot,\"ax\",@progbits\nf: ret\n.section .rodata\nmsg: .asciz \"x\"\n.section .data.rel,\"aw\"\n.text\n.weak g\ng: ret\n";
    let analysis = linux(text);
    let sections: Vec<bool> = analysis
        .items()
        .iter()
        .filter_map(|i| match i {
            Item::Section { text, .. } => Some(*text),
            _ => None,
        })
        .collect();
    assert_eq!(sections, [true, false, false, true]);
    let g = analysis
        .symbols()
        .iter()
        .find(|s| s.name == "g")
        .expect("g");
    assert!(g.exported, ".weak symbols link across files");
    assert_eq!(analysis.platform(), Platform::Linux);
}

#[test]
fn gnu_precedence() {
    // GNU: (1 << 3) + 1 = 9, and 50 + 9 fits in 64 bits. Darwin reads
    // 1 << (3 + 1) = 16, and 50 + 16 overflows.
    let text = "ubfx x0, x1, #50, #(1 << 3 + 1)\n";
    assert_eq!(linux(text).diagnostics(), []);
    let darwin = Analysis::new(text);
    assert!(darwin.diagnostics()[0].message.contains("overflows"));
}
