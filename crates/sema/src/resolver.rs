//! Finding included files on disk, preferring editor buffers that haven't
//! been saved (docs/decisions/0019-includes.md).

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use crate::Resolver;

/// Looks for an included file in each directory in turn.
#[derive(Debug, Clone, Default)]
pub struct FsResolver {
    pub dirs: Vec<PathBuf>,
    /// Text of files open in an editor, by canonical path, used instead of
    /// what's on disk.
    pub open: HashMap<PathBuf, String>,
}

impl Resolver for FsResolver {
    /// Returns the path as the directories spell it, `..` removed, so an
    /// editor gets back paths in the form it uses. Open buffers are matched
    /// by canonical path.
    fn resolve(&self, name: &str) -> Option<(PathBuf, String)> {
        for dir in &self.dirs {
            let path = normalize(&dir.join(name));
            let key = canonical(&path);
            if let Some((_, text)) = self.open.iter().find(|(p, _)| canonical(p) == key) {
                return Some((path, text.clone()));
            }
            if let Ok(text) = std::fs::read_to_string(&path) {
                return Some((path, text));
            }
        }
        None
    }
}

/// A path with symlinks and `..` resolved where it exists, to tell whether
/// two paths are the same file, as in an include cycle. Not for showing:
/// macOS's `/var` becomes `/private/var`.
pub fn canonical(path: &Path) -> PathBuf {
    std::fs::canonicalize(path).unwrap_or_else(|_| normalize(path))
}

/// Removes `.` and `name/..` from a path without touching the filesystem.
pub fn normalize(path: &Path) -> PathBuf {
    use std::path::Component;
    let mut out = PathBuf::new();
    for component in path.components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir
                if matches!(out.components().next_back(), Some(Component::Normal(_))) =>
            {
                out.pop();
            }
            other => out.push(other),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn searches_directories_in_order_and_prefers_open_buffers() {
        let root = std::env::temp_dir().join(format!("arm-sema-resolver-{}", std::process::id()));
        std::fs::create_dir_all(root.join("a")).expect("dir");
        std::fs::create_dir_all(root.join("b")).expect("dir");
        std::fs::write(root.join("b/m.s"), "from b").expect("write");
        let mut resolver = FsResolver {
            dirs: vec![root.join("a"), root.join("b")],
            open: HashMap::new(),
        };
        let (path, text) = resolver.resolve("m.s").expect("found in b");
        assert_eq!(text, "from b");
        // Spelled as the directories spell it, not canonicalized.
        assert_eq!(path, root.join("b/m.s"));
        resolver.open.insert(canonical(&path), "unsaved".into());
        assert_eq!(
            resolver.resolve("m.s").map(|(_, t)| t).as_deref(),
            Some("unsaved")
        );
        assert!(resolver.resolve("missing.s").is_none());
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn normalizes_dots_lexically() {
        assert_eq!(
            normalize(Path::new("/a/b/../c/./d.s")),
            PathBuf::from("/a/c/d.s")
        );
        assert_eq!(normalize(Path::new("../x.s")), PathBuf::from("../x.s"));
    }
}
