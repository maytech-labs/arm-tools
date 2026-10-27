//! The parser's contract: real programs parse without errors and round-trip,
//! invalid lines are reported, and clang agrees with both
//! (docs/decisions/0007-testing.md). Tree shapes are pinned by snapshots in
//! `data/parser/`; run with `UPDATE_SNAPSHOTS=1` to rewrite them, then review
//! the diff.

// Test helpers may panic on broken fixtures (AGENTS.md: unwrap/expect are fine in tests).
#![allow(clippy::expect_used)]

use std::path::{Path, PathBuf};
use std::process::Command;

use arm_syntax::parse;

fn data(dir: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/data")
        .join(dir)
}

/// The `.s` files under a directory, recursively, sorted.
fn sources(dir: &str) -> Vec<PathBuf> {
    fn walk(dir: &Path, out: &mut Vec<PathBuf>) {
        for entry in std::fs::read_dir(dir).expect("data directory exists") {
            let path = entry.expect("readable entry").path();
            if path.is_dir() {
                walk(&path, out);
            } else if path.extension().is_some_and(|e| e == "s") {
                out.push(path);
            }
        }
    }
    let mut paths = Vec::new();
    walk(&data(dir), &mut paths);
    paths.sort();
    assert!(!paths.is_empty(), "no .s files in {dir}");
    paths
}

fn read(path: &Path) -> String {
    std::fs::read_to_string(path).expect("readable fixture")
}

/// Assembles `source` with Apple's clang, or returns None off macOS.
fn clang_accepts(source: &Path) -> Option<Result<(), String>> {
    if !cfg!(target_os = "macos") {
        return None;
    }
    let object = std::env::temp_dir().join(format!(
        "arm-syntax-{}-{}.o",
        std::process::id(),
        source
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("case")
    ));
    let out = Command::new("xcrun")
        .args(["clang", "-c", "-arch", "arm64", "-o"])
        .arg(&object)
        .arg(source)
        .output()
        .expect("xcrun runs");
    let _ = std::fs::remove_file(&object);
    Some(if out.status.success() {
        Ok(())
    } else {
        Err(String::from_utf8_lossy(&out.stderr).into_owned())
    })
}

#[test]
fn corpus_parses_without_errors_and_round_trips() {
    for path in sources("corpus") {
        let text = read(&path);
        let parse = parse(&text);
        assert_eq!(parse.syntax().to_string(), text, "{}", path.display());
        assert_eq!(parse.errors(), [], "{}", path.display());
    }
}

#[test]
fn oracle_clang_accepts_the_corpus() {
    for path in sources("corpus") {
        match clang_accepts(&path) {
            None => return eprintln!("skipped: the clang oracle runs only on macOS"),
            Some(Ok(())) => {}
            Some(Err(stderr)) => panic!("clang rejected {}:\n{stderr}", path.display()),
        }
    }
}

fn invalid_lines() -> Vec<String> {
    read(&data("invalid.s"))
        .lines()
        .filter(|l| !l.trim().is_empty() && !l.starts_with("//"))
        .map(str::to_string)
        .collect()
}

#[test]
fn invalid_lines_are_reported() {
    for line in invalid_lines() {
        assert!(!parse(&line).errors().is_empty(), "no error for {line:?}");
    }
}

#[test]
fn oracle_clang_rejects_the_invalid_lines() {
    let dir = std::env::temp_dir().join(format!("arm-syntax-invalid-{}", std::process::id()));
    std::fs::create_dir_all(&dir).expect("temp dir");
    for (i, line) in invalid_lines().iter().enumerate() {
        let path = dir.join(format!("case{i}.s"));
        std::fs::write(&path, format!("{line}\n")).expect("write case");
        match clang_accepts(&path) {
            None => return eprintln!("skipped: the clang oracle runs only on macOS"),
            Some(Ok(())) => panic!("clang accepts {line:?}, so it isn't a syntax error"),
            Some(Err(_)) => {}
        }
    }
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn snapshots() {
    let update = std::env::var_os("UPDATE_SNAPSHOTS").is_some();
    let mut stale = Vec::new();
    for path in sources("parser") {
        let actual = parse(&read(&path)).debug_tree();
        let snapshot = path.with_extension("tree");
        if update {
            std::fs::write(&snapshot, &actual).expect("write snapshot");
        } else if std::fs::read_to_string(&snapshot).ok().as_deref() != Some(actual.as_str()) {
            stale.push(snapshot.display().to_string());
        }
    }
    assert!(
        stale.is_empty(),
        "snapshots differ: {stale:?}\nrun `UPDATE_SNAPSHOTS=1 cargo test -p arm-syntax --test parse` and review the diff"
    );
}
