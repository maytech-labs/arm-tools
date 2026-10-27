//! CPU feature data for `gen-isa`, from `llvm-tblgen --dump-json` and clang's
//! default target (docs/decisions/0022-cpu-features.md).

use std::collections::{BTreeMap, HashMap};
use std::fmt::Write as _;
use std::path::Path;
use std::process::Command;

use serde_json::Value;

pub struct Features {
    /// Every SubtargetFeature, sorted by record name.
    defs: Vec<Def>,
    /// For each assembler predicate (`HasSVE`): clauses that must all hold,
    /// each satisfied by any one of its features. None: can't be expressed,
    /// so treated as always satisfied.
    conditions: BTreeMap<String, Option<Vec<Vec<usize>>>>,
    processors: BTreeMap<String, Vec<usize>>,
    default: Vec<usize>,
}

struct Def {
    name: String,
    user: String,
    implies: Vec<usize>,
    default_exts: Vec<usize>,
}

pub fn extract(json: &Value, predicates: &[&str]) -> Result<Features, String> {
    let instances = |class: &str| -> Vec<String> {
        json["!instanceof"][class]
            .as_array()
            .map(|a| {
                a.iter()
                    .filter_map(|v| v.as_str().map(str::to_string))
                    .collect()
            })
            .unwrap_or_default()
    };
    let mut records = instances("SubtargetFeature");
    records.sort();
    let index: HashMap<&str, usize> = records
        .iter()
        .enumerate()
        .map(|(i, r)| (r.as_str(), i))
        .collect();
    let defs_of = |list: &Value| -> Vec<usize> {
        list.as_array()
            .into_iter()
            .flatten()
            .filter_map(|v| v["def"].as_str())
            .filter_map(|d| index.get(d).copied())
            .collect()
    };
    let defs: Vec<Def> = records
        .iter()
        .map(|r| {
            let record = &json[r.as_str()];
            Def {
                name: record["Name"].as_str().unwrap_or_default().to_string(),
                user: record["UserVisibleName"]
                    .as_str()
                    .unwrap_or_default()
                    .to_string(),
                implies: defs_of(&record["Implies"]),
                default_exts: defs_of(&record["DefaultExts"]),
            }
        })
        .collect();
    if defs.len() < 100 {
        return Err(format!(
            "only {} subtarget features in the JSON",
            defs.len()
        ));
    }

    let mut conditions = BTreeMap::new();
    for predicate in predicates {
        let dag = &json[*predicate]["AssemblerCondDag"];
        conditions.insert(predicate.to_string(), cnf(dag, &index));
    }

    let mut processors = BTreeMap::new();
    for model in instances("ProcessorModel") {
        let record = &json[model.as_str()];
        if let Some(name) = record["Name"].as_str() {
            processors.insert(name.to_string(), defs_of(&record["Features"]));
        }
    }
    for alias in instances("ProcessorAlias") {
        let record = &json[alias.as_str()];
        if let (Some(name), Some(target)) = (record["Name"].as_str(), record["Alias"].as_str())
            && let Some(features) = processors.get(target).cloned()
        {
            processors.insert(name.to_string(), features);
        }
    }

    let default = clang_default(&defs, &processors)?;
    Ok(Features {
        defs,
        conditions,
        processors,
        default,
    })
}

/// A condition dag as clauses: `(all_of A, (any_of B, C))` is [[A], [B, C]].
/// `FeatureAll` (the `-mattr=+all` pseudo-feature) is never enabled.
fn cnf(dag: &Value, index: &HashMap<&str, usize>) -> Option<Vec<Vec<usize>>> {
    let atom = |v: &Value| -> Option<Option<usize>> {
        let def = v["def"].as_str()?;
        if v["kind"].as_str() != Some("def") {
            return None;
        }
        Some(if def == "FeatureAll" {
            None
        } else {
            Some(*index.get(def)?)
        })
    };
    // A single feature, possibly wrapped in a one-argument all_of.
    let single = |v: &Value| -> Option<Option<usize>> {
        if let Some(a) = atom(v) {
            return Some(a);
        }
        let args = v["args"].as_array()?;
        (v["operator"]["def"].as_str() == Some("all_of") && args.len() == 1)
            .then(|| atom(&args[0][0]))
            .flatten()
    };
    let operator = dag["operator"]["def"].as_str()?;
    let args: Vec<&Value> = dag["args"].as_array()?.iter().map(|a| &a[0]).collect();
    match operator {
        "any_of" => {
            // Alternatives, flattening nested any_of: (any_of A, (any_of B, C)).
            let mut clause = Vec::new();
            for arg in args {
                match single(arg) {
                    Some(Some(feature)) => clause.push(feature),
                    Some(None) => {}
                    None if arg["operator"]["def"].as_str() == Some("any_of") => {
                        let [inner] = cnf(arg, index)?.try_into().ok()?;
                        clause.extend::<Vec<usize>>(inner);
                    }
                    None => return None,
                }
            }
            (!clause.is_empty()).then(|| vec![clause])
        }
        "all_of" => {
            let mut clauses = Vec::new();
            for arg in args {
                match single(arg) {
                    Some(Some(feature)) => clauses.push(vec![feature]),
                    Some(None) => return None,
                    None => clauses.extend(cnf(arg, index)?),
                }
            }
            Some(clauses)
        }
        _ => None,
    }
}

