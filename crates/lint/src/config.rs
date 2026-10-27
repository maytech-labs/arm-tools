//! `armtool.toml`: per-project rule levels.
//!
//! ```toml
//! [rules]
//! "abi/x18-reserved" = "error"
//! "style/unused-label" = "off"
//!
//! [include]
//! paths = ["include", "../shared"]   # relative to this file
//!
//! [format]
//! indent = 4
//! mnemonic_width = 8
//! comment_column = 40
//! max_blank_lines = 1
//! ```

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use serde::Deserialize;

use crate::RuleInfo;

/// How a rule reports.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Level {
    Error,
    Warning,
    Info,
    Hint,
    Off,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Config {
    levels: HashMap<String, Level>,
    /// Directories to search for `.include` files, as written.
    include: Vec<PathBuf>,
    pub format: arm_syntax::FormatOptions,
    /// Where it was read from, if anywhere.
    pub path: Option<PathBuf>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct File {
    #[serde(default)]
    rules: HashMap<String, Level>,
    #[serde(default)]
    include: IncludeSection,
    #[serde(default)]
    format: FormatSection,
}

#[derive(Deserialize, Default)]
#[serde(deny_unknown_fields)]
struct FormatSection {
    indent: Option<usize>,
    mnemonic_width: Option<usize>,
    comment_column: Option<usize>,
    max_blank_lines: Option<usize>,
}

#[derive(Deserialize, Default)]
#[serde(deny_unknown_fields)]
struct IncludeSection {
    #[serde(default)]
    paths: Vec<PathBuf>,
}

pub const FILE_NAME: &str = "armtool.toml";

impl Config {
    /// Parses a config file's text. Unknown rule IDs and levels are errors
    /// that say what's valid.
    pub fn parse(text: &str) -> Result<Config, String> {
        let file: File = toml::from_str(text).map_err(|e| {
            format!("{e}\nLevels are \"error\", \"warning\", \"info\", \"hint\", and \"off\".")
        })?;
        for id in file.rules.keys() {
            if crate::rule(id).is_none() {
                let mut message = format!("unknown rule '{id}'.");
                if let Some(close) = arm_sema::suggest(id, crate::rules().iter().map(|r| r.id)) {
                    message.push_str(&format!(" Did you mean '{close}'?"));
                }
                message.push_str(" Run `armtool rules` for the list.");
                return Err(message);
            }
        }
        let defaults = arm_syntax::FormatOptions::default();
        let f = file.format;
        Ok(Config {
            levels: file.rules,
            include: file.include.paths,
            format: arm_syntax::FormatOptions {
                indent: f.indent.unwrap_or(defaults.indent),
                mnemonic_width: f.mnemonic_width.unwrap_or(defaults.mnemonic_width),
                comment_column: f.comment_column.unwrap_or(defaults.comment_column),
                max_blank_lines: f.max_blank_lines.unwrap_or(defaults.max_blank_lines),
            },
            path: None,
        })
    }

    /// Reads the nearest `armtool.toml` at or above `start`, or the defaults
    /// if there's none.
    pub fn discover(start: &Path) -> Result<Config, String> {
        for dir in start.ancestors() {
            let candidate = dir.join(FILE_NAME);
            if candidate.is_file() {
                let text = std::fs::read_to_string(&candidate)
                    .map_err(|e| format!("can't read {}: {e}", candidate.display()))?;
                let mut config =
                    Config::parse(&text).map_err(|e| format!("{}: {e}", candidate.display()))?;
                config.path = Some(candidate);
                return Ok(config);
            }
        }
        Ok(Config::default())
    }

    /// Where to look for a file's `.include`s, in order: the configured
    /// paths, the project root (the config file's directory, or the nearest
    /// one with `.git`), then the file's own directory.
    pub fn include_dirs(&self, file: &Path) -> Vec<PathBuf> {
        let file_dir = file.parent().unwrap_or(Path::new(".")).to_path_buf();
        let config_dir = self
            .path
            .as_ref()
            .and_then(|p| p.parent())
            .map(Path::to_path_buf);
        let root = config_dir.clone().or_else(|| {
            file_dir
                .ancestors()
                .find(|d| d.join(".git").exists())
                .map(Path::to_path_buf)
        });
        let base = config_dir.unwrap_or_else(|| file_dir.clone());
        let mut dirs: Vec<PathBuf> = self.include.iter().map(|p| base.join(p)).collect();
        dirs.extend(root);
        dirs.push(file_dir);
        let mut seen = Vec::new();
        dirs.retain(|d| {
            let fresh = !seen.contains(d);
            seen.push(d.clone());
            fresh
        });
        dirs
    }

    /// The level for a rule: the file's setting, or the rule's default.
    pub fn level(&self, rule: &RuleInfo) -> Level {
        self.levels
            .get(rule.id)
            .copied()
            .unwrap_or(rule.default_level)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_levels() {
        let config = Config::parse("[rules]\n\"abi/x18-reserved\" = \"off\"\n").expect("valid");
        let rule = crate::rule("abi/x18-reserved").expect("rule");
        assert_eq!(config.level(rule), Level::Off);
    }

    #[test]
    fn empty_file_is_defaults() {
        let config = Config::parse("").expect("valid");
        let rule = crate::rule("abi/x18-reserved").expect("rule");
        assert_eq!(config.level(rule), rule.default_level);
    }

    #[test]
    fn errors_say_what_is_valid() {
        let unknown = Config::parse("[rules]\n\"abi/x18-reserve\" = \"off\"\n").unwrap_err();
        assert!(
            unknown.contains("Did you mean 'abi/x18-reserved'?"),
            "{unknown}"
        );
        let level = Config::parse("[rules]\n\"abi/x18-reserved\" = \"loud\"\n").unwrap_err();
        assert!(level.contains("Levels are"), "{level}");
        let section = Config::parse("[rulez]\n").unwrap_err();
        assert!(!section.is_empty());
    }

    #[test]
    fn format_options() {
        let config = Config::parse("[format]\nindent = 8\n").expect("valid");
        assert_eq!(config.format.indent, 8);
        assert_eq!(config.format.mnemonic_width, 8);
        assert!(Config::parse("[format]\nindnet = 8\n").is_err());
    }

    #[test]
    fn include_dirs_in_order() {
        let mut config =
            Config::parse("[include]\npaths = [\"inc\", \"../shared\"]\n").expect("valid");
        config.path = Some(PathBuf::from("/proj/armtool.toml"));
        assert_eq!(
            config.include_dirs(Path::new("/proj/src/a.s")),
            [
                PathBuf::from("/proj/inc"),
                PathBuf::from("/proj/../shared"),
                PathBuf::from("/proj"),
                PathBuf::from("/proj/src"),
            ]
        );
        // Without a config, the file's directory.
        let dirs = Config::default().include_dirs(Path::new("/no/git/here/a.s"));
        assert_eq!(dirs.last(), Some(&PathBuf::from("/no/git/here")));
    }

    #[test]
    fn discovers_the_nearest_file() {
        let root = std::env::temp_dir().join(format!("arm-lint-config-{}", std::process::id()));
        let nested = root.join("a/b");
        std::fs::create_dir_all(&nested).expect("dirs");
        std::fs::write(
            root.join(FILE_NAME),
            "[rules]\n\"abi/x18-reserved\" = \"off\"\n",
        )
        .expect("write");
        let config = Config::discover(&nested).expect("found");
        assert_eq!(config.path, Some(root.join(FILE_NAME)));
        let _ = std::fs::remove_dir_all(&root);
        let none = Config::discover(Path::new("/")).expect("defaults");
        assert_eq!(none.path, None);
    }
}
