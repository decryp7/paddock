//! `disk report`: what is on each disk, who owns it, what state it is in and how much is reclaimable. It reads the index; an
//! index that is missing, empty or corrupt is rebuilt from the owner records first (without sizes, which `disk scan` measures).

use super::check::free_bytes;
use super::config::{CommonArgs, Settings, gib};
use super::index::{FolderRow, Index, growth};
use super::scan::{Sizes, Snapshot, scan};
use super::state::State;
use crate::worktree::note::{format_utc, now_secs};
use clap::Args;
use paddock_cli::{CliError, Report};
use serde_json::json;
use std::collections::BTreeMap;

/// `disk report`'s arguments.
#[derive(Args, Debug, Clone)]
pub struct ReportArgs {
    #[command(flatten)]
    /// The shared settings flags.
    pub common: CommonArgs,
    /// List every orphan (default: the largest 25 per disk).
    #[arg(long)]
    pub all: bool,
    /// Markdown for a job summary: totals, growth and the largest folders only.
    #[arg(long)]
    pub summary: bool,
}

const ORPHANS_SHOWN: usize = 25;
const WEEK: u64 = 7 * 86_400;

/// Loads the folders and sizes the index has, rebuilding it first when it cannot be used. Returns the snapshot, the size
/// history and the notes to print.
pub fn load(s: &Settings, now: u64, note: &mut dyn FnMut(&str)) -> (Snapshot, BTreeMap<String, Vec<super::index::SizeRow>>) {
    let (index, mut notes) = Index::open_or_recover(&s.index, now);
    let mut index = index;
    let stored = index.as_ref().and_then(|i| i.folders().ok()).unwrap_or_default();
    let (mut snap, history) = if stored.is_empty() {
        note("disk: no usable index; rebuilding it from the owner records (sizes come with `./dev disk scan`)");
        let snap = scan(s, Sizes::Skip, index.as_mut(), note);
        let history = index.as_ref().and_then(|i| i.history().ok()).unwrap_or_default();
        (snap, history)
    } else {
        let sizes = index.as_ref().and_then(|i| i.latest_sizes().ok()).unwrap_or_default();
        let history = index.as_ref().and_then(|i| i.history().ok()).unwrap_or_default();
        (
            Snapshot {
                folders: stored,
                sizes,
                notes: Vec::new(),
            },
            history,
        )
    };
    notes.append(&mut snap.notes);
    snap.notes = notes;
    (snap, history)
}

fn ago(now: u64, then: u64) -> String {
    if then == 0 {
        return "-".to_string();
    }
    let d = now.saturating_sub(then);
    if d >= 2 * 86_400 {
        format!("{}d", d / 86_400)
    } else if d >= 3600 {
        format!("{}h", d / 3600)
    } else {
        format!("{}m", d / 60)
    }
}

/// Per-state totals: count, bytes of what is measured, and how many folders have no size yet.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Totals {
    /// Folders.
    pub count: usize,
    /// Measured bytes.
    pub bytes: u64,
    /// Folders without a measurement.
    pub unmeasured: usize,
}

/// The totals by state, in state order.
pub fn totals(snap: &Snapshot) -> BTreeMap<State, Totals> {
    let mut out: BTreeMap<State, Totals> = BTreeMap::new();
    for f in &snap.folders {
        let state = State::parse(&f.state).unwrap_or(State::Unsaved);
        let t = out.entry(state).or_default();
        t.count += 1;
        match snap.sizes.get(&f.path) {
            Some(s) => t.bytes += s.bytes,
            None => t.unmeasured += 1,
        }
    }
    out
}

fn size_of(snap: &Snapshot, f: &FolderRow) -> Option<u64> {
    snap.sizes.get(&f.path).map(|s| s.bytes)
}

fn size_text(snap: &Snapshot, f: &FolderRow) -> String {
    match snap.sizes.get(&f.path) {
        Some(s) => format!("{}{}", gib(s.bytes), if s.partial { "+" } else { "" }),
        None => "not measured".to_string(),
    }
}

