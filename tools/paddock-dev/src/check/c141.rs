//! C141: the S3 key of CI's compile cache reaches every setup-rust step, and nothing else (rule 79 / ER-42, ER-40; user decision
//! 2026-11-19, docs/research/2026-11-19-sccache.md section 8).
//!
//! CI's sccache can put the shared RustFS store behind each job's local disk (`./dev ci setup-rust --sccache-backend s3`). Its key is
//! two Gitea repository secrets, `PADDOCK_SCCACHE_S3_ACCESS_KEY_ID` and `PADDOCK_SCCACHE_S3_SECRET_ACCESS_KEY`. A composite action
//! cannot read `secrets`, and a workflow- or job-level `env:` would hand them to every step (least privilege), so each setup-rust use
//! passes them in its own `with:`. The lines are pasted once by hand (the user chose a check over a generator, KISS); this check keeps
//! them complete. Read with the YAML reader ([`crate::gates::yaml`]), every `.gitea/workflows/*.yml`:
//!
//! - every `uses: ./.gitea/actions/setup-rust` (or `./.paddock/.gitea/actions/setup-rust`) step has `sccache-s3-access-key-id` and
//!   `sccache-s3-secret-access-key` in its `with:`, each `${{ secrets.<name> }}`;
//! - every job that calls the reusable `./.gitea/workflows/app-fuzz.yml` or `app-release.yml` passes both secrets in its `secrets:`
//!   (`secrets: inherit` counts);
//! - those two reusable workflows declare both under `on.workflow_call.secrets`;
//! - no other line of a workflow, and no action other than setup-rust, names either secret (a job `env:`, a `run:`);
//! - the jobs and workflows in [`NO_SECRET`] hold no secret at all: a gates job that must run without any (conformance test
//!   `the_local_gates_are_the_first_jobs_of_a_trains_dispatch_run_and_run_without_a_token`) and the release path, whose kit steps
//!   run in the shell, where the `s3` backend is the local disk (a key passed there is exposure for nothing);
//! - the test-result cache's own key (user decision 2026-11-26, ADR 0116 §4: a separate RustFS key for CI's test cache), the two secrets
//!   [`TEST_CACHE_SECRETS`], appears only in a step that runs `./dev ci test-cache`, and nowhere else (not a workflow- or job-level
//!   `env:`, not another step, not an action file): [`test_cache_strays`];
//! - setup-rust's own `action.yml` hands each `inputs.sccache-s3-*` to exactly one step and no step that holds one writes to
//!   `$GITHUB_ENV` (read with the YAML reader, per step).
//!
//! Each finding is one spot, with the exact text to paste and where (after which line, at which indentation). A part the reader finds
//! but cannot place lines in (a flow mapping, a `uses:` not on its own line) is a finding that says so. No workflow read is not measured.
//!
//! **Errors, blocking (mode gate) since its first green run on the real tree, ci.yml run 8869 (cdd4c3d10, 2026-11-24; ER-41).** First count
//! (1ee61a03c, before the integrator pastes the lines): 34 setup-rust steps, 8 calls of the reusable workflows, 2 declarations.
//!
//! Rust host only.

use super::finding::Finding;
use super::outcome::Outcome;
use super::record::Precondition;
use super::{Check, CheckOutput, Control, Ctx};
use crate::gates::yaml::{self, Node, Value};
use std::collections::BTreeSet;

/// The inputs of the setup-rust action and the repository secrets they carry.
pub const KEYS: [(&str, &str); 2] = [
    ("sccache-s3-access-key-id", "PADDOCK_SCCACHE_S3_ACCESS_KEY_ID"),
    ("sccache-s3-secret-access-key", "PADDOCK_SCCACHE_S3_SECRET_ACCESS_KEY"),
];

/// The test-result cache's key: two Gitea repository secrets, for the steps that run `./dev ci test-cache` and no other.
pub const TEST_CACHE_SECRETS: [&str; 2] = ["PADDOCK_TEST_CACHE_KEY_ID", "PADDOCK_TEST_CACHE_SECRET"];
/// What a step that may hold [`TEST_CACHE_SECRETS`] runs.
const TEST_CACHE_COMMAND: &str = "ci test-cache";

const WORKFLOWS: &str = ".gitea/workflows";
/// The action that receives the key; the only action file that may name the secrets' inputs.
const ACTION: &str = ".gitea/actions/setup-rust/action.yml";
/// How a workflow names the action: from this repository, or from the kit checked out at `.paddock` (the reusable workflows).
const SETUP_USES: [&str; 2] = ["./.gitea/actions/setup-rust", "./.paddock/.gitea/actions/setup-rust"];
/// The reusable workflows whose setup-rust steps read the secrets their callers pass (app-release.yml does not: it is on the release
/// path, [`NO_SECRET`]).
const REUSABLE: [&str; 1] = ["app-fuzz.yml"];
const RULE: u32 = 79;

