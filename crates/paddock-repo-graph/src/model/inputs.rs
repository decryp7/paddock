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

/// The default named inputs: (name, files, reaches, attributes); every reach is `code`. A reach word as the inputs file spells it.
const DEFAULT_INPUTS: &[(&str, &[&str], &[&str], bool)] = &[
    (
        "rust-toolchain",
        &["rust-toolchain", "rust-toolchain.toml"],
        &["all"],
        false,
    ),
    (
        "npm-root",
        &["package.json", ".npmrc", ".nvmrc", "tsconfig/**"],
        &["all"],
        false,
    ),
    ("npm-lock", &["package-lock.json"], &["npm"], false),
    (
        "cargo-config",
        &[".cargo/**", "lint/**"],
        &["cargo", "project:crates", "project:paddock-dev"],
        false,
    ),
    (
        "git-attributes",
        &[".gitattributes"],
        &["project:conformance"],
        true,
    ),
    (
        "check-id-reservations",
        &["conformance/reserved/**"],
        &[],
        false,
    ),
    (
        "lessons",
        &["docs/lessons.md", "docs/rules.md", "docs/blocks/**"],
        &["project:conformance", "project:tools"],
        false,
    ),
    (
        "repo-graph",
        &["docs/repo-graph.toml"],
        &["project:paddock-dev", "project:conformance"],
        false,
    ),
    (
        "gates-register",
        &["docs/gates.toml"],
        &["project:paddock-dev", "project:conformance"],
        false,
    ),
    (
        "generators-register",
        &["docs/generators.toml"],
        &["project:paddock-dev"],
        false,
    ),
    (
        "known-copies",
        &["docs/known-copies.toml"],
        &["project:tools", "project:conformance"],
        false,
    ),
    (
        "harnesses",
        &["docs/harnesses.toml"],
        &["project:conformance"],
        false,
    ),
    (
        "ipc-endpoints",
        &["docs/ipc-endpoints.toml", "docs/ipc-endpoint-ratchet.toml"],
        &["project:conformance"],
        false,
    ),
    (
        "ci-scope-register",
        &["docs/ci-scope-first-runs.toml"],
        &["project:paddock-dev"],
        false,
    ),
    (
        "design-docs",
        &[
            "docs/design-guide.md",
            "docs/benchmarks.md",
            "docs/benchmarks/**",
        ],
        &["project:conformance"],
        false,
    ),
    (
        "doc-images",
        &[
            "docs/images/src/**",
            "docs/design/**",
            "**/*.drawio",
            "docs/**/svg/*.svg",
            "diagrams/**/svg/*.svg",
            "docs/images/*.svg",
        ],
        &["project:tools"],
        false,
    ),
    ("wiki", &["docs/wiki/**"], &["project:tools"], false),
    (
        "changelogs",
        &[
            "apps/*/CHANGELOG.md",
            "crates/*/CHANGELOG.md",
            "ui/projects/*/CHANGELOG.md",
            "apps/*/vendor/**/*.md",
        ],
        &["owner"],
        false,
    ),
    (
        "prose-read-by-code",
        &[
            "**/resources/**/*.md",
            "**/samples/**/*.md",
            "**/src/**/*.md",
            "**/tests/**/*.md",
            "**/test/**/*.md",
            "**/templates/**/*.md",
            "**/e2e/**/*.md",
            "**/fixtures/**/*.md",
        ],
        &["owner"],
        false,
    ),
    (
        "demo-readmes",
        &["apps/shell-demo/README.md", "apps/workbench-demo/README.md"],
        &["project:tools"],
        false,
    ),
    (
        "lint-wiring",
        &[
            "apps/*/app/eslint*.js",
            "apps/*/app/eslint*.cjs",
            "apps/*/app/eslint*.mjs",
            "apps/*/app/eslint-suppressions.json",
            "apps/*/app/angular.json",
            "apps/*/app/package.json",
            "apps/*/paddock.ci.json",
            "ui/eslint*.js",
            "ui/eslint*.cjs",
            "ui/eslint*.mjs",
            "ui/eslint-suppressions.json",
            "ui/angular.json",
            "ui/package.json",
            "ui/paddock.ci.json",
        ],
        &["owner", "project:eslint"],
        false,
    ),
    (
        "conformance-config",
        &[
            "paddock.conformance.json",
            "paddock.waivers.toml",
            ".gitleaks.toml",
        ],
        &["project:conformance"],
        false,
    ),
    (
        "dev-wrapper",
        &["dev", "dev.cmd"],
        &["project:paddock-dev"],
        false,
    ),
    (
        "scripts",
        &["scripts/**"],
        &["project:tools", "project:ui"],
        false,
    ),
    (
        "ci-image",
        &["ci-image/**"],
        &["project:ci-image", "project:ui", "project:tools"],
        false,
    ),
    (
        "repo-tooling",
        &[
            "templates/**",
            "renovate/**",
            "renovate.json",
            "CODEOWNERS",
            "wiki-lint.json",
            ".claude/hooks/**",
            ".claude/settings.json",
            "lefthook.yml",
        ],
        &["project:tools"],
        false,
    ),
    // Until the CI adapter (WP-2) reads workflows and actions, a non-prose file under .gitea reaches everything, as the planner
    // without its revision reader does today.
    ("ci-files", &[".gitea/**"], &["all"], false),
];

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

