//! Calls into the project's C functions are checked against their
//! prototypes (docs/decisions/0040-c-signatures.md).

// Test setup may panic on a broken environment (AGENTS.md: expect is fine in tests).
#![allow(clippy::expect_used)]

use std::collections::HashMap;

#[test]
fn prototypes_in_project_headers() {
    let root = std::env::temp_dir().join(format!("arm-lint-c-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(root.join(".git")).expect("project");
    std::fs::create_dir_all(root.join("include")).expect("dirs");
    std::fs::write(
        root.join("include/geometry.h"),
        "#pragma once\nlong area(long width, long height);\nstatic inline int hidden(int x) { return x; }\n",
    )
    .expect("header");
    let source = root.join("main.s");
    // x0 holds a pointer where area wants a long.
    let text = "; armtool: fn _f(x0: ptr<u8>) -> void\n.globl _f\n_f:\n    stp x29, x30, [sp, #-16]!\n    mov x29, sp\n    mov x1, #4\n    bl _area\n    ldp x29, x30, [sp], #16\n    ret\n";
    std::fs::write(&source, text).expect("source");
    let config = arm_lint::Config::discover(&root).expect("config");
    let checked = arm_lint::check(text, Some(&source), &config, HashMap::new(), true);
    let found: Vec<&str> = checked
        .diagnostics
        .iter()
        .filter(|d| d.code == "types/call")
        .map(|d| d.message.as_str())
        .collect();
    assert_eq!(found.len(), 1, "{:?}", checked.diagnostics);
    assert!(
        found[0].contains("_area takes x0 as i64, but x0 holds ptr<u8>"),
        "{found:?}"
    );
    let _ = std::fs::remove_dir_all(&root);
}
