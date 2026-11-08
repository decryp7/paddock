//! C100 itself (ADR 0060 §4): every check against the CI model and the register, as a pure function of its inputs. Findings
//! name the file and line, the cause and the next step (ER-27); what the model cannot tell is "check broken" (ER-33).

use super::checks::{self, Check, Kind};
use super::ci::{self, Level, Sources};
use super::docs::{self, Known};
use super::gating::{self, AppFacts, Env, Gating};
use super::register::{self, Date, Entry, Gate, Register, Version};
use super::tree::{Tree, any_glob, glob_regex};
use std::collections::{BTreeMap, BTreeSet};

/// Everything C100 reads.
pub struct Input<'a> {
    /// The tracked files.
    pub tree: &'a dyn Tree,
    /// The CI files and contracts.
    pub sources: &'a Sources,
    /// `paddock-conformance list --json`, or why it could not be read.
    pub conformance_list: Result<String, String>,
    /// `paddock-conformance commands --json` (a JSON array of subcommand names), if readable.
    pub conformance_commands: Option<String>,
    /// The `./dev` command paths.
    pub dev_commands: BTreeSet<String>,
    /// `docs/gates.toml`, if it exists.
    pub register_text: Option<String>,
    /// The register at the base (`origin/main`), if known.
    pub base_register_text: Option<String>,
    /// The register's history, oldest first.
    pub history: Vec<Version>,
    /// Today.
    pub today: Date,
    /// A release tag about to be made (`debrief-v1.3.0`).
    pub release_tag: Option<String>,
    /// `--warn`: every finding stays, the exit is clean.
    pub warn: bool,
    /// The checkout is shallow: the register's history and the base are not all there (ER-33: not measured).
    pub shallow: bool,
}

/// One finding.
#[derive(Debug, Clone, PartialEq)]
pub struct Finding {
    /// The file.
    pub file: Option<String>,
    /// The line.
    pub line: Option<usize>,
    /// The check or entry it is about.
    pub id: String,
    /// What is wrong.
    pub message: String,
    /// The next step.
    pub fix: String,
}

impl Finding {
    /// The annotation line (ER-27).
    pub fn annotation(&self) -> String {
        let loc = match (&self.file, self.line) {
            (Some(f), Some(l)) => format!(" file={f},line={l}"),
            (Some(f), None) => format!(" file={f}"),
            _ => String::new(),
        };
        format!("::error{loc}::C100 {} {} Fix: {}", self.id, self.message, self.fix)
    }
}

/// How one check came out.
#[derive(Debug, Clone, PartialEq)]
pub struct Row {
    /// The check.
    pub check: Check,
    /// Its gating.
    pub gating: Gating,
    /// `gated`, `conditional`, `report-only`, `registered`, `measurement`, `harness`, `broken` or `not gated`.
    pub state: &'static str,
}

/// The result.
#[derive(Debug, Default)]
pub struct Outcome {
    /// Findings.
    pub findings: Vec<Finding>,
    /// One row per check.
    pub rows: Vec<Row>,
    /// Register problems, kept apart for the summary.
    pub register_errors: usize,
}

fn required(check: &Check, reg: &Register) -> Option<Level> {
    if let Some(e) = reg.level.iter().find(|e| e.id == check.id) {
        return match e.level.as_deref() {
            Some("branch") => Some(Level::Branch),
            Some("main") => Some(Level::Main),
            Some("nightly") => Some(Level::Nightly),
            Some("release") => Some(Level::Release),
            _ => None,
        };
    }
    match check.kind {
        Kind::Conformance | Kind::DevCheck | Kind::Script | Kind::NpmCheck => Some(Level::Branch),
        _ => None,
    }
}

fn levels(set: &BTreeSet<Level>) -> String {
    if set.is_empty() {
        "no level".into()
    } else {
        set.iter().map(|l| l.name()).collect::<Vec<_>>().join(", ")
    }
}

fn entry<'a>(list: &'a [Entry], check: &Check) -> Option<&'a Entry> {
    list.iter().find(|e| e.id == check.id)
}

const REGISTER: &str = "docs/gates.toml";

