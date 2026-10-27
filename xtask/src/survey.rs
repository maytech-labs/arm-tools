//! `cargo xtask survey`: checks that every line arm-syntax reports as an error
//! is one clang rejects too (docs/decisions/0007-testing.md). Run it on large
//! bodies of real assembly we don't check in, like LLVM's MC tests.
//!
//! Clang sometimes stops reporting partway through a file, so a flagged line
//! clang didn't mention is assembled again on its own before it counts as a
//! disagreement.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode};

/// Clang flags that enable every extension, so feature errors don't hide syntax.
const CLANG: &[&str] = &[
    "clang",
    "-c",
    "-arch",
    "arm64",
    "-march=armv9.4-a+sve2+sme2",
];

pub fn run(paths: Vec<String>) -> ExitCode {
    if paths.is_empty() {
        eprintln!("usage: cargo xtask survey <path>...");
        return ExitCode::from(2);
    }
    let scratch = std::env::temp_dir().join(format!("arm-tools-survey-{}", std::process::id()));
    if let Err(error) = std::fs::create_dir_all(&scratch) {
        eprintln!("can't create {}: {error}", scratch.display());
        return ExitCode::FAILURE;
    }
    let mut files = Vec::new();
    for path in &paths {
        walk(Path::new(path), &mut files);
    }
    files.sort();

    let (mut flagged, mut agreed, mut isolated) = (0, 0, 0);
    let mut disagreements = Vec::new();
    for file in &files {
        let Ok(text) = std::fs::read_to_string(file) else {
            continue;
        };
        let ours = error_lines(&text);
        if ours.is_empty() {
            continue;
        }
        let theirs = match clang_error_lines(file, &scratch) {
            Ok(lines) => lines,
            Err(error) => {
                eprintln!("{error}");
                return ExitCode::FAILURE;
            }
        };
        for (line, message) in ours {
            flagged += 1;
            if theirs.contains(&line) {
                agreed += 1;
                continue;
            }
            let source = text.lines().nth(line - 1).unwrap_or_default();
            let alone = scratch.join("line.s");
            let rejected_alone = std::fs::write(&alone, format!("{source}\n")).is_ok()
                && clang_error_lines(&alone, &scratch).is_ok_and(|l| !l.is_empty());
            if rejected_alone {
                isolated += 1;
            } else {
                disagreements.push(format!("{}:{line}: {message}", file.display()));
            }
        }
    }
    let _ = std::fs::remove_dir_all(&scratch);

    for d in &disagreements {
        println!("{d}");
    }
    println!(
        "{} files, {flagged} lines we flag: clang rejects {agreed} in the file, {isolated} more on their own; {} disagree",
        files.len(),
        disagreements.len()
    );
    if disagreements.is_empty() {
        ExitCode::SUCCESS
    } else {
        println!(
            "each disagreement is a line clang accepts that arm-syntax reports: fix the parser, or add a test showing why"
        );
        ExitCode::FAILURE
    }
}

fn walk(path: &Path, out: &mut Vec<PathBuf>) {
    if path.is_dir() {
        let Ok(entries) = std::fs::read_dir(path) else {
            return;
        };
        for entry in entries.flatten() {
            walk(&entry.path(), out);
        }
    } else if path.extension().is_some_and(|e| e == "s") {
        out.push(path.to_path_buf());
    }
}

/// 1-based lines where arm-syntax reports an error, with the first message on each.
fn error_lines(text: &str) -> BTreeMap<usize, String> {
    let mut lines = BTreeMap::new();
    for error in arm_syntax::parse(text).errors() {
        let line = text[..error.range.start].matches('\n').count() + 1;
        lines.entry(line).or_insert_with(|| error.message.clone());
    }
    lines
}

/// 1-based lines of `file` where clang reports an error.
fn clang_error_lines(file: &Path, scratch: &Path) -> Result<BTreeSet<usize>, String> {
    let output = Command::new("xcrun")
        .args(CLANG)
        .arg("-o")
        .arg(scratch.join("out.o"))
        .arg(file)
        .output()
        .map_err(|e| {
            format!("can't run xcrun clang: {e}. The survey needs Xcode's clang (macOS).")
        })?;
    let stderr = String::from_utf8_lossy(&output.stderr);
    Ok(stderr
        .lines()
        .filter_map(|l| l.split_once(": error:"))
        .filter_map(|(location, _)| {
            // `path:line:col`; the path may itself contain colons.
            let mut parts = location.rsplitn(3, ':');
            let _col = parts.next()?;
            parts.next()?.parse().ok()
        })
        .collect())
}
