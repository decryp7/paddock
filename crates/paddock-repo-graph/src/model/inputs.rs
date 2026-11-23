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

/// The default named inputs: (name, files, reaches, content extractor); every reach is `code`. A reach word as the inputs file
/// spells it.
/// One default input row: (name, files, reaches, content extractor).
type InputRow = (
    &'static str,
    &'static [&'static str],
    &'static [&'static str],
    Option<&'static str>,
);

const DEFAULT_INPUTS: &[InputRow] = &[
    (
        "rust-toolchain",
        &["/rust-toolchain", "/rust-toolchain.toml"],
        &["all"],
        None,
    ),
    (
        "npm-root",
        &["/package.json", "/.npmrc", "/.nvmrc", "tsconfig/**"],
        &["all"],
        None,
    ),
    // Narrowed by the lock's content: only the workspaces whose resolved packages changed (and their dependents).
    ("npm-lock", &["/package-lock.json"], &[], Some("npm-lock")),
    (
        "cargo-config",
        &[".cargo/**", "lint/**"],
        &["cargo", "project:crates", "project:paddock-dev"],
        None,
    ),
    (
        "git-attributes",
        &["/.gitattributes"],
        &["project:conformance"],
        Some("attributes"),
    ),
    (
        "check-id-reservations",
        &["conformance/reserved/**"],
        &[],
        None,
    ),
    (
        "lessons",
        &["docs/lessons.md", "docs/rules.md", "docs/blocks/**"],
        &["project:conformance", "project:tools"],
        None,
    ),
    (
        "repo-graph",
        &["docs/repo-graph.toml"],
        &["project:paddock-dev", "project:conformance"],
        None,
    ),
    (
        "gates-register",
        &["docs/gates.toml"],
        &["project:paddock-dev", "project:conformance"],
        None,
    ),
    (
        "generators-register",
        &["docs/generators.toml"],
        &["project:paddock-dev"],
        None,
    ),
    (
        "known-copies",
        &["docs/known-copies.toml"],
        &["project:tools", "project:conformance"],
        None,
    ),
    (
        "harnesses",
        &["docs/harnesses.toml"],
        &["project:conformance"],
        None,
    ),
    (
        "ipc-endpoints",
        &["docs/ipc-endpoints.toml", "docs/ipc-endpoint-ratchet.toml"],
        &["project:conformance"],
        None,
    ),
    (
        "ci-scope-register",
        &["docs/ci-scope-first-runs.toml"],
        &["project:paddock-dev"],
        None,
    ),
    (
        "design-docs",
        &[
            "docs/design-guide.md",
            "docs/benchmarks.md",
            "docs/benchmarks/**",
        ],
        &["project:conformance"],
        None,
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
        None,
    ),
    ("wiki", &["docs/wiki/**"], &["project:tools"], None),
    (
        "changelogs",
        &[
            "apps/*/CHANGELOG.md",
            "crates/*/CHANGELOG.md",
            "ui/projects/*/CHANGELOG.md",
            "apps/*/vendor/**/*.md",
        ],
        &["owner"],
        None,
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
        None,
    ),
    (
        "demo-readmes",
        &["apps/shell-demo/README.md", "apps/workbench-demo/README.md"],
        &["project:tools"],
        None,
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
        None,
    ),
    (
        "conformance-config",
        &[
            "/paddock.conformance.json",
            "/paddock.waivers.toml",
            "/.gitleaks.toml",
        ],
        &["project:conformance"],
        None,
    ),
    (
        "dev-wrapper",
        &["/dev", "/dev.cmd"],
        &["project:paddock-dev"],
        None,
    ),
    (
        "scripts",
        &["scripts/**"],
        &["project:tools", "project:ui"],
        None,
    ),
    (
        "ci-image",
        &["ci-image/**"],
        &["project:ci-image", "project:ui", "project:tools"],
        None,
    ),
    // The diagram sources and their export script (the diagrams check renders them).
    (
        "diagram-sources",
        &["diagrams/**/*.xml", "diagrams/scripts/**"],
        &["project:tools"],
        None,
    ),
    // brief_check.rs `include_str!`s a template (review round 3).
    (
        "templates",
        &["templates/**"],
        &["project:tools", "project:paddock-dev"],
        None,
    ),
    (
        "repo-tooling",
        &[
            "renovate/**",
            "/renovate.json",
            "/CODEOWNERS",
            "/wiki-lint.json",
            ".claude/hooks/**",
            ".claude/settings.json",
            "/lefthook.yml",
        ],
        &["project:tools"],
        None,
    ),
];

