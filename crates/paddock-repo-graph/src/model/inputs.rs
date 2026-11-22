//! The declared inputs and owners of the repository (ADR 0117 §3): the defaults this crate carries ([`Inputs::defaults`]),
//! merged with the entries of `docs/repo-graph.toml` ([`Inputs::merged`]). The file is read by one accessor at the edge,
//! `paddock-dev`'s `repo_inputs` (ER-42), so this crate stays free of a TOML reader and of a change to every app's lockfile.
//!
//! A named input is a file set and what it reaches (Nx `namedInputs`, Turborepo `globalDependencies`); an owner rule says who owns
//! a path no project claims (`none`: nothing builds it). A file entry replaces the default input of the same name; its owners
//! come after the defaults and win a tie.

use super::Reach;
use super::glob;

/// What a named input reaches.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub enum Target {
    /// Every project (a true global; kept short, as Nx and Turborepo advise).
    All,
    /// Every project with a Cargo workspace (its Rust half).
    Cargo,
    /// Every project with an npm package (its UI half).
    Npm,
    /// Every app.
    Apps,
    /// One project by id (`crates`, `ui`, `conformance`, `tools`, `paddock-dev`, `app:dyno`).
    Project(String),
}

impl Target {
    /// The target a word of the inputs file names: `all`, `cargo`, `npm`, `apps` or `project:<id>`.
    pub fn parse(s: &str) -> Result<Target, String> {
        Ok(match s {
            "all" => Target::All,
            "cargo" => Target::Cargo,
            "npm" => Target::Npm,
            "apps" => Target::Apps,
            _ => match s.strip_prefix("project:") {
                Some(p) if !p.is_empty() => Target::Project(p.to_string()),
                _ => {
                    return Err(format!(
                        "unknown target {s:?} (all, cargo, npm, apps or project:<id>)"
                    ));
                }
            },
        })
    }
}

/// One named input.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Input {
    /// Its name (an entry of the file with the same name replaces the default).
    pub name: String,
    /// The path globs.
    pub files: Vec<String>,
    /// What it reaches.
    pub reaches: Vec<Target>,
    /// The kind of reach.
    pub reach: Reach,
    /// A `.gitattributes` file: only the attribute lines that changed reach anything (G2).
    pub attributes: bool,
}

/// The owner a declared pattern gives a path.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Owner {
    /// Nothing builds or tests it (a spike, a scratch folder).
    None,
    /// Documentation: only the docs checks read it.
    Doc,
    /// A project by id.
    Project(String),
}

/// One owner rule.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OwnerRule {
    /// The path globs.
    pub paths: Vec<String>,
    /// The owner.
    pub owner: Owner,
}

/// The declared inputs and owners, merged.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Inputs {
    /// Named inputs.
    pub inputs: Vec<Input>,
    /// Owner rules, in order (later wins a tie of specificity).
    pub owners: Vec<OwnerRule>,
}

impl Inputs {
    /// The defaults: the former `GLOBAL` list of the planner, narrowed (ADR 0117 §3, G2, G7, G8, G13), and the owners of the root
    /// folders no project claims.
    pub fn defaults() -> Inputs {
        let input =
            |name: &str, files: &[&str], reaches: Vec<Target>, reach: Reach, attributes: bool| {
                Input {
                    name: name.into(),
                    files: files.iter().map(|s| s.to_string()).collect(),
                    reaches,
                    reach,
                    attributes,
                }
            };
        let project = |p: &str| Target::Project(p.into());
        let owner = |paths: &[&str], owner: Owner| OwnerRule {
            paths: paths.iter().map(|s| s.to_string()).collect(),
            owner,
        };
        Inputs {
            inputs: vec![
                input(
                    "rust-toolchain",
                    &["rust-toolchain", "rust-toolchain.toml"],
                    vec![Target::All],
                    Reach::Code,
                    false,
                ),
                input(
                    "npm-root",
                    &["package.json", ".npmrc", ".nvmrc", "tsconfig/**"],
                    vec![Target::All],
                    Reach::Code,
                    false,
                ),
                input(
                    "cargo-config",
                    &[".cargo/**", "lint/**"],
                    vec![Target::Cargo, project("crates"), project("paddock-dev")],
                    Reach::Code,
                    false,
                ),
                input(
                    "git-attributes",
                    &[".gitattributes"],
                    vec![project("conformance")],
                    Reach::Code,
                    true,
                ),
                input(
                    "lessons",
                    &["docs/lessons.md"],
                    vec![project("conformance"), project("tools")],
                    Reach::Docs,
                    false,
                ),
            ],
            owners: vec![
                owner(
                    &["docs/**", "**/*.md", "LICENSE*", ".gitea/**/*.md"],
                    Owner::Doc,
                ),
                owner(&["conformance/reserved/**"], Owner::None),
                owner(&["spikes/**"], Owner::None),
            ],
        }
    }

