//! Test data: the inputs the crate's tests run on, a copy of what `docs/repo-graph.toml` declared when the defaults moved out of
//! the crate (ADR 0117 §14: the model's code holds no path). The repository's own declarations are tested against the real file
//! in paddock-dev (`repo_model` tests through `repo_inputs::load`).
#![allow(dead_code)]

use super::Reach;
use super::inputs::{Input, Inputs, Owner, OwnerRule, Paths, Prose, Target};

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
            "docs/pictures/src/**",
            "docs/mocks/**",
            "**/*.drawio",
            "docs/**/svg/*.svg",
            "diagrams/**/svg/*.svg",
            "docs/pictures/*.svg",
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
    // The CI files no workflow or action owns: the label set (pr/labels.rs reads it) and the workflow templates (the bench
    // template, read by paddock-bench and the CI-actions tests).
    (
        "gitea-labels",
        &["/.gitea/labels.toml"],
        &["project:paddock-dev"],
        None,
    ),
    (
        "workflow-templates",
        &[".gitea/workflow-templates/**"],
        &["project:conformance", "project:crates"],
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

/// The actions ADR 0117 §5 names setup-only: the Rust setup, the job split and the cache actions.
const DEFAULT_SETUP_ONLY: [&str; 6] = [
    "setup-rust",
    "build-jobs",
    "node-caches",
    "dev-tool-cache",
    "sccache-stats",
    "cache-report",
];

/// The actions that bundle, sign or publish (ADR 0117 §5).
const DEFAULT_RELEASE_ACTIONS: [&str; 5] = [
    "tauri-bundle",
    "publish-release",
    "macos-verify",
    "keychain",
    "strip-signing-keys",
];

/// The fixture inputs.
pub fn inputs() -> Inputs {
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
        gate_phases: vec!["checks".to_string()],
        hash: Default::default(),
        prose: Prose {
            project_names: ["README*", "CHANGELOG*", "LICENSE*"]
                .map(String::from)
                .to_vec(),
            project_folders: ["docs"].map(String::from).to_vec(),
            doc_owner: ["*.md", "*.txt", "LICENSE*"].map(String::from).to_vec(),
            doc_assets: ["*.png", "*.jpg", "*.jpeg", "*.gif", "*.webp", "*.svg"]
                .map(String::from)
                .to_vec(),
        },
        projects: Vec::new(),
        paths: Paths::default(),
    }
}
