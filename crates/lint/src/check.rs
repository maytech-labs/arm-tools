//! Checking one file the way both armls and armtool do: includes, `.S`
//! preprocessing, semantic errors, and lint rules (docs/decisions/0027-preprocessed-files.md).

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use arm_sema::{Analysis, Diagnostic, FsResolver, Severity};

use crate::Config;

/// A checked file: the analysis editor features use, and the diagnostics to
/// show. For a `.S` file the diagnostics come from the preprocessed text,
/// mapped back; the analysis is of the text as written.
pub struct Checked {
    pub analysis: Analysis,
    pub diagnostics: Vec<Diagnostic>,
}

/// Checks a file. `path` enables includes and `.S` preprocessing; `open` has
/// unsaved editor buffers by path; `lint` adds the lint rules.
pub fn check(
    text: &str,
    path: Option<&Path>,
    config: &Config,
    open: HashMap<PathBuf, String>,
    lint: bool,
) -> Checked {
    let platform = config.platform_for(text);
    let features = Some(config.features_for(platform));
    let Some(path) = path else {
        let analysis = Analysis::with_platform(text, None, None, features, platform);
        let diagnostics = findings(&analysis, config, lint, path);
        return Checked {
            analysis,
            diagnostics,
        };
    };
    let dirs = config.include_dirs(path);
    let resolver = FsResolver {
        dirs: dirs.clone(),
        open,
    };
    let analysis = Analysis::with_platform(
        text,
        Some(path.to_path_buf()),
        Some(&resolver),
        features.clone(),
        platform,
    );
    if !arm_sema::needs_preprocessing(path) {
        let diagnostics = findings(&analysis, config, lint, Some(path));
        return Checked {
            analysis,
            diagnostics,
        };
    }
    let diagnostics = match arm_sema::preprocess_for(text, path, &dirs, platform) {
        Ok(preprocessed) => {
            let output = Analysis::with_platform(
                &preprocessed.text,
                Some(path.to_path_buf()),
                Some(&resolver),
                features,
                platform,
            );
            preprocessed.map_diagnostics(text, findings(&output, config, lint, Some(path)))
        }
        Err(error) => {
            // Without the preprocessor, `#define`d names and `#if` blocks
            // can't be judged: report what's certain, and say why.
            let mut found: Vec<Diagnostic> = findings(&analysis, config, lint, Some(path))
                .into_iter()
                .filter(|d| d.code == "syntax")
                .collect();
            found.push(Diagnostic {
                range: 0..0,
                severity: Severity::Warning,
                code: "preprocess",
                message: format!("{error}. Only syntax errors are shown for this .S file."),
                fix: None,
            });
            found
        }
    };
    Checked {
        analysis,
        diagnostics,
    }
}

fn findings(
    analysis: &Analysis,
    config: &Config,
    lint: bool,
    path: Option<&Path>,
) -> Vec<Diagnostic> {
    let mut out = analysis.diagnostics().to_vec();
    if lint {
        // The C library, then the project's own C functions over it.
        let platform = analysis.platform();
        let mut externals = crate::cdecl::libc(platform);
        if let Some(root) = path.and_then(|p| config.project_root(p)) {
            externals.extend(crate::cdecl::project(&root, platform));
        }
        out.extend(crate::lint_with(analysis, config, &externals));
    }
    out.sort_by_key(|d| (d.range.start, d.range.end));
    out
}
