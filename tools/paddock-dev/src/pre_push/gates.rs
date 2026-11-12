//! The gates of one push, as data: which of ER-9's fast gates the change reaches (the planner's answer) and the exact command of each.
//! Pure: no process is started and no file is read here, so every verdict is tested with a plan and a list of files.
//!
//! Extension point for the per-commit gate (`./dev check --commits`, ADR 0038's 2026-11-08 amendment, X-3 in
//! docs/plans/ci-in-rust.md; it needs the user's yes and is not built): it calls [`plan_gates`] once per commit with that commit's
//! files and no ranges, in a worktree of its own. Nothing in this file depends on the push as a whole.

use serde_json::Value;
use std::collections::BTreeSet;

/// What a gate needs on the machine before it can say anything.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Need {
    /// Nothing beyond the program itself (checked when it starts).
    Nothing,
    /// A program on PATH (`cargo-xwin`, `cargo-deny`): without it the gate is "not measured", never a pass (ER-33).
    Tool(String),
    /// A running Docker daemon (the Linux Clippy): an agent never starts one (ER-14).
    Docker,
}

/// One command of the push gate.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Gate {
    /// The row of the table: `fmt crates`, `clippy-windows apps/dyno`, `conformance`.
    pub id: String,
    /// The folder it runs in, relative to the repository root (empty: the root).
    pub cwd: String,
    /// The program and its arguments, exactly as CI runs them.
    pub program: String,
    /// The arguments.
    pub args: Vec<String>,
    /// Variables this gate alone sets.
    pub env: Vec<(String, String)>,
    /// Runs through `heavy` (ER-24) with a core cap: compiles and lints.
    pub heavy: bool,
    /// What must exist before it can run.
    pub need: Need,
    /// Cheap gates first (0), compiles later: the common failures surface in seconds.
    pub rank: u8,
    /// What to do when it fails, in one clause.
    pub next: String,
}

impl Gate {
    /// The command line a person re-runs, with its folder.
    pub fn rerun(&self) -> String {
        let line = std::iter::once(self.program.as_str())
            .chain(self.args.iter().map(String::as_str))
            .collect::<Vec<_>>()
            .join(" ");
        if self.cwd.is_empty() {
            line
        } else {
            format!("cd {} && {line}", self.cwd)
        }
    }
}

/// A gate the change reaches, or the reason it does not, or a check that cannot be trusted.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Planned {
    /// Run it.
    Run(Gate),
    /// Not reached by this change, with the reason (printed in the table).
    Skip {
        /// The row.
        id: String,
        /// Why.
        reason: String,
    },
    /// The check itself is broken (its input cannot be read): the push does not pass (ER-33).
    Broken {
        /// The row.
        id: String,
        /// What is wrong.
        cause: String,
        /// What to do.
        next: String,
    },
}

/// What the plan alone cannot say; read from the repository once by the caller.
#[derive(Debug, Clone, Default)]
pub struct Facts {
    /// The conformance command's arguments after `node`, read from ci.yml's one line, or why that failed.
    pub conformance: Option<Result<Vec<String>, String>>,
    /// Workspace folders whose CI lints Windows too (`apps/<app>`, from `windowsClippy`); `crates` always does.
    pub windows_clippy: BTreeSet<String>,
    /// The CI image reference (from `ci-image/VERSION`), or why it cannot be read.
    pub image: Option<Result<String, String>>,
    /// Pitwall's script tests (`scripts/ci/*.test.mjs`, relative to `apps/pitwall`): no shell expands a glob here.
    pub pitwall_tests: Vec<String>,
    /// This program, for the checks `./dev` runs itself.
    pub dev_exe: String,
    /// Run the Linux Clippy in the CI image too (`--linux`): off by default, CI covers it (ER-9's overlap model) and in the
    /// measured run it did not fit the budget.
    pub linux: bool,
    /// A kept build folder per workspace (`<it>/<workspace>`), used when the caller sets no `CARGO_TARGET_DIR`: the throw-away
    /// worktree of a pushed commit must not hold the builds (they would start cold every run).
    pub kept_targets: Option<String>,
    /// The caller's `CARGO_TARGET_DIR`: the Windows lint gets `<it>/xwin` (ADR 0021 §3).
    pub target_dir: Option<String>,
}

/// The push: its changed files and the commits to lint.
#[derive(Debug, Clone, Default)]
pub struct Change {
    /// Repository-relative files: committed since the merge base, modified and untracked.
    pub files: Vec<String>,
    /// The commit ranges to lint (`<merge base>..<pushed commit>`, one per pushed ref); none when there is no commit of its own.
    pub ranges: Vec<String>,
}