/// What `clang -arch arm64` enables: its target CPU's features plus the
/// `-target-feature` flags the driver adds, from `clang -###`.
fn clang_default(
    defs: &[Def],
    processors: &BTreeMap<String, Vec<usize>>,
) -> Result<Vec<usize>, String> {
    let dir = std::env::temp_dir().join(format!("arm-tools-target-{}", std::process::id()));
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let source = dir.join("probe.s");
    std::fs::write(&source, "nop\n").map_err(|e| e.to_string())?;
    let out = Command::new("xcrun")
        .args(["clang", "-arch", "arm64", "-###", "-c"])
        .arg(&source)
        .arg("-o")
        .arg(dir.join("probe.o"))
        .output()
        .map_err(|e| format!("can't run xcrun clang: {e}"))?;
    let _ = std::fs::remove_dir_all(&dir);
    let text = String::from_utf8_lossy(&out.stderr);
    let words: Vec<&str> = text
        .split_whitespace()
        .map(|w| w.trim_matches('"'))
        .collect();
    let mut enabled = Vec::new();
    let mut cpu = None;
    for pair in words.windows(2) {
        match pair[0] {
            "-target-cpu" => cpu = Some(pair[1].to_string()),
            "-target-feature" => {
                if let Some(name) = pair[1].strip_prefix('+') {
                    let found = defs.iter().position(|d| d.name == name);
                    enabled.push(
                        found.ok_or_else(|| format!("clang enables unknown feature '{name}'"))?,
                    );
                }
            }
            _ => {}
        }
    }
    let cpu = cpu.ok_or("clang -### printed no -target-cpu")?;
    enabled.extend(
        processors
            .get(&cpu)
            .ok_or_else(|| format!("no processor '{cpu}' in the tables"))?,
    );
    enabled.sort_unstable();
    enabled.dedup();
    if enabled.len() < 10 {
        return Err(format!(
            "only {} default features; is clang working?",
            enabled.len()
        ));
    }
    Ok(enabled)
}

impl Features {
    pub fn render(&self, w: &mut String, predicates: &[&str]) {
        let list = |v: &[usize]| {
            v.iter()
                .map(usize::to_string)
                .collect::<Vec<_>>()
                .join(", ")
        };
        let _ = writeln!(w, "\npub(crate) static FEATURE_DEFS: &[FeatureDef] = &[");
        for d in &self.defs {
            let _ = writeln!(
                w,
                "    FeatureDef {{ name: {:?}, user: {:?}, implies: &[{}], default_exts: &[{}] }},",
                d.name,
                d.user,
                list(&d.implies),
                list(&d.default_exts)
            );
        }
        let _ = writeln!(w, "];\n");
        let _ = writeln!(
            w,
            "/// For each entry in FEATURES: clauses of FEATURE_DEFS that must all hold,\n/// any one feature per clause. None: always satisfied.\npub(crate) static CONDITIONS: &[Option<&[&[u16]]>] = &["
        );
        for predicate in predicates {
            match self.conditions.get(*predicate).cloned().flatten() {
                Some(clauses) => {
                    let clauses: Vec<String> =
                        clauses.iter().map(|c| format!("&[{}]", list(c))).collect();
                    let _ = writeln!(w, "    Some(&[{}]),", clauses.join(", "));
                }
                None => {
                    let _ = writeln!(w, "    None,");
                }
            }
        }
        let _ = writeln!(w, "];\n");
        let _ = writeln!(
            w,
            "/// Sorted by name.\npub(crate) static PROCESSORS: &[(&str, &[u16])] = &["
        );
        for (name, features) in &self.processors {
            let _ = writeln!(w, "    ({name:?}, &[{}]),", list(features));
        }
        let _ = writeln!(w, "];\n");
        let _ = writeln!(
            w,
            "/// What `clang -arch arm64` enables before closure.\npub(crate) static DEFAULT_FEATURES: &[u16] = &[{}];",
            list(&self.default)
        );
    }

    pub fn summary(&self) -> String {
        format!(
            "{} CPU features, {} processors",
            self.defs.len(),
            self.processors.len()
        )
    }
}

pub fn dump_json(tblgen: &Path, llvm: &Path, target: &Path) -> Result<Value, String> {
    let out = Command::new(tblgen)
        .arg("--dump-json")
        .arg("-I")
        .arg(llvm.join("llvm/include"))
        .arg("-I")
        .arg(target)
        .arg(target.join("AArch64.td"))
        .output()
        .map_err(|e| format!("can't run {}: {e}", tblgen.display()))?;
    if !out.status.success() {
        return Err(format!(
            "llvm-tblgen --dump-json failed:\n{}",
            String::from_utf8_lossy(&out.stderr)
        ));
    }
    serde_json::from_slice(&out.stdout).map_err(|e| format!("bad JSON from llvm-tblgen: {e}"))
}