/// The workflows (`file.yml`) and jobs (`file.yml:job`) that hold no S3 key. Only ever shrinks: a new entry is a decision of the
/// user (ER-46), and the test `the_no_secret_list_only_shrinks` pins its content so a growing list shows in a diff.
pub const NO_SECRET: [&str; 6] = [
    "app-release.yml",
    "throttle-release.yml",
    "dyno-release.yml",
    "pitcrew-release.yml",
    "pitwall-release.yml",
    "spotter-release.yml",
];

fn no_secret(workflow: &str, job: &str) -> bool {
    NO_SECRET.iter().any(|e| *e == workflow || *e == format!("{workflow}:{job}"))
}

/// One spot that lacks lines.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Edit {
    /// Replace the line (0-based) with this text.
    Replace(usize, String),
    /// Insert these lines before the line (0-based): after 1-based line `at`.
    Insert(usize, Vec<String>),
}

fn secret_value(secret: &str) -> String {
    format!("${{{{ secrets.{secret} }}}}")
}

fn indent_of(line: &str) -> usize {
    line.len() - line.trim_start_matches(' ').len()
}

fn blank(line: &str) -> bool {
    let t = line.trim();
    t.is_empty() || t.starts_with('#')
}

/// The index after the last line of the block that `start` opens: every following line indented deeper than `start`'s key
/// (`key_col`), blank and comment lines inside it included, trailing ones not.
fn block_end(lines: &[&str], start: usize, key_col: usize) -> usize {
    let mut end = start + 1;
    let mut j = start + 1;
    while j < lines.len() {
        if blank(lines[j]) {
            j += 1;
            continue;
        }
        if indent_of(lines[j]) <= key_col {
            break;
        }
        j += 1;
        end = j;
    }
    end
}

/// The 0-based line and column of `key:` at or after 1-based `line` (the key of a scalar written on its own line, also after a
/// `- ` sequence marker).
fn key_at(lines: &[&str], line: usize, key: &str) -> Option<(usize, usize)> {
    let i = line.checked_sub(1)?;
    let l = lines.get(i)?;
    let rest = l.trim_start_matches([' ', '-']);
    let col = l.len() - rest.len();
    rest.starts_with(&format!("{key}:")).then_some((i, col))
}

/// The edits that make the map under `key` (in a mapping whose `anchor_key` is at `anchor`) hold `want` entries, each a
/// (name, value) pair written `name: value`; `owned` collects the lines of those entries.
fn ensure_entries(
    lines: &[&str],
    map: Option<&Node>,
    anchor: (usize, usize),
    key: &str,
    want: &[(String, String)],
    edits: &mut Vec<Edit>,
    owned: &mut Vec<(usize, usize)>,
) -> Result<(), String> {
    let (anchor_line, col) = anchor;
    match map {
        None => {
            // No `key:` yet: it goes right after the anchor's own line, which must be a one-line scalar.
            let at = anchor_line + 1;
            let mut new = vec![format!("{}{key}:", " ".repeat(col))];
            new.extend(want.iter().map(|(k, v)| format!("{}{k}: {v}", " ".repeat(col + 2))));
            edits.push(Edit::Insert(at, new));
        }
        Some(node) => {
            let Value::Map(entries) = &node.value else {
                return Err(format!(
                    "line {}: `{key}:` is not a block mapping, so the check cannot say where its lines go",
                    node.line
                ));
            };
            let mut last: Option<usize> = None;
            let mut entry_col: Option<usize> = None;
            for (k, v) in entries {
                let Some((i, c)) = key_at(lines, v.line, k) else {
                    return Err(format!(
                        "line {}: the entry `{k}` of `{key}:` is not written `{k}: <value>` on one line",
                        v.line
                    ));
                };
                if matches!(v.value, Value::Scalar { block: true, .. }) || !matches!(v.value, Value::Scalar { .. }) {
                    return Err(format!("line {}: the entry `{k}` of `{key}:` is not a one-line value", i + 1));
                }
                entry_col = Some(c);
                last = Some(last.map_or(i, |l: usize| l.max(i)));
                if let Some((_, want_v)) = want.iter().find(|(wk, _)| wk == k) {
                    let line = format!("{}{k}: {want_v}", " ".repeat(c));
                    if lines[i] != line {
                        edits.push(Edit::Replace(i, line));
                    }
                    owned.push((i, 1));
                }
            }
            let (Some(last), Some(c)) = (last, entry_col) else {
                return Err(format!("line {}: `{key}:` is empty", node.line));
            };
            let missing: Vec<String> = want
                .iter()
                .filter(|(wk, _)| !entries.iter().any(|(k, _)| k == wk))
                .map(|(k, v)| format!("{}{k}: {v}", " ".repeat(c)))
                .collect();
            if !missing.is_empty() {
                edits.push(Edit::Insert(last + 1, missing));
            }
        }
    }
    Ok(())
}

