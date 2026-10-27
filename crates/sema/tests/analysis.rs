//! arm-sema's contract: what it defines, resolves, and reports, checked
//! against clang where clang has an opinion (docs/decisions/0007-testing.md).

// Test helpers may panic on broken fixtures (AGENTS.md: expect is fine in tests).
#![allow(clippy::expect_used)]

use std::path::{Path, PathBuf};
use std::process::Command;

use arm_sema::{Analysis, Severity, SymbolKind};

fn files(dir: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    let mut stack = vec![dir.to_path_buf()];
    while let Some(dir) = stack.pop() {
        for entry in std::fs::read_dir(&dir).into_iter().flatten().flatten() {
            let path = entry.path();
            if path.is_dir() {
                stack.push(path);
            } else if path.extension().is_some_and(|e| e == "s") {
                out.push(path);
            }
        }
    }
    out.sort();
    assert!(!out.is_empty(), "no .s files in {}", dir.display());
    out
}

fn manifest() -> &'static Path {
    Path::new(env!("CARGO_MANIFEST_DIR"))
}

fn read(path: &Path) -> String {
    std::fs::read_to_string(path).unwrap_or_else(|e| panic!("{}: {e}", path.display()))
}

/// Whether clang accepts the file, or None off macOS.
fn clang_accepts(path: &Path) -> Option<bool> {
    if !cfg!(target_os = "macos") {
        return None;
    }
    let object = std::env::temp_dir().join(format!("arm-sema-{}.o", std::process::id()));
    let status = Command::new("xcrun")
        .args(["clang", "-c", "-arch", "arm64", "-o"])
        .arg(&object)
        .arg(path)
        .output()
        .map(|o| o.status.success());
    let _ = std::fs::remove_file(&object);
    Some(status.unwrap_or_else(|e| panic!("xcrun: {e}")))
}

/// (code, text at the range) for each diagnostic.
fn diagnostics(text: &str) -> Vec<(&'static str, String)> {
    Analysis::new(text)
        .diagnostics()
        .iter()
        .map(|d| (d.code, text[d.range.clone()].to_string()))
        .collect()
}

#[test]
fn valid_corpus_has_no_diagnostics() {
    for path in files(&manifest().join("../syntax/tests/data/corpus")) {
        let text = read(&path);
        let analysis = Analysis::new(&text);
        assert_eq!(analysis.diagnostics(), [], "{}", path.display());
    }
}

#[test]
fn invalid_cases_are_reported() {
    for path in files(&manifest().join("tests/data/invalid")) {
        let analysis = Analysis::new(&read(&path));
        assert!(
            analysis
                .diagnostics()
                .iter()
                .any(|d| d.severity == Severity::Error),
            "no error for {}",
            path.display()
        );
    }
}

#[test]
fn oracle_clang_rejects_the_invalid_cases() {
    for path in files(&manifest().join("tests/data/invalid")) {
        match clang_accepts(&path) {
            None => return eprintln!("skipped: the clang oracle runs only on macOS"),
            Some(true) => panic!("clang accepts {}, so it isn't an error", path.display()),
            Some(false) => {}
        }
    }
}

#[test]
fn valid_cases_have_no_diagnostics() {
    for path in files(&manifest().join("tests/data/valid")) {
        let analysis = Analysis::new(&read(&path));
        assert_eq!(analysis.diagnostics(), [], "{}", path.display());
    }
}

#[test]
fn oracle_clang_accepts_the_valid_cases() {
    for path in files(&manifest().join("tests/data/valid")) {
        match clang_accepts(&path) {
            None => return eprintln!("skipped: the clang oracle runs only on macOS"),
            Some(false) => panic!("clang rejects {}, so it isn't valid", path.display()),
            Some(true) => {}
        }
    }
}

#[test]
fn inactive_branches_are_reported_for_greying_out() {
    let text = ".if 0\nfrob\n.else\nnop\n.endif\n";
    let analysis = Analysis::new(text);
    assert_eq!(analysis.inactive().len(), 1);
    let skipped = &text[analysis.inactive()[0].clone()];
    assert!(
        skipped.contains("frob") && !skipped.contains("nop"),
        "{skipped:?}"
    );
}

#[test]
fn errors_point_at_the_problem() {
    assert_eq!(
        diagnostics("_f: nop\n_f: nop"),
        [("duplicate-symbol", "_f".to_string())]
    );
    assert_eq!(
        diagnostics("_f: b 1f"),
        [("undefined-local-label", "1f".to_string())]
    );
    assert_eq!(
        diagnostics("b.xx _f"),
        [("invalid-condition", "b.xx".to_string())]
    );
    assert_eq!(
        diagnostics(".macro m\nnop"),
        [("unbalanced-block", ".macro".to_string())]
    );
}

