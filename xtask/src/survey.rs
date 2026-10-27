//! `cargo xtask survey`: checks that every line arm-sema reports an error on
//! (syntax or semantic) is one clang rejects too (docs/decisions/0007-testing.md). Run it on large
//! bodies of real assembly we don't check in, like LLVM's MC tests.
//!
//! Clang sometimes stops reporting partway through a file, so a flagged line
//! clang didn't mention is assembled again on its own before it counts as a
//! disagreement. Some errors, like duplicate symbols and ELF relocations, clang
//! raises while writing the object file, at a line in a temporary file. In a
//! file with N of those, up to N lines we flag count as agreeing.

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

pub fn run(args: Vec<String>) -> ExitCode {
    let recall = args.iter().any(|a| a == "--recall");
    let paths: Vec<String> = args.into_iter().filter(|a| a != "--recall").collect();
    if paths.is_empty() {
        eprintln!("usage: cargo xtask survey [--recall] <path>...");
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

    let (mut flagged, mut agreed, mut isolated, mut late) = (0, 0, 0, 0);
    // With --recall: lines clang rejects, and how many of those we flag too.
    let (mut clang_lines, mut caught) = (0, 0);
    let mut disagreements = Vec::new();
    for file in &files {
        let Ok(text) = std::fs::read_to_string(file) else {
            continue;
        };
        let ours = error_lines(&text);
        if ours.is_empty() && !recall {
            continue;
        }
        let mut theirs = match clang_errors(file, &scratch) {
            Ok(errors) => errors,
            Err(error) => {
                eprintln!("{error}");
                return ExitCode::FAILURE;
            }
        };
        clang_lines += theirs.lines.len();
        caught += theirs.lines.iter().filter(|l| ours.contains_key(l)).count();
        for (line, message) in ours {
            flagged += 1;
            if theirs.lines.contains(&line) {
                agreed += 1;
                continue;
            }
            if theirs.late > 0 {
                theirs.late -= 1;
                late += 1;
                continue;
            }
            let source = text.lines().nth(line - 1).unwrap_or_default();
            let alone = scratch.join("line.s");
            let rejected_alone = std::fs::write(&alone, format!("{source}\n")).is_ok()
                && clang_errors(&alone, &scratch).is_ok_and(|e| !e.lines.is_empty() || e.late > 0);
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
        "{} files, {flagged} lines we flag: clang rejects {agreed} in the file, {late} while writing the object, {isolated} more on their own; {} disagree",
        files.len(),
        disagreements.len()
    );
    if recall {
        println!(
            "recall: clang rejects {clang_lines} lines; arm-sema flags {caught} of them ({:.1}%)",
            100.0 * caught as f64 / clang_lines.max(1) as f64
        );
    }
    if disagreements.is_empty() {
        ExitCode::SUCCESS
    } else {
        println!(
            "each disagreement is a line clang accepts that arm-sema reports: fix it, or add a test showing why"
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

/// 1-based lines where arm-sema reports an error, with the first message on each.
fn error_lines(text: &str) -> BTreeMap<usize, String> {
    let mut lines = BTreeMap::new();
    // The same target as the clang command below.
    // On Darwin, -march adds to the apple-m1 CPU's features.
    let mut features = arm_isa::FeatureSet::apple_default();
    let features = features
        .add_arch("armv9.4-a+sve2+sme2")
        .ok()
        .map(|()| features);
    let analysis = arm_sema::Analysis::with_options(text, None, None, features);
    let errors = analysis
        .diagnostics()
        .iter()
        .filter(|d| d.severity == arm_sema::Severity::Error);
    for error in errors {
        let line = text[..error.range.start].matches('\n').count() + 1;
        lines
            .entry(line)
            .or_insert_with(|| format!("[{}] {}", error.code, error.message));
    }
    lines
}

/// Where clang reports errors in a file.
struct ClangErrors {
    /// 1-based lines in the file.
    lines: BTreeSet<usize>,
    /// Errors located in some other file, raised while writing the object.
    late: usize,
}

fn clang_errors(file: &Path, scratch: &Path) -> Result<ClangErrors, String> {
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
    let mut errors = ClangErrors {
        lines: BTreeSet::new(),
        late: 0,
    };
    let file = file.to_string_lossy();
    for (location, _) in stderr.lines().filter_map(|l| l.split_once(": error:")) {
        // `path:line:col`; the path may itself contain colons.
        let mut parts = location.rsplitn(3, ':');
        let (_col, line, path) = (parts.next(), parts.next(), parts.next());
        match (line.and_then(|l| l.parse().ok()), path) {
            (Some(line), Some(path)) if path == file => {
                errors.lines.insert(line);
            }
            _ => errors.late += 1,
        }
    }
    Ok(errors)
}