/// The spots one workflow lacks, and the lines (0-based) of secret entries that are already right.
pub fn plan(name: &str, text: &str) -> Result<(Vec<Edit>, BTreeSet<usize>), String> {
    let doc = yaml::parse(text)?;
    let lines: Vec<&str> = text.split('\n').collect();
    let mut edits: Vec<Edit> = Vec::new();
    let mut owned: Vec<(usize, usize)> = Vec::new();
    let inputs: Vec<(String, String)> = KEYS.iter().map(|(i, s)| (i.to_string(), secret_value(s))).collect();
    let passed: Vec<(String, String)> = KEYS.iter().map(|(_, s)| (s.to_string(), secret_value(s))).collect();

    for (job_id, job) in doc.get("jobs").map(Node::entries).unwrap_or_default() {
        if no_secret(name, job_id) {
            continue;
        }
        for step in job.get("steps").map(Node::items).unwrap_or_default() {
            let Some(uses) = step.get("uses") else { continue };
            if !uses.text().is_some_and(|u| SETUP_USES.contains(&u.trim())) {
                continue;
            }
            let anchor = key_at(&lines, uses.line, "uses")
                .ok_or_else(|| format!("line {}: the setup-rust `uses:` is not on a line of its own", uses.line))?;
            ensure_entries(&lines, step.get("with"), anchor, "with", &inputs, &mut edits, &mut owned)?;
        }
        if let Some(uses) = job.get("uses")
            && uses
                .text()
                .is_some_and(|u| REUSABLE.iter().any(|r| u.trim() == format!("./{WORKFLOWS}/{r}")))
        {
            let secrets = job.get("secrets");
            if secrets.and_then(Node::text).is_some_and(|t| t.trim() == "inherit") {
                continue;
            }
            let anchor = key_at(&lines, uses.line, "uses")
                .ok_or_else(|| format!("line {}: the reusable workflow's `uses:` is not on a line of its own", uses.line))?;
            ensure_entries(&lines, secrets, anchor, "secrets", &passed, &mut edits, &mut owned)?;
        }
    }

    if REUSABLE.contains(&name) {
        declare_secrets(&lines, &doc, &mut edits, &mut owned)?;
    }
    Ok((edits, owned.into_iter().map(|(i, _)| i).collect()))
}

/// `on.workflow_call.secrets` declares both secrets (`required: false`): a reusable workflow reads only what it declares.
fn declare_secrets(lines: &[&str], doc: &Node, edits: &mut Vec<Edit>, owned: &mut Vec<(usize, usize)>) -> Result<(), String> {
    let Some(call) = doc.get("on").and_then(|o| o.get("workflow_call")) else {
        return Err("a reusable workflow without `on: workflow_call:`".into());
    };
    // The line of `workflow_call:` and its column, found by its first entry's line (the mapping starts there).
    let wc = lines
        .iter()
        .position(|l| l.trim() == "workflow_call:")
        .ok_or("no `workflow_call:` line of its own")?;
    let wc_col = indent_of(lines[wc]);
    let end = block_end(lines, wc, wc_col);
    let declared = call.get("secrets");
    let entry = |s: &str, col: usize| {
        vec![
            format!("{}{s}:", " ".repeat(col)),
            format!(
                "{}description: CI's compile cache on the shared S3 store (C141; docs/research/2026-11-19-sccache.md section 8).",
                " ".repeat(col + 2)
            ),
            format!("{}required: false", " ".repeat(col + 2)),
        ]
    };
    match declared {
        None => {
            let mut new = vec![format!("{}secrets:", " ".repeat(wc_col + 2))];
            for (_, s) in KEYS {
                new.extend(entry(s, wc_col + 4));
            }
            edits.push(Edit::Insert(end, new));
        }
        Some(node) => {
            let sl = (wc..end)
                .find(|&i| indent_of(lines[i]) == wc_col + 2 && lines[i].trim() == "secrets:")
                .ok_or_else(|| format!("line {}: `secrets:` of workflow_call is not a block on its own line", node.line))?;
            let s_end = block_end(lines, sl, wc_col + 2);
            let mut new = Vec::new();
            for (_, s) in KEYS {
                if node.get(s).is_none() {
                    new.extend(entry(s, wc_col + 4));
                }
            }
            for (i, l) in lines.iter().enumerate().take(s_end).skip(sl) {
                if KEYS.iter().any(|(_, s)| l.trim() == format!("{s}:")) {
                    owned.push((i, 1));
                }
            }
            if !new.is_empty() {
                edits.push(Edit::Insert(s_end, new));
            }
        }
    }
    Ok(())
}

