//! `.include`: what's imported, cycles, missing files, and clang's agreement
//! (docs/decisions/0019-includes.md).

// Test setup may panic on a broken environment (AGENTS.md: expect is fine in tests).
#![allow(clippy::expect_used)]

use std::collections::HashMap;
use std::path::PathBuf;
use std::process::Command;

use arm_sema::{Analysis, Resolver, Severity, SymbolKind};

/// Files by name, in memory.
struct Files(HashMap<&'static str, &'static str>);

impl Resolver for Files {
    fn resolve(&self, name: &str) -> Option<(PathBuf, String)> {
        self.0
            .get(name)
            .map(|text| (PathBuf::from(format!("/project/{name}")), text.to_string()))
    }
}

const MACROS: &str = "\
.macro zero r
    mov \\r, #0
.endm
limit = 4095
tmp .req x9
_shared:
    ret
";

fn analyze(text: &str, files: &[(&'static str, &'static str)]) -> Analysis {
    let files = Files(files.iter().copied().collect());
    Analysis::with_includes(text, Some(PathBuf::from("/project/main.s")), &files)
}

fn codes(analysis: &Analysis) -> Vec<&'static str> {
    analysis.diagnostics().iter().map(|d| d.code).collect()
}

#[test]
fn included_definitions_are_known() {
    let text = ".include \"macros.s\"\n_f:\n    zero x0\n    add x0, x0, #limit\n    mov tmp, #1\n    b _shared\n";
    let analysis = analyze(text, &[("macros.s", MACROS)]);
    assert_eq!(codes(&analysis), Vec::<&str>::new());
}

#[test]
fn imported_constants_are_checked_as_values() {
    // 4097 fits no add encoding (4096 is #1, lsl #12), so the value must be known.
    let files = [("c.s", "big = 4097\n")];
    let analysis = analyze(".include \"c.s\"\nadd x0, x0, #big\n", &files);
    assert_eq!(codes(&analysis), ["invalid-operand"]);
}

#[test]
fn definitions_link_to_the_included_file() {
    let text = ".include \"macros.s\"\n    zero x0\n    b _shared\n";
    let analysis = analyze(text, &[("macros.s", MACROS)]);
    let call = text.find("zero x0").expect("call");
    let symbol = &analysis.symbols()[analysis.symbol_at(call).expect("resolves")];
    assert_eq!(symbol.kind, SymbolKind::Macro);
    let origin = symbol.origin.as_ref().expect("imported");
    assert_eq!(origin.path, PathBuf::from("/project/macros.s"));
    assert_eq!(&origin.text[symbol.range.clone()], "zero");
}

#[test]
fn nested_includes_keep_their_own_origin() {
    let files = [
        ("outer.s", ".include \"inner.s\"\n"),
        ("inner.s", "deep = 1\n"),
    ];
    let analysis = analyze(".include \"outer.s\"\nmov x0, #deep\n", &files);
    assert_eq!(codes(&analysis), Vec::<&str>::new());
    let deep = analysis
        .symbols()
        .iter()
        .find(|s| s.name == "deep")
        .expect("deep");
    let origin = deep.origin.as_ref().expect("imported");
    assert_eq!(origin.path, PathBuf::from("/project/inner.s"));
}

#[test]
fn imported_symbols_dont_shadow_positions_in_this_file() {
    // `_shared` is at offset 50-something in macros.s; nothing at that offset
    // here should resolve to it.
    let text = ".include \"macros.s\"\n";
    let analysis = analyze(text, &[("macros.s", MACROS)]);
    for offset in 0..text.len() {
        assert_eq!(analysis.symbol_at(offset), None, "{offset}");
    }
}

#[test]
fn a_label_defined_twice_across_files_is_an_error() {
    let analysis = analyze(
        ".include \"macros.s\"\n_shared: ret\n",
        &[("macros.s", MACROS)],
    );
    assert_eq!(codes(&analysis), ["duplicate-symbol"]);
}

