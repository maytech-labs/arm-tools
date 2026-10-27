//! Every instruction in the parser's corpus is one clang knows: real code from
//! the compiler, HelloSilicon, and hand-written cases
//! (docs/decisions/0013-isa-from-llvm-tablegen.md).

use std::path::{Path, PathBuf};

use arm_syntax::ast::Statement;

fn corpus() -> Vec<PathBuf> {
    fn walk(dir: &Path, out: &mut Vec<PathBuf>) {
        for entry in std::fs::read_dir(dir).into_iter().flatten().flatten() {
            let path = entry.path();
            if path.is_dir() {
                walk(&path, out);
            } else if path.extension().is_some_and(|e| e == "s") {
                out.push(path);
            }
        }
    }
    let mut out = Vec::new();
    walk(
        &Path::new(env!("CARGO_MANIFEST_DIR")).join("../syntax/tests/data/corpus"),
        &mut out,
    );
    assert!(!out.is_empty(), "the syntax corpus is missing");
    out
}

#[test]
fn every_corpus_mnemonic_is_known() {
    let mut unknown = Vec::new();
    let mut checked = 0;
    for path in corpus() {
        let text = std::fs::read_to_string(&path).unwrap_or_default();
        for statement in arm_syntax::parse(&text).tree().statements() {
            let Statement::Instruction(insn) = statement else {
                continue;
            };
            let Some(mnemonic) = insn.mnemonic() else {
                continue;
            };
            checked += 1;
            if !arm_isa::is_instruction(mnemonic.text()) {
                unknown.push(format!("{}: {}", path.display(), mnemonic.text()));
            }
        }
    }
    assert!(checked > 500, "only {checked} instructions in the corpus");
    assert_eq!(unknown, Vec::<String>::new());
}