#[test]
fn messages_suggest_fixes() {
    let message = |text: &str| Analysis::new(text).diagnostics()[0].message.clone();
    assert!(message("movv x0, x1").contains("did you mean 'mov'?"));
    assert!(message(".globll _f").contains("did you mean '.global'?"));
    assert!(message(".type _f, @function").contains("ELF only"));
    assert!(!message(".type _f, @function").contains("did you mean"));
    assert!(message("b.eqq _f").contains("did you mean 'eq'?"));
    assert!(message("m\n.macro m\n.endm").contains("move the .macro above"));
}

#[test]
fn operand_errors_use_clangs_words() {
    let message = |text: &str| {
        let analysis = Analysis::new(text);
        let d = analysis
            .diagnostics()
            .first()
            .cloned()
            .expect("a diagnostic");
        assert_eq!(d.code, "invalid-operand", "{text}");
        (text[d.range.clone()].to_string(), d.message)
    };
    let (at, text) = message("add x0, x0, #5000");
    assert_eq!(at, "#5000");
    assert!(text.contains("[0, 4095]"), "{text}");
    let (at, text) = message("ldp x0, x1, [sp, #3]");
    assert_eq!(at, "#3");
    assert!(text.contains("multiple of 8"), "{text}");
    let (at, text) = message("add x0, w1, x2");
    assert_eq!(at, "w1");
    assert!(text.contains("the closest form is"), "{text}");
    let (_, text) = message("add x0, x1");
    assert!(text.starts_with("too few operands"), "{text}");
}

#[test]
fn valid_operands_have_no_diagnostics() {
    let text = "\
counter .req x19
.set limit, 4095
_f:
    add x0, x0, #limit
    add x0, x0, #1, lsl #12
    add x0, x0, _g@PAGEOFF
    ldr x1, [x0, _g@GOTPAGEOFF]
    mov counter, #0x10000
    mov x0, #-1
    and x0, x1, #0xff00ff00ff00ff00
    ldp x29, x30, [sp], #16
    ld1 {v0.16b-v3.16b}, [x0], #64
    fmov d0, #1.5
    fcmp s0, #0.0
    csel x0, x1, x2, ne
    sxtw x0, w1
    casp x0, x1, x2, x3, [x4]
    ret
";
    assert_eq!(Analysis::new(text).diagnostics(), []);
}

/// The text after applying the first diagnostic's fix.
fn fixed(text: &str) -> String {
    let analysis = Analysis::new(text);
    let fix = analysis.diagnostics()[0].fix.clone().expect("a fix");
    let mut out = text.to_string();
    for (range, new) in fix.edits.iter().rev() {
        out.replace_range(range.clone(), new);
    }
    out
}

#[test]
fn suggestions_come_with_fixes() {
    assert_eq!(fixed("movv x0, x1\n"), "mov x0, x1\n");
    assert_eq!(fixed(".globll _f\n"), ".global _f\n");
    assert_eq!(fixed("b.eqq _f\n"), "b.eq _f\n");
    assert_eq!(fixed(".type _f, @function\nret\n"), "\nret\n");
    // No suggestion, no fix.
    let analysis = Analysis::new("frobnicate x0\n");
    assert!(analysis.diagnostics()[0].fix.is_none());
}

#[test]
fn cpu_features_follow_directives() {
    let codes = |text: &str| -> Vec<&'static str> {
        Analysis::new(text)
            .diagnostics()
            .iter()
            .map(|d| d.code)
            .collect()
    };
    assert_eq!(codes("abs x0, x1\n"), ["missing-feature"]);
    assert!(codes(".arch_extension cssc\nabs x0, x1\n").is_empty());
    assert!(codes(".cpu apple-m4\nbfmmla v0.4s, v1.8h, v2.8h\n").is_empty());
    assert_eq!(
        codes(".arch armv8-a\nldaddal x0, x1, [x2]\n"),
        ["missing-feature"]
    );
    // An .arch we can't read turns feature checks off rather than guessing.
    assert!(codes(".arch armv99-z\nabs x0, x1\n").is_empty());
    let analysis = Analysis::new("abs x0, x1\n");
    let message = &analysis.diagnostics()[0].message;
    assert!(
        message.starts_with("instruction requires: cssc"),
        "{message}"
    );
    assert!(message.contains(".arch_extension cssc"), "{message}");
}

#[test]
fn unknown_instruction_after_include_is_a_warning() {
    let analysis = Analysis::new(".include \"macros.s\"\nmy_macro x0\n");
    let severities: Vec<_> = analysis.diagnostics().iter().map(|d| d.severity).collect();
    assert_eq!(severities, [Severity::Warning]);
}

#[test]
fn macro_bodies_are_checked_only_when_called() {
    // Clang checks a macro's lines when it expands them, not at the definition.
    let text = ".macro m op\n\\op x0, x1\nfrob\n_x: nop\n.endm\n";
    assert_eq!(Analysis::new(text).diagnostics(), []);
}