/// The additive inputs: a check or test that reads files other projects own (ADR 0117 §3, the planner's former `C99_INPUT`,
/// `C100_INPUTS`, `PARITY_INPUT`, `HOOKS_PARITY_INPUT`, `APP_CAPABILITY_INPUT`, `PER_MACHINE_INPUT`, `E2E_GUARD_INPUTS`,
/// `NOTICES_INPUTS`, `GENERATED` and the cause-code scan). The file keeps its owner; the input adds its targets. (name, files,
/// reaches, except.)
/// One additive input row: (name, files, reaches, except).
type AddRow = (
    &'static str,
    &'static [&'static str],
    &'static [&'static str],
    &'static [&'static str],
);

const DEFAULT_ADDS: &[AddRow] = &[
    (
        "c99-reads",
        &[
            "apps/**/Cargo.toml",
            "apps/**/Cargo.lock",
            "crates/**/Cargo.toml",
            "crates/**/Cargo.lock",
            "apps/**/*.rs",
            "crates/**/*.rs",
            "apps/**/*.c",
            "crates/**/*.c",
            "apps/**/*.cc",
            "crates/**/*.cc",
            "apps/**/*.cpp",
            "crates/**/*.cpp",
            "apps/**/*.m",
            "crates/**/*.m",
            "apps/**/*.mm",
            "crates/**/*.mm",
            "apps/**/*.h",
            "crates/**/*.h",
            "apps/**/*.swift",
            "crates/**/*.swift",
            "**/app/src/**/*.ts",
            "**/ui/projects/**/*.ts",
            "**/src-tauri/capabilities/*.json",
            "paddock.ci.json",
            "conformance/src/**",
            "conformance/bin/**",
            ".cargo/config.toml",
            "/paddock.waivers.toml",
        ],
        &["project:c99"],
        &[],
    ),
    (
        "cause-code-scan",
        &["crates/**/*.rs", "apps/**/*.rs"],
        &["project:ui"],
        &[
            "**/target/**",
            "**/node_modules/**",
            "**/tests/**",
            "**/benches/**",
            "tests.rs",
        ],
    ),
    (
        "c100-reads",
        &[
            "docs/gates.toml",
            "apps/*/paddock.ci.json",
            "package.json",
            "/lefthook.yml",
            "/AGENTS.md",
            "apps/*/AGENTS.md",
            "apps/*/docs/performance.md",
            "README.md",
            "docs/wiki/**/*.md",
            "conformance/src/checks/**",
            "conformance/src/cli.mjs",
            "tools/paddock-dev/src/gates/**",
            "scripts/**/check-*.mjs",
            "scripts/**/check-*.sh",
            "scripts/**/check-*.ps1",
            "scripts/**/*.test.mjs",
            "conformance/**/check-*.mjs",
            "conformance/**/check-*.sh",
            "conformance/**/check-*.ps1",
            "conformance/**/*.test.mjs",
            "tools/**/check-*.mjs",
            "tools/**/check-*.sh",
            "tools/**/check-*.ps1",
            "tools/**/*.test.mjs",
            "apps/*/scripts/**/check-*.mjs",
            "apps/*/scripts/**/check-*.sh",
            "apps/*/scripts/**/check-*.ps1",
            "apps/*/scripts/**/*.test.mjs",
        ],
        &["project:conformance"],
        &[],
    ),
    (
        "conformance-parity",
        &[
            "conformance/src/**/*.mjs",
            "conformance/src/**/*.json",
            "conformance/bin/**/*.mjs",
            "conformance/bin/**/*.json",
            "conformance/baselines/**/*.mjs",
            "conformance/baselines/**/*.json",
        ],
        &["project:paddock-dev"],
        &[],
    ),
    (
        "hooks-parity",
        &["tools/paddock-dev/src/places.rs", "docs/known-copies.toml"],
        &["project:tools"],
        &[],
    ),
    (
        "app-capabilities",
        &[
            "apps/*/src-tauri/Cargo.toml",
            "apps/*/app/src-tauri/Cargo.toml",
            "apps/*/src-tauri/capabilities/**",
            "apps/*/app/src-tauri/capabilities/**",
        ],
        &["project:conformance"],
        &[],
    ),
    (
        "per-machine",
        &[
            "apps/*/app/src-tauri/tauri.conf.json",
            "apps/*/app/src-tauri/tauri.windows.conf.json",
            "apps/*/app/src-tauri/windows/**",
            "crates/paddock-setup/nsis/paddock-per-machine.nsh",
        ],
        &["project:conformance", "project:crates"],
        &[],
    ),
    // The icon tools render every app's icons (the planner's `icons/` rule; `icons/<app>/` is the app's own, an open gap).
    (
        "icon-tools",
        &["icons/tools/**", "icons/package.json"],
        &["apps"],
        &[],
    ),
    (
        "e2e-guards",
        &[
            "ui/projects/paddock-e2e/src/env.ts",
            "ui/projects/paddock-e2e/src/build-guard.ts",
        ],
        &["apps"],
        &[],
    ),
    (
        "notices",
        &[
            "scripts/notices.sh",
            "templates/repo/about.toml",
            "conformance/src/notices/**",
        ],
        &["apps"],
        &[],
    ),
    (
        "generated-bindings",
        &["ui/**/generated/*.ts", "ui/**/generated/*.json"],
        &["project:crates"],
        &["**/public-api.ts", "**/ng-package.json"],
    ),
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

/// The declared inputs and owners, merged.
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
}

