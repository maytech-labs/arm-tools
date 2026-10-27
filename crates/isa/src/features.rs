//! Which CPU features are on, as clang tracks them for `-arch arm64`,
//! `.arch`, `.cpu`, and `.arch_extension` (docs/decisions/0022-cpu-features.md).

use crate::Form;
use crate::generated::{CONDITIONS, DEFAULT_FEATURES, FEATURE_DEFS, PROCESSORS};

/// A set of enabled features, closed under implication: enabling `sve`
/// enables `fullfp16`, which it implies.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FeatureSet {
    on: Vec<bool>,
}

impl FeatureSet {
    fn empty() -> FeatureSet {
        FeatureSet {
            on: vec![false; FEATURE_DEFS.len()],
        }
    }

    /// What `clang -arch arm64` enables on macOS: the apple-m1 CPU and the
    /// features the driver adds.
    pub fn apple_default() -> FeatureSet {
        let mut set = FeatureSet::empty();
        for &i in DEFAULT_FEATURES {
            set.enable(usize::from(i));
        }
        set
    }

    /// What `clang --target=aarch64-linux-gnu` enables: the generic CPU,
    /// Armv8-A with FP and NEON.
    pub fn linux_default() -> FeatureSet {
        FeatureSet::for_cpu("generic").unwrap_or_else(|_| FeatureSet::empty())
    }

    /// What clang enables by default for a platform.
    pub fn default_for(platform: crate::Platform) -> FeatureSet {
        match platform {
            crate::Platform::Darwin => FeatureSet::apple_default(),
            crate::Platform::Linux => FeatureSet::linux_default(),
        }
    }

    /// The features `.arch` or `-march` selects: `armv8.5-a`,
    /// `armv9-a+sve2+nolse`. Like clang, this replaces the current set.
    pub fn for_arch(spec: &str) -> Result<FeatureSet, String> {
        let mut parts = spec.split('+');
        let arch = parts.next().unwrap_or_default().to_ascii_lowercase();
        let short = arch
            .strip_prefix("arm")
            .unwrap_or(&arch)
            .replace("-a", "a")
            .replace("-r", "r");
        let index = FEATURE_DEFS
            .iter()
            .position(|d| d.name == short && d.name.starts_with('v'))
            .ok_or_else(|| format!("unknown architecture '{arch}'"))?;
        let mut set = FeatureSet::empty();
        set.enable(index);
        for &ext in FEATURE_DEFS[index].default_exts {
            set.enable(usize::from(ext));
        }
        for modifier in parts {
            set.modify(modifier)?;
        }
        Ok(set)
    }

    /// Adds an architecture and its default extensions to the set, as
    /// `-march` does on Darwin: clang keeps the target CPU's features and
    /// adds the architecture's. (The `.arch` directive replaces instead; see
    /// [`FeatureSet::for_arch`].)
    pub fn add_arch(&mut self, spec: &str) -> Result<(), String> {
        let arch = FeatureSet::for_arch(spec)?;
        for (on, add) in self.on.iter_mut().zip(arch.on) {
            *on |= add;
        }
        Ok(())
    }

    /// The features `.cpu` or `-mcpu` selects: `apple-m4`, `apple-m1+sve`.
    pub fn for_cpu(spec: &str) -> Result<FeatureSet, String> {
        let mut parts = spec.split('+');
        let cpu = parts.next().unwrap_or_default().to_ascii_lowercase();
        let (_, features) = PROCESSORS
            .iter()
            .find(|(name, _)| *name == cpu)
            .ok_or_else(|| format!("unknown CPU '{cpu}'"))?;
        let mut set = FeatureSet::empty();
        for &i in *features {
            set.enable(usize::from(i));
        }
        for modifier in parts {
            set.modify(modifier)?;
        }
        Ok(set)
    }

    /// Turns an extension on, or off with a `no` prefix, as
    /// `.arch_extension sve` and `.arch_extension nolse` do.
    pub fn modify(&mut self, extension: &str) -> Result<(), String> {
        let lower = extension.trim().to_ascii_lowercase();
        let (name, on) = match lower.strip_prefix("no") {
            Some(rest) if find_extension(rest).is_some() => (rest, false),
            _ => (lower.as_str(), true),
        };
        let index = find_extension(name).ok_or_else(|| format!("unknown extension '{name}'"))?;
        if on {
            self.enable(index);
        } else {
            self.disable(index);
        }
        Ok(())
    }

    fn enable(&mut self, index: usize) {
        if std::mem::replace(&mut self.on[index], true) {
            return;
        }
        for &implied in FEATURE_DEFS[index].implies {
            self.enable(usize::from(implied));
        }
    }

    /// Turns a feature off, and everything that implies it, as LLVM does.
    fn disable(&mut self, index: usize) {
        if !std::mem::replace(&mut self.on[index], false) {
            return;
        }
        for (other, def) in FEATURE_DEFS.iter().enumerate() {
            if def.implies.iter().any(|&i| usize::from(i) == index) {
                self.disable(other);
            }
        }
    }

    pub fn has(&self, name: &str) -> bool {
        FEATURE_DEFS
            .iter()
            .position(|d| d.name == name)
            .is_some_and(|i| self.on[i])
    }

