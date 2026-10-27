//! The one diagnostic shape every layer produces (docs/design.md#architecture).
//! `armls` turns these into LSP diagnostics; `armtool` prints them.

use std::ops::Range;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Severity {
    Error,
    Warning,
    Info,
    Hint,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Diagnostic {
    /// Byte range in the file.
    pub range: Range<usize>,
    pub severity: Severity,
    /// What kind of problem: `syntax`, `unknown-instruction`, or a lint rule ID.
    pub code: &'static str,
    /// What's wrong and what to do about it.
    pub message: String,
    /// An edit that fixes it, offered in the editor as a quick fix.
    pub fix: Option<Fix>,
}

/// A quick fix: text replacements in the same file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Fix {
    /// What the fix does, as the editor shows it: "Change to 'mov'".
    pub title: String,
    /// Byte ranges and their replacements. An empty range inserts.
    pub edits: Vec<(Range<usize>, String)>,
}

impl Diagnostic {
    pub fn error(range: Range<usize>, code: &'static str, message: impl Into<String>) -> Self {
        Diagnostic {
            range,
            severity: Severity::Error,
            code,
            message: message.into(),
            fix: None,
        }
    }

    /// The same diagnostic, with a fix that replaces its range with `text`.
    pub fn replace_with(mut self, title: impl Into<String>, text: impl Into<String>) -> Self {
        self.fix = Some(Fix {
            title: title.into(),
            edits: vec![(self.range.clone(), text.into())],
        });
        self
    }
}
