//! An index of the project's assembly files, for navigation between files
//! that are assembled separately and linked together (docs/decisions/0029-workspace-index.md).
//!
//! Only global labels (`.globl`, `.private_extern`) can be linked to from
//! another object file on Mach-O, so those are the definitions indexed. Each
//! file's uses of names it doesn't define are indexed too, for references.
//! C and Objective-C files add their non-static function definitions, under
//! the symbol names the compiler gives them: `foo` is `_foo` on Darwin and
//! `foo` on Linux (docs/decisions/0034-c-functions.md, 0038-linux.md).

use std::collections::HashMap;
use std::ops::Range;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use arm_sema::{Analysis, Platform, SymbolKind};

/// Directories never worth searching.
const SKIP_DIRS: &[&str] = &["target", "build", "node_modules", "DerivedData", ".build"];
/// Enough for big projects, small enough to bound startup time.
const MAX_FILES: usize = 10_000;
const MAX_FILE_BYTES: u64 = 4 << 20;

#[derive(Debug, Clone)]
pub struct IndexedFile {
    pub text: Arc<str>,
    /// Global labels defined here: name and the name's range.
    pub definitions: Vec<(String, Range<usize>)>,
    /// Uses of names this file doesn't define, and of its global labels.
    pub references: Vec<(String, Range<usize>)>,
    /// Functions and the calls they make, for call hierarchy.
    pub calls: arm_lint::CallGraph,
}

#[derive(Debug, Default)]
pub struct Index {
    files: HashMap<PathBuf, IndexedFile>,
}

/// A place in an indexed file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Place {
    pub path: PathBuf,
    pub range: Range<usize>,
}

impl Index {
    /// Indexes every `.s` and `.S` file under the roots, skipping hidden and
    /// build directories.
    pub fn scan(roots: &[PathBuf]) -> Index {
        let mut index = Index::default();
        let mut paths = Vec::new();
        for root in roots {
            walk(root, &mut paths);
        }
        paths.sort();
        paths.dedup();
        paths.truncate(MAX_FILES);
        // Analysis is the cost; spread it over the cores.
        let threads = std::thread::available_parallelism().map_or(4, |n| n.get());
        let chunk = paths.len().div_ceil(threads).max(1);
        let parts: Vec<Vec<(PathBuf, IndexedFile)>> = std::thread::scope(|scope| {
            let workers: Vec<_> = paths
                .chunks(chunk)
                .map(|part| {
                    scope.spawn(move || {
                        let mut one = Index::default();
                        for path in part {
                            if let Ok(text) = std::fs::read_to_string(path) {
                                one.update(path, &text);
                            }
                        }
                        one.files.into_iter().collect::<Vec<_>>()
                    })
                })
                .collect();
            // A worker that panicked loses its files, not the server.
            workers.into_iter().filter_map(|w| w.join().ok()).collect()
        });
        index.files.extend(parts.into_iter().flatten());
        index
    }

    /// Reindexes one file from its current text, for the platform its
    /// `armtool.toml` sets, else what the file gives away, else Darwin.
    pub fn update(&mut self, path: &Path, text: &str) {
        let configured = path
            .parent()
            .and_then(|dir| arm_lint::Config::discover(dir).ok())
            .and_then(|c| c.platform);
        let file = if is_c(path) {
            // A C function is `_foo` on Darwin and `foo` on Linux; without a
            // configured platform, index both.
            let mut definitions = c_functions(text);
            match configured {
                Some(Platform::Darwin) => {}
                Some(Platform::Linux) => {
                    for (name, _) in &mut definitions {
                        name.remove(0);
                    }
                }
                None => {
                    let plain: Vec<_> = definitions
                        .iter()
                        .map(|(name, range)| (name[1..].to_string(), range.clone()))
                        .collect();
                    definitions.extend(plain);
                }
            }
            IndexedFile {
                text: Arc::from(text),
                definitions,
                references: Vec::new(),
                calls: arm_lint::CallGraph::default(),
            }
        } else {
            let platform = configured
                .or_else(|| Platform::detect(text))
                .unwrap_or_default();
            summarize(&Analysis::for_platform(text, platform))
        };
        self.files.insert(path.to_path_buf(), file);
    }

