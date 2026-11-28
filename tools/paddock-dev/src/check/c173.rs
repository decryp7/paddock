//! C173: no new non-Rust tooling file (ADR 0022 §3.7 (a) and (b); ER-21 class "new tooling lands as shell and JavaScript again": 11
//! files since 2026-11-12, because the check the ADR promised was never built; the user approved building it as part of the full
//! Rust port, 2026-11-26). Tooling is Rust, run through `./dev` (rule 83's neighbour: the port ends only if nothing new lands beside it).
//!
//! Read: git's tracked list (`git ls-files`, ER-42; a tree in memory or a folder that is no work tree has only its file listing).
//! Flagged, one finding per file: a tracked `.sh`, `.ps1`, `.py`, `.mjs`, `.js`, `.cjs` or `.ts` file in a tooling path (`scripts/`,
//! `tools/`, `conformance/`, `.gitea/`, `ci-image/` and `apps/<app>/scripts/`) that no entry of `conformance/tooling-files.json` names.
//! Not tooling, so never flagged: data under a `fixture`, `fixtures`, `golden` or `goldens` folder, and test-suite files (`*.spec.ts`,
//! `*.test.ts`, `playwright.config.ts`, `vitest.config.ts`: Playwright and Vitest suites stay TypeScript, ADR 0022 §4). The ESLint preset
//! and its rules live in `eslint/`, outside the tooling paths.
//!
//! The one list is [`ALLOW`], read by the shared [`super::allow_list`]: **permanent** entries (the bootstrap and the helper it needs
//! before `./dev` exists, the hook's wrapper) and **baseline** entries (every tooling file of 2026-11-26, each removed with the port that
//! deletes the file). The reader compares the list with the merge base, and an added entry or raised `count` is growth: only the user
//! approves it (ER-46). A deleted file whose entry stays is a stale warning, so the baseline shrinks in the same change as the port.
//!
//! **Errors, blocking (mode gate): the user approved blocking on 2026-11-26 (ER-41), so a new tooling file is an error from the start.
//! First green run on the real tree: `./dev check run --only C173` on feat/tooling-file-baseline, 2026-11-26, 0 errors, 443 tooling
//! files named by the list (record the CI run id here after the landing's first conformance run, ER-41).**
//! The growth guard is a warning until the list is on the target branch ([`SPEC`]'s `growth_blocks`; ER-41's checklist: a list created on
//! a branch is growth in full). Flip `growth_blocks` to true in the change that follows the landing (follow-up of this branch).
//!
//! Review date: 2026-12-26. If it caught no new file by then it is deleted, not extended (ER-41).

use super::allow_list::{self, Admit};
use super::baseline;
use super::finding::Finding;
use super::outcome::Outcome;
use super::{Check, CheckOutput, CheckScope, Control, Ctx, Mode};
use std::collections::HashSet;

/// The allow file: permanent entries and the shrink-only baseline.
pub const ALLOW: &str = "conformance/tooling-files.json";
/// The one `site` every entry carries: the entry names the whole file, and the file has one kind of site.
pub const SITE: &str = "tooling file";
/// The rule the check enforces: lists only shrink (ER-46), for ADR 0022's tooling.
const RULE: u32 = 83;
const EXTENSIONS: [&str; 7] = ["sh", "ps1", "py", "mjs", "js", "cjs", "ts"];
const ROOTS: [&str; 5] = ["scripts/", "tools/", "conformance/", ".gitea/", "ci-image/"];
const BROKEN_FIX: &str =
    "Restore conformance/tooling-files.json: {\"allow\": [{\"path\": \"...\", \"site\": \"tooling file\", \"reason\": \"...\"}]}.";
const FIX: &str = "Write it in Rust as a `./dev` command (ADR 0022: tooling is Rust; tools/paddock-dev). A file that must stay shell or JavaScript \
(it runs before `./dev` exists, or the tool it drives is JavaScript) needs an entry in conformance/tooling-files.json, which the user approves.";

/// The growth guard's spec: a warning until the list is on the target branch (see the module).
pub static SPEC: allow_list::Spec = allow_list::Spec {
    id: "C173",
    rule: RULE,
    file: ALLOW,
    unseen: "a tooling file",
    restore: BROKEN_FIX,
    growth_blocks: false,
};

/// Whether `path` lies in a tooling path.
fn in_tooling_path(path: &str) -> bool {
    if ROOTS.iter().any(|r| path.starts_with(r)) {
        return true;
    }
    let mut seg = path.split('/');
    matches!((seg.next(), seg.next(), seg.next()), (Some("apps"), Some(_), Some("scripts")))
}

/// Whether `path` is test data (a fixture or golden folder) or a Playwright or Vitest suite file: not tooling.
fn is_data_or_suite(path: &str) -> bool {
    let name = path.rsplit('/').next().unwrap_or(path);
    let folders = path.split('/').count().saturating_sub(1);
    path.split('/')
        .take(folders)
        .any(|s| matches!(s, "fixture" | "fixtures" | "golden" | "goldens"))
        || name.ends_with(".spec.ts")
        || name.ends_with(".test.ts")
        || name == "playwright.config.ts"
        || name == "vitest.config.ts"
}

/// Whether `path` is a tooling file in the sense of the check.
pub fn is_tooling_file(path: &str) -> bool {
    let ext = path.rsplit('/').next().and_then(|n| n.rsplit_once('.')).map_or("", |(_, e)| e);
    EXTENSIONS.contains(&ext) && in_tooling_path(path) && !is_data_or_suite(path)
}

