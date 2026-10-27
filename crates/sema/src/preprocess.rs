//! `.S` files: run clang's C preprocessor, analyze its output, and map what
//! we find back to the original lines (docs/decisions/0027-preprocessed-files.md).

use std::io::Write;
use std::ops::Range;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use crate::{Diagnostic, Encoding, LineIndex};

/// The preprocessor's output, and where each of its lines came from.
#[derive(Debug, Clone)]
pub struct Preprocessed {
    pub text: String,
    /// For each output line, its zero-based line in the original file, or
    /// None for lines from an included header or from the preprocessor.
    origin: Vec<Option<u32>>,
}

/// Whether a path is a file clang preprocesses: `.S`, not `.s`.
pub fn needs_preprocessing(path: &Path) -> bool {
    path.extension().is_some_and(|e| e == "S")
}

/// Runs `clang -E -x assembler-with-cpp` on `text` (fed on stdin, so unsaved
/// edits count), from the file's directory, with `-I` for each directory.
pub fn preprocess(
    text: &str,
    file: &Path,
    include_dirs: &[PathBuf],
) -> Result<Preprocessed, String> {
    let dir = file
        .parent()
        .filter(|d| d.is_dir())
        .unwrap_or(Path::new("."));
    let mut command = Command::new("xcrun");
    command
        .args(["clang", "-E", "-x", "assembler-with-cpp", "-arch", "arm64"])
        .current_dir(dir)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    for include in include_dirs {
        command.arg("-I").arg(include);
    }
    command.arg("-");
    let mut child = command
        .spawn()
        .map_err(|e| format!("can't run the C preprocessor (xcrun clang): {e}"))?;
    if let Some(mut stdin) = child.stdin.take() {
        stdin
            .write_all(text.as_bytes())
            .map_err(|e| format!("can't write to the preprocessor: {e}"))?;
    }
    let output = child
        .wait_with_output()
        .map_err(|e| format!("the preprocessor failed: {e}"))?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        let first = stderr
            .lines()
            .find(|l| l.contains("error"))
            .unwrap_or("it failed");
        return Err(format!(
            "the C preprocessor reports: {}",
            first.replace("<stdin>", "this file")
        ));
    }
    let text = String::from_utf8(output.stdout)
        .map_err(|_| "the preprocessor wrote invalid UTF-8".to_string())?;
    Ok(Preprocessed::from_output(text))
}

impl Preprocessed {
    /// Reads the `# 12 "<stdin>"` line markers in preprocessor output.
    pub fn from_output(text: String) -> Preprocessed {
        let mut origin = Vec::new();
        let mut current: Option<u32> = None;
        for line in text.lines() {
            if let Some((number, file)) = marker(line) {
                origin.push(None);
                current = (file == "<stdin>").then(|| number.saturating_sub(1));
                continue;
            }
            origin.push(current);
            current = current.map(|l| l + 1);
        }
        Preprocessed { text, origin }
    }

    /// The original line of an offset in the output.
    pub fn original_line(&self, offset: usize) -> Option<u32> {
        let line = self.text[..offset.min(self.text.len())]
            .matches('\n')
            .count();
        self.origin.get(line).copied().flatten()
    }