#[test]
fn missing_includes_warn_and_say_how_to_fix() {
    let analysis = analyze(".include \"nope.s\"\nmy_macro x0\n", &[]);
    let found: Vec<_> = analysis
        .diagnostics()
        .iter()
        .map(|d| (d.code, d.severity))
        .collect();
    assert_eq!(
        found,
        [
            ("include-not-found", Severity::Warning),
            ("unknown-instruction", Severity::Warning)
        ]
    );
    assert!(
        analysis.diagnostics()[0]
            .message
            .contains("[include] paths")
    );
}

#[test]
fn include_cycles_are_errors() {
    let files = [("a.s", ".include \"b.s\"\n"), ("b.s", ".include \"a.s\"\n")];
    let analysis = Analysis::with_includes(
        ".include \"a.s\"\n",
        Some(PathBuf::from("/project/main.s")),
        &Files(files.iter().copied().collect()),
    );
    let cycle = analysis
        .diagnostics()
        .iter()
        .find(|d| d.code == "include-cycle");
    // The cycle is inside the included files; their errors stay with them,
    // and this file still analyzes without hanging.
    assert!(cycle.is_none());
    let direct = Files([("main.s", ".include \"main.s\"\n")].into_iter().collect());
    let analysis = Analysis::with_includes(
        ".include \"main.s\"\n",
        Some(PathBuf::from("/project/main.s")),
        &direct,
    );
    let message = &analysis.diagnostics()[0].message;
    assert!(message.contains("main.s includes main.s"), "{message}");
    assert!(message.contains("loops forever"), "{message}");
}

/// Resolves from a directory on disk, like clang run there with -I.
struct Disk(Vec<PathBuf>);

impl Resolver for Disk {
    fn resolve(&self, name: &str) -> Option<(PathBuf, String)> {
        self.0.iter().map(|dir| dir.join(name)).find_map(|path| {
            let text = std::fs::read_to_string(&path).ok()?;
            Some((path, text))
        })
    }
}

#[test]
fn oracle_clang_agrees_on_a_project() {
    if !cfg!(target_os = "macos") {
        return eprintln!("skipped: the clang oracle runs only on macOS");
    }
    let root = std::env::temp_dir().join(format!("arm-sema-includes-{}", std::process::id()));
    std::fs::create_dir_all(root.join("inc")).expect("dirs");
    std::fs::write(root.join("inc/macros.s"), MACROS).expect("write");
    let good = ".include \"macros.s\"\n_f:\n    zero x0\n    add x0, x0, #limit\n    mov tmp, #1\n    b _shared\n";
    let bad = ".include \"macros.s\"\n_f:\n    add x0, x0, #limit + 2\n";
    let resolver = Disk(vec![root.join("inc")]);
    for (text, valid) in [(good, true), (bad, false)] {
        let path = root.join("main.s");
        std::fs::write(&path, text).expect("write");
        let analysis = Analysis::with_includes(text, Some(path.clone()), &resolver);
        let errors = analysis
            .diagnostics()
            .iter()
            .filter(|d| d.severity == Severity::Error)
            .count();
        let clang = Command::new("xcrun")
            .args([
                "clang", "-c", "-arch", "arm64", "-I", "inc", "-o", "main.o", "main.s",
            ])
            .current_dir(&root)
            .output()
            .expect("xcrun");
        assert_eq!(clang.status.success(), valid, "clang on {text:?}");
        assert_eq!(
            errors == 0,
            valid,
            "arm-sema on {text:?}: {:?}",
            analysis.diagnostics()
        );
    }
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn found_includes_are_listed_for_links() {
    let text = ".include \"macros.s\"\n.include \"missing.s\"\n";
    let analysis = analyze(text, &[("macros.s", MACROS)]);
    let found: Vec<(&str, PathBuf)> = analysis
        .includes()
        .iter()
        .map(|(range, path)| (&text[range.clone()], path.clone()))
        .collect();
    assert_eq!(found, [("macros.s", PathBuf::from("/project/macros.s"))]);
    // Without a resolver nothing is looked up.
    assert!(Analysis::new(text).includes().is_empty());
}