    /// Reindexes an assembly file from an analysis already made, as armls
    /// has after each edit.
    pub fn update_from(&mut self, path: &Path, analysis: &Analysis) {
        self.files.insert(path.to_path_buf(), summarize(analysis));
    }

    /// Reindexes a file from disk, or drops it if it's gone.
    pub fn reload(&mut self, path: &Path) {
        match std::fs::read_to_string(path) {
            Ok(text) => self.update(path, &text),
            Err(_) => {
                self.files.remove(path);
            }
        }
    }

    pub fn file(&self, path: &Path) -> Option<&IndexedFile> {
        self.files.get(path)
    }

    /// Every indexed file, in path order.
    pub fn files(&self) -> Vec<(&Path, &IndexedFile)> {
        let mut out: Vec<(&Path, &IndexedFile)> =
            self.files.iter().map(|(p, f)| (p.as_path(), f)).collect();
        out.sort_by_key(|(p, _)| *p);
        out
    }

    /// Where a global label is defined, in any indexed file but `except`.
    pub fn definitions(&self, name: &str, except: Option<&Path>) -> Vec<Place> {
        let mut out: Vec<Place> = self
            .files
            .iter()
            .filter(|(path, _)| Some(path.as_path()) != except)
            .flat_map(|(path, file)| {
                file.definitions
                    .iter()
                    .filter(|(n, _)| n == name)
                    .map(|(_, range)| Place {
                        path: path.clone(),
                        range: range.clone(),
                    })
            })
            .collect();
        out.sort_by(|a, b| (&a.path, a.range.start).cmp(&(&b.path, b.range.start)));
        out
    }

    /// Every use of a name, in any indexed file but `except`.
    pub fn references(&self, name: &str, except: Option<&Path>) -> Vec<Place> {
        let mut out: Vec<Place> = self
            .files
            .iter()
            .filter(|(path, _)| Some(path.as_path()) != except)
            .flat_map(|(path, file)| {
                file.references
                    .iter()
                    .filter(|(n, _)| n == name)
                    .map(|(_, range)| Place {
                        path: path.clone(),
                        range: range.clone(),
                    })
            })
            .collect();
        out.sort_by(|a, b| (&a.path, a.range.start).cmp(&(&b.path, b.range.start)));
        out
    }

    /// Global labels whose names contain `query`, ignoring case: for
    /// workspace symbol search.
    pub fn search(&self, query: &str) -> Vec<(String, Place)> {
        let query = query.to_ascii_lowercase();
        let mut out: Vec<(String, Place)> = self
            .files
            .iter()
            .flat_map(|(path, file)| {
                file.definitions.iter().map(move |(name, range)| {
                    (
                        name.clone(),
                        Place {
                            path: path.clone(),
                            range: range.clone(),
                        },
                    )
                })
            })
            .filter(|(name, _)| name.to_ascii_lowercase().contains(&query))
            .collect();
        out.sort_by(|a, b| a.0.cmp(&b.0).then(a.1.path.cmp(&b.1.path)));
        out
    }

    /// Every global label's name, sorted, once each: for completion.
    pub fn global_names(&self) -> Vec<String> {
        let mut names: Vec<String> = self
            .files
            .values()
            .flat_map(|f| f.definitions.iter().map(|(name, _)| name.clone()))
            .collect();
        names.sort();
        names.dedup();
        names
    }

    #[cfg(test)]
    pub fn len(&self) -> usize {
        self.files.len()
    }
}