/// The declared inputs and owners, merged.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Inputs {
    /// Named inputs.
    pub inputs: Vec<Input>,
    /// Owner rules, in order (later wins a tie of specificity).
    pub owners: Vec<OwnerRule>,
}

impl Inputs {
    /// The defaults: the former `GLOBAL` list of the planner narrowed (ADR 0117 §3, G2, G7, G8, G13), the declared doc inputs and
    /// root-file owners it carried (`DOC_INPUTS`, `OWNERS`, `LINT_WIRING`), the registers under `docs/` with the checks that read
    /// them (review round 2, P1-A), and the owners of the paths outside every project folder.
    pub fn defaults() -> Inputs {
        use Reach::Code;
        let inputs = DEFAULT_INPUTS
            .iter()
            .map(|(name, files, reaches, attributes)| Input {
                name: name.to_string(),
                files: files.iter().map(|s| s.to_string()).collect(),
                // The table is this crate's own literal; its words are checked by the tests below.
                reaches: reaches
                    .iter()
                    .filter_map(|r| Target::parse(r).ok())
                    .collect(),
                reach: Code,
                attributes: *attributes,
                except: if *name == "ci-files" {
                    vec!["**/*.md".to_string()]
                } else {
                    Vec::new()
                },
            })
            .collect();
        let owner = |paths: &[&str], owner: Owner| OwnerRule {
            paths: paths.iter().map(|s| s.to_string()).collect(),
            owner,
        };
        Inputs {
            inputs,
            owners: vec![
                owner(
                    &[
                        "docs/**",
                        "diagrams/**",
                        "*.md",
                        "LICENSE*",
                        ".gitea/**/*.md",
                    ],
                    Owner::Doc,
                ),
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
        // The CI files reach everything only until the CI adapter (WP-2) reads them.
        assert_eq!(all, ["rust-toolchain", "npm-root", "ci-files"]);
        assert!(
            i.inputs_of(".gitea/actions/README.md").is_empty(),
            "G7: prose under .gitea is the doc owner's"
        );
        assert!(i.inputs_of(".gitattributes")[0].attributes);
        assert_eq!(
            i.owner_of(".gitea/actions/README.md"),
            Some(&Owner::Doc),
            "G7"
        );
        assert!(
            i.inputs_of("conformance/reserved/C156-x")[0]
                .reaches
                .is_empty(),
            "G8: claimed, reaches nothing"
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
                except: Vec::new(),
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
    fn every_default_reach_word_parses() {
        for (name, _, reaches, _) in DEFAULT_INPUTS {
            for r in *reaches {
                assert!(Target::parse(r).is_ok(), "{name}: {r}");
            }
        }
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
