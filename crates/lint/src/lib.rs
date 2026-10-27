//! Lint rules for Apple AArch64 assembly (docs/decisions/0018-lint.md). Rules
//! work from `arm_sema::Analysis`: its items, operands, and symbols. They run
//! in `armls` as diagnostics and in `armtool lint` for CI.

mod config;
mod flow;
mod rules;

use std::collections::HashMap;

use arm_sema::{Analysis, Diagnostic, Encoding, Severity};
use arm_syntax::SyntaxKind;

pub use config::{Config, Level};

/// A lint rule's description, for docs, `armtool rules`, and hover.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RuleInfo {
    /// `group/name`, like `abi/x18-reserved`.
    pub id: &'static str,
    pub default_level: Level,
    /// One line: what it catches.
    pub summary: &'static str,
    /// Why it matters and how to fix it.
    pub explanation: &'static str,
}

/// Every rule, in the order they run.
pub fn rules() -> &'static [RuleInfo] {
    rules::RULES
}

pub fn rule(id: &str) -> Option<&'static RuleInfo> {
    rules::RULES.iter().find(|r| r.id == id)
}

/// Runs every enabled rule on a file. Findings on a line with
/// `; armtool: allow(rule-id)`, or right below one, are dropped.
pub fn lint(analysis: &Analysis, config: &Config) -> Vec<Diagnostic> {
    let allows = allow_comments(analysis);
    let line_of = |offset: usize| {
        analysis
            .line_index()
            .line_col(analysis.text(), offset, Encoding::Utf8)
            .line
    };
    let mut out = Vec::new();
    for finding in rules::run(analysis) {
        let Some(info) = rule(finding.rule) else {
            continue;
        };
        let level = config.level(info);
        let Some(severity) = level.severity() else {
            continue;
        };
        let line = line_of(finding.range.start);
        let allowed = [Some(line), line.checked_sub(1)]
            .into_iter()
            .flatten()
            .any(|l| {
                allows
                    .get(&l)
                    .is_some_and(|ids| ids.iter().any(|id| id == info.id || id == "all"))
            });
        if allowed {
            continue;
        }
        out.push(Diagnostic {
            range: finding.range,
            severity,
            code: info.id,
            message: finding.message,
            fix: finding.fix,
        });
    }
    out.sort_by_key(|d| (d.range.start, d.range.end));
    out
}

/// One finding, before configuration decides its severity.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Finding {
    pub rule: &'static str,
    pub range: std::ops::Range<usize>,
    pub message: String,
    pub fix: Option<arm_sema::Fix>,
}

/// Lines with `armtool: allow(a, b)` in a comment, and the rule IDs they name.
fn allow_comments(analysis: &Analysis) -> HashMap<u32, Vec<String>> {
    let mut out: HashMap<u32, Vec<String>> = HashMap::new();
    for token in analysis
        .parse()
        .syntax()
        .descendants_with_tokens()
        .filter_map(|e| e.into_token())
        .filter(|t| t.kind() == SyntaxKind::Comment)
    {
        let text = token.text();
        let Some(rest) = text.split_once("armtool: allow(").map(|(_, r)| r) else {
            continue;
        };
        let Some(list) = rest.split(')').next() else {
            continue;
        };
        let start = usize::from(token.text_range().start());
        let line = analysis
            .line_index()
            .line_col(analysis.text(), start, Encoding::Utf8)
            .line;
        out.entry(line).or_default().extend(
            list.split(',')
                .map(|id| id.trim().to_string())
                .filter(|id| !id.is_empty()),
        );
    }
    out
}

impl Level {
    /// The diagnostic severity for this level, or None when it's off.
    pub fn severity(self) -> Option<Severity> {
        match self {
            Level::Error => Some(Severity::Error),
            Level::Warning => Some(Severity::Warning),
            Level::Info => Some(Severity::Info),
            Level::Hint => Some(Severity::Hint),
            Level::Off => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Rule IDs found, leaving out the unused-label hint the snippets trip.
    fn codes(text: &str) -> Vec<&'static str> {
        lint(&Analysis::new(text), &Config::default())
            .into_iter()
            .map(|d| d.code)
            .filter(|c| *c != "style/unused-label")
            .collect()
    }

    #[test]
    fn every_rule_is_documented() {
        for rule in rules() {
            assert!(rule.id.contains('/'), "{}", rule.id);
            assert!(
                !rule.summary.is_empty() && !rule.explanation.is_empty(),
                "{}",
                rule.id
            );
        }
        let mut ids: Vec<_> = rules().iter().map(|r| r.id).collect();
        ids.sort_unstable();
        ids.dedup();
        assert_eq!(ids.len(), rules().len(), "duplicate rule IDs");
    }

    #[test]
    fn allow_comments_silence_a_rule() {
        let text = "_f:\n    mov x18, #1\n    ret\n";
        assert_eq!(codes(text), ["abi/x18-reserved"]);
        let same_line = "_f:\n    mov x18, #1 ; armtool: allow(abi/x18-reserved)\n    ret\n";
        assert_eq!(codes(same_line), Vec::<&str>::new());
        let line_above = "_f:\n    // armtool: allow(abi/x18-reserved)\n    mov x18, #1\n    ret\n";
        assert_eq!(codes(line_above), Vec::<&str>::new());
        let other_rule = "_f:\n    mov x18, #1 ; armtool: allow(style/unused-label)\n    ret\n";
        assert_eq!(codes(other_rule), ["abi/x18-reserved"]);
    }

    #[test]
    fn config_turns_rules_off_and_changes_severity() {
        let analysis = Analysis::new(".globl _f\n_f:\n    mov x18, #1\n    ret\n");
        let off = Config::parse("[rules]\n\"abi/x18-reserved\" = \"off\"\n").expect("valid");
        assert!(lint(&analysis, &off).is_empty());
        let error = Config::parse("[rules]\n\"abi/x18-reserved\" = \"error\"\n").expect("valid");
        assert_eq!(lint(&analysis, &error)[0].severity, Severity::Error);
    }
}
