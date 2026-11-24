// ci-runs-reader: the fix items of a run that is green with fix items (ADR 0117 WP-4, ER-12)
//! The one accessor that records the fix items of a run that is green with fix items ([`super::green::GradedResult`]).
//!
//! ER-12: a FLAKY test of a crate or app the change did not reach lands, and is recorded with its log as a fix item for the next train.
//! Three places read what this module writes, and none formats it twice:
//! - the pull request comment the queue writes before it lands ([`lines`], read back by [`already_commented`], so a retried tick
//!   never writes it twice);
//! - the landing comment ([`lines`] again);
//! - the project's fix-item home, `docs/lessons.md` ([`record`]: the bullet of [`lessons_entry`], in the shape of the earlier ones,
//!   for example f707a4965, added once whatever the number of calls). The queue cannot commit, so a landing, the queue's or an
//!   integrator's by hand, records here with `./dev ci fix-items --workflow <file> --sha <sha>` on the landed commit.

use super::green::FixItem;
use std::path::Path;

/// The fix-item home: the lessons file of the repository.
pub const LESSONS: &str = "docs/lessons.md";

/// One line a comment gives for each item: workflow, run, job (with its id), test and the job's page.
pub fn lines(workflow: &str, run: u64, items: &[FixItem]) -> Vec<String> {
    items.iter().map(|i| format!("- {workflow}: {}", i.line(run))).collect()
}

/// Whether `existing` (the comments already on a pull request) holds exactly this comment: a retried tick writes nothing twice.
pub fn already_commented(existing: &[String], text: &str) -> bool {
    existing.iter().any(|c| c.trim() == text.trim())
}

fn short(sha: &str) -> &str {
    sha.get(..9).unwrap_or(sha)
}

/// The lessons bullet for the fix items of `workflow` run `run` on `sha`, in the shape of the earlier ones.
pub fn lessons_entry(workflow: &str, run: u64, sha: &str, items: &[FixItem]) -> String {
    let mut out = format!(
        "- **From the full {workflow} run {run} ({}), in crates and apps the change did not reach (each FLAKY: failed once, passed its retry):**\n",
        short(sha)
    );
    for i in items {
        out.push_str(&format!("  `{}` (job {}, {}; log {}).\n", i.test, i.job_id, i.job, i.url));
    }
    out.push_str("  Next: a failing reproduction for each first.\n");
    out
}

/// Whether the file already holds every item of the run: the run's name and each job id, as [`lessons_entry`] writes them.
fn recorded(text: &str, workflow: &str, run: u64, items: &[FixItem]) -> bool {
    text.contains(&format!("full {workflow} run {run} "))
        && items.iter().all(|i| text.contains(&format!("`{}` (job {},", i.test, i.job_id)))
}

/// The lessons text with the entry added at the end, `None` when the run's items are already in it.
pub fn add_to_lessons(text: &str, workflow: &str, run: u64, sha: &str, items: &[FixItem]) -> Option<String> {
    if items.is_empty() || recorded(text, workflow, run, items) {
        return None;
    }
    let mut out = text.to_string();
    if !out.ends_with('\n') {
        out.push('\n');
    }
    // After a paragraph the bullet needs a blank line; after another bullet it continues the list.
    let last = text.trim_end().lines().last().unwrap_or("");
    if !(last.starts_with("- ") || last.starts_with("  ")) {
        out.push('\n');
    }
    out.push_str(&lessons_entry(workflow, run, sha, items));
    Some(out)
}

/// What [`record`] did.
#[derive(Debug, PartialEq, Eq)]
pub enum Recorded {
    /// The entry was added to the lessons file.
    Added,
    /// Every item was in it already; nothing was written.
    Already,
}

