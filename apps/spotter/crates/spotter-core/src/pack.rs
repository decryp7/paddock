//! Declarative packs: presets for well-known software and custom version schemes, written in TOML. New presets and
//! schemes are new pack files, not code (AGENTS.md). The default pack is embedded; users may add their own.
//!
//! ```toml
//! [[scheme]]
//! id = "ubuntu"
//! name = "Ubuntu releases"
//! pattern = '(?<major>\d{2})\.(?<minor>\d{2})'
//!
//! [[preset]]
//! id = "ripgrep"
//! name = "ripgrep"
//! description = "Fast recursive search."
//! category = "Command-line tools"
//! source = { kind = "github", values = { repo = "BurntSushi/ripgrep" } }
//! ```
use crate::filter::VersionFilter;
use crate::notify::NotificationPolicy;
use crate::source::SourceConfig;
use crate::version::VersionScheme;
use crate::watch::{Watch, WatchSpec};
use serde::Deserialize;

/// Largest pack file read (512 KiB).
pub const MAX_PACK_BYTES: usize = 512 << 10;
/// Most presets and schemes per pack.
pub const MAX_ENTRIES: usize = 500;

/// A ready-made watch for well-known software.
#[derive(Debug, Clone, PartialEq)]
pub struct Preset {
    /// Stable id (`ripgrep`).
    pub id: String,
    /// Display name.
    pub name: String,
    /// One sentence about it.
    pub description: String,
    /// Group in the preset list (`Command-line tools`).
    pub category: String,
    /// The watch to start from. Its `id` is empty: the backend assigns one when the user saves it.
    pub spec: WatchSpec,
}

/// A custom version scheme defined by a pack.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct SchemeDef {
    /// Id watches refer to in `versions`.
    pub id: String,
    /// Display name.
    pub name: String,
    /// The pattern, as for the `custom` scheme (named groups `id`, `major`, `minor`, `patch`, `build`, `channel`,
    /// `variant`, `pre`, and any other name for display fields).
    pub pattern: String,
    /// A name it reads, shown in the wizard's helper.
    #[serde(default)]
    pub example: String,
}

/// A parsed pack.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Pack {
    /// Presets.
    pub presets: Vec<Preset>,
    /// Version schemes.
    pub schemes: Vec<SchemeDef>,
}

#[derive(Deserialize)]
struct PresetDef {
    id: String,
    name: String,
    #[serde(default)]
    description: String,
    #[serde(default)]
    category: String,
    source: SourceConfig,
    #[serde(default)]
    versions: Option<String>,
    #[serde(default)]
    version_pattern: Option<String>,
    #[serde(default)]
    include_prereleases: bool,
    #[serde(default)]
    name_filter: String,
    #[serde(default)]
    interval_secs: Option<u32>,
    #[serde(default)]
    filter: VersionFilter,
    #[serde(default)]
    allow_local: bool,
}

impl PresetDef {
    fn into_preset(self, schemes: &[SchemeDef]) -> Result<Preset, String> {
        let spec = WatchSpec {
            id: String::new(),
            name: self.name.clone(),
            source: self.source,
            versions: self.versions.unwrap_or_else(|| "semver".into()),
            version_pattern: self.version_pattern,
            include_prereleases: self.include_prereleases,
            name_filter: self.name_filter,
            schedule: crate::schedule::Schedule::every(self.interval_secs.unwrap_or(6 * 3600)),
            notify: NotificationPolicy::default(),
            actions: Vec::new(),
            channels: Vec::new(),
            version_filter: self.filter,
            allow_local: self.allow_local,
            paused: false,
            tracking: Default::default(),
        };
        // The preset must be a watch once it has an id.
        let probe = Pack {
            presets: Vec::new(),
            schemes: schemes.to_vec(),
        }
        .resolve(&WatchSpec {
            id: "preset".into(),
            ..spec.clone()
        });
        Watch::parse(&probe).map_err(|p| p.join(" "))?;
        if self.id.trim().is_empty() {
            return Err("it has no id".into());
        }
        Ok(Preset {
            id: self.id,
            name: self.name,
            description: self.description,
            category: if self.category.is_empty() { "Other".into() } else { self.category },
            spec,
        })
    }
}