    /// These inputs with `file`'s entries merged in: an input of the same name is replaced, owners are appended (they win a tie).
    pub fn merged(mut self, file: Inputs) -> Inputs {
        for i in file.inputs {
            match self.inputs.iter_mut().find(|x| x.name == i.name) {
                Some(x) => *x = i,
                None => self.inputs.push(i),
            }
        }
        self.owners.extend(file.owners);
        self
    }

    /// The named inputs `path` is one of.
    pub fn inputs_of(&self, path: &str) -> Vec<&Input> {
        self.inputs
            .iter()
            .filter(|i| i.files.iter().any(|g| glob::matches(g, path)))
            .collect()
    }

    /// The declared owner of `path`: the most specific matching pattern, the later rule on a tie; `None` when no rule matches.
    pub fn owner_of(&self, path: &str) -> Option<&Owner> {
        let mut best: Option<(usize, &Owner)> = None;
        for r in &self.owners {
            for g in &r.paths {
                if glob::matches(g, path) {
                    let s = glob::specificity(g);
                    if best.is_none_or(|(b, _)| s >= b) {
                        best = Some((s, &r.owner));
                    }
                }
            }
        }
        best.map(|(_, o)| o)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_defaults_keep_the_global_list_short_and_own_the_root_folders() {
        let i = Inputs::defaults();
        let all: Vec<&str> = i
            .inputs
            .iter()
            .filter(|x| x.reaches.contains(&Target::All))
            .map(|x| x.name.as_str())
            .collect();
        assert_eq!(all, ["rust-toolchain", "npm-root"]);
        assert!(i.inputs_of(".gitattributes")[0].attributes);
        assert_eq!(
            i.owner_of(".gitea/actions/README.md"),
            Some(&Owner::Doc),
            "G7"
        );
        assert_eq!(
            i.owner_of("conformance/reserved/C156-x"),
            Some(&Owner::None),
            "G8"
        );
        assert_eq!(
            i.owner_of("spikes/dialog-hidden/Cargo.lock"),
            Some(&Owner::None),
            "G3"
        );
        assert_eq!(i.owner_of("crates/a/src/lib.rs"), None);
    }

    #[test]
    fn a_file_entry_replaces_the_default_of_its_name_and_its_owner_wins_a_tie() {
        let file = Inputs {
            inputs: vec![Input {
                name: "npm-root".into(),
                files: vec!["package.json".into()],
                reaches: vec![Target::Npm],
                reach: Reach::Code,
                attributes: false,
            }],
            owners: vec![OwnerRule {
                paths: vec!["spikes/**".into()],
                owner: Owner::Project("tools".into()),
            }],
        };
        let i = Inputs::defaults().merged(file);
        let npm = i.inputs.iter().find(|x| x.name == "npm-root").unwrap();
        assert_eq!(npm.reaches, [Target::Npm]);
        assert_eq!(i.inputs.len(), Inputs::defaults().inputs.len());
        assert_eq!(
            i.owner_of("spikes/x"),
            Some(&Owner::Project("tools".into()))
        );
    }

    #[test]
    fn targets_parse_from_their_words() {
        assert_eq!(
            Target::parse("project:app:dyno"),
            Ok(Target::Project("app:dyno".into()))
        );
        assert_eq!(Target::parse("cargo"), Ok(Target::Cargo));
        assert!(
            Target::parse("everything")
                .unwrap_err()
                .contains("unknown target")
        );
        assert!(Target::parse("project:").is_err());
    }
}
