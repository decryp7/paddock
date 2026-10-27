//! Watched folders with keep-for times (WP-10, ADR 0019). The core decides what may be watched and what is due
//! ([`dyno_core::watch`]); this module lists folders, re-checks every file just before it moves, moves it into the
//! quarantine (restorable for seven days) and remembers the list in `watched-folders.json`.
//!
//! Deletion only ever happens to a folder the person chose in the OS dialog, configured (keep-for time), previewed and
//! turned on; only files directly or (when asked) below it, never links, never in use, never changed since they were listed.
//! A periodic sweep catches everything; file-change notifications are deliberately not used (ADR 0019).

use crate::cleanup::quarantine::Quarantine;
use crate::files;
use dyno_core::cleanup::{is_inside, Os};
use dyno_core::ports::FileLocks;
use dyno_core::time::Timestamp;
use dyno_core::watch::{
    check_keep, overlaps, plan, template_filter, template_infos, validate_root, FoundFile, SweepPlan, SweepSummary, WatchFilter,
    WatchOverview, WatchPreview, WatchedFolder, WatchedFolderView, WatchedFolders, MAX_DEPTH, MAX_FOLDERS, MAX_SCANNED,
};
use std::path::{Path, PathBuf};
use std::sync::Mutex;

/// What a listing found.
pub struct Listing {
    /// The files (links included, flagged).
    pub files: Vec<FoundFile>,
}

fn millis(t: std::time::SystemTime) -> Timestamp {
    Timestamp(
        t.duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |d| i64::try_from(d.as_millis()).unwrap_or(i64::MAX)),
    )
}

/// Lists the files of `root` (and below it when `recursive`, at most [`MAX_DEPTH`] levels), never following a link. It
/// stops after [`MAX_SCANNED`] + 1 entries, which the plan reports as truncated. `trust_created` reads the creation time.
pub fn list_folder(root: &Path, recursive: bool, trust_created: bool) -> std::io::Result<Listing> {
    let mut files = Vec::new();
    let mut stack: Vec<(PathBuf, usize)> = vec![(root.to_path_buf(), 0)];
    while let Some((dir, depth)) = stack.pop() {
        let rd = match std::fs::read_dir(&dir) {
            Ok(rd) => rd,
            Err(e) if dir == root => return Err(e),
            Err(_) => continue,
        };
        for entry in rd.flatten() {
            if files.len() > MAX_SCANNED {
                return Ok(Listing { files });
            }
            let path = entry.path();
            let Ok(meta) = std::fs::symlink_metadata(&path) else { continue };
            let name = entry.file_name().to_string_lossy().into_owned();
            let link = meta.file_type().is_symlink();
            if meta.is_dir() && !link {
                if recursive && depth < MAX_DEPTH {
                    stack.push((path, depth + 1));
                }
                continue;
            }
            files.push(FoundFile {
                path: path.to_string_lossy().into_owned(),
                name,
                bytes: meta.len(),
                modified: meta.modified().map_or(Timestamp(0), millis),
                created: if trust_created { meta.created().ok().map(millis) } else { None },
                link,
            });
        }
    }
    Ok(Listing { files })
}

/// Strips the `\\?\` prefix Windows' `canonicalize` adds.
fn display(p: &Path) -> String {
    let s = p.to_string_lossy();
    s.strip_prefix(r"\\?\").unwrap_or(&s).to_string()
}

/// What one sweep needs from outside.
pub struct SweepCtx<'a> {
    /// The user's home folder.
    pub home: &'a str,
    /// The quarantine.
    pub quarantine: &'a Quarantine,
    /// Who holds a file open.
    pub locks: &'a mut dyn FileLocks,
    /// The id of the quarantine batch to create.
    pub batch_id: String,
    /// Read creation times (production); tests with temp files turn it off.
    pub trust_created: bool,
}

