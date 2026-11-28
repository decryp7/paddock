//! C182: a remote action or reusable workflow is pinned to a full 40-character commit SHA (ER-26: every action CI uses is pinned to an exact
//! version or digest in the repo; user decision 2026-11-28, in chat: pin every remote `uses:`). A tag such as `actions/checkout@v4`
//! moves: whoever controls the action's repository changes what runs in a job, with the runner's secrets, and a build is no longer
//! reproducible (the same reason ER-57 gives for waiting before an update).
//!
//! What is read, and how: every workflow under `.gitea/workflows/` and every composite action under `.gitea/actions/`, through
//! `paddock_ci_config::yaml` (ER-42), at `jobs.<id>.uses` (a reusable workflow), `jobs.<id>.steps[].uses` and `runs.steps[].uses`.
//! Never matched as text: a comment or a `run:` body that names `uses:` is no step.
//!
//! What is exempt: a local path (`./.gitea/actions/...`, `./.paddock/...`, any value starting with `.`) and a `docker://` image
//! (a digest there is `@sha256:...`, a different reference form that this check does not judge). The trailing `# v4.4.0` comment that
//! tells a reader the version is the convention, and what Renovate's `helpers:pinGitHubActionDigests` writes and bumps; the YAML
//! reader does not keep comments, so the check judges the SHA only.
//!
//! Report-only (warnings, mode report-only; ER-41) until its first green run on the real tree, recorded here and in docs/gates.toml
//! (`[[report_only]]` C182). No baseline file (ER-46): the report is the list. Rust host only: born in Rust.

use super::finding::Finding;
use super::record::Precondition;
use super::{Check, CheckOutput, CheckScope, Control, Ctx, Mode};
use paddock_ci_config::yaml::{self, Node};

/// The project rule field. No numbered project rule covers this yet; 0 names ER-26 in the message.
const RULE: u32 = 0;

/// Where the pinned workflows and actions live: a root-relative path starts with one of these.
fn is_ci_file(rel: &str) -> bool {
    let yaml = rel.ends_with(".yml") || rel.ends_with(".yaml");
    yaml && (rel.starts_with(".gitea/workflows/") || rel.starts_with(".gitea/actions/"))
}

/// Whether a `uses:` value needs no pin: a local path or a container image.
pub fn is_exempt(uses: &str) -> bool {
    uses.starts_with('.') || uses.starts_with("docker://")
}

/// Whether a remote reference ends in `@` and a full 40-hex commit SHA (lower case, as `git` prints it).
pub fn is_pinned(uses: &str) -> bool {
    uses.rsplit_once('@')
        .is_some_and(|(_, r)| r.len() == 40 && r.bytes().all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b)))
}

/// One remote reference that is not pinned: its line and the value as written.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Site {
    /// The 1-based line of the value.
    pub line: usize,
    /// The `uses:` value.
    pub uses: String,
}

/// Every `uses:` of a workflow or composite action: the nodes at `jobs.*.uses`, `jobs.*.steps[].uses` and `runs.steps[].uses`.
pub fn uses_nodes(doc: &Node) -> Vec<&Node> {
    fn of_steps<'a>(steps: Option<&'a Node>, out: &mut Vec<&'a Node>) {
        for step in steps.map(Node::items).unwrap_or_default() {
            out.extend(step.get("uses"));
        }
    }
    let mut out = Vec::new();
    for (_, job) in doc.get("jobs").map(Node::entries).unwrap_or_default() {
        out.extend(job.get("uses"));
        of_steps(job.get("steps"), &mut out);
    }
    of_steps(doc.get("runs").and_then(|r| r.get("steps")), &mut out);
    out
}

/// The unpinned remote references of one parsed file.
pub fn scan(doc: &Node) -> Vec<Site> {
    let mut sites: Vec<Site> = uses_nodes(doc)
        .into_iter()
        .filter_map(|n| {
            let uses = n.text()?.trim();
            (!is_exempt(uses) && !is_pinned(uses)).then(|| Site {
                line: n.line,
                uses: uses.to_string(),
            })
        })
        .collect();
    sites.sort_by_key(|s| s.line);
    sites
}

const FIX: &str = "Pin it to the full commit SHA of the version with the version as a trailing comment: `uses: owner/repo@<40-hex> # v1.2.3` (`git ls-remote https://github.com/<owner>/<repo> refs/tags/<tag>`, peel an annotated tag with `^{}`). Renovate (helpers:pinGitHubActionDigests) keeps the SHA current.";

/// The check.
pub struct C182;

