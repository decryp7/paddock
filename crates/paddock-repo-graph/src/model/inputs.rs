//! The declared inputs and owners of the repository (ADR 0117 §3): every one of them is data in `docs/repo-graph.toml`, read by
//! one accessor at the edge, `paddock-dev`'s `repo_inputs` (ER-42), so this crate holds no path (§14, [`super::guard`]) and no
//! TOML reader (which would change every app's lockfile through the workspace hack).
//!
//! A named input is a file set and what it reaches (Nx `namedInputs`, Turborepo `globalDependencies`); an owner rule says who owns
//! a path no project claims (`none`: nothing builds it); the most specific owner pattern wins, a later rule a tie.

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
    /// The project whose folder holds the changed file (a changelog compiled into its app).
    Owner,
}

impl Target {
    /// The target a word of the inputs file names: `all`, `cargo`, `npm`, `apps` or `project:<id>`.
    pub fn parse(s: &str) -> Result<Target, String> {
        Ok(match s {
            "all" => Target::All,
            "cargo" => Target::Cargo,
            "npm" => Target::Npm,
            "apps" => Target::Apps,
            "owner" => Target::Owner,
            _ => match s.strip_prefix("project:") {
                Some(p) if !p.is_empty() => Target::Project(p.to_string()),
                _ => {
                    return Err(format!(
                        "unknown target {s:?} (all, cargo, npm, apps, owner or project:<id>)"
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
    /// The content extractor that narrows it (`attributes`, `npm-lock`, `guards`; [`super::narrow`]).
    pub extractor: Option<String>,
    /// An additive input: the file keeps its owner and the input adds its targets (a check that also reads a project's file).
    pub adds: bool,
    /// Globs taken back out of `files` (the CI files' input leaves their prose to the doc owner).
    pub except: Vec<String>,
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

/// The declared inputs and owners (an empty set owns nothing: every file is unowned and the plan answers everything).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Inputs {
    /// Named inputs.
    pub inputs: Vec<Input>,
    /// Owner rules, in order (later wins a tie of specificity).
    pub owners: Vec<OwnerRule>,
    /// Local actions declared setup-only: plumbing even when a release job uses them (rule 54). Narrowing the set of release
    /// reaches, so adding a name is the unsafe direction (ADR 0117 §14).
    pub setup_only: Vec<String>,
    /// Local actions whose use makes a job a release job (bundling, signing, publishing).
    pub release_actions: Vec<String>,
    /// What counts as prose (review round 3, P1-A).
    pub prose: Prose,
}

/// What counts as prose (ADR 0117 §2, review round 3 P1-A): globs over a file's name (no `/`) or path.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Prose {
    /// Prose inside a project folder, by file name (`README*`, `CHANGELOG*`, `LICENSE*`).
    pub project_names: Vec<String>,
    /// A project's own folders whose files are prose (`docs`).
    pub project_folders: Vec<String>,
    /// Prose under a doc owner (`*.md`, `*.txt`, `LICENSE*`).
    pub doc_owner: Vec<String>,
    /// Documentation images under a doc owner, by name (`*.png`); content detection may override (D14).
    pub doc_assets: Vec<String>,
}

impl Inputs {
    /// The named inputs `path` is one of.
    pub fn inputs_of(&self, path: &str) -> Vec<&Input> {
        self.inputs
            .iter()
            .filter(|i| {
                i.files.iter().any(|g| glob::matches(g, path))
                    && !i.except.iter().any(|g| glob::matches(g, path))
            })
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

    /// Prose under a doc owner.
    pub fn is_prose(&self, path: &str) -> bool {
        self.prose.doc_owner.iter().any(|g| glob::matches(g, path))
    }

    /// Prose inside the project folder `project_dir`: a declared name, or a file under one of the project's prose folders.
    pub fn is_project_prose(&self, project_dir: &str, path: &str) -> bool {
        let rel = path
            .strip_prefix(project_dir)
            .and_then(|r| r.strip_prefix('/'));
        self.prose
            .project_names
            .iter()
            .any(|g| glob::matches(g, path))
            || rel.is_some_and(|r| {
                self.prose.project_folders.iter().any(|d| {
                    r.strip_prefix(d.as_str())
                        .is_some_and(|x| x.starts_with('/'))
                })
            })
    }

    /// A documentation image by its name.
    pub fn is_doc_asset(&self, path: &str) -> bool {
        self.prose.doc_assets.iter().any(|g| glob::matches(g, path))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn owner(paths: &[&str], owner: Owner) -> OwnerRule {
        OwnerRule {
            paths: paths.iter().map(|s| s.to_string()).collect(),
            owner,
        }
    }

    #[test]
    fn the_most_specific_owner_wins_and_a_later_rule_wins_a_tie() {
        let i = Inputs {
            owners: vec![
                owner(&["docs/**", "*.md"], Owner::Doc),
                owner(&["docs/x/**"], Owner::None),
                owner(&["spikes/**"], Owner::None),
                owner(&["spikes/**"], Owner::Project("tools".into())),
            ],
            ..Inputs::default()
        };
        assert_eq!(i.owner_of("docs/a.md"), Some(&Owner::Doc));
        assert_eq!(i.owner_of("docs/x/a.md"), Some(&Owner::None));
        assert_eq!(
            i.owner_of("spikes/a"),
            Some(&Owner::Project("tools".into()))
        );
        assert_eq!(i.owner_of("crates/a/src/lib.rs"), None);
        assert_eq!(
            Inputs::default().owner_of("anything"),
            None,
            "an empty set owns nothing (fail closed)"
        );
    }

    #[test]
    fn prose_comes_from_the_declared_globs() {
        let i = Inputs {
            prose: Prose {
                project_names: vec!["README*".into()],
                project_folders: vec!["docs".into()],
                doc_owner: vec!["*.md".into()],
                doc_assets: vec!["*.png".into()],
            },
            ..Inputs::default()
        };
        assert!(i.is_project_prose("crates/a", "crates/a/README.md"));
        assert!(i.is_project_prose("apps/x", "apps/x/docs/guide/a.md"));
        assert!(
            !i.is_project_prose("apps/x", "apps/x/src/docs/a.md"),
            "only the project's own docs folder"
        );
        assert!(!i.is_project_prose("crates/a", "crates/a/tests/data.txt"));
        assert!(i.is_prose("diagrams/a.md") && !i.is_prose("diagrams/a.xml"));
        assert!(i.is_doc_asset("docs/pictures/a.png"));
        // Control: with nothing declared nothing is prose.
        assert!(!Inputs::default().is_project_prose("crates/a", "crates/a/README.md"));
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