/// Runs C100.
pub fn evaluate(input: &Input) -> Outcome {
    let mut out = Outcome::default();
    let finding = |out: &mut Outcome, file: Option<&str>, line: Option<usize>, id: &str, message: String, fix: &str| {
        out.findings.push(Finding {
            file: file.map(str::to_string),
            line,
            id: id.to_string(),
            message,
            fix: fix.to_string(),
        });
    };

    // The register.
    let (reg, reg_errors) = match &input.register_text {
        Some(t) => register::parse(t),
        None => (Register::default(), vec![format!("{REGISTER} does not exist")]),
    };
    for e in &reg_errors {
        finding(
            &mut out,
            Some(REGISTER),
            None,
            "register",
            e.clone(),
            "Fix the register; it is documented in ADR 0060 section 3.",
        );
    }
    out.register_errors = reg_errors.len();
    let mut problems = register::check_dates(&reg, input.today);
    problems.extend(register::date_history(&input.history));
    if input.shallow {
        finding(
            &mut out,
            Some(REGISTER),
            None,
            "register",
            "not measured: the checkout is shallow, so the date history of the register and the ratchet base cannot be read".into(),
            "Check out with fetch-depth: 0 (git fetch --unshallow).",
        );
    }
    let base = input.base_register_text.as_deref().map(|t| register::parse(t).0);
    if !reg.ratchet.is_empty() && base.is_none() && !input.shallow {
        finding(
            &mut out,
            Some(REGISTER),
            None,
            "register",
            "not measured: the register at the base revision could not be read, so a ratchet list cannot be compared".into(),
            "Fetch the base (git fetch origin main), or pass --base <rev>.",
        );
    }
    problems.extend(register::check_ratchets(&reg, base.as_ref(), input.release_tag.as_deref()));
    for p in problems {
        finding(&mut out, Some(REGISTER), None, "register", p.message, &p.fix);
    }

    // The CI model.
    let mut src = input.sources.clone();
    src.vars = reg.vars.clone();
    let model = ci::build(&src);
    for (file, why) in &model.problems {
        finding(
            &mut out,
            Some(file),
            None,
            "ci-model",
            format!("check broken: the CI model cannot read {file}: {why}"),
            "Fix the file, or the reference to it; a CI file the model cannot read gates nothing.",
        );
    }
    let index = gating::index(&model, input.tree);
    let apps = AppFacts::from_contracts(&src.contracts);

    // The checks.
    let mut all: Vec<Check> = Vec::new();
    match &input.conformance_list {
        Ok(j) => match checks::conformance_checks(j) {
            Ok(c) => all.extend(c),
            Err(e) => finding(
                &mut out,
                Some("conformance/src/checks/index.mjs"),
                None,
                "conformance",
                format!("check broken: {e}"),
                "Declare scope and mode in the check module.",
            ),
        },
        Err(e) => finding(
            &mut out,
            None,
            None,
            "conformance",
            format!("check broken: the conformance checks could not be listed: {e}"),
            "Run `node conformance/bin/paddock-conformance.mjs list --json` and fix what it reports.",
        ),
    }
    all.extend(checks::dev_checks());
    all.extend(checks::script_checks(input.tree));
    all.extend(checks::npm_checks(input.tree));
    all.extend(checks::action_checks(&src.actions.keys().cloned().collect()));
    let callable: BTreeSet<String> = src
        .workflows
        .iter()
        .filter(|(_, t)| t.contains("workflow_call"))
        .map(|(p, _)| p.clone())
        .collect();
    all.extend(checks::workflow_checks(&callable));
    all.extend(checks::test_checks(input.tree));

    // Harness files.
    let mut harness_files: Vec<(String, String, String)> = Vec::new();
    for h in &reg.harness {
        let globs: Vec<_> = h.paths.iter().map(|g| glob_regex(g)).collect();
        for p in input.tree.files().iter().filter(|p| any_glob(&globs, p)) {
            if let Some(t) = input.tree.read(p) {
                harness_files.push((h.name.clone(), p.clone(), t));
            }
        }
    }
    let env = Env {
        model: &model,
        index: &index,
        tree: input.tree,
        apps: &apps,
        harness_files: &harness_files,
    };

    let ids: BTreeSet<String> = all.iter().map(|c| c.id.clone()).collect();
    let bench = ["docs/benchmarks.md"]
        .iter()
        .map(|s| s.to_string())
        .chain(input.tree.files().iter().filter(|f| f.ends_with("/docs/performance.md")).cloned())
        .filter_map(|p| input.tree.read(&p))
        .collect::<Vec<_>>()
        .join("\n");

    for check in all {
        let g = gating::gating(&check, &env);
        let want = required(&check, &reg);
        let ok = g.not_applicable
            || match want {
                Some(l) => g.gated.contains(&l),
                None => !g.gated.is_empty(),
            };
        let loc = check.file.clone();
        let line = Some(check.line);
        let label = format!("{} {}", check.kind.name(), check.id);
        let ng = entry(&reg.not_gated, &check);
        if ok && g.missing_apps.is_empty() {
            if let Some(e) = ng.filter(|_| {
                matches!(
                    check.kind,
                    Kind::Conformance | Kind::DevCheck | Kind::Script | Kind::NpmCheck | Kind::Action | Kind::Workflow
                )
            }) {
                finding(
                    &mut out,
                    Some(REGISTER),
                    None,
                    &check.id,
                    format!("[[not_gated]] {} is stale: the check is gated at {}", e.id, levels(&g.gated)),
                    "Remove the entry.",
                );
            }
            for e in reg.conditional.iter().chain(&reg.report_only).filter(|e| e.id == check.id) {
                finding(
                    &mut out,
                    Some(REGISTER),
                    None,
                    &check.id,
                    format!(
                        "the register entry for {} is stale: the check is gated at {}",
                        e.id,
                        levels(&g.gated)
                    ),
                    "Remove the entry.",
                );
            }
            let state = if g.not_applicable { "n/a" } else { "gated" };
            out.rows.push(Row {
                check: check.clone(),
                gating: g,
                state,
            });
            // fall through for tests: a registered gate that is no longer needed is checked below
            if !matches!(check.kind, Kind::IgnoredTest | Kind::EnvGatedTest) {
                continue;
            }
            let (file, name) = check.id.split_once("::").unwrap_or((&check.id, ""));
            if let Some(e) = reg
                .ignored
                .iter()
                .find(|e| e.path == file && (e.tests.iter().any(|t| t == name || t == "*")))
                && e.gate == Gate::NotGated
            {
                finding(
                    &mut out,
                    Some(REGISTER),
                    None,
                    &check.id,
                    format!("[[ignored]] {file} lists {name} as not gated, but CI runs it"),
                    "Remove the test from the entry.",
                );
            }
            continue;
        }

        // Not gated at the required level.
        let mut covered = false;
        let mut state = "not gated";
        if let Some(e) = ng
            && (g.missing_apps.is_empty() || (!e.apps.is_empty() && g.missing_apps.iter().all(|a| e.apps.contains(a))))
        {
            {
                let extra: Vec<&String> = e.apps.iter().filter(|a| !g.missing_apps.contains(a)).collect();
                if !extra.is_empty() {
                    finding(
                        &mut out,
                        Some(REGISTER),
                        None,
                        &check.id,
                        format!(
                            "[[not_gated]] {} lists {} which {} now gated",
                            e.id,
                            extra.iter().map(|s| s.as_str()).collect::<Vec<_>>().join(", "),
                            if extra.len() == 1 { "is" } else { "are" }
                        ),
                        "Remove those apps from the entry's apps list.",
                    );
                }
                covered = true;
                state = "registered";
            }
        }
        if !covered && !g.conditional.is_empty() && g.gated.is_empty() && entry(&reg.conditional, &check).is_some() {
            covered = true;
            state = "conditional";
        }
        if !covered && !g.report_only.is_empty() && g.gated.is_empty() && entry(&reg.report_only, &check).is_some() {
            covered = true;
            state = "report-only";
        }
        if matches!(check.kind, Kind::IgnoredTest | Kind::EnvGatedTest) {
            let (file, name) = check.id.split_once("::").unwrap_or((&check.id, ""));
            if let Some(e) = reg
                .ignored
                .iter()
                .find(|e| e.path == file && (e.tests.iter().any(|t| t == name) || e.tests.iter().any(|t| t == "*")))
            {
                match &e.gate {
                    Gate::NotGated => {
                        covered = true;
                        state = "registered";
                    }
                    Gate::Measurement => {
                        if bench.contains(file) || (!name.is_empty() && bench.contains(name)) {
                            covered = true;
                            state = "measurement";
                        } else {
                            finding(
                                &mut out,
                                Some(REGISTER),
                                None,
                                &check.id,
                                format!(
                                    "[[ignored]] {file}: {name} is registered as a measurement, but no results block in docs/benchmarks.md or an app's docs/performance.md names it"
                                ),
                                "Record the measurement (rule 48), or register the test as not-gated with an owner.",
                            );
                            covered = true;
                            state = "broken";
                        }
                    }
                    Gate::Job(j) => {
                        finding(
                            &mut out,
                            Some(REGISTER),
                            None,
                            &check.id,
                            format!(
                                "[[ignored]] {file}: {name} claims job:{j}, but no resolved CI line runs it (gated: {})",
                                levels(&g.gated)
                            ),
                            "Fix the entry or add the CI line that runs the test.",
                        );
                        covered = true;
                        state = "broken";
                    }
                    Gate::Harness(h) => {
                        if g.harness.contains(h) {
                            covered = true;
                            state = "harness";
                        } else {
                            finding(
                                &mut out,
                                Some(REGISTER),
                                None,
                                &check.id,
                                format!(
                                    "[[ignored]] {file}: {name} claims harness:{h}, but no file of that harness runs it with a cargo or nextest line that selects it"
                                ),
                                "Register the test as not-gated with an owner, or fix the harness.",
                            );
                            covered = true;
                            state = "broken";
                        }
                    }
                    Gate::Tool(t) => {
                        if input
                            .tree
                            .read(t)
                            .is_some_and(|x| gating::tool_runs(&x, file, name, check.kind == Kind::IgnoredTest))
                        {
                            covered = true;
                            state = "harness";
                        } else {
                            finding(
                                &mut out,
                                Some(REGISTER),
                                None,
                                &check.id,
                                format!("[[ignored]] {file}: {name} claims tool:{t}, but that file does not run it"),
                                "Fix the entry.",
                            );
                            covered = true;
                            state = "broken";
                        }
                    }
                }
            }
        }
        if !covered {
            let broken = !g.unresolved.is_empty();
            if broken {
                state = "broken";
                finding(
                    &mut out,
                    loc.as_deref(),
                    line,
                    &label,
                    format!(
                        "check broken: the CI model cannot resolve {} (a line that would run it), so it cannot say the check is gated",
                        g.unresolved.join(", ")
                    ),
                    "Make the line resolvable (a literal command, a declared input), or register the check under [[not_gated]] in docs/gates.toml with a reason, owner and review_by.",
                );
            } else {
                let how = if !g.missing_apps.is_empty() {
                    format!(
                        "is not gated for {} (no reachable conformance line names it)",
                        g.missing_apps.join(", ")
                    )
                } else if !g.gated.is_empty() {
                    format!(
                        "is gated only at {} but needs {}",
                        levels(&g.gated),
                        want.map_or("any level", |l| l.name())
                    )
                } else if !g.conditional.is_empty() {
                    format!(
                        "runs only under a condition the model cannot decide ({}), so it is not a gate on branches",
                        levels(&g.conditional)
                    )
                } else if !g.report_only.is_empty() {
                    format!("runs only as report-only at {} (it cannot fail the job)", levels(&g.report_only))
                } else {
                    "is run by no reachable, resolved CI line".to_string()
                };
                finding(
                    &mut out,
                    loc.as_deref(),
                    line,
                    &label,
                    how,
                    "Add the CI line that runs it, or register it under the right [[...]] table of docs/gates.toml with a reason, owner and review_by.",
                );
                state = if !g.conditional.is_empty() {
                    "conditional"
                } else if !g.report_only.is_empty() {
                    "report-only"
                } else {
                    "not gated"
                };
            }
        }
        out.rows.push(Row { check, gating: g, state });
    }

    // Stale entries.
    for e in &reg.not_gated {
        if !ids.contains(&e.id) {
            finding(
                &mut out,
                Some(REGISTER),
                None,
                &e.id,
                "[[not_gated]] names a check that no longer exists".into(),
                "Remove the entry.",
            );
        }
    }
    for (kind, list) in [
        ("conditional", &reg.conditional),
        ("report_only", &reg.report_only),
        ("level", &reg.level),
    ] {
        for e in list.iter().filter(|e| !ids.contains(&e.id)) {
            finding(
                &mut out,
                Some(REGISTER),
                None,
                &e.id,
                format!("[[{kind}]] names a check that no longer exists"),
                "Remove the entry.",
            );
        }
    }
    for p in &reg.planned {
        if ids.contains(&p.id) {
            finding(
                &mut out,
                Some(REGISTER),
                None,
                &p.id,
                format!("[[planned]] {} is stale: the check exists now", p.id),
                "Remove the entry.",
            );
        }
    }
    let present: BTreeSet<(String, String)> = out
        .rows
        .iter()
        .filter_map(|r| r.check.id.split_once("::").map(|(a, b)| (a.to_string(), b.to_string())))
        .collect();
    for e in &reg.ignored {
        for t in &e.tests {
            let found = if t == "*" {
                present.iter().any(|(f, _)| *f == e.path)
            } else {
                present.contains(&(e.path.clone(), t.clone()))
            };
            if !found {
                finding(
                    &mut out,
                    Some(REGISTER),
                    None,
                    &e.path,
                    format!("[[ignored]] lists {t}, which is no longer an ignored or env-gated test in this file"),
                    "Remove it from the entry.",
                );
            }
        }
    }

    // Docs (§5).
    let conformance_commands: Option<BTreeSet<String>> = input
        .conformance_commands
        .as_deref()
        .and_then(|j| serde_json::from_str::<Vec<String>>(j).ok())
        .map(|v| v.into_iter().collect());
    let known = Known {
        tree: input.tree,
        check_ids: ids.iter().filter(|i| !i.contains(['/', ':'])).cloned().collect(),
        allowed_ids: reg
            .planned
            .iter()
            .map(|p| p.id.clone())
            .chain(reg.retired.iter().map(|r| r.id.clone()))
            .collect(),
        conformance_commands,
        dev_commands: input.dev_commands.clone(),
    };
    for p in docs::check_docs(input.tree, &reg, &known, &docs::default_kinds()) {
        finding(&mut out, Some(&p.path), Some(p.line), "docs", p.message, &p.fix);
    }
    out
}