fn totals_text(t: &BTreeMap<State, Totals>) -> String {
    let mut text = String::from("Reclaimable by state (clean --apply removes only merged and stale-pushed):\n");
    for state in [State::Merged, State::StalePushed, State::Unsaved, State::Orphan, State::Live] {
        let x = t.get(&state).cloned().unwrap_or_default();
        text.push_str(&format!(
            "  {:<13} {:>4} folder(s)  {:>11}{}\n",
            state.word(),
            x.count,
            gib(x.bytes),
            if x.unmeasured > 0 {
                format!("  ({} not measured)", x.unmeasured)
            } else {
                String::new()
            }
        ));
    }
    let r: u64 = [State::Merged, State::StalePushed]
        .iter()
        .filter_map(|s| t.get(s))
        .map(|x| x.bytes)
        .sum();
    text.push_str(&format!("  reclaimable now: {}\n", gib(r)));
    text
}

/// Runs the report.
pub fn run(a: &ReportArgs, env: &dyn Fn(&str) -> Option<String>, note: &mut dyn FnMut(&str)) -> Result<Report, CliError> {
    let s = Settings::resolve(&a.common, env);
    let now = now_secs();
    let (snap, history) = load(&s, now, note);
    let t = totals(&snap);
    let grew = growth(&history, now, WEEK, 10);
    let reclaimable: u64 = [State::Merged, State::StalePushed]
        .iter()
        .filter_map(|x| t.get(x))
        .map(|x| x.bytes)
        .sum();
    let scanned = snap.sizes.values().map(|r| r.measured_at).max();
    let df = env("PADDOCK_DF").filter(|v| !v.is_empty()).unwrap_or_else(|| "df".to_string());

    let mut text = String::new();
    text.push_str(&format!(
        "Disk report: {} tracked folder(s) under {} root(s); index {}; sizes measured {}\n",
        snap.folders.len(),
        s.roots.len(),
        s.index.display(),
        scanned.map_or("never (run ./dev disk scan)".to_string(), |m| format!("{} ago", ago(now, m)))
    ));
    for n in &snap.notes {
        text.push_str(&format!("note: {n}\n"));
    }
    let mut disks = Vec::new();
    for root in &s.roots {
        let free = free_bytes(root, &df).ok();
        let mut rows: Vec<&FolderRow> = snap
            .folders
            .iter()
            .filter(|f| std::path::Path::new(&f.path).starts_with(root))
            .collect();
        rows.sort_by(|x, y| size_of(&snap, y).cmp(&size_of(&snap, x)).then_with(|| x.path.cmp(&y.path)));
        let disk_total: u64 = rows.iter().filter_map(|f| size_of(&snap, f)).sum();
        if !a.summary {
            text.push_str(&format!(
                "\n{}  free {}  tracked folders {}\n",
                root.display(),
                free.map_or("not measured".to_string(), gib),
                gib(disk_total)
            ));
            text.push_str("  size          state         kind      last used  owner / branch  folder\n");
            let mut orphans_shown = 0;
            let mut hidden = 0;
            for f in &rows {
                if f.state == State::Orphan.word() && !a.all {
                    if orphans_shown >= ORPHANS_SHOWN {
                        hidden += 1;
                        continue;
                    }
                    orphans_shown += 1;
                }
                let who = match (f.owner.is_empty(), f.branch.is_empty()) {
                    (true, _) => "-".to_string(),
                    (false, true) => f.owner.clone(),
                    (false, false) => format!("{} / {}", f.owner, f.branch),
                };
                text.push_str(&format!(
                    "  {:<13} {:<13} {:<9} {:<10} {}  {}\n",
                    size_text(&snap, f),
                    f.state,
                    if f.kind.is_empty() { "-" } else { &f.kind },
                    ago(now, f.last_used),
                    who,
                    std::path::Path::new(&f.path)
                        .strip_prefix(root)
                        .map_or(f.path.clone(), |p| p.display().to_string())
                ));
            }
            if hidden > 0 {
                text.push_str(&format!("  ... {hidden} more orphan folder(s) (--all lists them)\n"));
            }
        }
        disks.push(
            json!({"root": root.display().to_string(), "free_bytes": free, "tracked_bytes": disk_total,
                          "folders": rows.iter().map(|f| json!({
                              "path": f.path, "kind": f.kind, "owner": f.owner, "branch": f.branch, "head": f.head,
                              "created": f.created, "last_used": f.last_used, "state": f.state,
                              "bytes": size_of(&snap, f), "partial": snap.sizes.get(&f.path).map(|x| x.partial),
                          })).collect::<Vec<_>>()}),
        );
    }
    text.push('\n');
    text.push_str(&totals_text(&t));
    text.push_str("\nGrew most in the last 7 days:\n");
    if grew.is_empty() {
        text.push_str("  nothing to compare yet (needs two scans at least a day apart)\n");
    }
    for g in &grew {
        text.push_str(&format!("  +{:<11} now {:<11} {}\n", gib(g.grew_bytes), gib(g.now_bytes), g.path));
    }
    if a.summary {
        let mut big: Vec<&FolderRow> = snap.folders.iter().collect();
        big.sort_by_key(|x| std::cmp::Reverse(size_of(&snap, x)));
        text.push_str("\nLargest folders:\n");
        for f in big.iter().take(10) {
            text.push_str(&format!("  {:<13} {:<13} {}\n", size_text(&snap, f), f.state, f.path));
        }
        text = format!("### Disk usage (as of {})\n\n```\n{text}```\n", format_utc(now));
    }
    Ok(Report::new(
        json!({
            "index": s.index.display().to_string(),
            "disks": disks,
            "totals": t.iter().map(|(k, v)| (k.word().to_string(), json!({"count": v.count, "bytes": v.bytes, "unmeasured": v.unmeasured}))).collect::<serde_json::Map<_, _>>(),
            "reclaimable_bytes": reclaimable,
            "grew_7d": grew.iter().map(|g| json!({"path": g.path, "now_bytes": g.now_bytes, "grew_bytes": g.grew_bytes})).collect::<Vec<_>>(),
            "notes": snap.notes,
        }),
        text,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::disk::index::SizeRow;

    fn row(path: &str, state: &str) -> FolderRow {
        FolderRow {
            path: path.into(),
            state: state.into(),
            kind: "build".into(),
            ..FolderRow::default()
        }
    }

    fn size(path: &str, bytes: u64) -> (String, SizeRow) {
        (
            path.into(),
            SizeRow {
                path: path.into(),
                bytes,
                partial: false,
                measured_at: 1,
            },
        )
    }

    #[test]
    fn totals_add_up_by_state_and_count_unmeasured_folders() {
        let snap = Snapshot {
            folders: vec![
                row("/a", "merged"),
                row("/b", "merged"),
                row("/c", "stale-pushed"),
                row("/d", "orphan"),
            ],
            sizes: [size("/a", 10), size("/b", 20), size("/c", 5)].into_iter().collect(),
            notes: vec![],
        };
        let t = totals(&snap);
        assert_eq!(
            t[&State::Merged],
            Totals {
                count: 2,
                bytes: 30,
                unmeasured: 0
            }
        );
        assert_eq!(t[&State::StalePushed].bytes, 5);
        assert_eq!(
            t[&State::Orphan],
            Totals {
                count: 1,
                bytes: 0,
                unmeasured: 1
            }
        );
    }

    #[test]
    fn ages_read_in_minutes_hours_and_days() {
        assert_eq!(ago(10_000, 10_000 - 120), "2m");
        assert_eq!(ago(100_000, 100_000 - 7_300), "2h");
        assert_eq!(ago(1_000_000, 1_000_000 - 3 * 86_400), "3d");
        assert_eq!(ago(5, 0), "-");
    }
}
