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
}

impl Diagnostic {
    pub fn error(range: Range<usize>, code: &'static str, message: impl Into<String>) -> Self {
        Diagnostic {
            range,
            severity: Severity::Error,
            code,
            message: message.into(),
        }
    }
}