/// The files the run looks at that git tracks: git's list in a work tree, else the tree's own listing (a control, a fixture folder).
fn tracked(ctx: &Ctx) -> Result<Vec<String>, Box<Finding>> {
    let listed = ctx.files();
    if ctx.is_in_memory() || !baseline::is_work_tree(ctx.root()) {
        return Ok(listed.iter().cloned().collect());
    }
    match baseline::git(ctx.root(), &["ls-files", "-z"]) {
        Ok(out) => {
            let on_disk: HashSet<&str> = listed.iter().map(String::as_str).collect();
            Ok(out.split('\0').filter(|p| on_disk.contains(p)).map(str::to_string).collect())
        }
        Err((cmd, why)) => Err(Box::new(
            Finding::error(
                "C173",
                RULE,
                ALLOW,
                1,
                format!(
                    "not measured: {cmd} failed ({}), so the tracked tooling files cannot be listed",
                    why.trim()
                ),
                "Run it in the repository's work tree and fix what git says.",
            )
            .unwaivable()
            .with_outcome(Outcome::NotMeasured),
        )),
    }
}

/// The check.
pub struct C173;

impl C173 {
    /// The run, with the growth guard's `spec` (the tests use a blocking one).
    pub fn run_with(&self, ctx: &Ctx, spec: &'static allow_list::Spec) -> CheckOutput {
        let (mut allow, ()) = match allow_list::read(ctx, spec, allow_list::entries_only(ALLOW)) {
            Ok(read) => read,
            Err(broken) => return *broken,
        };
        let mut findings = Vec::new();
        let files = match tracked(ctx) {
            Ok(files) => files,
            Err(f) => return allow.output(ctx, vec![*f]),
        };
        for f in files.iter().filter(|f| is_tooling_file(f)) {
            if let Err(unread) = allow.read(ctx, f) {
                findings.push(*unread);
                continue;
            }
            if allow.admit(f, |site| site == SITE) != Admit::Allowed {
                findings.push(Finding::error(
                    "C173",
                    RULE,
                    f,
                    1,
                    format!("new non-Rust tooling file `{f}`: tooling is written in Rust and run through `./dev` (ADR 0022 §3.7)"),
                    FIX,
                ));
            }
        }
        allow.output(ctx, findings)
    }
}

impl Check for C173 {
    fn id(&self) -> &'static str {
        "C173"
    }

    fn rule(&self) -> u32 {
        RULE
    }

    fn title(&self) -> &'static str {
        "No new shell, PowerShell, Python or JavaScript tooling file outside the allow list and its shrink-only baseline"
    }

    fn scope(&self) -> CheckScope {
        CheckScope::Root
    }

    fn mode(&self) -> Mode {
        Mode::Gate
    }

    fn origin(&self) -> super::Origin {
        super::Origin::BornInRust
    }

    fn allow_list(&self) -> Option<&'static allow_list::Spec> {
        Some(&SPEC)
    }

    fn control(&self) -> Control {
        Control {
            files: vec![
                (
                    ALLOW,
                    "{\"allow\":[{\"path\":\"scripts/old.sh\",\"site\":\"tooling file\",\"reason\":\"baseline: ported later\"}]}",
                ),
                ("scripts/new.sh", "#!/bin/sh\n"),
                ("conformance/src/new.mjs", "export {};\n"),
                ("scripts/old.sh", "#!/bin/sh\n"),
                ("scripts/data.json", "{}\n"),
                ("ui/app.mjs", "export {};\n"),
                ("tools/x/tests/fixtures/f.sh", "#!/bin/sh\n"),
                ("tools/x/run.spec.ts", "export {};\n"),
            ],
            expect: vec![("conformance/src/new.mjs", 1), ("scripts/new.sh", 1)],
        }
    }

    fn run(&self, ctx: &Ctx) -> CheckOutput {
        self.run_with(ctx, &SPEC)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tooling_extensions_in_tooling_paths_are_files_of_the_check() {
        for f in [
            "scripts/a.sh",
            "tools/x/y.ps1",
            "conformance/src/c.mjs",
            ".gitea/actions/z/run.py",
            "ci-image/v.sh",
            "apps/dyno/scripts/wp0/a.cjs",
            "scripts/test-helpers/s.ts",
            "tools/a.js",
        ] {
            assert!(is_tooling_file(f), "{f}");
        }
    }

    #[test]
    fn other_kinds_other_paths_data_and_suites_are_not() {
        for f in [
            "scripts/a.json",
            "scripts/a.rs",
            "scripts/a",
            "ui/a.mjs",
            "apps/dyno/app/e2e/a.ts",
            "apps/dyno/scriptsx/a.sh",
            "tools/paddock-dev/tests/golden/steps/a.mjs",
            "tools/paddock-dev/src/hook/fixtures/a.sh",
            "tools/a/x.spec.ts",
            "scripts/y.test.ts",
            "scripts/playwright.config.ts",
            "tools/vitest.config.ts",
        ] {
            assert!(!is_tooling_file(f), "{f}");
        }
    }

    #[test]
    fn a_file_named_like_a_fixture_folder_is_still_a_file_of_the_check() {
        assert!(is_tooling_file("scripts/fixtures.sh"));
        assert!(is_tooling_file("scripts/golden.mjs"));
    }
}