/// The lines of `text` (0-based) that name one of the secrets but are not in `owned`: the key passed somewhere else.
pub fn stray_lines(text: &str, owned: &BTreeSet<usize>) -> Vec<usize> {
    text.split('\n')
        .enumerate()
        .filter(|(i, l)| !owned.contains(i) && KEYS.iter().any(|(_, s)| l.contains(s)))
        .map(|(i, _)| i)
        .collect()
}

/// The lines of `text` (0-based) that name a [`TEST_CACHE_SECRETS`] secret outside a step that runs `./dev ci test-cache`: a workflow
/// or job `env:`, another step, a `with:`. A step's text runs from its first line to the next step's or job's. A workflow that cannot
/// be read is a finding at line 0 of its own (the caller reports it).
pub fn test_cache_strays(text: &str) -> Result<Vec<usize>, String> {
    let doc = yaml::parse(text)?;
    let lines: Vec<&str> = text.split('\n').collect();
    // Delimiters: (1-based line, whether a step starts there).
    let mut marks: Vec<(usize, bool)> = Vec::new();
    for (_, job) in doc.get("jobs").map(Node::entries).unwrap_or_default() {
        marks.push((job.line, false));
        marks.extend(job.get("steps").map(Node::items).unwrap_or_default().iter().map(|st| (st.line, true)));
    }
    marks.sort();
    let mut out = Vec::new();
    for (i, l) in lines.iter().enumerate() {
        if l.trim_start().starts_with('#') || !TEST_CACHE_SECRETS.iter().any(|s| l.contains(s)) {
            continue;
        }
        let line = i + 1;
        let at = marks.iter().rposition(|(m, _)| *m <= line);
        let ok = at.is_some_and(|k| {
            let (from, is_step) = marks[k];
            let to = marks.get(k + 1).map_or(lines.len() + 1, |(n, _)| *n);
            is_step
                && lines[from.saturating_sub(1)..(to - 1).min(lines.len())]
                    .iter()
                    .any(|t| !t.trim_start().starts_with('#') && t.contains(TEST_CACHE_COMMAND))
        });
        if !ok {
            out.push(i);
        }
    }
    Ok(out)
}

/// The `inputs.sccache-s3-*` of setup-rust's `action.yml` and where they go: exactly one step names each, and no step that names one
/// writes to `$GITHUB_ENV` (a later step, test or build script would see the key). The findings are `(line, message)`; a file the
/// reader cannot parse or without steps is a finding too. Read per step: the step's text is from its own first line to the next
/// step's.
pub fn action_leaks(text: &str) -> Vec<(usize, String)> {
    let doc = match yaml::parse(text) {
        Ok(d) => d,
        Err(e) => return vec![(1, format!("setup-rust's action.yml cannot be read: {e}"))],
    };
    let steps = doc.get("runs").and_then(|r| r.get("steps")).map(Node::items).unwrap_or_default();
    if steps.is_empty() {
        return vec![(1, "setup-rust's action.yml has no steps the check can read".into())];
    }
    let lines: Vec<&str> = text.split('\n').collect();
    let starts: Vec<usize> = steps.iter().map(|n| n.line).collect();
    let mut out = Vec::new();
    for (key_input, _) in KEYS {
        let name = format!("inputs.{key_input}");
        let holders: Vec<usize> = starts
            .iter()
            .enumerate()
            .filter(|(i, from)| {
                let to = starts.get(i + 1).copied().unwrap_or(lines.len() + 1);
                lines[from.saturating_sub(1)..(to - 1).min(lines.len())]
                    .iter()
                    .any(|l| !l.trim_start().starts_with('#') && l.contains(&name))
            })
            .map(|(_, from)| *from)
            .collect();
        if holders.len() != 1 {
            out.push((
                holders.get(1).copied().unwrap_or(1),
                format!("`{name}` is named by {} steps; exactly one (the step that runs `./dev ci setup-rust`) may name it", holders.len()),
            ));
        }
        for from in holders {
            let to = starts.iter().copied().find(|s| *s > from).unwrap_or(lines.len() + 1);
            if lines[from - 1..(to - 1).min(lines.len())]
                .iter()
                .any(|l| !l.trim_start().starts_with('#') && l.contains("GITHUB_ENV"))
            {
                out.push((from, format!("the step that names `{name}` also writes to $GITHUB_ENV: a later step would see the key")));
            }
        }
    }
    out
}