impl Pack {
    /// Parses a pack. Entries that are not valid are skipped with a warning; `Err` only when nothing can be read.
    pub fn parse(text: &str) -> Result<(Pack, Vec<String>), String> {
        if text.len() > MAX_PACK_BYTES {
            return Err("The pack is larger than 512 KiB.".into());
        }
        let table: toml::Table = toml::from_str(text).map_err(|e| format!("The pack is not valid TOML: {}", e.message()))?;
        let mut pack = Pack::default();
        let mut warnings = Vec::new();
        for (i, v) in list(&table, "scheme").into_iter().enumerate().take(MAX_ENTRIES) {
            match v.try_into::<SchemeDef>() {
                Ok(s) => match VersionScheme::custom(&s.pattern) {
                    Ok(_) if is_builtin_scheme(&s.id) || pack.schemes.iter().any(|x| x.id == s.id) || s.id.trim().is_empty() => warnings
                        .push(format!(
                            "Scheme {} (\"{}\") has a reserved or repeated id; it was skipped.",
                            i + 1,
                            s.id
                        )),
                    Ok(_) => pack.schemes.push(s),
                    Err(e) => warnings.push(format!("Scheme \"{}\" was skipped: {e}", s.id)),
                },
                Err(e) => warnings.push(format!("Scheme {} could not be read ({}); it was skipped.", i + 1, e.message())),
            }
        }
        for (i, v) in list(&table, "preset").into_iter().enumerate().take(MAX_ENTRIES) {
            match v.try_into::<PresetDef>() {
                Ok(d) => {
                    let id = d.id.clone();
                    match d.into_preset(&pack.schemes) {
                        Ok(_) if pack.presets.iter().any(|p| p.id == id) => {
                            warnings.push(format!("Preset \"{id}\" is repeated; it was skipped."))
                        }
                        Ok(p) => pack.presets.push(p),
                        Err(e) => warnings.push(format!("Preset \"{id}\" was skipped: {e}")),
                    }
                }
                Err(e) => warnings.push(format!("Preset {} could not be read ({}); it was skipped.", i + 1, e.message())),
            }
        }
        Ok((pack, warnings))
    }

    /// The pack that ships with Spotter.
    pub fn builtin() -> Pack {
        let mut all = Pack::default();
        for text in BUILTIN {
            // The shipped packs are tested to parse without warnings.
            if let Ok((p, _)) = Pack::parse(text) {
                all.merge(p);
            }
        }
        all
    }

    /// Adds another pack's entries; ids already present win.
    pub fn merge(&mut self, other: Pack) {
        for p in other.presets {
            if !self.presets.iter().any(|x| x.id == p.id) {
                self.presets.push(p);
            }
        }
        for s in other.schemes {
            if !self.schemes.iter().any(|x| x.id == s.id) {
                self.schemes.push(s);
            }
        }
    }

    /// The spec with a pack scheme resolved: a watch whose `versions` names a pack scheme becomes a `custom` one.
    /// Unknown ids are left for [`Watch::parse`] to report.
    pub fn resolve(&self, spec: &WatchSpec) -> WatchSpec {
        match self.schemes.iter().find(|s| s.id == spec.versions) {
            Some(s) => WatchSpec {
                versions: "custom".into(),
                version_pattern: Some(s.pattern.clone()),
                ..spec.clone()
            },
            None => spec.clone(),
        }
    }
}

fn is_builtin_scheme(id: &str) -> bool {
    matches!(id, "semver" | "numeric" | "build-number" | "date" | "custom")
}

fn list(t: &toml::Table, key: &str) -> Vec<toml::Value> {
    match t.get(key) {
        Some(toml::Value::Array(a)) => a.clone(),
        _ => Vec::new(),
    }
}

