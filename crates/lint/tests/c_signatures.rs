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

#[test]
fn structs_in_project_headers() {
    let root = std::env::temp_dir().join(format!("arm-lint-cs-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(root.join(".git")).expect("project");
    std::fs::write(
        root.join("list.h"),
        "typedef struct node {\n    struct node *next;\n    int value;\n} node_t;\nnode_t *head(void);\n",
    )
    .expect("header");
    let source = root.join("walk.s");
    // head() returns a node_t *: a ptr<node> laid out from the header.
    let text = "\
; armtool: fn _sum(x0: ptr<node>) -> i64
node_value = 8
.globl _sum
_sum:
    stp     x29, x30, [sp, #-16]!
    mov     x29, sp
    bl      _head
    ldr     w1, [x0, #8]
    ldr     x2, [x0, #8]
    ldr     x0, [x0]
    ldp     x29, x30, [sp], #16
    ret
";
    std::fs::write(&source, text).expect("source");
    let config = arm_lint::Config::discover(&root).expect("config");
    let checked = arm_lint::check(text, Some(&source), &config, HashMap::new(), true);
    let found: Vec<(&str, &str)> = checked
        .diagnostics
        .iter()
        .filter(|d| d.code.starts_with("types/"))
        .map(|d| (d.code, d.message.as_str()))
        .collect();
    // The annotation's struct name is known from C; value is an int, so
    // the 8-byte load is a finding; and the return is a ptr<node>, not i64.
    assert_eq!(found.len(), 2, "{found:?}");
    assert!(
        found[0]
            .1
            .contains("node.value is i32 (4 bytes), but this loads 8"),
        "{found:?}"
    );
    assert!(
        found[1]
            .1
            .contains("_sum returns i64, but x0 holds ptr<node>"),
        "{found:?}"
    );
    let _ = std::fs::remove_dir_all(&root);
}
