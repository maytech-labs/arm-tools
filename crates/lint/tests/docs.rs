//! docs/lint.md has a section for every rule, with its current summary and
//! default level, so the docs can't fall behind the rules.

#[test]
fn every_rule_is_in_docs_lint_md() {
    let docs = std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/../../docs/lint.md"))
        .unwrap_or_default();
    for rule in arm_lint::rules() {
        let heading = format!("### `{}`", rule.id);
        let Some(section) = docs.split(&heading).nth(1) else {
            panic!("docs/lint.md has no section for {}", rule.id);
        };
        let section = section.split("### ").next().unwrap_or_default();
        assert!(
            section.contains(rule.summary),
            "{}: summary differs",
            rule.id
        );
        assert!(
            section.contains(rule.explanation),
            "{}: explanation differs",
            rule.id
        );
        let level = format!("{:?}", rule.default_level).to_lowercase();
        assert!(
            section.contains(&format!("Default level: {level}.")),
            "{}: default level differs",
            rule.id
        );
    }
}