fn summarize(analysis: &Analysis) -> IndexedFile {
    let symbols = analysis.symbols();
    let definitions = symbols
        .iter()
        .filter(|s| s.kind == SymbolKind::Label && s.exported && s.origin.is_none())
        .map(|s| (s.name.clone(), s.range.clone()))
        .collect();
    let references = analysis
        .references()
        .iter()
        .filter(|r| match r.symbol {
            None => true,
            Some(i) => symbols[i].kind == SymbolKind::Label && symbols[i].exported,
        })
        .map(|r| (r.name.clone(), r.range.clone()))
        .collect();
    IndexedFile {
        text: Arc::from(analysis.text()),
        definitions,
        references,
        calls: arm_lint::call_graph(analysis),
    }
}

/// C and Objective-C sources, whose functions assembly can call.
pub fn is_c(path: &Path) -> bool {
    path.extension().is_some_and(|e| e == "c" || e == "m")
}

/// The functions a C file defines that other files can link to, by symbol
/// name (`_foo` for `foo`), each with the range of its name. Skips `static`
/// functions, prototypes, and anything inside braces. A scan, not a parser:
/// it reads comments, strings, and preprocessor lines as nothing, and takes
/// the name before the last parenthesized group ahead of a top-level `{`.
pub fn c_functions(text: &str) -> Vec<(String, Range<usize>)> {
    let bytes = text.as_bytes();
    let mut out = Vec::new();
    let mut depth = 0usize;
    let mut parens = 0usize;
    // The statement so far at the top level.
    let mut candidate: Option<Range<usize>> = None;
    let mut is_static = false;
    let mut assigns = false;
    let mut last_ident: Option<Range<usize>> = None;
    let mut line_start = true;
    let mut i = 0;
    while i < bytes.len() {
        let c = bytes[i];
        let at_line_start = line_start;
        line_start = c == b'\n' || (line_start && (c == b' ' || c == b'\t'));
        match c {
            b'#' if at_line_start => {
                // A preprocessor line, with its continuations.
                while i < bytes.len() && bytes[i] != b'\n' {
                    if bytes[i] == b'\\' {
                        i += 1;
                    }
                    i += 1;
                }
                line_start = true;
            }
            b'/' if bytes.get(i + 1) == Some(&b'/') => {
                while i < bytes.len() && bytes[i] != b'\n' {
                    i += 1;
                }
                line_start = true;
            }
            b'/' if bytes.get(i + 1) == Some(&b'*') => {
                i += 2;
                while i + 1 < bytes.len() && !(bytes[i] == b'*' && bytes[i + 1] == b'/') {
                    i += 1;
                }
                i += 1;
            }
            b'"' | b'\'' => {
                i += 1;
                while i < bytes.len() && bytes[i] != c && bytes[i] != b'\n' {
                    if bytes[i] == b'\\' {
                        i += 1;
                    }
                    i += 1;
                }
                last_ident = None;
            }
            b'{' => {
                if depth == 0
                    && parens == 0
                    && let Some(name) = candidate.take().filter(|_| !is_static && !assigns)
                {
                    out.push((format!("_{}", &text[name.clone()]), name));
                }
                depth += 1;
                last_ident = None;
            }
            b'}' => {
                depth = depth.saturating_sub(1);
                if depth == 0 {
                    candidate = None;
                    is_static = false;
                    assigns = false;
                }
                last_ident = None;
            }
            _ if depth > 0 => {}
            b'(' => {
                if parens == 0 {
                    let attribute = last_ident.as_ref().is_some_and(|r| {
                        matches!(
                            &text[r.clone()],
                            "__attribute__" | "__asm" | "__asm__" | "asm"
                        )
                    });
                    if !attribute {
                        candidate = last_ident.take();
                    }
                }
                parens += 1;
                last_ident = None;
            }
            b')' => {
                parens = parens.saturating_sub(1);
                last_ident = None;
            }
            b';' => {
                if parens == 0 {
                    candidate = None;
                    is_static = false;
                    assigns = false;
                }
                last_ident = None;
            }
            b'=' if parens == 0 => {
                assigns = true;
                last_ident = None;
            }
            c if c.is_ascii_alphabetic() || c == b'_' => {
                let start = i;
                while i < bytes.len() && (bytes[i].is_ascii_alphanumeric() || bytes[i] == b'_') {
                    i += 1;
                }
                if parens == 0 && &text[start..i] == "static" {
                    is_static = true;
                }
                last_ident = Some(start..i);
                continue;
            }
            c if c.is_ascii_whitespace() => {}
            _ => last_ident = None,
        }
        i += 1;
    }
    out
}