/// The summary table: per kind, how many checks are in each state.
pub fn summary(rows: &[Row]) -> String {
    let mut by: BTreeMap<&str, BTreeMap<&str, usize>> = BTreeMap::new();
    for r in rows {
        *by.entry(r.check.kind.name()).or_default().entry(r.state).or_default() += 1;
    }
    let states = [
        "gated",
        "n/a",
        "conditional",
        "report-only",
        "registered",
        "measurement",
        "harness",
        "broken",
        "not gated",
    ];
    let mut s = format!(
        "| kind | total | {} |\n|---|---|{}|\n",
        states.join(" | "),
        "---|".repeat(states.len())
    );
    for (k, m) in &by {
        let total: usize = m.values().sum();
        s.push_str(&format!(
            "| {k} | {total} | {} |\n",
            states
                .iter()
                .map(|st| m.get(st).copied().unwrap_or(0).to_string())
                .collect::<Vec<_>>()
                .join(" | ")
        ));
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::gates::tree::MemTree;

    const LIST: &str = r#"[{"id":"C06","scope":"root","mode":"gate","needs":"all","file":"conformance/src/checks/C06.mjs"},{"id":"C13","scope":"apps","mode":"gate","needs":"rust","file":"conformance/src/checks/C13.mjs"}]"#;
    const CI: &str = "on:\n  pull_request:\njobs:\n  c:\n    runs-on: ubuntu-latest\n    steps:\n      - run: node conformance/bin/paddock-conformance.mjs --app paddock --root . --only C06\n      - run: ./dev gates check\n";
    const REG_HEAD: &str = "version = 1\n[docs]\ninclude = [\"AGENTS.md\"]\nexclude = []\n";

    fn run(register: Option<&str>, files: &[(&str, &str)], contracts: &[(&str, &str)]) -> Outcome {
        let tree = MemTree::new(files);
        let mut sources = Sources::default();
        sources.workflows.insert(".gitea/workflows/ci.yml".into(), CI.into());
        for (a, t) in contracts {
            sources.contracts.insert(a.to_string(), t.to_string());
        }
        evaluate(&Input {
            tree: &tree,
            sources: &sources,
            conformance_list: Ok(LIST.into()),
            conformance_commands: Some(r#"["list","commands"]"#.into()),
            dev_commands: ["gates", "gates check"].iter().map(|s| s.to_string()).collect(),
            register_text: register.map(str::to_string),
            base_register_text: None,
            history: Vec::new(),
            today: Date::parse("2026-11-10").unwrap(),
            release_tag: None,
            warn: false,
            shallow: false,
        })
    }

    fn messages(o: &Outcome) -> Vec<String> {
        o.findings.iter().map(|f| format!("{} {}", f.id, f.message)).collect()
    }

    const RUST_APP: &str = r#"{"cargo": "."}"#;

    #[test]
    fn an_ungated_check_is_a_finding_and_a_registered_one_is_not() {
        let o = run(Some(REG_HEAD), &[], &[("dyno", RUST_APP)]);
        let m = messages(&o);
        assert_eq!(m.len(), 1, "{m:?}");
        assert!(m[0].starts_with("conformance C13 is not gated for dyno"), "{m:?}");
        assert!(
            o.findings[0]
                .annotation()
                .starts_with("::error file=conformance/src/checks/C13.mjs,line=1::C100 conformance C13 ")
        );
        assert!(o.findings[0].annotation().contains(" Fix: "));
        // registered: no finding
        let reg = format!(
            "{REG_HEAD}\n[[not_gated]]\nid = \"C13\"\napps = [\"dyno\"]\nreason = \"x\"\nowner = \"o\"\nsince = 2026-11-08\nreview_by = 2026-12-06\napproved = \"coordinator\"\n"
        );
        assert!(run(Some(&reg), &[], &[("dyno", RUST_APP)]).findings.is_empty());
    }

    #[test]
    fn the_register_cannot_go_stale() {
        // the entry still lists an app that is no longer a gap (pitcrew has no Rust, so it is not applicable)
        let reg = format!(
            "{REG_HEAD}\n[[not_gated]]\nid = \"C13\"\napps = [\"dyno\", \"pitcrew\"]\nreason = \"x\"\nowner = \"o\"\nsince = 2026-11-08\nreview_by = 2026-12-06\napproved = \"coordinator\"\n"
        );
        let m = messages(&run(Some(&reg), &[], &[("dyno", RUST_APP), ("pitcrew", "{}")]));
        assert!(m.iter().any(|x| x.contains("lists pitcrew which is now gated")), "{m:?}");
        // an entry for a check that is gated, and one that does not exist
        let reg = format!(
            "{REG_HEAD}\n[[not_gated]]\nid = \"C06\"\nreason = \"x\"\nowner = \"o\"\nsince = 2026-11-08\nreview_by = 2026-12-06\napproved = \"coordinator\"\n\n[[not_gated]]\nid = \"C99\"\nreason = \"x\"\nowner = \"o\"\nsince = 2026-11-08\nreview_by = 2026-12-06\napproved = \"coordinator\"\n"
        );
        let m = messages(&run(Some(&reg), &[], &[]));
        assert!(m.iter().any(|x| x.contains("C06") && x.contains("stale")), "{m:?}");
        assert!(m.iter().any(|x| x.contains("C99") && x.contains("no longer exists")), "{m:?}");
        // an expired entry
        let reg = format!(
            "{REG_HEAD}\n[[not_gated]]\nid = \"C13\"\nreason = \"x\"\nowner = \"o\"\nsince = 2026-10-01\nreview_by = 2026-10-20\napproved = \"coordinator\"\n"
        );
        let m = messages(&run(Some(&reg), &[], &[]));
        assert!(m.iter().any(|x| x.contains("has passed")), "{m:?}");
    }

    #[test]
    fn a_missing_or_broken_register_is_a_finding() {
        assert!(
            messages(&run(None, &[], &[]))
                .iter()
                .any(|m| m.contains("docs/gates.toml does not exist"))
        );
        assert!(
            messages(&run(Some("version = 1\n[[not_gated]]\nid = \"C13\"\n"), &[], &[]))
                .iter()
                .any(|m| m.contains("lacks"))
        );
    }

    #[test]
    fn ignored_tests_are_registered_by_file_with_a_gate() {
        let files = [
            ("crates/a/Cargo.toml", "[workspace]\n"),
            (
                "crates/a/tests/it.rs",
                "#[test]\n#[ignore = \"vm\"]\nfn vm_one() {}\n#[test]\n#[ignore]\nfn bench_one() {}\n",
            ),
        ];
        // bad: no entry
        let m = messages(&run(Some(REG_HEAD), &files, &[]));
        assert_eq!(m.iter().filter(|x| x.starts_with("ignored-test")).count(), 2, "{m:?}");
        // good: one entry not-gated, one measurement that a results block records
        let reg = format!(
            "{REG_HEAD}\n[[ignored]]\npath = \"crates/a/tests/it.rs\"\ntests = [\"vm_one\"]\ngate = \"not-gated\"\nreason = \"vm\"\nowner = \"o\"\nsince = 2026-11-08\nreview_by = 2026-12-06\napproved = \"coordinator\"\n\n[[ignored]]\npath = \"crates/a/tests/it.rs\"\ntests = [\"bench_one\"]\ngate = \"measurement\"\nreason = \"bench\"\nowner = \"o\"\nsince = 2026-11-08\nreview_by = 2026-12-06\napproved = \"coordinator\"\n"
        );
        let mut with_bench = files.to_vec();
        with_bench.push(("docs/benchmarks.md", "results: crates/a/tests/it.rs bench_one 12 ms"));
        assert!(run(Some(&reg), &with_bench, &[]).findings.is_empty());
        // bad: the measurement nobody records
        let m = messages(&run(Some(&reg), &files, &[]));
        assert!(m.iter().any(|x| x.contains("registered as a measurement")), "{m:?}");
        // bad: an entry for a test that is gone
        let gone = reg.replace("vm_one", "vm_gone");
        let m = messages(&run(Some(&gone), &with_bench, &[]));
        assert!(m.iter().any(|x| x.contains("vm_gone")), "{m:?}");
    }

    #[test]
    fn docs_naming_a_missing_check_fail_unless_planned() {
        let files = [("AGENTS.md", "register check C58 and C06\n")];
        let m = messages(&run(Some(REG_HEAD), &files, &[]));
        assert!(m.iter().any(|x| x.starts_with("docs names check C58")), "{m:?}");
        let reg = format!(
            "{REG_HEAD}\n[[planned]]\nid = \"C58\"\nadr = \"0034\"\nowner = \"o\"\nsince = 2026-11-08\nreview_by = 2026-12-06\napproved = \"coordinator\"\n"
        );
        assert!(run(Some(&reg), &files, &[]).findings.is_empty());
    }

    #[test]
    fn what_cannot_be_measured_is_reported_not_passed() {
        // a ratchet with no base to compare against, and a shallow checkout
        let reg = format!("{REG_HEAD}\n[[ratchet]]\nid = \"C13\"\napp = \"debrief\"\nnot_yet = [\"x\"]\nowner = \"o\"\ntarget = \"debrief-v1.3.0\"\napproved = \"user\"\n");
        let m = messages(&run(Some(&reg), &[], &[]));
        assert!(m.iter().any(|x| x.contains("not measured: the register at the base revision")), "{m:?}");
    }

    #[test]
    fn the_summary_counts_every_kind() {
        let o = run(Some(REG_HEAD), &[], &[]);
        let s = summary(&o.rows);
        assert!(s.contains("| conformance | 2 |"), "{s}");
        assert!(s.contains("| dev-check | 1 | 1 |"), "{s}");
    }
}