    /// Diagnostics on the output moved to the original text: the same columns
    /// when the line came through unchanged, else the whole line. Problems in
    /// included headers are dropped; they're reported in the header.
    pub fn map_diagnostics(&self, original: &str, diagnostics: Vec<Diagnostic>) -> Vec<Diagnostic> {
        let out_index = LineIndex::new(&self.text);
        let original_index = LineIndex::new(original);
        let original_lines: Vec<&str> = original.split('\n').collect();
        let output_lines: Vec<&str> = self.text.split('\n').collect();
        let mut mapped = Vec::new();
        for mut d in diagnostics {
            let Some(line) = self.original_line(d.range.start) else {
                continue;
            };
            let at = out_index.line_col(&self.text, d.range.start, Encoding::Utf8);
            let end = out_index.line_col(&self.text, d.range.end, Encoding::Utf8);
            let same = output_lines
                .get(at.line as usize)
                .map(|l| l.trim_end_matches('\r'))
                == original_lines
                    .get(line as usize)
                    .map(|l| l.trim_end_matches('\r'));
            let line_range = |line: u32| -> Range<usize> {
                let start = original_index.offset(
                    original,
                    crate::LineCol { line, col: 0 },
                    Encoding::Utf8,
                );
                let text = original_lines.get(line as usize).copied().unwrap_or("");
                let indent = text.len() - text.trim_start().len();
                start + indent..start + text.trim_end().len()
            };
            d.range = if same && end.line == at.line {
                let start = original_index.offset(
                    original,
                    crate::LineCol { line, col: at.col },
                    Encoding::Utf8,
                );
                start..start + (end.col - at.col) as usize
            } else {
                line_range(line)
            };
            // Fixes were computed on the output.
            d.fix = d.fix.filter(|_| same);
            mapped.push(d);
        }
        mapped
    }
}

/// `# 12 "<stdin>" 2` -> (12, "<stdin>").
fn marker(line: &str) -> Option<(u32, &str)> {
    let rest = line.strip_prefix("# ")?;
    let (number, rest) = rest.split_once(' ')?;
    let file = rest.trim_start().strip_prefix('"')?.split('"').next()?;
    Some((number.parse().ok()?, file))
}

#[cfg(test)]
mod tests {
    use super::*;

    const OUTPUT: &str = "# 1 \"<stdin>\"\n# 1 \"<built-in>\" 1\n# 1 \"<stdin>\" 2\n# 1 \"./defs.h\" 1\nX = 1\n# 2 \"<stdin>\" 2\n\n.globl _f\n_f:\n  add x0, x0, #5000\n";

    #[test]
    fn markers_map_output_lines_to_original_lines() {
        let p = Preprocessed::from_output(OUTPUT.into());
        let offset = |needle: &str| p.text.find(needle).expect("needle");
        assert_eq!(p.original_line(offset("X = 1")), None);
        assert_eq!(p.original_line(offset(".globl")), Some(2));
        assert_eq!(p.original_line(offset("add")), Some(4));
    }

    #[test]
    fn diagnostics_keep_columns_on_unchanged_lines() {
        let p = Preprocessed::from_output(OUTPUT.into());
        let original = "#include \"defs.h\"\n#define N 1\n.globl _f\n_f:\n  add x0, x0, #5000\n";
        let at = p.text.find("#5000").expect("immediate");
        let d = Diagnostic::error(at..at + 5, "invalid-operand", "range");
        let mapped = p.map_diagnostics(original, vec![d]);
        assert_eq!(&original[mapped[0].range.clone()], "#5000");
    }

    #[test]
    fn needs_preprocessing_only_for_capital_s() {
        assert!(needs_preprocessing(Path::new("a.S")));
        assert!(!needs_preprocessing(Path::new("a.s")));
    }

    #[test]
    fn oracle_preprocesses_with_clang() {
        if !cfg!(target_os = "macos") {
            return eprintln!("skipped: needs xcrun clang");
        }
        let text = "#define N 4\n#define CALL(f) bl f\n.globl _f\n_f:\n#if 0\n  frob junk\n#endif\n  mov x0, #N\n  CALL(_g)\n  add x0, x0, #5000\n  ret\n";
        let p = preprocess(text, Path::new("/tmp/a.S"), &[]).expect("clang -E");
        assert!(p.text.contains("mov x0, #4") && p.text.contains("bl _g"));
        assert!(!p.text.contains("frob"));
        let analysis = crate::Analysis::new(&p.text);
        let mapped = p.map_diagnostics(text, analysis.diagnostics().to_vec());
        let found: Vec<_> = mapped
            .iter()
            .map(|d| (d.code, &text[d.range.clone()]))
            .collect();
        assert_eq!(found, [("invalid-operand", "#5000")]);
    }
}