impl Check for C182 {
    fn id(&self) -> &'static str {
        "C182"
    }

    fn rule(&self) -> u32 {
        RULE
    }

    fn title(&self) -> &'static str {
        "a remote `uses:` in a workflow or action is pinned to a full commit SHA (local paths are exempt)"
    }

    fn scope(&self) -> CheckScope {
        CheckScope::Root
    }

    fn mode(&self) -> Mode {
        Mode::ReportOnly
    }

    fn control(&self) -> Control {
        Control {
            files: vec![
                // Red: a moving tag in a step, a branch in a composite step, an abbreviated SHA, a reusable workflow on a tag.
                (
                    ".gitea/workflows/w.yml",
                    "jobs:\n  a:\n    runs-on: x\n    steps:\n      - uses: actions/checkout@v4\n      - uses: actions/cache@0057852\n  b:\n    uses: o/r/.gitea/workflows/x.yml@v1\n",
                ),
                (
                    ".gitea/actions/b/action.yml",
                    "runs:\n  using: composite\n  steps:\n    - uses: taiki-e/install-action@master\n",
                ),
                // Green: a pinned SHA with a version comment, local paths, a container image, and the word in a run body and a comment.
                (
                    ".gitea/workflows/ok.yml",
                    "jobs:\n  a:\n    runs-on: x\n    steps:\n      - uses: actions/checkout@11d5960a326750d5838078e36cf38b85af677262 # v4.4.0\n      - uses: ./.gitea/actions/plan\n      - uses: ./.paddock/.gitea/actions/setup-rust\n      - uses: docker://alpine:3\n      # - uses: actions/checkout@v4\n      - run: |\n          echo uses: actions/checkout@v4\n",
                ),
            ],
            expect: vec![
                (".gitea/actions/b/action.yml", 4),
                (".gitea/workflows/w.yml", 5),
                (".gitea/workflows/w.yml", 6),
                (".gitea/workflows/w.yml", 8),
            ],
        }
    }

    fn run(&self, ctx: &Ctx) -> CheckOutput {
        let mut findings = Vec::new();
        let (mut probes, mut files, mut remote) = (0, 0, 0);
        let mut completed = true;
        let mut unread: Vec<String> = Vec::new();
        for f in ctx.files().iter().filter(|f| is_ci_file(f)) {
            let Some(src) = ctx.read(f) else { continue };
            files += 1;
            let doc = match yaml::parse(&src) {
                Ok(d) => d,
                Err(e) => {
                    // A workflow or action the reader cannot read was not looked at (ER-33).
                    completed = false;
                    unread.push(format!("{f}: {e}"));
                    continue;
                }
            };
            probes += uses_nodes(&doc).len();
            remote += uses_nodes(&doc)
                .iter()
                .filter(|n| n.text().is_some_and(|u| !is_exempt(u.trim())))
                .count();
            for s in scan(&doc) {
                findings.push(Finding::warn(
                    "C182",
                    RULE,
                    f,
                    s.line,
                    format!(
                        "`uses: {}` is not pinned to a full 40-character commit SHA (ER-26): a tag or branch moves under the job",
                        s.uses
                    ),
                    FIX,
                ));
            }
        }
        CheckOutput {
            findings,
            probes,
            completed,
            precondition: Precondition {
                met: probes > 0 || ctx.is_in_memory(),
                detail: format!(
                    "{files} workflows and actions read, {probes} `uses:` lines, {remote} of them remote{}",
                    if unread.is_empty() {
                        String::new()
                    } else {
                        format!("; not read: {}", unread.join("; "))
                    }
                ),
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lines(text: &str) -> Vec<usize> {
        scan(&yaml::parse(text).unwrap()).into_iter().map(|s| s.line).collect()
    }

    #[test]
    fn only_a_full_lowercase_hex_sha_is_a_pin() {
        let sha = "11d5960a326750d5838078e36cf38b85af677262";
        assert!(is_pinned(&format!("actions/checkout@{sha}")));
        assert!(is_pinned(&format!("actions/cache/save@{sha}")));
        for bad in [
            "actions/checkout@v4",
            "actions/checkout",
            "a/b@11d5960",
            &format!("a/b@{}", sha.to_uppercase()),
            &format!("a/b@{sha}0"),
            "a/b@sha256:abc",
        ] {
            assert!(!is_pinned(bad), "{bad}");
        }
    }

    #[test]
    fn local_paths_and_images_are_exempt() {
        for ok in ["./.gitea/actions/plan", "./.paddock/.gitea/actions/x", "../x", "docker://alpine:3"] {
            assert!(is_exempt(ok), "{ok}");
        }
        assert!(!is_exempt("decryp7/paddock/.gitea/actions/plan@v0.3.0"));
    }

    #[test]
    fn steps_jobs_and_composite_steps_are_read_with_their_lines() {
        let t = "jobs:\n  a:\n    steps:\n      - uses: a/b@v1\n      - run: echo uses\n  c:\n    uses: o/r/.gitea/workflows/x.yml@v2\n";
        assert_eq!(lines(t), vec![4, 7]);
        assert_eq!(lines("runs:\n  using: composite\n  steps:\n    - uses: a/b@v1\n"), vec![4]);
    }

    #[test]
    fn a_trailing_comment_does_not_change_the_value() {
        assert!(lines("jobs:\n  a:\n    steps:\n      - uses: a/b@11d5960a326750d5838078e36cf38b85af677262 # v4.4.0\n").is_empty());
    }
}
