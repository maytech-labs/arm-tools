//! An index of the project's assembly files, for navigation between files
//! that are assembled separately and linked together (docs/decisions/0029-workspace-index.md).
//!
//! Only global labels (`.globl`, `.private_extern`) can be linked to from
//! another object file on Mach-O, so those are the definitions indexed. Each
//! file's uses of names it doesn't define are indexed too, for references.

use std::collections::HashMap;
use std::ops::Range;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use arm_sema::{Analysis, SymbolKind};

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
        for path in paths.into_iter().take(MAX_FILES) {
            if let Ok(text) = std::fs::read_to_string(&path) {
                index.update(&path, &text);
            }
        }
        index
    }

    /// Reindexes one file from its current text.
    pub fn update(&mut self, path: &Path, text: &str) {
        let analysis = Analysis::new(text);
        self.files.insert(path.to_path_buf(), summarize(&analysis));
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
    }
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
            && path.extension().is_some_and(|e| e == "s" || e == "S")
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