/// Lists and plans `folder` without touching anything.
pub fn dry_run(folder: &WatchedFolder, home: &str, now: Timestamp, trust_created: bool) -> Result<SweepPlan, String> {
    let root = Path::new(&folder.path);
    let meta = root
        .symlink_metadata()
        .map_err(|_| "The folder is not there any more. Choose it again or stop watching it.".to_string())?;
    if meta.file_type().is_symlink() || !meta.is_dir() {
        return Err("The watched path is not a plain folder any more.".into());
    }
    let listing = list_folder(root, folder.recursive, trust_created).map_err(|e| format!("The folder cannot be read: {e}"))?;
    Ok(plan(folder, &listing.files, home, now))
}

/// Moves what is due in `folder` to the quarantine, re-checking each file first. The summary says what happened.
pub fn sweep(folder: &WatchedFolder, now: Timestamp, ctx: SweepCtx<'_>) -> Result<SweepSummary, String> {
    let planned = dry_run(folder, ctx.home, now, ctx.trust_created)?;
    let mut summary = SweepSummary {
        at: now,
        moved: 0,
        bytes: 0,
        failed: 0,
        first_failure: None,
        quarantine_id: None,
    };
    if planned.due.is_empty() {
        return Ok(summary);
    }
    let root = std::fs::canonicalize(&folder.path).map_err(|e| format!("The folder cannot be read: {e}"))?;
    let root_text = display(&root);
    let mut batch = ctx.quarantine.begin(&ctx.batch_id, now)?;
    let fail = |summary: &mut SweepSummary, why: String| {
        summary.failed += 1;
        summary.first_failure.get_or_insert(why);
    };
    for item in &planned.due {
        let path = Path::new(&item.path);
        let meta = match path.symlink_metadata() {
            Ok(m) => m,
            Err(_) => {
                fail(&mut summary, format!("{} is no longer there.", item.path));
                continue;
            }
        };
        if meta.file_type().is_symlink() || !meta.is_file() {
            fail(&mut summary, format!("{} is not a plain file any more.", item.path));
            continue;
        }
        if meta.modified().map_or(Timestamp(0), millis) != item.modified {
            fail(&mut summary, format!("{} changed after it was listed; it stays.", item.path));
            continue;
        }
        // A link in the middle of the path would send the move somewhere else: use the real parent and check it.
        let (Some(parent), Some(name)) = (path.parent(), path.file_name()) else {
            fail(&mut summary, format!("{} has no folder.", item.path));
            continue;
        };
        let Ok(real_parent) = std::fs::canonicalize(parent) else {
            fail(&mut summary, format!("The folder of {} cannot be read.", item.path));
            continue;
        };
        let real = real_parent.join(name);
        if !is_inside(&display(&real), &root_text) {
            fail(&mut summary, format!("{} is outside the watched folder; it stays.", item.path));
            continue;
        }
        if let Ok(found) = ctx.locks.lockers(&real) {
            if let Some(first) = found.first() {
                fail(&mut summary, format!("{} is in use by {}; it stays.", item.path, first.name));
                continue;
            }
        }
        match batch.stage(&real, meta.len()) {
            Ok(()) => {
                summary.moved += 1;
                summary.bytes = summary.bytes.saturating_add(meta.len());
            }
            Err(why) => fail(&mut summary, why),
        }
    }
    let id = batch.id().to_string();
    // An empty batch is removed by finish; only a batch that holds files is reported.
    let kept = batch.finish().is_some() && summary.moved > 0;
    summary.quarantine_id = kept.then_some(id);
    Ok(summary)
}

/// The list of watched folders and its file.
pub struct WatchCentre {
    path: PathBuf,
    state: Mutex<WatchedFolders>,
    counter: std::sync::atomic::AtomicU64,
}

fn guard(m: &Mutex<WatchedFolders>) -> std::sync::MutexGuard<'_, WatchedFolders> {
    m.lock().unwrap_or_else(std::sync::PoisonError::into_inner)
}