/// Workspaces whose lints are not a push gate: they have a CI job of their own that is not part of a branch run.
const OWN_JOB: &[&str] = &["/fuzz", "/templates/"];

fn gate(id: String, cwd: &str, program: &str, args: &[&str], rank: u8, next: String) -> Gate {
    Gate {
        id,
        cwd: cwd.to_string(),
        program: program.to_string(),
        args: args.iter().map(|a| a.to_string()).collect(),
        env: Vec::new(),
        heavy: false,
        need: Need::Nothing,
        rank,
        next,
    }
}

/// A workspace folder as one folder name: `apps/dyno` is `apps-dyno`.
fn slug(ws: &str) -> String {
    ws.replace('/', "-")
}

fn strs(v: &[&str]) -> Vec<String> {
    v.iter().map(|s| s.to_string()).collect()
}

/// The affected Cargo workspaces, in the planner's order.
pub fn workspaces(plan: &Value) -> Vec<String> {
    plan["packages"]
        .as_object()
        .map(|o| o.keys().filter(|k| !OWN_JOB.iter().any(|s| k.contains(s))).cloned().collect())
        .unwrap_or_default()
}

/// The workspaces that hold a changed file themselves (the longest workspace folder that is a prefix of the file): the ones whose
/// own Clippy and format the change can break. The others that the plan reaches are dependents, which CI lints.
pub fn own_workspaces(plan: &Value, files: &[String]) -> std::collections::BTreeSet<String> {
    let all = workspaces(plan);
    files
        .iter()
        .filter_map(|f| {
            all.iter()
                .filter(|w| f.starts_with(&format!("{w}/")))
                .max_by_key(|w| w.len())
                .cloned()
        })
        .collect()
}

