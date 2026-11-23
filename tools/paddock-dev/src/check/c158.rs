//! C158: no code reads a workflow's text outside the CI model (rule 79, ER-42; ER-21 class "a script, a check or a test slices
//! `.gitea/workflows/*.yml` itself and drifts from what the workflow means": `scripts/publish.mjs` `crateStepsMissing`, conformance C137
//! (it passed on the text of an unrelated job), the train-8 `sed` over ci.yml in a registry entry). The one reader of a workflow is the
//! CI model: `./dev gates model` (`tools/paddock-dev/src/gates/`, `gates::ci`: `workflow_text`, `workflow_files`, `workflow_inputs`), the YAML reader
//! behind it, `Ctx::workflows` (`check/util/workflows.rs`), and the JavaScript side's accessors (`conformance/test/lib/workflow.mjs` for
//! tests, `ctx.workflowText` in `conformance/src/context.mjs` for checks). Everything else asks the model.
//!
//! Read: Rust, JavaScript, TypeScript and shell sources in `scripts/`, `conformance/` and `tools/` (tests included), outside
//! `node_modules`, `target`, fixtures and recorded goldens. A site is a line that names a workflow file (`.gitea/workflows`,
//! `workflows/<name>.yml`, or `workflows` and a `.yml` name on one line) with a read of file text on it or within the next three lines
//! (`readFileSync`, `read_to_string`, `ctx.read(`, `grep`, `sed`, `awk`, `cat`, `git show`, `git grep` and the like). Comment lines are
//! not read. The model's own files ([`MODEL`]) and this check's ([`IMPLEMENTATION`]) are exempt.
//!
//! Exceptions are the per-site entries of `conformance/c158-allow.json`, read by the shared [`super::allow_list`]: they only shrink
//! (the reader compares them with the merge base, ER-46), a stale entry is a warning, a site beyond an entry's count is reported at its
//! own line. Convert a site to the model and delete its entry.
//!
//! **Report-only (warnings) until its first green run on the real tree is recorded here and in docs/gates.toml (ER-41).** Recorded: not
//! yet (2026-11-23).

use super::allow_list::{self, Admit, Spec};
use super::finding::Finding;
use super::jsre::rx;
use super::{Check, CheckOutput, CheckScope, Control, Ctx, Mode};
use regex::Regex;
use std::sync::LazyLock;

/// The allow file.
pub const ALLOW: &str = "conformance/c158-allow.json";

/// The CI model: the only code that may read a workflow's text (prefixes of paths).
pub const MODEL: [&str; 6] = [
    "conformance/test/lib/workflow.mjs",
    "conformance/src/context.mjs",
    "tools/paddock-dev/src/gates/",
    "tools/paddock-dev/src/check/util/workflows.rs",
    "tools/paddock-dev/src/repo_model/ci.rs",
    "tools/paddock-dev/src/repo_model/ci/",
];

/// The check's own files: they name the patterns.
pub const IMPLEMENTATION: [&str; 5] = [
    "tools/paddock-dev/src/check/c158.rs",
    // C121 polices the same pattern in registry entries and tests; its sources and tests hold invented lines that name a workflow.
    "tools/paddock-dev/src/check/c121.rs",
    "tools/paddock-dev/tests/it/check_c121.rs",
    "tools/paddock-dev/tests/it/check_c158.rs",
    ALLOW,
];