/// What the person chose when adding a folder.
#[derive(Debug, Clone, Copy)]
pub struct NewFolder<'a> {
    /// The folder from the OS dialog.
    pub chosen: &'a str,
    /// The template it started from.
    pub template: Option<&'a str>,
    /// How long files may stay, seconds.
    pub keep_for_secs: u32,
    /// Include subfolders.
    pub recursive: bool,
}

/// How a request to change a folder looks, already parsed.
#[derive(Debug, Clone, Copy, Default)]
pub struct Change {
    /// A new keep-for time, seconds.
    pub keep_for_secs: Option<u32>,
    /// Sweeping on or off.
    pub enabled: Option<bool>,
    /// Subfolders or not.
    pub recursive: Option<bool>,
}

impl WatchCentre {
    /// Reads `path`; a damaged or newer file starts an empty list and the note says so.
    pub fn load(path: PathBuf) -> (WatchCentre, Option<String>) {
        let (list, note) = files::load(&path, WatchedFolders::parse);
        // `load` returns the parse warning for a damaged file; a missing one is simply empty.
        (
            WatchCentre {
                path,
                state: Mutex::new(list.unwrap_or_default()),
                counter: std::sync::atomic::AtomicU64::new(0),
            },
            note,
        )
    }

    fn save(&self, list: &WatchedFolders) -> Result<(), String> {
        files::save(&self.path, list).map_err(|e| format!("The watched folders could not be saved: {e}"))
    }

    /// The page's data.
    pub fn overview(&self) -> WatchOverview {
        let list = guard(&self.state);
        WatchOverview {
            folders: list
                .folders
                .iter()
                .map(|f| WatchedFolderView {
                    exists: Path::new(&f.path).is_dir(),
                    filter_text: f.filter.describe(),
                    next_sweep: f
                        .enabled
                        .then(|| f.last_sweep.as_ref().map_or(f.added, |s| s.at).plus(f.sweep_every_ms())),
                    folder: f.clone(),
                })
                .collect(),
            templates: template_infos(),
        }
    }

    /// A copy of one folder.
    pub fn get(&self, id: &str) -> Option<WatchedFolder> {
        guard(&self.state).get(id).cloned()
    }

    /// The folders that are due for a sweep.
    pub fn due(&self, now: Timestamp) -> Vec<WatchedFolder> {
        guard(&self.state).folders.iter().filter(|f| f.due(now)).cloned().collect()
    }

    /// A new id.
    pub fn new_id(&self, now: Timestamp) -> String {
        let n = self.counter.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        format!("w-{}-{n}", now.millis())
    }

    /// Adds the folder the person chose, switched off. `data` is Dyno's own data folder.
    pub fn add(&self, new: NewFolder<'_>, os: Os, home: &str, data: &str, now: Timestamp) -> Result<(), String> {
        let NewFolder {
            chosen,
            template,
            keep_for_secs,
            recursive,
        } = new;
        check_keep(keep_for_secs)?;
        let filter = match template {
            Some(t) => template_filter(t).ok_or_else(|| "That template is not known.".to_string())?,
            None => WatchFilter::default(),
        };
        let real = std::fs::canonicalize(chosen).map_err(|_| "That folder cannot be found.".to_string())?;
        if !real.is_dir() {
            return Err("Choose a folder, not a file.".into());
        }
        let shown = display(&real);
        validate_root(&shown, home, data, os)?;
        let mut list = guard(&self.state);
        if list.folders.len() >= MAX_FOLDERS {
            return Err(format!("At most {MAX_FOLDERS} folders can be watched; stop watching one first."));
        }
        if let Some(other) = list.folders.iter().find(|f| overlaps(&f.path, &shown)) {
            return Err(format!("{} is already watched, and watched folders may not overlap.", other.path));
        }
        let id = self.new_id(now);
        list.folders.push(WatchedFolder {
            id,
            path: shown,
            keep_for_secs,
            filter,
            recursive,
            enabled: false,
            previewed: false,
            template: template.map(str::to_string),
            added: now,
            last_sweep: None,
        });
        self.save(&list)
    }

