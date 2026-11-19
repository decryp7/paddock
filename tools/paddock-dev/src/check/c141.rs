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
//! - no other line of a workflow, and no action other than setup-rust, names either secret (a job `env:`, a `run:`).
//!
//! Each finding is one spot, with the exact text to paste and where (after which line, at which indentation). A part the reader finds
//! but cannot place lines in (a flow mapping, a `uses:` not on its own line) is a finding that says so. No workflow read is not measured.
//!
//! **Report-only (warnings) until its first green run on the real tree is recorded here (ER-41).** Recorded: not yet. First count
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

const WORKFLOWS: &str = ".gitea/workflows";
/// The action that receives the key; the only action file that may name the secrets' inputs.
const ACTION: &str = ".gitea/actions/setup-rust/action.yml";
/// How a workflow names the action: from this repository, or from the kit checked out at `.paddock` (the reusable workflows).
const SETUP_USES: [&str; 2] = ["./.gitea/actions/setup-rust", "./.paddock/.gitea/actions/setup-rust"];
/// The reusable workflows whose setup-rust steps read the secrets their callers pass.
const REUSABLE: [&str; 2] = ["app-fuzz.yml", "app-release.yml"];
const RULE: u32 = 79;

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

    for (_, job) in doc.get("jobs").map(Node::entries).unwrap_or_default() {
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

/// The finding for one spot: where, and the exact text to paste.
fn spot(file: &str, e: &Edit) -> Finding {
    let (line, message) = match e {
        Edit::Insert(at, new) => (
            (*at).max(1),
            format!("the S3 key lines are missing: paste after line {at}:\n{}", new.join("\n")),
        ),
        Edit::Replace(i, l) => (i + 1, format!("this line passes another value: replace it with:\n{l}")),
    };
    Finding::warn(
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
        super::Mode::ReportOnly
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
                (
                    "docs/not-a-workflow.yml",
                    "jobs:\n  a:\n    steps:\n      - uses: ./.gitea/actions/setup-rust\n",
                ),
            ],
            expect: vec![(".gitea/workflows/unwired.yml", 4)],
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
                        findings.push(Finding::warn(
                            "C141",
                            RULE,
                            f,
                            i + 1,
                            "names an S3 key secret outside a setup-rust step's `with:` or a reusable workflow's `secrets:`",
                            "Remove it here: the key reaches only the setup-rust step (least privilege; docs/research/2026-11-19-sccache.md section 8).",
                        ));
                    }
                }
                Err(e) => findings.push(Finding::warn(
                    "C141",
                    RULE,
                    f,
                    1,
                    format!("the S3 key lines cannot be placed: {e}"),
                    "Write that part as a block on lines of its own.",
                )),
            }
        }
        for f in files
            .iter()
            .filter(|f| f.starts_with(".gitea/actions/") && f.ends_with(".yml") && f.as_str() != ACTION)
        {
            let Some(text) = ctx.read(f) else { continue };
            for i in stray_lines(&text, &BTreeSet::new()) {
                findings.push(Finding::warn(
                    "C141",
                    RULE,
                    f,
                    i + 1,
                    "an action other than setup-rust names an S3 key secret",
                    "Remove it: only .gitea/actions/setup-rust/action.yml receives the key.",
                ));
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
        let caller = "jobs:\n  rel:\n    uses: ./.gitea/workflows/app-release.yml\n    with:\n      app-id: dyno\n  other:\n    uses: ./.gitea/workflows/app-fuzz.yml\n    secrets: inherit\n";
        let (edits, _) = plan("dyno-release.yml", caller).unwrap();
        assert_eq!(
            edits,
            vec![Edit::Insert(
                3,
                vec![
                    "    secrets:".into(),
                    "      PADDOCK_SCCACHE_S3_ACCESS_KEY_ID: ${{ secrets.PADDOCK_SCCACHE_S3_ACCESS_KEY_ID }}".into(),
                    "      PADDOCK_SCCACHE_S3_SECRET_ACCESS_KEY: ${{ secrets.PADDOCK_SCCACHE_S3_SECRET_ACCESS_KEY }}".into(),
                ]
            )]
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
}