/// The finding for one spot: where, and the exact text to paste.
fn spot(file: &str, e: &Edit) -> Finding {
    let (line, message) = match e {
        Edit::Insert(at, new) => (
            (*at).max(1),
            format!("the S3 key lines are missing: paste after line {at}:\n{}", new.join("\n")),
        ),
        Edit::Replace(i, l) => (i + 1, format!("this line passes another value: replace it with:\n{l}")),
    };
    Finding::error(
        "C141",
        RULE,
        file,
        line,
        message,
        "Paste the lines exactly as shown (the indentation is the file's own); docs/research/2026-11-19-sccache.md section 8.",
    )
}

/// The check.
pub struct C141;

impl Check for C141 {
    fn id(&self) -> &'static str {
        "C141"
    }

    fn rule(&self) -> u32 {
        RULE
    }

    fn title(&self) -> &'static str {
        "every setup-rust use passes the S3 cache key in its own with:, and nothing else names it"
    }

    fn scope(&self) -> super::CheckScope {
        super::CheckScope::Root
    }

    fn mode(&self) -> super::Mode {
        super::Mode::Gate
    }

    fn control(&self) -> Control {
        Control {
            files: vec![
                (
                    ".gitea/workflows/wired.yml",
                    "jobs:\n  a:\n    steps:\n      - uses: ./.gitea/actions/setup-rust\n        with:\n          sccache-s3-access-key-id: ${{ secrets.PADDOCK_SCCACHE_S3_ACCESS_KEY_ID }}\n          sccache-s3-secret-access-key: ${{ secrets.PADDOCK_SCCACHE_S3_SECRET_ACCESS_KEY }}\n",
                ),
                (
                    ".gitea/workflows/unwired.yml",
                    "jobs:\n  a:\n    steps:\n      - uses: ./.gitea/actions/setup-rust\n      - run: echo hi\n",
                ),
                // On the no-secret list: unwired is right here (no finding) and a key passed is a stray line.
                (
                    ".gitea/workflows/app-release.yml",
                    "jobs:\n  a:\n    steps:\n      - uses: ./.gitea/actions/setup-rust\n",
                ),
                (
                    ".gitea/workflows/dyno-release.yml",
                    "jobs:\n  a:\n    steps:\n      - uses: ./.gitea/actions/setup-rust\n        with:\n          sccache-s3-access-key-id: ${{ secrets.PADDOCK_SCCACHE_S3_ACCESS_KEY_ID }}\n",
                ),
                (
                    ".gitea/actions/setup-rust/action.yml",
                    "name: setup-rust\nruns:\n  using: composite\n  steps:\n    - name: one\n      env:\n        K: ${{ inputs.sccache-s3-access-key-id }}\n        S: ${{ inputs.sccache-s3-secret-access-key }}\n      run: echo hi\n    - name: leak\n      run: echo \"K=${{ inputs.sccache-s3-secret-access-key }}\" >> \"$GITHUB_ENV\"\n",
                ),
                // The test cache's key: right in the test-cache step's env, a stray line in a job env and in another step.
                (
                    ".gitea/workflows/tc.yml",
                    "jobs:\n  a:\n    env:\n      X: ${{ secrets.PADDOCK_TEST_CACHE_SECRET }}\n    steps:\n      - run: ./dev ci test-cache record\n        env:\n          ID: ${{ secrets.PADDOCK_TEST_CACHE_KEY_ID }}\n      - run: echo ${{ secrets.PADDOCK_TEST_CACHE_KEY_ID }}\n",
                ),
                (
                    "docs/not-a-workflow.yml",
                    "jobs:\n  a:\n    steps:\n      - uses: ./.gitea/actions/setup-rust\n",
                ),
            ],
            // The action's second step names the secret input again and writes to $GITHUB_ENV (two findings at its line 10).
            expect: vec![
                (".gitea/actions/setup-rust/action.yml", 10),
                (".gitea/actions/setup-rust/action.yml", 10),
                (".gitea/workflows/dyno-release.yml", 6),
                (".gitea/workflows/tc.yml", 4),
                (".gitea/workflows/tc.yml", 9),
                (".gitea/workflows/unwired.yml", 4),
            ],
        }
    }

    fn run(&self, ctx: &Ctx) -> CheckOutput {
        let files = ctx.files();
        let workflows: Vec<&String> = files
            .iter()
            .filter(|f| {
                f.strip_prefix(&format!("{WORKFLOWS}/"))
                    .is_some_and(|n| !n.contains('/') && (n.ends_with(".yml") || n.ends_with(".yaml")))
            })
            .collect();
        let mut findings = Vec::new();
        if workflows.is_empty() {
            findings.push(
                Finding::error(
                    "C141",
                    RULE,
                    WORKFLOWS,
                    1,
                    "not measured: the run read no workflow, so no setup-rust use was looked at",
                    "Run the check over the whole repository.",
                )
                .unwaivable()
                .with_outcome(Outcome::NotMeasured),
            );
        }
        for f in &workflows {
            let Some(text) = ctx.read(f) else {
                findings.push(
                    Finding::error(
                        "C141",
                        RULE,
                        f,
                        1,
                        format!("not measured: {f} could not be read"),
                        "Make it readable and run the check again.",
                    )
                    .unwaivable()
                    .with_outcome(Outcome::NotMeasured),
                );
                continue;
            };
            let name = f.rsplit('/').next().unwrap_or_default();
            match plan(name, &text) {
                Ok((edits, owned)) => {
                    findings.extend(edits.iter().map(|e| spot(f, e)));
                    for i in stray_lines(&text, &owned) {
                        findings.push(Finding::error(
                            "C141",
                            RULE,
                            f,
                            i + 1,
                            "names an S3 key secret outside a setup-rust step's `with:` or a reusable workflow's `secrets:`",
                            "Remove it here: the key reaches only the setup-rust step (least privilege; docs/research/2026-11-19-sccache.md section 8).",
                        ));
                    }
                }
                Err(e) => findings.push(Finding::error(
                    "C141",
                    RULE,
                    f,
                    1,
                    format!("the S3 key lines cannot be placed: {e}"),
                    "Write that part as a block on lines of its own.",
                )),
            }
            match test_cache_strays(&text) {
                Ok(strays) => findings.extend(strays.into_iter().map(|i| {
                    Finding::error(
                        "C141",
                        RULE,
                        f,
                        i + 1,
                        "names the test cache's key secret outside a step that runs `./dev ci test-cache`",
                        "Remove it here: PADDOCK_TEST_CACHE_KEY_ID and PADDOCK_TEST_CACHE_SECRET reach only the test-cache steps, in their own `env:` (ADR 0116 section 4).",
                    )
                })),
                Err(e) => findings.push(Finding::error("C141", RULE, f, 1, format!("the test cache's key lines cannot be placed: {e}"), "Write that part as a block on lines of its own.")),
            }
        }
        if let Some(text) = ctx.read(ACTION) {
            for (line, message) in action_leaks(&text) {
                findings.push(Finding::error(
                    "C141",
                    RULE,
                    ACTION,
                    line,
                    message,
                    "Hand each input to the one step that runs ./dev ci setup-rust, in its `env:`, and write nothing of it to $GITHUB_ENV.",
                ));
            }
        }
        for f in files
            .iter()
            .filter(|f| f.starts_with(".gitea/actions/") && f.ends_with(".yml") && f.as_str() != ACTION)
        {
            let Some(text) = ctx.read(f) else { continue };
            for i in stray_lines(&text, &BTreeSet::new()) {
                findings.push(Finding::error(
                    "C141",
                    RULE,
                    f,
                    i + 1,
                    "an action other than setup-rust names an S3 key secret",
                    "Remove it: only .gitea/actions/setup-rust/action.yml receives the key.",
                ));
            }
            for (i, l) in text.split('\n').enumerate() {
                if !l.trim_start().starts_with('#') && TEST_CACHE_SECRETS.iter().any(|s| l.contains(s)) {
                    findings.push(Finding::error(
                        "C141",
                        RULE,
                        f,
                        i + 1,
                        "an action names the test cache's key secret",
                        "Remove it: the test cache's key is read in the `env:` of the step that runs `./dev ci test-cache`, never by an action.",
                    ));
                }
            }
        }
        CheckOutput {
            findings,
            probes: workflows.len().max(1),
            completed: true,
            precondition: Precondition {
                met: true,
                detail: format!("{} workflow(s) read", workflows.len()),
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const STEP_WITHOUT_WITH: &str = "jobs:\n  a:\n    steps:\n      - uses: actions/checkout@v4\n      - uses: ./.gitea/actions/setup-rust\n      - name: Build\n        run: cargo build\n";
    const STEP_WITH_WITH: &str = "jobs:\n  a:\n    steps:\n      - name: Rust\n        uses: ./.gitea/actions/setup-rust\n        with:\n          tauri-linux-deps: \"true\"\n      - run: echo hi\n";
    const PAIR: &str = "sccache-s3-access-key-id: ${{ secrets.PADDOCK_SCCACHE_S3_ACCESS_KEY_ID }}";

    #[test]
    fn a_step_without_with_is_told_the_with_block_to_paste_after_its_uses_line() {
        let (edits, _) = plan("ci.yml", STEP_WITHOUT_WITH).unwrap();
        assert_eq!(
            edits,
            vec![Edit::Insert(
                5,
                vec![
                    "        with:".into(),
                    format!("          {PAIR}"),
                    "          sccache-s3-secret-access-key: ${{ secrets.PADDOCK_SCCACHE_S3_SECRET_ACCESS_KEY }}".into(),
                ]
            )]
        );
        let f = spot(".gitea/workflows/ci.yml", &edits[0]);
        assert_eq!(f.line, 5);
        assert!(
            f.message.contains("paste after line 5") && f.message.contains(PAIR),
            "{}",
            f.message
        );
    }

    #[test]
    fn a_step_with_with_is_told_the_two_lines_after_its_last_entry() {
        let (edits, _) = plan("ci.yml", STEP_WITH_WITH).unwrap();
        assert_eq!(edits.len(), 1);
        let Edit::Insert(at, new) = &edits[0] else { panic!("{edits:?}") };
        assert_eq!((*at, new.len()), (7, 2));
        assert_eq!(new[0], format!("          {PAIR}"));
    }

    #[test]
    fn a_wired_step_has_no_finding_and_its_lines_are_its_own() {
        let wired = STEP_WITH_WITH.replace(
            "tauri-linux-deps: \"true\"\n",
            &format!("tauri-linux-deps: \"true\"\n          {PAIR}\n          sccache-s3-secret-access-key: ${{{{ secrets.PADDOCK_SCCACHE_S3_SECRET_ACCESS_KEY }}}}\n"),
        );
        let (edits, owned) = plan("ci.yml", &wired).unwrap();
        assert!(edits.is_empty(), "{edits:?}");
        assert_eq!(owned, [7, 8].into_iter().collect());
        assert!(stray_lines(&wired, &owned).is_empty());
        let wrong = wired.replace("secrets.PADDOCK_SCCACHE_S3_ACCESS_KEY_ID", "secrets.SOMETHING_ELSE");
        let (edits, _) = plan("ci.yml", &wrong).unwrap();
        assert_eq!(edits, vec![Edit::Replace(7, format!("          {PAIR}"))]);
    }

    const TC_OK: &str = "jobs:\n  a:\n    steps:\n      - name: Record\n        run: ./dev ci test-cache record --shared\n        env:\n          PADDOCK_TEST_CACHE_KEY_ID: ${{ secrets.PADDOCK_TEST_CACHE_KEY_ID }}\n          PADDOCK_TEST_CACHE_SECRET: ${{ secrets.PADDOCK_TEST_CACHE_SECRET }}\n";

    #[test]
    fn the_test_cache_key_is_allowed_in_a_test_cache_step_and_nowhere_else() {
        assert_eq!(test_cache_strays(TC_OK).unwrap(), Vec::<usize>::new());
        // Positive controls: the same lines in a workflow env, a job env, another step and a `with:` are each found.
        let job_env = TC_OK.replace("  a:\n", "  a:\n    env:\n      K: ${{ secrets.PADDOCK_TEST_CACHE_SECRET }}\n");
        assert_eq!(test_cache_strays(&job_env).unwrap(), vec![3]);
        let wf_env = format!("env:\n  K: ${{{{ secrets.PADDOCK_TEST_CACHE_KEY_ID }}}}\n{TC_OK}");
        assert_eq!(test_cache_strays(&wf_env).unwrap(), vec![1]);
        let other = TC_OK.replace("./dev ci test-cache record --shared", "cargo test");
        assert_eq!(test_cache_strays(&other).unwrap(), vec![6, 7]);
        let with = format!("{TC_OK}      - uses: ./.gitea/actions/setup-rust\n        with:\n          k: ${{{{ secrets.PADDOCK_TEST_CACHE_SECRET }}}}\n");
        assert_eq!(test_cache_strays(&with).unwrap(), vec![10]);
        // A comment that names it is not a use.
        assert!(test_cache_strays(&format!("# PADDOCK_TEST_CACHE_SECRET\n{TC_OK}")).unwrap().is_empty());
    }

    #[test]
    fn the_key_named_in_a_job_env_is_a_stray_line() {
        let leaked = STEP_WITHOUT_WITH.replace(
            "  a:\n",
            "  a:\n    env:\n      K: ${{ secrets.PADDOCK_SCCACHE_S3_SECRET_ACCESS_KEY }}\n",
        );
        let (_, owned) = plan("ci.yml", &leaked).unwrap();
        assert_eq!(stray_lines(&leaked, &owned), vec![3]);
    }

    #[test]
    fn a_caller_of_a_reusable_workflow_passes_the_secrets_and_inherit_counts() {
        let caller = "jobs:\n  rel:\n    uses: ./.gitea/workflows/app-fuzz.yml\n    with:\n      app-id: dyno\n  other:\n    uses: ./.gitea/workflows/app-fuzz.yml\n    secrets: inherit\n  release:\n    uses: ./.gitea/workflows/app-release.yml\n";
        let (edits, _) = plan("nightly.yml", caller).unwrap();
        assert_eq!(
            edits,
            vec![Edit::Insert(
                3,
                vec![
                    "    secrets:".into(),
                    "      PADDOCK_SCCACHE_S3_ACCESS_KEY_ID: ${{ secrets.PADDOCK_SCCACHE_S3_ACCESS_KEY_ID }}".into(),
                    "      PADDOCK_SCCACHE_S3_SECRET_ACCESS_KEY: ${{ secrets.PADDOCK_SCCACHE_S3_SECRET_ACCESS_KEY }}".into(),
                ]
            )],
            "only the app-fuzz caller is told to pass the key: app-release is on the release path and takes none"
        );
    }

    #[test]
    fn a_reusable_workflow_is_told_the_secrets_to_declare() {
        let reusable = "name: app-fuzz\non:\n  workflow_call:\n    inputs:\n      dir:\n        type: string\njobs:\n  f:\n    steps:\n      - uses: ./.paddock/.gitea/actions/setup-rust\n        with:\n          x: y\n";
        let (edits, _) = plan("app-fuzz.yml", reusable).unwrap();
        assert_eq!(edits.len(), 2, "{edits:?}");
        let Edit::Insert(at, new) = &edits[1] else { panic!("{edits:?}") };
        assert_eq!(*at, 6, "after the workflow_call block");
        assert_eq!(new[0], "    secrets:");
        assert_eq!(new[1], "      PADDOCK_SCCACHE_S3_ACCESS_KEY_ID:");
        assert!(new.contains(&"        required: false".to_string()));
    }

    #[test]
    fn the_no_secret_list_only_shrinks() {
        // Pinned: a longer list shows in this diff and needs the user's yes (ER-46).
        assert_eq!(
            NO_SECRET,
            ["app-release.yml", "throttle-release.yml", "dyno-release.yml", "pitcrew-release.yml", "pitwall-release.yml", "spotter-release.yml"]
        );
    }

    #[test]
    fn a_job_on_the_no_secret_list_is_not_asked_for_the_key_and_a_key_there_is_a_stray_line() {
        // The list names whole workflows since ci.yml's train-gates job was removed (chore/cut-ci-and-gates): every job of one is a
        // no-secret job; the same jobs in a workflow off the list are each told to pass the key.
        let two = "jobs:\n  gates:\n    steps:\n      - uses: ./.gitea/actions/setup-rust\n  other:\n    steps:\n      - uses: ./.gitea/actions/setup-rust\n";
        let (edits, _) = plan("dyno-release.yml", two).unwrap();
        assert!(edits.is_empty(), "no job of a no-secret workflow is told to pass the key: {edits:?}");
        let (edits, _) = plan("ci.yml", two).unwrap();
        assert_eq!(edits.len(), 2, "both jobs of a workflow off the list are told to pass the key: {edits:?}");
        let with_key = "jobs:\n  gates:\n    steps:\n      - uses: ./.gitea/actions/setup-rust\n        with:\n          sccache-s3-access-key-id: ${{ secrets.PADDOCK_SCCACHE_S3_ACCESS_KEY_ID }}\n";
        let (edits, owned) = plan("dyno-release.yml", with_key).unwrap();
        assert!(edits.is_empty());
        assert_eq!(stray_lines(with_key, &owned), vec![5], "the key in a no-secret job is a stray line");
    }

    #[test]
    fn the_action_hands_each_input_to_one_step_that_does_not_write_to_github_env() {
        let ok = "runs:\n  using: composite\n  steps:\n    - name: a\n      run: echo\n    - name: b\n      env:\n        K: ${{ inputs.sccache-s3-access-key-id }}\n        S: ${{ inputs.sccache-s3-secret-access-key }}\n      run: ./dev\n";
        assert!(action_leaks(ok).is_empty(), "{:?}", action_leaks(ok));
        let twice = format!("{ok}    - name: c\n      run: echo ${{{{ inputs.sccache-s3-access-key-id }}}}\n");
        assert_eq!(action_leaks(&twice).len(), 1, "{:?}", action_leaks(&twice));
        let to_env = ok.replace("run: ./dev", "run: echo \"K=$K\" >> \"$GITHUB_ENV\"");
        assert_eq!(action_leaks(&to_env).len(), 2, "one per input: {:?}", action_leaks(&to_env));
        assert_eq!(action_leaks("name: x\n").len(), 1, "an action without steps is a finding, not a pass");
    }

    #[test]
    fn the_real_setup_rust_action_hands_the_keys_to_one_step() {
        let text = std::fs::read_to_string(std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../.gitea/actions/setup-rust/action.yml")).unwrap();
        assert_eq!(action_leaks(&text), Vec::new());
    }
}