    /// Changes a folder. A shorter keep-for time or turning subfolders on removes what the preview vouched for, so the
    /// folder goes off until it is previewed again; turning it on needs a preview of the current settings.
    pub fn change(&self, id: &str, change: Change) -> Result<(), String> {
        let mut list = guard(&self.state);
        let f = list.get_mut(id).ok_or("Dyno no longer watches that folder.")?;
        if let Some(k) = change.keep_for_secs {
            check_keep(k)?;
            if k < f.keep_for_secs {
                f.previewed = false;
                f.enabled = false;
            }
            f.keep_for_secs = k;
        }
        if let Some(r) = change.recursive {
            if r && !f.recursive {
                f.previewed = false;
                f.enabled = false;
            }
            f.recursive = r;
        }
        if let Some(on) = change.enabled {
            if on {
                if !f.previewed {
                    return Err("Look at the preview first: it shows exactly what would be moved.".into());
                }
                if !Path::new(&f.path).is_dir() {
                    return Err("The folder is not there any more.".into());
                }
            }
            f.enabled = on;
        }
        self.save(&list)
    }

    /// Stops watching a folder (its files are left alone).
    pub fn remove(&self, id: &str) -> Result<(), String> {
        let mut list = guard(&self.state);
        let before = list.folders.len();
        list.folders.retain(|f| f.id != id);
        if list.folders.len() == before {
            return Err("Dyno no longer watches that folder.".into());
        }
        self.save(&list)
    }

    /// Remembers that the preview of the folder's current settings was shown.
    pub fn mark_previewed(&self, id: &str) -> Result<(), String> {
        let mut list = guard(&self.state);
        list.get_mut(id).ok_or("Dyno no longer watches that folder.")?.previewed = true;
        self.save(&list)
    }

    /// Stores the result of a sweep.
    pub fn record(&self, id: &str, summary: SweepSummary) {
        let mut list = guard(&self.state);
        if let Some(f) = list.get_mut(id) {
            f.last_sweep = Some(summary);
            let _ = self.save(&list);
        }
    }
}