/// Records the fix items in the lessons file of the checkout at `root`.
pub fn record(root: &Path, workflow: &str, run: u64, sha: &str, items: &[FixItem]) -> Result<Recorded, String> {
    let file = root.join(LESSONS);
    let text = std::fs::read_to_string(&file)
        .map_err(|e| format!("{} cannot be read ({e}): run it in a checkout of the repository", file.display()))?;
    match add_to_lessons(&text, workflow, run, sha, items) {
        None => Ok(Recorded::Already),
        Some(next) => {
            std::fs::write(&file, next).map_err(|e| format!("{} cannot be written ({e})", file.display()))?;
            Ok(Recorded::Added)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn item(id: u64, test: &str) -> FixItem {
        FixItem {
            job: "crate (windows-latest)".into(),
            job_id: id,
            test: test.into(),
            url: format!("https://gitea.invalid/example/paddock/actions/runs/8610/jobs/{id}"),
        }
    }

    const SHA: &str = "b3c254a2d50f1f21e4ce1be3721994e8070e6be5";

    #[test]
    fn the_entry_has_the_shape_of_the_earlier_fix_item_blocks_and_is_added_once() {
        let items = [
            item(44761, "paddock-chromium::it real_browser::a_test"),
            item(44768, "spotter-render::real_browser b_test"),
        ];
        // The end of a list: a bullet continues it, the file stays whole, and C145's rule (a blank line before a heading) holds.
        let lessons = "# Lessons\n\n## A section\n- **From train 18:** `x` (job 1).\n  Next: a failing reproduction for each first.\n";
        let once = add_to_lessons(lessons, "ci.yml", 8610, SHA, &items).expect("added");
        assert!(
            once.starts_with(lessons) && once.contains("- **From the full ci.yml run 8610 (b3c254a2d)"),
            "{once}"
        );
        assert!(
            once.contains("`paddock-chromium::it real_browser::a_test` (job 44761, crate (windows-latest); log https://gitea.invalid/")
        );
        assert!(once.ends_with("  Next: a failing reproduction for each first.\n"));
        assert!(!once.contains("\n\n- **From the full"), "it continues the list: {once}");
        assert!(crate::check::c145::lessons_sites(&once).iter().all(|s| s.kind != "glued"));
        // Idempotent: the same run again adds nothing; another run adds its own.
        assert_eq!(add_to_lessons(&once, "ci.yml", 8610, SHA, &items), None);
        assert!(add_to_lessons(&once, "ci.yml", 8611, SHA, &items).is_some());
        // After a paragraph the bullet gets its blank line.
        let para = add_to_lessons("# L\n\nA paragraph.\n", "ci.yml", 8610, SHA, &items[..1]).unwrap();
        assert!(para.contains("A paragraph.\n\n- **From the full"), "{para}");
    }

    #[test]
    fn record_writes_the_file_once_and_names_a_missing_one() {
        let dir = tempfile::tempdir().unwrap();
        let items = [item(44761, "t::a")];
        let e = record(dir.path(), "ci.yml", 8610, SHA, &items).unwrap_err();
        assert!(e.contains("cannot be read"), "{e}");
        std::fs::create_dir_all(dir.path().join("docs")).unwrap();
        std::fs::write(dir.path().join(LESSONS), "# Lessons\n").unwrap();
        assert_eq!(record(dir.path(), "ci.yml", 8610, SHA, &items), Ok(Recorded::Added));
        assert_eq!(record(dir.path(), "ci.yml", 8610, SHA, &items), Ok(Recorded::Already));
        let text = std::fs::read_to_string(dir.path().join(LESSONS)).unwrap();
        assert_eq!(text.matches("run 8610").count(), 1, "{text}");
    }

    #[test]
    fn a_comment_already_on_the_pull_request_is_not_written_again() {
        let text = format!("queue: x\n{}", lines("ci.yml", 8610, &[item(44761, "t::a")]).join("\n"));
        assert!(already_commented(&["other".into(), format!("{text}\n")], &text));
        assert!(!already_commented(&["other".into()], &text));
    }
}