static SOURCE: LazyLock<Regex> = LazyLock::new(|| rx(r"\.(?:rs|mjs|cjs|js|ts|sh|bash|ps1)$"));
static ROOTS: LazyLock<Regex> = LazyLock::new(|| rx(r"^(?:scripts|conformance|tools)/"));
static NOT_READ: LazyLock<Regex> = LazyLock::new(|| rx(r"(?:^|/)(?:node_modules|target|fixtures|golden|vendor)/"));
static NAMES_WORKFLOW: LazyLock<Regex> =
    LazyLock::new(|| rx(r#"\.gitea[/"'`, ]+workflows|workflows/[A-Za-z0-9_.-]+\.ya?ml|workflows.*[A-Za-z0-9_-]\.ya?ml\b"#));
static READS: LazyLock<Regex> = LazyLock::new(|| {
    rx(
        r"readFileSync|readFile\b|readTextFile|read_text\b|read_to_string|\bfs::read\b|\bctx\.read\(|\.read\(|\bgrep\b|\bsed\b|\bawk\b|\bcat\b|git\s+(?:show|grep)|git[\x22', ]+(?:show|grep)",
    )
});

const FIX: &str = "Ask the CI model instead of reading the file: `./dev gates model` (tools/paddock-dev/src/gates/ci.rs), or `Ctx::workflows()` in a check. A site that has to stay is listed in conformance/c158-allow.json (path, site, reason, count), which the user approves.";

/// Whether `path` is a file this check reads.
pub fn readable(path: &str) -> bool {
    SOURCE.is_match(path)
        && ROOTS.is_match(path)
        && !NOT_READ.is_match(path)
        && !IMPLEMENTATION.contains(&path)
        && !MODEL.iter().any(|m| path.starts_with(m))
}

/// `(line, the line's text)` of every read of a workflow's text in `text`.
pub fn sites(text: &str) -> Vec<(usize, String)> {
    let lines: Vec<&str> = text.split('\n').collect();
    let is_comment = |l: &str| {
        let t = l.trim_start();
        t.starts_with("//") || t.starts_with('#') || t.starts_with('*') || t.starts_with("/*")
    };
    let mut out = Vec::new();
    for (i, line) in lines.iter().enumerate() {
        if is_comment(line) || line.contains("ls-files") || !NAMES_WORKFLOW.is_match(line) {
            continue;
        }
        let from = i.saturating_sub(1);
        let to = (i + 4).min(lines.len());
        if lines[from..to].iter().any(|l| !is_comment(l) && READS.is_match(l)) {
            out.push((i + 1, (*line).to_string()));
        }
    }
    out
}

static SPEC: Spec = Spec {
    id: "C158",
    rule: 79,
    file: ALLOW,
    unseen: "a read of a workflow",
    restore: "Restore conformance/c158-allow.json: {\"allow\": [{\"path\": \"...\", \"site\": \"ci.yml\", \"reason\": \"...\", \"count\": 1}]}.",
};

/// The check.
pub struct C158;

impl Check for C158 {
    fn id(&self) -> &'static str {
        "C158"
    }

    fn rule(&self) -> u32 {
        79
    }

    fn title(&self) -> &'static str {
        "no code reads a workflow's text outside the CI model"
    }

    fn scope(&self) -> CheckScope {
        CheckScope::Root
    }

    fn mode(&self) -> Mode {
        Mode::ReportOnly
    }

    fn allow_list(&self) -> Option<&'static Spec> {
        Some(&SPEC)
    }

    fn control(&self) -> Control {
        Control {
            files: vec![
                (
                    ALLOW,
                    "{\"allow\":[{\"path\":\"scripts/ok.mjs\",\"site\":\"ci.yml\",\"reason\":\"baseline: predates the check\"}]}",
                ),
                ("scripts/ok.mjs", "const t = readFileSync('.gitea/workflows/ci.yml', 'utf8');\n"),
                (
                    "scripts/bad.mjs",
                    "const x = 1;\nconst t = readFileSync(join(root, '.gitea/workflows/publish.yml'), 'utf8');\n",
                ),
                ("conformance/test/sed.sh", "sed -n '1,9p' .gitea/workflows/ci.yml\n"),
                ("scripts/comment.mjs", "// readFileSync('.gitea/workflows/ci.yml')\nconst a = 1;\n"),
                ("scripts/mention.mjs", "const doc = 'see .gitea/workflows/ci.yml';\n"),
                (
                    "tools/paddock-dev/src/gates/ci.rs",
                    "let t = std::fs::read_to_string(\".gitea/workflows/ci.yml\");\n",
                ),
            ],
            expect: vec![("conformance/test/sed.sh", 1), ("scripts/bad.mjs", 2)],
        }
    }

    fn run(&self, ctx: &Ctx) -> CheckOutput {
        let mut allow = match allow_list::read(ctx, &SPEC, allow_list::entries_only(ALLOW)) {
            Ok((allow, ())) => allow,
            Err(broken) => return *broken,
        };
        let mut findings = Vec::new();
        for f in ctx.files().iter() {
            if !readable(f) {
                continue;
            }
            let text = match allow.read(ctx, f) {
                Ok(text) => text,
                Err(unread) => {
                    findings.push(*unread);
                    continue;
                }
            };
            for (line, at) in sites(&text) {
                let (what, fix) = match allow.admit(f, |site| at.contains(site)) {
                    Admit::Allowed => continue,
                    Admit::Beyond => (
                        "reads a workflow's text beyond the count of its allow entry".to_string(),
                        "Use the CI model, or raise the entry's `count` in conformance/c158-allow.json with the user's approval."
                            .to_string(),
                    ),
                    Admit::Unlisted => (
                        "reads a workflow's text itself: it drifts from what the CI model says the workflow means".to_string(),
                        FIX.to_string(),
                    ),
                };
                findings.push(Finding::warn("C158", 79, f, line, what, fix));
            }
        }
        allow.output(ctx, findings)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lines(text: &str) -> Vec<usize> {
        sites(text).into_iter().map(|s| s.0).collect()
    }

    #[test]
    fn a_read_of_a_workflow_file_is_a_site_on_the_line_that_names_it() {
        assert_eq!(lines("const t = readFileSync('.gitea/workflows/ci.yml', 'utf8');"), vec![1]);
        assert_eq!(
            lines("let t = std::fs::read_to_string(root.join(\".gitea/workflows/ci.yml\"));"),
            vec![1]
        );
        assert_eq!(lines("sed -n '1,20p' .gitea/workflows/ci.yml"), vec![1]);
        assert_eq!(lines("git show origin/main:.gitea/workflows/ci.yml | grep run"), vec![1]);
    }

    #[test]
    fn the_name_and_the_read_may_be_a_few_lines_apart_but_not_far() {
        assert_eq!(lines("const p = '.gitea/workflows/ci.yml';\nconst t = readFileSync(p);"), vec![1]);
        let far = "const p = '.gitea/workflows/ci.yml';\n\n\n\n\nconst t = readFileSync(p);";
        assert!(lines(far).is_empty());
    }

    #[test]
    fn a_mention_or_a_comment_is_not_a_read() {
        assert!(lines("const doc = 'see .gitea/workflows/ci.yml';").is_empty());
        assert!(lines("// readFileSync('.gitea/workflows/ci.yml')\nconst a = 1;").is_empty());
        assert!(lines("readFileSync('package.json')").is_empty());
    }

    #[test]
    fn a_listing_of_paths_is_not_a_read_of_text() {
        assert!(lines("const r = git('ls-files', '--', '.gitea/workflows/*.yml');").is_empty());
    }

    #[test]
    fn c121_which_polices_the_same_pattern_keeps_its_invented_lines_to_itself() {
        assert!(!readable("tools/paddock-dev/src/check/c121.rs"));
        assert!(!readable("tools/paddock-dev/tests/it/check_c121.rs"));
        assert!(readable("tools/paddock-dev/src/check/c122.rs"));
    }

    #[test]
    fn the_model_the_checks_own_files_and_fixtures_are_not_read() {
        assert!(readable("scripts/publish.mjs"));
        assert!(readable("tools/paddock-dev/tests/it/x.rs"));
        assert!(!readable("tools/paddock-dev/src/gates/ci.rs"));
        assert!(!readable("tools/paddock-dev/src/check/util/workflows.rs"));
        assert!(!readable("tools/paddock-dev/src/check/c158.rs"));
        assert!(!readable("tools/paddock-dev/tests/fixtures/check/x.rs"));
        assert!(!readable("apps/dyno/scripts/x.mjs"));
        assert!(!readable("scripts/README.md"));
    }
}