/// The preview for the page: the dry run, remembered as shown.
pub fn preview(centre: &WatchCentre, id: &str, home: &str, now: Timestamp) -> Result<WatchPreview, String> {
    let folder = centre.get(id).ok_or("Dyno no longer watches that folder.")?;
    let planned = dry_run(&folder, home, now, true)?;
    centre.mark_previewed(id)?;
    Ok(WatchPreview::of(id, planned))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cleanup::testing::{backdate, now as real_now};
    use dyno_core::ports::PlatformError;
    use dyno_core::process::Locker;
    use std::time::Duration;

    struct NoLocks;
    impl FileLocks for NoLocks {
        fn lockers(&mut self, _p: &Path) -> Result<Vec<Locker>, PlatformError> {
            Ok(vec![])
        }
    }

    struct Held(String);
    impl FileLocks for Held {
        fn lockers(&mut self, p: &Path) -> Result<Vec<Locker>, PlatformError> {
            Ok(if p.to_string_lossy().ends_with(&self.0) {
                vec![Locker {
                    process: dyno_core::process::ProcessRef {
                        pid: 1,
                        started: Timestamp(1),
                    },
                    name: "Preview".into(),
                    kind: None,
                }]
            } else {
                vec![]
            })
        }
    }

    struct Fx {
        _t: tempfile::TempDir,
        home: String,
        dir: PathBuf,
        q: Quarantine,
    }

    fn fx() -> Fx {
        let t = tempfile::tempdir().unwrap();
        let base = std::fs::canonicalize(t.path()).unwrap();
        let dir = base.join("home").join("watched");
        std::fs::create_dir_all(&dir).unwrap();
        Fx {
            home: base.join("home").to_string_lossy().into_owned(),
            q: Quarantine::new(base.join("q")),
            dir,
            _t: t,
        }
    }

    fn put(dir: &Path, name: &str, days_old: u64) -> PathBuf {
        let p = dir.join(name);
        if let Some(parent) = p.parent() {
            std::fs::create_dir_all(parent).unwrap();
        }
        std::fs::write(&p, name.as_bytes()).unwrap();
        backdate(&p, days_old);
        p
    }

    fn folder(f: &Fx, keep: u32) -> WatchedFolder {
        WatchedFolder {
            id: "w-1".into(),
            path: f.dir.to_string_lossy().into_owned(),
            keep_for_secs: keep,
            ..Default::default()
        }
    }

    fn ctx<'a>(f: &'a Fx, locks: &'a mut dyn FileLocks, id: &str) -> SweepCtx<'a> {
        SweepCtx {
            home: &f.home,
            quarantine: &f.q,
            locks,
            batch_id: id.into(),
            trust_created: false,
        }
    }

    #[test]
    fn a_sweep_moves_only_old_files_of_the_folder_into_the_quarantine_and_restore_puts_them_back() {
        let f = fx();
        let old = put(&f.dir, "old.dmg", 5);
        let young = put(&f.dir, "young.dmg", 0);
        let keep = 2 * 86_400;
        let s = sweep(&folder(&f, keep), real_now(), ctx(&f, &mut NoLocks, "w-batch-1")).unwrap();
        assert_eq!((s.moved, s.failed), (1, 0));
        assert_eq!(s.quarantine_id.as_deref(), Some("w-batch-1"));
        assert!(!old.exists() && young.exists());
        let r = f.q.restore("w-batch-1").unwrap();
        assert_eq!(r.restored, 1);
        assert!(old.exists(), "the quarantine put it back where it was");
    }

    #[test]
    fn nothing_due_leaves_no_quarantine_batch() {
        let f = fx();
        put(&f.dir, "young.bin", 0);
        let s = sweep(&folder(&f, 86_400), real_now(), ctx(&f, &mut NoLocks, "w-batch-2")).unwrap();
        assert_eq!((s.moved, s.quarantine_id), (0, None));
        assert!(f.q.list().is_empty());
    }

    #[test]
    fn subfolders_are_swept_only_when_asked_and_links_are_never_followed() {
        let f = fx();
        let deep = put(&f.dir, "sub/deep.bin", 5);
        let outside = f._t.path().join("outside");
        std::fs::create_dir_all(&outside).unwrap();
        let secret = put(&outside, "secret.bin", 9);
        #[cfg(unix)]
        {
            std::os::unix::fs::symlink(&outside, f.dir.join("linked-dir")).unwrap();
            std::os::unix::fs::symlink(&secret, f.dir.join("linked-file")).unwrap();
        }
        let mut w = folder(&f, 86_400);
        let s = sweep(&w, real_now(), ctx(&f, &mut NoLocks, "w-b3")).unwrap();
        assert_eq!(s.moved, 0, "subfolders are not included by default");
        assert!(deep.exists());
        w.recursive = true;
        let s = sweep(&w, real_now(), ctx(&f, &mut NoLocks, "w-b4")).unwrap();
        assert_eq!(s.moved, 1);
        assert!(!deep.exists());
        assert!(secret.exists(), "a link out of the folder is never followed");
        #[cfg(unix)]
        {
            assert!(f.dir.join("linked-file").symlink_metadata().is_ok(), "the link itself stays");
            assert!(f.dir.join("linked-dir").symlink_metadata().is_ok());
        }
    }

    #[test]
    fn a_file_in_use_stays_and_the_reason_names_the_program() {
        let f = fx();
        let busy = put(&f.dir, "busy.pdf", 5);
        let free = put(&f.dir, "free.pdf", 5);
        let s = sweep(&folder(&f, 86_400), real_now(), ctx(&f, &mut Held("busy.pdf".into()), "w-b5")).unwrap();
        assert_eq!((s.moved, s.failed), (1, 1));
        assert!(s.first_failure.unwrap().contains("in use by Preview"));
        assert!(busy.exists() && !free.exists());
    }

    #[test]
    fn a_file_changed_after_the_listing_stays() {
        let f = fx();
        let p = put(&f.dir, "edit.txt", 5);
        let w = folder(&f, 86_400);
        let planned = dry_run(&w, &f.home, real_now(), false).unwrap();
        assert_eq!(planned.due.len(), 1);
        // The re-check compares the modification time it was planned with.
        let touched = std::fs::OpenOptions::new().write(true).open(&p).unwrap(); // Windows needs write access to set times
        touched
            .set_modified(std::time::SystemTime::now() - Duration::from_secs(3600))
            .unwrap();
        let meta = p.symlink_metadata().unwrap();
        assert_ne!(meta.modified().map_or(Timestamp(0), millis), planned.due[0].modified);
    }

    #[test]
    fn a_missing_or_replaced_folder_is_reported_not_swept() {
        let f = fx();
        let mut w = folder(&f, 86_400);
        w.path = f.dir.join("gone").to_string_lossy().into_owned();
        let err = sweep(&w, real_now(), ctx(&f, &mut NoLocks, "w-b6")).unwrap_err();
        assert!(err.contains("not there"));
        #[cfg(unix)]
        {
            let link = f.dir.join("as-link");
            std::os::unix::fs::symlink(&f.dir, &link).unwrap();
            w.path = link.to_string_lossy().into_owned();
            assert!(sweep(&w, real_now(), ctx(&f, &mut NoLocks, "w-b7"))
                .unwrap_err()
                .contains("not a plain folder"));
        }
    }

    #[test]
    fn housekeeping_files_and_partial_downloads_stay() {
        let f = fx();
        put(&f.dir, ".DS_Store", 30);
        put(&f.dir, "movie.mp4.crdownload", 30);
        let s = sweep(&folder(&f, 86_400), real_now(), ctx(&f, &mut NoLocks, "w-b8")).unwrap();
        assert_eq!(s.moved, 0);
    }

    fn centre(f: &Fx) -> WatchCentre {
        WatchCentre::load(f._t.path().join("watched-folders.json")).0
    }

    fn add(c: &WatchCentre, f: &Fx, path: &Path, keep: u32) -> Result<(), String> {
        c.add(
            NewFolder {
                chosen: path.to_str().unwrap(),
                template: Some("installers"),
                keep_for_secs: keep,
                recursive: false,
            },
            Os::Linux,
            &f.home,
            "/nonexistent/dyno",
            real_now(),
        )
    }

    #[test]
    fn adding_validates_the_folder_and_starts_it_off() {
        let f = fx();
        let c = centre(&f);
        add(&c, &f, &f.dir, 7 * 86_400).unwrap();
        let o = c.overview();
        assert_eq!(o.folders.len(), 1);
        let v = &o.folders[0];
        assert!(!v.folder.enabled && !v.folder.previewed && v.exists && v.next_sweep.is_none());
        assert_eq!(v.filter_text, ".dmg, .pkg, .exe, .msi, .deb, .rpm, .appimage, .iso");
        assert_eq!(o.templates.len(), 3);
        // Overlaps, bad times, unknown templates, files, missing folders and unsafe roots are refused with a reason.
        assert!(add(&c, &f, &f.dir, 7 * 86_400).unwrap_err().contains("already watched"));
        assert!(add(&c, &f, &f.dir.join("inner"), 7 * 86_400).is_err());
        let other = f._t.path().join("other");
        std::fs::create_dir_all(&other).unwrap();
        assert!(add(&c, &f, &other, 5).unwrap_err().contains("between 10 minutes and 10 days"));
        assert!(c
            .add(
                NewFolder {
                    chosen: other.to_str().unwrap(),
                    template: Some("nope"),
                    keep_for_secs: 600,
                    recursive: false,
                },
                Os::Linux,
                &f.home,
                "",
                real_now()
            )
            .unwrap_err()
            .contains("template"));
        let file = put(&other, "a.txt", 0);
        assert!(add(&c, &f, &file, 600).unwrap_err().contains("not a file"));
        assert!(add(&c, &f, &other.join("missing"), 600).unwrap_err().contains("cannot be found"));
        assert!(add(&c, &f, Path::new(&f.home), 600).is_err(), "the home folder cannot be watched");
        assert!(add(&c, &f, Path::new("/"), 600).is_err());
    }

    #[test]
    fn turning_a_folder_on_needs_its_preview_and_the_list_survives_a_restart() {
        let f = fx();
        let c = centre(&f);
        add(&c, &f, &f.dir, 7 * 86_400).unwrap();
        let id = c.overview().folders[0].folder.id.clone();
        let on = Change {
            enabled: Some(true),
            ..Default::default()
        };
        assert!(c.change(&id, on).unwrap_err().contains("preview first"));
        put(&f.dir, "old.dmg", 30);
        // Whether the file is due depends on the file system's creation time (macOS lowers it with the modification time),
        // so only the preview's effect is asserted here; the arrival rule itself is tested in the core.
        let p = preview(&c, &id, &f.home, real_now()).unwrap();
        assert_eq!(p.id, id);
        assert!(c.get(&id).unwrap().previewed);
        c.change(&id, on).unwrap();
        assert!(c.get(&id).unwrap().enabled);
        // A shorter keep-for time turns it off again until previewed; a longer one does not.
        c.change(
            &id,
            Change {
                keep_for_secs: Some(8 * 86_400),
                ..Default::default()
            },
        )
        .unwrap();
        assert!(c.get(&id).unwrap().enabled);
        c.change(
            &id,
            Change {
                keep_for_secs: Some(86_400),
                ..Default::default()
            },
        )
        .unwrap();
        let g = c.get(&id).unwrap();
        assert!(!g.enabled && !g.previewed);
        c.change(
            &id,
            Change {
                recursive: Some(true),
                ..Default::default()
            },
        )
        .unwrap();
        preview(&c, &id, &f.home, real_now()).unwrap();
        c.change(&id, on).unwrap();
        c.change(
            &id,
            Change {
                recursive: Some(true),
                ..Default::default()
            },
        )
        .unwrap();
        assert!(c.get(&id).unwrap().enabled, "no change from the previewed setting");
        // A restart reads the same list.
        let again = WatchCentre::load(f._t.path().join("watched-folders.json")).0;
        assert_eq!(again.get(&id), c.get(&id));
        assert!(c.change("w-nope", on).is_err());
        c.remove(&id).unwrap();
        assert!(c.remove(&id).is_err());
        assert!(c.overview().folders.is_empty());
        assert!(f.dir.exists(), "stopping the watch leaves the folder alone");
    }

    #[test]
    fn a_damaged_list_starts_empty_with_a_note() {
        let f = fx();
        let path = f._t.path().join("watched-folders.json");
        std::fs::write(&path, "{broken").unwrap();
        let (c, note) = WatchCentre::load(path);
        assert!(c.overview().folders.is_empty());
        assert!(note.unwrap().contains("damaged"));
    }

    #[test]
    fn due_folders_follow_the_schedule() {
        let f = fx();
        let c = centre(&f);
        add(&c, &f, &f.dir, 600).unwrap();
        let id = c.overview().folders[0].folder.id.clone();
        preview(&c, &id, &f.home, real_now()).unwrap();
        assert!(c.due(real_now()).is_empty(), "off folders are not due");
        c.change(
            &id,
            Change {
                enabled: Some(true),
                ..Default::default()
            },
        )
        .unwrap();
        assert_eq!(c.due(real_now()).len(), 1);
        c.record(
            &id,
            SweepSummary {
                at: real_now(),
                moved: 0,
                bytes: 0,
                failed: 0,
                first_failure: None,
                quarantine_id: None,
            },
        );
        assert!(c.due(real_now()).is_empty());
        assert!(c.overview().folders[0].next_sweep.is_some());
    }
}