fn walk(dir: &Path, out: &mut Vec<PathBuf>) {
    if out.len() >= MAX_FILES {
        return;
    }
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let name = entry.file_name();
        let name = name.to_string_lossy();
        let Ok(kind) = entry.file_type() else {
            continue;
        };
        if kind.is_dir() {
            if !name.starts_with('.') && !SKIP_DIRS.contains(&name.as_ref()) {
                walk(&path, out);
            }
        } else if kind.is_file()
            && (path.extension().is_some_and(|e| e == "s" || e == "S") || is_c(&path))
            && entry.metadata().is_ok_and(|m| m.len() <= MAX_FILE_BYTES)
        {
            out.push(path);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A project in its own directory: tests run in parallel.
    fn project(name: &str) -> PathBuf {
        let root = std::env::temp_dir().join(format!("armls-index-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(root.join("src/lib")).expect("dirs");
        std::fs::create_dir_all(root.join("build")).expect("dirs");
        std::fs::create_dir_all(root.join(".hidden")).expect("dirs");
        std::fs::write(
            root.join("src/main.s"),
            ".globl _main\n_main:\n    bl _helper\n    bl _printf\n    ret\n",
        )
        .expect("write");
        std::fs::write(
            root.join("src/lib/util.s"),
            ".globl _helper\n_helper:\n    b local\nlocal:\n    ret\n.private_extern _shared\n_shared:\n    ret\n",
        )
        .expect("write");
        std::fs::write(root.join("build/stale.s"), ".globl _helper\n_helper: ret\n")
            .expect("write");
        std::fs::write(root.join(".hidden/x.s"), ".globl _helper\n_helper: ret\n").expect("write");
        root
    }

    #[test]
    fn indexes_global_labels_across_files() {
        let root = project("globals");
        let index = Index::scan(std::slice::from_ref(&root));
        assert_eq!(index.len(), 2, "build and hidden directories are skipped");
        let helper = index.definitions("_helper", None);
        assert_eq!(helper.len(), 1);
        assert!(helper[0].path.ends_with("src/lib/util.s"));
        let text = &index.file(&helper[0].path).expect("file").text;
        assert_eq!(&text[helper[0].range.clone()], "_helper");
        // File-local labels aren't linkable, so they aren't indexed.
        assert!(index.definitions("local", None).is_empty());
        // .private_extern is linkable within the image.
        assert_eq!(index.definitions("_shared", None).len(), 1);
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn references_and_search() {
        let root = project("search");
        let index = Index::scan(std::slice::from_ref(&root));
        // The call in main.s, and the .globl that names it in util.s.
        let uses = index.references("_helper", None);
        assert_eq!(uses.len(), 2);
        assert!(uses.iter().any(|u| u.path.ends_with("src/main.s")));
        assert!(uses.iter().any(|u| u.path.ends_with("src/lib/util.s")));
        let found: Vec<String> = index.search("HELP").into_iter().map(|(n, _)| n).collect();
        assert_eq!(found, ["_helper"]);
        assert_eq!(index.global_names(), ["_helper", "_main", "_shared"]);
        let (path, main) = index.files()[1];
        assert!(path.ends_with("src/main.s"));
        let callees: Vec<&str> = main.calls.calls.iter().map(|c| c.callee.as_str()).collect();
        assert_eq!(callees, ["_helper", "_printf"]);
        let _ = std::fs::remove_dir_all(&root);
    }

    fn c_names(text: &str) -> Vec<(String, &str)> {
        c_functions(text)
            .into_iter()
            .map(|(symbol, range)| (symbol, &text[range]))
            .collect()
    }

    #[test]
    fn finds_c_function_definitions() {
        let text = r#"#include <stdio.h>
#define MAX(a, b) \
    ((a) > (b) ? (a) : (b))
/* int fake(void) { } */
// int fake2(void) { }
struct point { int x, y; };
int table[] = { 1, 2 };
int prototype(int x);
static int hidden(void) { return 0; }
static inline int hidden2(void) { return 0; }
const char *greet(const char *name) {
    if (name) { puts("{"); }
    return name;
}
void (*callback)(int) = 0;
__attribute__((noreturn)) void die(int code) __attribute__((cold)) {
    for (;;) {}
}
int
main(int argc, char **argv)
{
    return greet("x") != 0;
}
- (void)method:(int)x {
}
"#;
        let found: Vec<String> = c_names(text).into_iter().map(|(s, _)| s).collect();
        assert_eq!(found, ["_greet", "_die", "_main"]);
        assert!(c_names(text).iter().all(|(s, name)| s[1..] == **name));
    }

    #[test]
    fn indexes_c_files_beside_assembly() {
        let root = project("c");
        std::fs::write(
            root.join("src/lib/math.c"),
            "int add2(int a, int b) { return a + b; }\n",
        )
        .expect("write");
        std::fs::write(root.join("src/lib/view.m"), "void draw(void) {}\n").expect("write");
        std::fs::write(root.join("src/lib/math.h"), "int add2(int a, int b);\n").expect("write");
        let index = Index::scan(std::slice::from_ref(&root));
        let add = index.definitions("_add2", None);
        assert_eq!(add.len(), 1, "the header's prototype isn't a definition");
        assert!(add[0].path.ends_with("math.c"));
        assert_eq!(index.definitions("_draw", None).len(), 1);
        assert!(index.global_names().contains(&"_add2".to_string()));
        assert!(!is_c(Path::new("math.h")) && is_c(Path::new("view.m")));
        // No configured platform: the Linux name too.
        assert_eq!(index.definitions("add2", None).len(), 1);
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn files_are_indexed_for_their_platform() {
        let root = project("platform");
        std::fs::write(
            root.join("armtool.toml"),
            "[target]\nplatform = \"linux\"\n",
        )
        .expect("config");
        // On Linux `;` separates, so `.globl helper` is its own statement.
        std::fs::write(
            root.join("src/main.s"),
            "bl helper ; .globl helper\nhelper: ret\n",
        )
        .expect("write");
        std::fs::write(
            root.join("src/lib/math.c"),
            "int add2(int a, int b) { return a + b; }\n",
        )
        .expect("write");
        let index = Index::scan(std::slice::from_ref(&root));
        assert_eq!(index.definitions("helper", None).len(), 1);
        assert_eq!(index.definitions("add2", None).len(), 1);
        assert!(
            index.definitions("_add2", None).is_empty(),
            "Linux C names have no underscore"
        );
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn updates_follow_edits() {
        let root = project("updates");
        let mut index = Index::scan(std::slice::from_ref(&root));
        let util = root.join("src/lib/util.s");
        index.update(&util, ".globl _renamed\n_renamed:\n    ret\n");
        assert!(index.definitions("_helper", None).is_empty());
        assert_eq!(index.definitions("_renamed", None).len(), 1);
        std::fs::remove_file(&util).expect("remove");
        index.reload(&util);
        assert!(index.definitions("_renamed", None).is_empty());
        let _ = std::fs::remove_dir_all(&root);
    }
}