    /// What a form needs that isn't on: each inner list is one requirement,
    /// met by any of its features. Empty when the form is allowed.
    pub fn missing(&self, form: Form) -> Vec<Vec<&'static str>> {
        let mut out = Vec::new();
        for feature in form.features() {
            let Some(Some(clauses)) = CONDITIONS.get(feature.0) else {
                continue;
            };
            for clause in clauses.iter() {
                let met = clause.iter().any(|&i| self.on[usize::from(i)]);
                if !met {
                    out.push(
                        clause
                            .iter()
                            .map(|&i| display_name(usize::from(i)))
                            .collect(),
                    );
                }
            }
        }
        out
    }
}

fn find_extension(name: &str) -> Option<usize> {
    FEATURE_DEFS
        .iter()
        .position(|d| d.user == name)
        .or_else(|| FEATURE_DEFS.iter().position(|d| d.name == name))
}

/// The name people write: `.arch_extension`'s spelling where there is one.
fn display_name(index: usize) -> &'static str {
    let def = &FEATURE_DEFS[index];
    if def.user.is_empty() {
        def.name
    } else {
        def.user
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::forms;

    fn form(mnemonic: &str, opcode: &str) -> Form {
        forms(mnemonic)
            .find(|f| f.opcode() == opcode)
            .unwrap_or_else(|| panic!("no {opcode}"))
    }

    #[test]
    fn apple_default_matches_clang() {
        let set = FeatureSet::apple_default();
        for on in [
            "neon", "lse", "v8.5a", "v8.1a", "fullfp16", "sha3", "dotprod", "pauth",
        ] {
            assert!(set.has(on), "{on} should be on");
        }
        for off in ["sve", "bf16", "mops", "cssc", "mte"] {
            assert!(!set.has(off), "{off} should be off");
        }
    }

    #[test]
    fn missing_features_by_their_written_names() {
        let set = FeatureSet::apple_default();
        assert_eq!(set.missing(form("abs", "ABSXr")), [vec!["cssc"]]);
        assert!(set.missing(form("add", "ADDXri")).is_empty());
        let sve = forms("ld1d")
            .find(|f| f.opcode() == "LD1D_IMM")
            .expect("LD1D_IMM");
        assert_eq!(set.missing(sve), [vec!["sve", "sme"]]);
    }

    #[test]
    fn extensions_on_and_off() {
        let mut set = FeatureSet::apple_default();
        set.modify("sve").expect("known");
        assert!(set.has("sve") && set.has("fullfp16"));
        set.modify("nolse").expect("known");
        assert!(!set.has("lse"));
        assert!(set.modify("frobnicate").is_err());
        // User-visible spellings work: fp16 is fullfp16.
        let mut set = FeatureSet::for_arch("armv8-a").expect("arch");
        set.modify("fp16").expect("known");
        assert!(set.has("fullfp16"));
    }

    #[test]
    fn march_adds_to_the_cpu() {
        // clang -arch arm64 -march=armv9.4-a+sve2+sme2 keeps apple-m1's crypto.
        let mut set = FeatureSet::apple_default();
        set.add_arch("armv9.4-a+sve2+sme2").expect("arch");
        for on in [
            "aes", "sha2", "sha3", "fp16fml", "sve2", "sme2", "mops", "cssc",
        ] {
            assert!(set.has(on), "{on}");
        }
    }

    #[test]
    fn arch_and_cpu_replace_the_set() {
        let v8 = FeatureSet::for_arch("armv8-a").expect("arch");
        assert!(!v8.has("lse"));
        assert!(v8.has("neon"));
        let v9 = FeatureSet::for_arch("armv9-a").expect("arch");
        assert!(v9.has("sve2") && v9.has("lse"));
        let custom = FeatureSet::for_arch("armv8.5-a+sve+nolse").expect("arch");
        assert!(custom.has("sve") && !custom.has("lse"));
        let m4 = FeatureSet::for_cpu("apple-m4").expect("cpu");
        assert!(m4.has("sme") && m4.has("bf16"));
        let m1 = FeatureSet::for_cpu("apple-m1").expect("alias");
        assert!(m1.has("lse") && !m1.has("sme"));
        assert!(FeatureSet::for_arch("armv99-a").is_err());
        assert!(FeatureSet::for_cpu("pentium").is_err());
    }
}

#[cfg(test)]
mod platform_tests {
    use super::*;

    /// Checked against `clang --target=aarch64-linux-gnu`: these need
    /// features the generic CPU doesn't have, unlike apple-m1.
    #[test]
    fn linux_starts_from_the_generic_cpu() {
        let linux = FeatureSet::linux_default();
        assert!(linux.has("neon") && linux.has("fp-armv8"));
        for missing in ["lse", "crc", "aes", "dotprod", "rcpc", "fullfp16", "sha2"] {
            assert!(!linux.has(missing), "{missing}");
            assert!(
                FeatureSet::apple_default().has(missing),
                "{missing} on apple-m1"
            );
        }
        assert_eq!(
            FeatureSet::default_for(crate::Platform::Darwin),
            FeatureSet::apple_default()
        );
    }
}
