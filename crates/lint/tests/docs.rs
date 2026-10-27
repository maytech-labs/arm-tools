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

/// The docs site's table of contents (docs/SUMMARY.md, for mdBook) lists
/// every page and every decision, so none is left off the site.
#[test]
fn the_site_lists_every_page() {
    let docs = std::path::Path::new(concat!(env!("CARGO_MANIFEST_DIR"), "/../../docs"));
    let summary = std::fs::read_to_string(docs.join("SUMMARY.md")).unwrap_or_default();
    let mut pages: Vec<String> = Vec::new();
    for dir in ["", "decisions"] {
        for entry in std::fs::read_dir(docs.join(dir))
            .into_iter()
            .flatten()
            .flatten()
        {
            let name = entry.file_name().to_string_lossy().into_owned();
            if name.ends_with(".md") && name != "SUMMARY.md" && !name.starts_with("0000") {
                pages.push(if dir.is_empty() {
                    name
                } else {
                    format!("{dir}/{name}")
                });
            }
        }
    }
    assert!(pages.len() > 40, "{}", pages.len());
    for page in pages {
        assert!(
            summary.contains(&format!("]({page})")),
            "docs/SUMMARY.md doesn't list {page}"
        );
    }
}
