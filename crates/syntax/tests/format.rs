//! The formatter's promises on real code (docs/decisions/0020-formatter.md):
//! it changes only whitespace, it's stable, and clang builds the same object.

// Test helpers may panic on broken fixtures (AGENTS.md: expect is fine in tests).
#![allow(clippy::expect_used)]

use std::path::{Path, PathBuf};
use std::process::Command;

use arm_syntax::{FormatOptions, SyntaxKind, format, lex};

fn corpus() -> Vec<PathBuf> {
    fn walk(dir: &Path, out: &mut Vec<PathBuf>) {
        for entry in std::fs::read_dir(dir).expect("dir").flatten() {
            let path = entry.path();
            if path.is_dir() {
                walk(&path, out);
            } else if path.extension().is_some_and(|e| e == "s") {
                out.push(path);
            }
        }
    }
    let mut out = Vec::new();
    walk(
        &Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/data"),
        &mut out,
    );
    out.sort();
    out
}

/// Every token but whitespace and newlines, in order.
fn tokens(text: &str) -> Vec<(SyntaxKind, String)> {
    lex(text)
        .ranges()
        .filter(|(k, _)| !matches!(k, SyntaxKind::Whitespace | SyntaxKind::Newline))
        .map(|(k, r)| (k, text[r].trim_end().to_string()))
        .collect()
}

#[test]
fn only_whitespace_changes() {
    for path in corpus() {
        let text = std::fs::read_to_string(&path).expect("read");
        let formatted = format(&text, &FormatOptions::default());
        assert_eq!(tokens(&formatted), tokens(&text), "{}", path.display());
    }
}

#[test]
fn formatting_is_stable() {
    for path in corpus() {
        let text = std::fs::read_to_string(&path).expect("read");
        let once = format(&text, &FormatOptions::default());
        assert_eq!(
            format(&once, &FormatOptions::default()),
            once,
            "{}",
            path.display()
        );
    }
}

#[test]
fn oracle_clang_builds_the_same_object() {
    if !cfg!(target_os = "macos") {
        return eprintln!("skipped: the clang oracle runs only on macOS");
    }
    let dir = std::env::temp_dir().join(format!("arm-syntax-format-{}", std::process::id()));
    std::fs::create_dir_all(&dir).expect("dir");
    let assemble = |name: &str, text: &str| -> Option<Vec<u8>> {
        let source = dir.join(format!("{name}.s"));
        let object = dir.join(format!("{name}.o"));
        std::fs::write(&source, text).expect("write");
        let ok = Command::new("xcrun")
            .args(["clang", "-c", "-arch", "arm64", "-o"])
            .arg(&object)
            .arg(&source)
            .output()
            .expect("xcrun")
            .status
            .success();
        ok.then(|| std::fs::read(&object).expect("object"))
    };
    let mut compared = 0;
    for path in corpus() {
        let text = std::fs::read_to_string(&path).expect("read");
        // Only files clang builds as they are.
        let Some(before) = assemble("before", &text) else {
            continue;
        };
        let after = assemble("after", &format(&text, &FormatOptions::default()))
            .unwrap_or_else(|| panic!("clang rejects {} after formatting", path.display()));
        assert!(
            before == after,
            "{} builds differently after formatting",
            path.display()
        );
        compared += 1;
    }
    let _ = std::fs::remove_dir_all(&dir);
    assert!(compared >= 10, "only {compared} files compared");
}