/// The gates `change` reaches under `plan`, in table order. Each is CI's own command (the one source is named in `next` or the
/// doc of the check that reads it).
pub fn plan_gates(plan: &Value, change: &Change, facts: &Facts) -> Vec<Planned> {
    let mut out = Vec::new();
    let own = own_workspaces(plan, &change.files);
    let mut dependents = Vec::new();
    for ws in workspaces(plan) {
        let all_features = ws == "crates";
        out.push(Planned::Run(gate(
            format!("lockfile {ws}"),
            &ws,
            "cargo",
            &["metadata", "--locked", "--format-version", "1"],
            1,
            format!("Cargo.lock in {ws} is out of date: run `cargo metadata` there without --locked, review the lock change and commit it"),
        )));
        if !own.contains(&ws) {
            dependents.push(ws);
            continue;
        }
        out.push(Planned::Run(gate(
            format!("fmt {ws}"),
            &ws,
            "cargo",
            &["fmt", "--all", "--check"],
            0,
            "format your own files (`rustfmt --edition 2024 <file>`; never `cargo fmt --all` while others edit), then re-run `./dev check --pre-push`".to_string(),
        )));
        let mut clippy: Vec<&str> = vec!["clippy", "--workspace", "--all-targets"];
        if all_features {
            clippy.push("--all-features");
        }
        clippy.extend(["--locked", "--", "-D", "warnings"]);
        let mut g = gate(
            format!("clippy {ws}"),
            &ws,
            "cargo",
            &clippy,
            2,
            "fix the lints it printed (a Clippy that needs `--fix` runs on your own files only)".into(),
        );
        g.heavy = true;
        if let (None, Some(k)) = (&facts.target_dir, &facts.kept_targets) {
            g.env
                .push(("CARGO_TARGET_DIR".into(), format!("{}/{}", k.trim_end_matches('/'), slug(&ws))));
        }
        out.push(Planned::Run(g));
        if all_features || facts.windows_clippy.contains(&ws) {
            let mut args: Vec<&str> = vec![
                "xwin",
                "clippy",
                "--target",
                "x86_64-pc-windows-msvc",
                "--workspace",
                "--all-targets",
            ];
            if all_features {
                args.push("--all-features");
            }
            args.extend(["--locked", "--", "-D", "warnings"]);
            let mut g = gate(
                format!("clippy-windows {ws}"),
                &ws,
                "cargo",
                &args,
                3,
                "an import or helper used only by a unix-gated test is dead code on Windows (lessons.md, \"Windows-only assumptions in tests\"): gate it with cfg too".into(),
            );
            g.heavy = true;
            g.need = Need::Tool("cargo-xwin".into());
            match (&facts.target_dir, &facts.kept_targets) {
                (Some(dir), _) => g
                    .env
                    .push(("CARGO_TARGET_DIR".into(), format!("{}/xwin", dir.trim_end_matches('/')))),
                (None, Some(k)) => g
                    .env
                    .push(("CARGO_TARGET_DIR".into(), format!("{}/{}/xwin", k.trim_end_matches('/'), slug(&ws)))),
                (None, None) => {}
            }
            out.push(Planned::Run(g));
        } else {
            out.push(Planned::Skip {
                id: format!("clippy-windows {ws}"),
                reason: "CI lints this workspace on the host OSes only (no windowsClippy in its paddock.ci.json)".into(),
            });
        }
    }
    if workspaces(plan).is_empty() {
        out.push(Planned::Skip {
            id: "fmt, clippy, lockfiles".into(),
            reason: "the change reaches no Cargo workspace".into(),
        });
    }
    if !dependents.is_empty() {
        out.push(Planned::Skip {
            id: "clippy dependents".into(),
            reason: format!(
                "{} workspace(s) reached through dependencies, none with a changed file of its own ({}): CI covers their Clippy (ER-9 overlap)",
                dependents.len(),
                dependents.join(", ")
            ),
        });
    }
    out.push(linux_clippy(change, facts));
    out.push(conformance(plan, facts));
    out.extend(commit_lint(change));
    if plan["apps"].as_array().is_some_and(|a| a.iter().any(|x| x == "pitwall")) {
        let mut args = vec!["--test".to_string()];
        args.extend(facts.pitwall_tests.iter().cloned());
        let mut g = gate(
            "pitwall-scripts".into(),
            "apps/pitwall",
            "node",
            &[],
            0,
            "run `cd apps/pitwall && node --test scripts/ci/*.test.mjs` and fix the test it names".into(),
        );
        g.args = args;
        out.push(if facts.pitwall_tests.is_empty() {
            Planned::Broken {
                id: "pitwall-scripts".into(),
                cause: "apps/pitwall/scripts/ci has no *.test.mjs file".into(),
                next: "restore the script tests or remove this gate from pre_push/gates.rs".into(),
            }
        } else {
            Planned::Run(g)
        });
        let mut d = gate(
            "pitwall-deny".into(),
            "apps/pitwall",
            "cargo",
            &["deny", "--workspace", "check"],
            1,
            "`cargo deny --workspace check` in apps/pitwall names the advisory or licence; fix or record it in deny.toml".into(),
        );
        d.need = Need::Tool("cargo-deny".into());
        out.push(Planned::Run(d));
    } else {
        out.push(Planned::Skip {
            id: "pitwall-scripts, pitwall-deny".into(),
            reason: "the change does not reach Pitwall".into(),
        });
    }
    if change.files.iter().any(|f| f.ends_with(".rs") || f == "docs/ipc-endpoints.toml") {
        out.push(Planned::Run(Gate {
            args: strs(&["check", "ipc-endpoints"]),
            ..gate(
                "ipc-endpoints".into(),
                "",
                &facts.dev_exe,
                &[],
                0,
                "add the listen site to docs/ipc-endpoints.toml with an ending test or a reason (C102)".into(),
            )
        }));
    } else {
        out.push(Planned::Skip {
            id: "ipc-endpoints".into(),
            reason: "no Rust file or docs/ipc-endpoints.toml changed".into(),
        });
    }
    // Dyno's wire fixtures (the UI contract) are regenerated from the Rust types when dyno-core's sources changed; a stale one
    // fails the push with the files named (apps/dyno/scripts/regen-wire-fixtures.sh runs cargo test).
    if change
        .files
        .iter()
        .any(|f| f.starts_with("apps/dyno/crates/dyno-core/src/") && f.ends_with(".rs"))
    {
        out.push(Planned::Run(Gate {
            heavy: true,
            ..gate(
                "dyno-wire-fixtures".into(),
                "",
                "sh",
                &["apps/dyno/scripts/regen-wire-fixtures.sh"],
                1,
                "update the *.contract.spec.ts the script names and commit the regenerated *.wire.json with it".into(),
            )
        }));
    } else {
        out.push(Planned::Skip {
            id: "dyno-wire-fixtures".into(),
            reason: "no file under apps/dyno/crates/dyno-core/src changed".into(),
        });
    }
    out
}