#[test]
fn expansions_are_checked_at_the_call() {
    let codes = |text: &str| -> Vec<(&'static str, String)> {
        let analysis = Analysis::new(text);
        analysis
            .diagnostics()
            .iter()
            .map(|d| (d.code, text[d.range.clone()].to_string()))
            .collect()
    };
    let call = ".macro m r\nadd \\r, \\r, #5000\n.endm\nm x0\n";
    assert_eq!(codes(call), [("invalid-operand", "m x0".to_string())]);
    let good = ".macro m r\nadd \\r, \\r, #1\n.endm\nm x0\n";
    assert!(codes(good).is_empty());
    let rept = ".rept 2\nfrob\n.endr\n";
    assert_eq!(codes(rept)[0].0, "unknown-instruction");
    let irp = ".irp r, x0, x1\nadd \\r, \\r, #5000\n.endr\n";
    assert_eq!(
        codes(irp).len(),
        1,
        "one error, not one per value: {:?}",
        codes(irp)
    );
    let analysis = Analysis::new(call);
    let message = &analysis.diagnostics()[0].message;
    assert!(message.ends_with("(in the expansion of 'm')"), "{message}");
}

#[test]
fn macro_arguments_are_bound_like_clang() {
    let codes = |text: &str| -> Vec<&'static str> {
        Analysis::new(text)
            .diagnostics()
            .iter()
            .map(|d| d.code)
            .collect()
    };
    // Brackets don't group arguments: this is three, for two parameters.
    assert_eq!(
        codes(".macro m a, b\nldr \\a, \\b\n.endm\nm x0, [x1, #8]\n"),
        ["macro-arguments"]
    );
    assert_eq!(
        codes(".macro m a:req\n.quad \\a\n.endm\nm\n"),
        ["macro-arguments"]
    );
    assert!(codes(".macro m a=1, b=2\n.quad \\a, \\b\n.endm\nm b=5\n").is_empty());
    assert!(codes(".macro m a, b:vararg\n.quad \\a, \\b\n.endm\nm 1, 2, 3\n").is_empty());
    assert!(codes(".macro m\nl\\@: nop\n.endm\nm\nm\n").is_empty());
    assert!(codes(".macro m\nnop\n.exitm\nfrob\n.endm\nm\n").is_empty());
}

#[test]
fn symbols() {
    let text = "\
.globl _main
_main:
1:  b 1b
counter .req x19
.set limit, 10
size = 4
.macro push2 a, b
.endm
";
    let analysis = Analysis::new(text);
    let symbols: Vec<_> = analysis
        .symbols()
        .iter()
        .map(|s| (s.name.as_str(), s.kind, s.exported))
        .collect();
    assert_eq!(
        symbols,
        [
            ("_main", SymbolKind::Label, true),
            ("1", SymbolKind::LocalLabel, false),
            ("counter", SymbolKind::RegisterAlias, false),
            ("limit", SymbolKind::Constant, false),
            ("size", SymbolKind::Constant, false),
            ("push2", SymbolKind::Macro, false),
        ]
    );
}

#[test]
fn local_labels_resolve_by_direction() {
    let text = "1: nop\nb 1f\nb 1b\n1: nop\nb 1b\n";
    let analysis = Analysis::new(text);
    let targets: Vec<usize> = analysis
        .references()
        .iter()
        .map(|r| analysis.symbols()[r.symbol.expect("resolved")].range.start)
        .collect();
    let second = text.rfind("1:").expect("second label");
    // 1f goes to the second label; the first 1b to the first; the last 1b to the second.
    assert_eq!(targets, [second, 0, second]);
}

#[test]
fn references_resolve_to_definitions() {
    let text = "_f: b _g\n_g: adrp x0, _g@PAGE\nbl _printf\n";
    let analysis = Analysis::new(text);
    let g = analysis
        .symbols()
        .iter()
        .position(|s| s.name == "_g")
        .expect("_g");
    assert_eq!(analysis.references_to(g).count(), 2);
    let printf = analysis
        .references()
        .iter()
        .find(|r| r.name == "_printf")
        .expect("_printf");
    assert_eq!(printf.symbol, None);
    let at = text.find("_g@").expect("use") + 1;
    assert_eq!(analysis.symbol_at(at), Some(g));
}

#[test]
fn register_aliases_are_scoped_and_case_insensitive() {
    let text = "tmp .req x9\nmov TMP, #1\n.unreq tmp\nmov tmp, #1\n";
    let analysis = Analysis::new(text);
    let resolved: Vec<_> = analysis
        .references()
        .iter()
        .filter(|r| r.name.eq_ignore_ascii_case("tmp"))
        .map(|r| r.symbol.is_some())
        .collect();
    assert_eq!(resolved, [true, true, false]);
}

#[test]
fn macro_calls_resolve_to_the_macro() {
    let text = ".macro zero r\nmov \\r, #0\n.endm\nzero x0\n";
    let analysis = Analysis::new(text);
    let call = text.rfind("zero").expect("call");
    let symbol = analysis.symbol_at(call).expect("resolves");
    assert_eq!(analysis.symbols()[symbol].kind, SymbolKind::Macro);
}