/// The shipped packs.
const BUILTIN: [&str; 5] = [
    include_str!("../packs/languages.toml"),
    include_str!("../packs/tools.toml"),
    include_str!("../packs/selfhosted.toml"),
    include_str!("../packs/registries.toml"),
    include_str!("../packs/vendor-schemes.toml"),
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_shipped_scheme_reads_its_own_example() {
        let pack = Pack::builtin();
        let shipped: Vec<_> = pack.schemes.iter().filter(|s| !s.example.is_empty()).collect();
        assert!(shipped.len() >= 6, "{}", shipped.len());
        for s in shipped {
            let scheme = VersionScheme::custom(&s.pattern).unwrap();
            let x = scheme.extract_full(&s.example).unwrap_or_else(|e| panic!("{}: {e:?}", s.id));
            assert!(!x.version.parts.is_empty(), "{}", s.id);
        }
        let letters = VersionScheme::custom(&pack.schemes.iter().find(|s| s.id == "letters-build").unwrap().pattern).unwrap();
        assert_eq!(letters.extract("Setup-R1.04.00T12.exe").unwrap().text, "R1.04.00T12");
    }

    #[test]
    fn the_shipped_packs_are_clean_and_big_enough() {
        for text in BUILTIN {
            let (_, warnings) = Pack::parse(text).unwrap();
            assert!(warnings.is_empty(), "{warnings:?}");
        }
        let pack = Pack::builtin();
        assert!(pack.presets.len() >= 25, "{}", pack.presets.len());
        let mut ids: Vec<&str> = pack.presets.iter().map(|p| p.id.as_str()).collect();
        ids.sort_unstable();
        ids.dedup();
        assert_eq!(ids.len(), pack.presets.len(), "ids are unique");
        for p in &pack.presets {
            assert!(!p.description.is_empty() && !p.category.is_empty(), "{}", p.id);
            let w = Watch::parse(&WatchSpec {
                id: "x".into(),
                ..pack.resolve(&p.spec)
            });
            assert!(w.is_ok(), "{}: {w:?}", p.id);
        }
    }

    #[test]
    fn schemes_resolve_and_bad_entries_are_skipped_with_warnings() {
        let text = r#"
[[scheme]]
id = "ubuntu"
name = "Ubuntu"
pattern = '(?<major>\d{2})\.(?<minor>\d{2})'
[[scheme]]
id = "semver"
name = "Clash"
pattern = '(?<major>\d+)'
[[scheme]]
id = "nogroups"
name = "Bad"
pattern = '\d+'
[[scheme]]
id = "ubuntu"
name = "Again"
pattern = '(?<major>\d+)'
[[scheme]]
oops = 1
[[preset]]
id = "good"
name = "Good"
category = "Tools"
source = { kind = "github", values = { repo = "o/r" } }
versions = "ubuntu"
[[preset]]
id = "good"
name = "Repeated"
source = { kind = "github", values = { repo = "o/r" } }
[[preset]]
id = "badsource"
name = "Bad"
source = { kind = "teleport", values = {} }
[[preset]]
id = " "
name = "No id"
source = { kind = "github", values = { repo = "o/r" } }
[[preset]]
name = "No id at all"
"#;
        let (pack, warnings) = Pack::parse(text).unwrap();
        assert_eq!(pack.schemes.len(), 1);
        assert_eq!(pack.presets.len(), 1);
        assert_eq!(warnings.len(), 8, "{warnings:#?}");
        let spec = pack.resolve(&pack.presets[0].spec);
        assert_eq!((spec.versions.as_str(), spec.version_pattern.is_some()), ("custom", true));
        assert_eq!(pack.presets[0].category, "Tools");
        assert_eq!(pack.presets[0].spec.id, "");
        let untouched = pack.resolve(&WatchSpec {
            versions: "semver".into(),
            ..spec.clone()
        });
        assert_eq!(untouched.versions, "semver");
    }

    #[test]
    fn whole_packs_can_be_refused() {
        assert!(Pack::parse("= nope").unwrap_err().contains("TOML"));
        assert!(Pack::parse(&" ".repeat(MAX_PACK_BYTES + 1)).unwrap_err().contains("512"));
        let (empty, w) = Pack::parse("").unwrap();
        assert_eq!((empty, w.len()), (Pack::default(), 0));
        let (odd, _) = Pack::parse("scheme = 5\npreset = 'x'").unwrap();
        assert_eq!(odd, Pack::default());
    }

    #[test]
    fn merging_keeps_existing_ids() {
        let mut a = Pack::builtin();
        let (n, ns) = (a.presets.len(), a.schemes.len());
        let (b, _) = Pack::parse(
            "[[preset]]\nid='ripgrep'\nname='Other'\nsource={kind='github',values={repo='o/r'}}\n[[preset]]\nid='mine'\nname='Mine'\nsource={kind='github',values={repo='o/r'}}\n[[scheme]]\nid='s'\nname='S'\npattern='(?<major>\\d+)'",
        )
        .unwrap();
        a.merge(b.clone());
        a.merge(b);
        assert_eq!(a.presets.len(), n + 1);
        assert_eq!(a.schemes.len(), ns + 1);
        assert_eq!(a.presets.iter().find(|p| p.id == "ripgrep").unwrap().name, "ripgrep");
    }
}