/// The actions ADR 0117 §5 names setup-only: the Rust setup, the job split and the cache actions.
pub const DEFAULT_SETUP_ONLY: [&str; 6] = [
    "setup-rust",
    "build-jobs",
    "node-caches",
    "dev-tool-cache",
    "sccache-stats",
    "cache-report",
];

/// The actions that bundle, sign or publish (ADR 0117 §5).
pub const DEFAULT_RELEASE_ACTIONS: [&str; 5] = [
    "tauri-bundle",
    "publish-release",
    "macos-verify",
    "keychain",
    "strip-signing-keys",
];

impl Inputs {
    /// The defaults: the former `GLOBAL` list of the planner narrowed (ADR 0117 §3, G2, G7, G8, G13), the declared doc inputs and
    /// root-file owners it carried (`DOC_INPUTS`, `OWNERS`, `LINT_WIRING`), the registers under `docs/` with the checks that read
    /// them (review round 2, P1-A), and the owners of the paths outside every project folder.
    pub fn defaults() -> Inputs {
        use Reach::Code;
        let inputs = DEFAULT_INPUTS
            .iter()
            .map(|(name, files, reaches, extractor)| Input {
                name: name.to_string(),
                files: files.iter().map(|s| s.to_string()).collect(),
                // The table is this crate's own literal; its words are checked by the tests below.
                reaches: reaches
                    .iter()
                    .filter_map(|r| Target::parse(r).ok())
                    .collect(),
                reach: Code,
                extractor: extractor.map(str::to_string),
                adds: false,
                except: Vec::new(),
            })
            .chain(DEFAULT_ADDS.iter().map(|(name, files, reaches, except)| {
                Input {
                    name: name.to_string(),
                    files: files.iter().map(|s| s.to_string()).collect(),
                    reaches: reaches
                        .iter()
                        .filter_map(|r| Target::parse(r).ok())
                        .collect(),
                    reach: Code,
                    extractor: None,
                    adds: true,
                    except: except.iter().map(|s| s.to_string()).collect(),
                }
            }))
            .chain(std::iter::once(Input {
                // C100 reads the ignore attributes and environment guards of every test source: a changed guard adds conformance,
                // the file keeps its owner.
                name: "test-guards".to_string(),
                files: vec!["*.rs".to_string(), "*.ts".to_string()],
                reaches: vec![Target::Project("conformance".to_string())],
                reach: Code,
                extractor: Some("guards".to_string()),
                adds: true,
                except: vec!["**/node_modules/**".to_string()],
            }))
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
                // Editor and VCS metadata at the root and the issue templates: nothing builds or tests them (the planner's former
                // `META`).
                owner(
                    &[
                        ".gitignore",
                        ".editorconfig",
                        ".DS_Store",
                        ".gitea/ISSUE_TEMPLATE/**",
                    ],
                    Owner::None,
                ),
            ],
            setup_only: DEFAULT_SETUP_ONLY.iter().map(|s| s.to_string()).collect(),
            release_actions: DEFAULT_RELEASE_ACTIONS
                .iter()
                .map(|s| s.to_string())
                .collect(),
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
        // A list in the file replaces the default list.
        if !file.setup_only.is_empty() {
            self.setup_only = file.setup_only;
        }
        if !file.release_actions.is_empty() {
            self.release_actions = file.release_actions;
        }
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
        // A .gitea file is reached through the CI adapter's nodes (WP-2), or, owned by none, fails closed.
        assert_eq!(all, ["rust-toolchain", "npm-root"]);
        assert!(
            i.inputs_of(".gitea/actions/README.md")
                .iter()
                .all(|x| x.adds),
            "G7: prose under .gitea is the doc owner's (C100 also reads a README)"
        );
        assert_eq!(
            i.inputs_of(".gitattributes")[0].extractor.as_deref(),
            Some("attributes")
        );
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
                extractor: None,
                adds: false,
                except: Vec::new(),
            }],
            owners: vec![OwnerRule {
                paths: vec!["spikes/**".into()],
                owner: Owner::Project("tools".into()),
            }],
            ..Inputs::default()
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
        for (name, _, reaches, _) in DEFAULT_ADDS {
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
