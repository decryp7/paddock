//! C146: a file that starts a `notify` watcher reads its results through paddock-watch's shared mapping
//! (`paddock_watch::NotifySource::change_from`, which turns a Rescan flag or an error into a rescan with its cause). ER-21 class "a
//! watcher adapter loses notify's Rescan": paddock-watch sent a change only `if res.is_ok()` (fixed in d6b5c79ee), and Debrief's live
//! tail passed a Rescan with no path on as an empty batch that looked at nothing. Apple's FSEvents guide and Microsoft's
//! `ReadDirectoryChangesW` page both say a dropped-events notice means a full scan; share first (rule 42): one mapping, written once.
//!
//! Flagged, one finding per line, in Rust code outside `crates/paddock-watch/` (comments and string literals are not read): a line
//! that starts a watcher (`recommended_watcher(`, `RecommendedWatcher::new(`, `PollWatcher::new(`, `INotifyWatcher::new(`,
//! `FsEventWatcher::new(`, `KqueueWatcher::new(`, `ReadDirectoryChangesWatcher::new(`, `<_ as Watcher>::new(`, `new_debouncer(`,
//! `new_debouncer_opt(`) or implements notify's `EventHandler`, in a file that never calls `NotifySource::change_from(` (a
//! function of another type that is also called `change_from` does not count). A site that has to read
//! notify's results another way carries `// conformance: notify-direct — <why>` on its line or the line above (listed as waived).
//!
//! Not flagged, said out loud: a watcher started through a helper of another file (the helper's file is the one read), a
//! `NotifySource` imported under another name, and a file that calls `NotifySource::change_from(` but still drops a Rescan elsewhere (the check sees the call, not every path through the handler). The
//! check's own files are not read ([`IMPLEMENTATION`]).
//!
//! **Report-only (warnings) until its first green run on the real tree is recorded here (ER-41).** Recorded: pending (2026-11-21): the
//! train integrator records the run id of the first green run on the real tree here and in docs/gates.toml, and only then is `mode`
//! switched to blocking. Rust host only (no JavaScript module, pair P3).

use super::finding::Finding;
use super::inline_waiver::{CommentView, Kind};
use super::jsre::{line_at, rx};
use super::mask::mask_rust;
use super::record::Precondition;
use super::{Check, CheckOutput, Control, Ctx};
use regex::Regex;
use std::sync::LazyLock;

/// The check's own files: they name the patterns, their invented lines are test data.
pub const IMPLEMENTATION: [&str; 2] = ["tools/paddock-dev/src/check/c146.rs", "tools/paddock-dev/tests/it/check_c146.rs"];

/// The waiver tag.
pub const TAG: &str = "notify-direct";

static SITE: LazyLock<Regex> = LazyLock::new(|| {
    rx(concat!(
        r"\brecommended_watcher\s*\(",
        r"|\b(?:Recommended|Poll|INotify|FsEvent|Kqueue|ReadDirectoryChanges)Watcher::new\s*\(",
        r"|<\s*\w+\s+as\s+(?:notify::)?Watcher\s*>::new\s*\(",
        r"|\bnew_debouncer(?:_opt)?\s*\(",
        r"|\bimpl\b[^{;]*\bEventHandler\s+for\b",
    ))
});
/// The shared mapping, named through its type: another function called `change_from` is not it.
static SHARED: LazyLock<Regex> = LazyLock::new(|| rx(r"\bNotifySource::change_from\s*\("));
static NOT_READ: LazyLock<Regex> = LazyLock::new(|| rx(r"(?:^|/)(?:target|vendor|node_modules)/|^crates/paddock-watch/"));

const FIX: &str = "Read each notify result through `paddock_watch::NotifySource::change_from` (feature `notify`): a `Change::Rescan` means look at everything the watch covers, with its cause in the log. A site that has to read notify's results another way carries `// conformance: notify-direct — <why>`.";

/// Whether `path` is a Rust file this check reads.
pub fn readable(path: &str) -> bool {
    path.ends_with(".rs") && !NOT_READ.is_match(path) && !IMPLEMENTATION.contains(&path)
}

/// The 1-based lines of `text` (the Rust file `path`) that start a notify watcher or implement its handler, in a file that does not
/// call the shared mapping. Comments and literals are masked first.
pub fn sites(path: &str, text: &str) -> Vec<usize> {
    if !readable(path) || !text.contains("notify") && !text.contains("Watcher") && !text.contains("debouncer") {
        return Vec::new();
    }
    let code = mask_rust(text);
    if SHARED.is_match(&code) {
        return Vec::new();
    }
    let mut lines: Vec<usize> = SITE.find_iter(&code).map(|m| line_at(&code, m.start())).collect();
    lines.dedup();
    lines
}

/// The check.
pub struct C146;

impl Check for C146 {
    fn id(&self) -> &'static str {
        "C146"
    }

    fn rule(&self) -> u32 {
        42
    }

    fn title(&self) -> &'static str {
        "a notify watcher reads its results through paddock-watch's shared mapping (change_from)"
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
                    "apps/x/src/watch.rs",
                    "fn start() {\n    let w = notify::recommended_watcher(move |res| tx.send(res));\n}\n",
                ),
                (
                    "apps/x/src/shared.rs",
                    "fn start() {\n    let w = notify::recommended_watcher(move |res| tx.send(NotifySource::change_from(&res)));\n}\n",
                ),
                (
                    "apps/x/src/waived.rs",
                    "fn start() {\n    // conformance: notify-direct — the poller feeds a test double\n    let w = notify::PollWatcher::new(h, c);\n}\n",
                ),
                (
                    "apps/x/src/comment.rs",
                    "// notify::recommended_watcher(handler) was used here\nfn a() {}\n",
                ),
                ("crates/paddock-watch/src/native.rs", "fn w() { notify::recommended_watcher(h); }\n"),
            ],
            expect: vec![("apps/x/src/watch.rs", 2)],
        }
    }

    fn run(&self, ctx: &Ctx) -> CheckOutput {
        let mut findings = Vec::new();
        let mut probes = 0;
        for f in ctx.files().iter().filter(|f| readable(f)) {
            let Some(text) = ctx.read(f) else { continue };
            probes += 1;
            let found = sites(f, &text);
            if found.is_empty() {
                continue;
            }
            let view = CommentView::of(&text, Kind::Rust);
            for line in found {
                let site = Finding::warn(
                    "C146",
                    42,
                    f,
                    line,
                    "starts a notify watcher whose results skip paddock-watch's shared mapping: a Rescan (dropped events) or an error can be lost",
                    FIX,
                );
                findings.push(view.waiver(line, TAG).apply(site, || {
                    Finding::warn(
                        "C146",
                        42,
                        f,
                        line,
                        "a notify-direct waiver without a reason",
                        "Give the reason after the dashes: `// conformance: notify-direct — <why>`.",
                    )
                }));
            }
        }
        CheckOutput {
            findings,
            probes,
            completed: true,
            precondition: Precondition {
                met: true,
                detail: "the Rust files of the run, outside crates/paddock-watch".to_string(),
            },
        }
    }
}