fn linux_clippy(change: &Change, facts: &Facts) -> Planned {
    let id = "clippy-linux crates".to_string();
    if !facts.linux {
        return Planned::Skip {
            id,
            reason: "CI covers it (ER-9 overlap); `--linux` runs it here, in the CI image".into(),
        };
    }
    let reaches = |f: &String| (f.starts_with("crates/") || f.starts_with(".cargo/")) && !docs_only(f);
    if !change.files.iter().any(reaches) {
        return Planned::Skip {
            id,
            reason: "nothing but documentation, or nothing at all, changed under crates/ or .cargo/".into(),
        };
    }
    let image = match &facts.image {
        Some(Ok(i)) => i.clone(),
        Some(Err(e)) => {
            return Planned::Broken {
                id,
                cause: e.clone(),
                next: "run from a full checkout (ci-image/VERSION is needed)".into(),
            };
        }
        None => {
            return Planned::Broken {
                id,
                cause: "the CI image was not looked up".into(),
                next: "report this: the caller of plan_gates must fill Facts::image".into(),
            };
        }
    };
    let command = "rustc --version && cargo clippy --version && cd crates && cargo clippy --workspace --all-targets --all-features --locked -- -D warnings";
    let mut g = gate(
        id,
        "",
        "bash",
        &[
            "tools/linux-container.sh",
            "--id",
            "paddock",
            "--image",
            &image,
            "--platform",
            "linux/amd64",
            "--",
            "sh",
            "-c",
            command,
        ],
        4,
        "a lint that exists only on Linux (a `mode_t` width, dead code behind a cfg): fix it, or run the same container command above"
            .into(),
    );
    g.heavy = true;
    g.need = Need::Docker;
    Planned::Run(g)
}

fn conformance(plan: &Value, facts: &Facts) -> Planned {
    let id = "conformance".to_string();
    if plan["conformance"] != true {
        return Planned::Skip {
            id,
            reason: "the plan does not reach the conformance job".into(),
        };
    }
    match &facts.conformance {
        Some(Ok(args)) => Planned::Run(Gate {
            args: args.clone(),
            ..gate(id, "", "node", &[], 0, "fix the finding it printed; the list of checks is ci.yml's own (\"Paddock itself\" step)".into())
        }),
        Some(Err(e)) => Planned::Broken { id, cause: e.clone(), next: "keep exactly one `node conformance/bin/paddock-conformance.mjs --app paddock ... --only ...` line in .gitea/workflows/ci.yml".into() },
        None => Planned::Broken { id, cause: "ci.yml was not read".into(), next: "report this: the caller of plan_gates must fill Facts::conformance".into() },
    }
}

fn commit_lint(change: &Change) -> Vec<Planned> {
    if change.ranges.is_empty() {
        return vec![Planned::Skip {
            id: "commit-lint".into(),
            reason: "no commit of its own since the base".into(),
        }];
    }
    change
        .ranges
        .iter()
        .map(|range| {
            let id = if change.ranges.len() == 1 {
                "commit-lint".to_string()
            } else {
                format!("commit-lint {range}")
            };
            Planned::Run(gate(
                id,
                "",
                "node",
                &["conformance/bin/paddock-conformance.mjs", "commit-lint", "--range", range],
                0,
                "reword the commit it names (a Conventional Commit subject of at most 100 characters; no trailers the rules forbid)".into(),
            ))
        })
        .collect()
}

/// A file that cannot change what Clippy says: documentation next to the code.
fn docs_only(path: &str) -> bool {
    [".md", ".txt", ".adoc"].iter().any(|e| path.ends_with(e))
}

/// What CI runs on a branch that the push gate does not, with the reason; printed with every run (ADR 0038 §3: the gate prints
/// the difference). Each line is a gap to close by a work package or a decision, not a promise that CI will not find it.
pub const NOT_RUN: &[(&str, &str)] = &[
    (
        "cargo hakari verify (workspace hack)",
        "needs the CI image's pinned cargo-hakari; WP-3 moves it into the contract",
    ),
    (
        "cargo machete (unused dependencies)",
        "needs the CI image's pinned cargo-machete; WP-3",
    ),
    (
        "cargo deny outside Pitwall",
        "each workspace's deny.toml is checked by CI only; WP-3",
    ),
    ("wiki lint (tools/wiki)", "runs in CI's paddock-dev job against the ER text"),
    (
        "lint and type-check of touched TypeScript (eslint, tsc)",
        "per-project commands live in YAML until WP-3's contracts",
    ),
    (
        "Linux Clippy of the shared crates in the CI image",
        "CI covers it; `./dev check --pre-push --linux` runs it here (ER-9 overlap, 2026-11-12)",
    ),
    ("Linux Clippy of the apps' workspaces", "the apps' Linux legs are CI's"),
    (
        "Clippy and the Windows lint of workspaces reached only through dependencies",
        "CI covers them (ER-9 overlap); only workspaces with a changed file of their own are linted here",
    ),
    (
        "tests (nextest, vitest, Playwright)",
        "slower local checks run beside CI after dispatch (ER-9), not in the push gate",
    ),
];
