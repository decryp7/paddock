//! The quarantine (ADR 0005 item 5): deleted items wait here for seven days and can be put back. One folder per cleanup
//! (`<id>/manifest.json` plus `<id>/data/<n>`), so an interrupted run leaves either a restorable batch or nothing.
//! A damaged manifest is skipped, never fatal; an expired batch is deleted for good.

use crate::files;
use dyno_core::activity::quarantine_expiry;
use dyno_core::cleanup::{FailedItem, QuarantineEntry};
use dyno_core::time::Timestamp;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

/// The manifest format.
const VERSION: u32 = 1;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct Held {
    original: String,
    stored: String,
    bytes: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct Manifest {
    version: u32,
    id: String,
    at: Timestamp,
    expires: Timestamp,
    activity: Option<String>,
    items: Vec<Held>,
}

/// What a restore did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Restored {
    /// Items put back.
    pub restored: usize,
    /// Items that could not be put back, with the reason (they stay in the quarantine).
    pub failed: Vec<FailedItem>,
    /// The activity entry that recorded the cleanup, to mark undone.
    pub activity: Option<String>,
}

/// The quarantine folder.
pub struct Quarantine {
    dir: PathBuf,
}

fn plain_id(id: &str) -> bool {
    !id.is_empty() && id.len() <= 64 && id.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
}

fn moved(from: &Path, to: &Path) -> Result<(), String> {
    match std::fs::rename(from, to) {
        Ok(()) => Ok(()),
        Err(_) => {
            // Another volume: copy, then remove the source (links are refused, never followed).
            copy_tree(from, to).inspect_err(|_| {
                let _ = remove_tree(to);
            })?;
            remove_tree(from)
        }
    }
}

fn copy_tree(from: &Path, to: &Path) -> Result<(), String> {
    let meta = from.symlink_metadata().map_err(|e| e.to_string())?;
    if meta.file_type().is_symlink() {
        return Err(format!("{} is a link and cannot be moved to another drive.", from.display()));
    }
    if meta.is_dir() {
        std::fs::create_dir_all(to).map_err(|e| e.to_string())?;
        for e in std::fs::read_dir(from).map_err(|e| e.to_string())?.flatten() {
            copy_tree(&e.path(), &to.join(e.file_name()))?;
        }
        Ok(())
    } else {
        std::fs::copy(from, to).map(|_| ()).map_err(|e| e.to_string())
    }
}

fn remove_tree(p: &Path) -> Result<(), String> {
    match p.symlink_metadata() {
        Ok(m) if m.is_dir() && !m.file_type().is_symlink() => std::fs::remove_dir_all(p),
        Ok(_) => std::fs::remove_file(p),
        Err(_) => Ok(()),
    }
    .map_err(|e| e.to_string())
}

impl Quarantine {
    /// A quarantine in `dir` (created when first used).
    pub fn new(dir: impl Into<PathBuf>) -> Quarantine {
        Quarantine { dir: dir.into() }
    }

    fn batch_dir(&self, id: &str) -> PathBuf {
        self.dir.join(id)
    }

    fn read(&self, id: &str) -> Option<Manifest> {
        let text = std::fs::read_to_string(self.batch_dir(id).join("manifest.json")).ok()?;
        let m: Manifest = serde_json::from_str(&text).ok()?;
        (m.version == VERSION && m.id == id).then_some(m)
    }

    fn write(&self, m: &Manifest) -> Result<(), String> {
        files::save(&self.batch_dir(&m.id).join("manifest.json"), m).map_err(|e| format!("The quarantine could not be written: {e}"))
    }

    /// Starts a batch. `id` must be a plain id.
    pub fn begin(&self, id: &str, at: Timestamp) -> Result<Batch<'_>, String> {
        if !plain_id(id) {
            return Err("That is not a valid quarantine id.".into());
        }
        std::fs::create_dir_all(self.batch_dir(id).join("data")).map_err(|e| format!("The quarantine folder could not be created: {e}"))?;
        let m = Manifest {
            version: VERSION,
            id: id.into(),
            at,
            expires: quarantine_expiry(at),
            activity: None,
            items: vec![],
        };
        self.write(&m)?;
        Ok(Batch { q: self, m })
    }

    /// The quarantines waiting, newest first. Batches whose manifest is damaged are not listed.
    pub fn list(&self) -> Vec<QuarantineEntry> {
        let Ok(rd) = std::fs::read_dir(&self.dir) else { return vec![] };
        let mut out: Vec<QuarantineEntry> = rd
            .flatten()
            .filter_map(|e| self.read(&e.file_name().to_string_lossy()))
            .filter(|m| !m.items.is_empty())
            .map(|m| QuarantineEntry {
                id: m.id,
                at: m.at,
                bytes: m.items.iter().map(|i| i.bytes).sum(),
                item_count: m.items.len(),
                expires: m.expires,
            })
            .collect();
        out.sort_by(|a, b| b.at.cmp(&a.at).then(a.id.cmp(&b.id)));
        out
    }

    /// Records which activity entry describes a batch, so restoring can mark it undone.
    pub fn link_activity(&self, id: &str, activity: &str) {
        if let Some(mut m) = self.read(id) {
            m.activity = Some(activity.to_string());
            let _ = self.write(&m);
        }
    }

    /// Puts a batch back where it came from. Items whose place is taken, or whose folder cannot be written, stay.
    pub fn restore(&self, id: &str) -> Result<Restored, String> {
        if !plain_id(id) {
            return Err("That is not a valid quarantine id.".into());
        }
        let Some(mut m) = self.read(id) else {
            return Err("That quarantine was already restored or has expired.".into());
        };
        let data = self.batch_dir(id).join("data");
        let mut failed = Vec::new();
        let mut kept = Vec::new();
        let mut restored = 0;
        for held in std::mem::take(&mut m.items) {
            match self.put_back(&data, &held) {
                Ok(()) => restored += 1,
                Err(reason) => {
                    failed.push(FailedItem {
                        path: held.original.clone(),
                        reason,
                    });
                    kept.push(held);
                }
            }
        }
        let activity = m.activity.clone();
        if kept.is_empty() {
            let _ = std::fs::remove_dir_all(self.batch_dir(id));
        } else {
            m.items = kept;
            self.write(&m)?;
        }
        Ok(Restored {
            restored,
            failed,
            activity,
        })
    }

    fn put_back(&self, data: &Path, held: &Held) -> Result<(), String> {
        // The manifest is a file on disk: trust neither the stored name nor the destination.
        if held.stored.is_empty() || held.stored.contains(['/', '\\']) || held.stored == ".." || held.stored == "." {
            return Err("The quarantine record is damaged.".into());
        }
        let dest = Path::new(&held.original);
        if !dest.is_absolute() || dyno_core::cleanup::has_dot_segment(dest) {
            return Err("The original location is not a plain full path.".into());
        }
        let source = data.join(&held.stored);
        if source.symlink_metadata().is_err() {
            return Err("The held copy is missing.".into());
        }
        if dest.symlink_metadata().is_ok() {
            return Err("Something else is already at the original location.".into());
        }
        if let Some(parent) = dest.parent() {
            std::fs::create_dir_all(parent).map_err(|e| format!("The original folder cannot be recreated: {e}"))?;
        }
        moved(&source, dest).map_err(|e| format!("It could not be moved back: {e}"))
    }

    /// Deletes for good the batches whose seven days are over, and leftovers without a readable manifest that are older
    /// than that. Returns how many batches went.
    pub fn expire(&self, now: Timestamp) -> usize {
        let Ok(rd) = std::fs::read_dir(&self.dir) else { return 0 };
        let mut gone = 0;
        for e in rd.flatten() {
            let name = e.file_name().to_string_lossy().into_owned();
            let expired = match self.read(&name) {
                Some(m) => now >= m.expires,
                None => e
                    .metadata()
                    .ok()
                    .is_some_and(|md| now >= quarantine_expiry(super::walk::modified(&md))),
            };
            if expired && e.path().is_dir() && std::fs::remove_dir_all(e.path()).is_ok() {
                gone += 1;
            }
        }
        gone
    }
}

/// A batch being filled.
pub struct Batch<'a> {
    q: &'a Quarantine,
    m: Manifest,
}

impl Batch<'_> {
    /// The batch's id.
    pub fn id(&self) -> &str {
        &self.m.id
    }

    /// Moves `original` into the batch. The manifest is written first with the item, so a crash after the move still
    /// leaves a restorable record.
    pub fn stage(&mut self, original: &Path, bytes: u64) -> Result<(), String> {
        let stored = self.m.items.len().to_string();
        self.m.items.push(Held {
            original: original.to_string_lossy().into_owned(),
            stored: stored.clone(),
            bytes,
        });
        self.q.write(&self.m)?;
        let to = self.q.batch_dir(&self.m.id).join("data").join(&stored);
        if let Err(e) = moved(original, &to) {
            self.m.items.pop();
            self.q.write(&self.m)?;
            return Err(e);
        }
        Ok(())
    }

    /// Items staged so far.
    pub fn len(&self) -> usize {
        self.m.items.len()
    }

    /// Whether nothing was staged.
    pub fn is_empty(&self) -> bool {
        self.m.items.is_empty()
    }

    /// Ends the batch; an empty one leaves nothing behind. Returns when it expires.
    pub fn finish(self) -> Option<Timestamp> {
        if self.m.items.is_empty() {
            let _ = std::fs::remove_dir_all(self.q.batch_dir(&self.m.id));
            None
        } else {
            Some(self.m.expires)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use dyno_core::time::DAY;

    fn fixture() -> (tempfile::TempDir, Quarantine, PathBuf) {
        let t = tempfile::tempdir().unwrap();
        let src = t.path().join("src");
        std::fs::create_dir_all(&src).unwrap();
        let q = Quarantine::new(t.path().join("quarantine"));
        (t, q, src)
    }

    #[test]
    fn items_can_be_held_and_put_back() {
        let (_t, q, src) = fixture();
        std::fs::write(src.join("a.txt"), b"hello").unwrap();
        std::fs::create_dir_all(src.join("dir/sub")).unwrap();
        std::fs::write(src.join("dir/sub/b.txt"), b"world").unwrap();
        let mut b = q.begin("q-1", Timestamp(1000)).unwrap();
        b.stage(&src.join("a.txt"), 5).unwrap();
        b.stage(&src.join("dir"), 5).unwrap();
        assert_eq!((b.len(), b.is_empty(), b.id()), (2, false, "q-1"));
        assert_eq!(b.finish(), Some(Timestamp(1000).plus(7 * DAY)));
        assert!(!src.join("a.txt").exists() && !src.join("dir").exists());
        let listed = q.list();
        assert_eq!(
            (listed[0].item_count, listed[0].bytes, listed[0].expires),
            (2, 10, Timestamp(1000).plus(7 * DAY))
        );
        q.link_activity("q-1", "act-9");
        let r = q.restore("q-1").unwrap();
        assert_eq!((r.restored, r.failed.len(), r.activity.as_deref()), (2, 0, Some("act-9")));
        assert_eq!(std::fs::read(src.join("a.txt")).unwrap(), b"hello");
        assert_eq!(std::fs::read(src.join("dir/sub/b.txt")).unwrap(), b"world");
        assert!(q.list().is_empty());
        assert!(q.restore("q-1").unwrap_err().contains("already restored"));
    }

    #[test]
    fn a_taken_place_keeps_the_item_in_the_quarantine() {
        let (_t, q, src) = fixture();
        std::fs::write(src.join("a.txt"), b"old").unwrap();
        let mut b = q.begin("q-2", Timestamp(0)).unwrap();
        b.stage(&src.join("a.txt"), 3).unwrap();
        b.finish();
        std::fs::write(src.join("a.txt"), b"new").unwrap();
        let r = q.restore("q-2").unwrap();
        assert_eq!(r.restored, 0);
        assert!(r.failed[0].reason.contains("already at the original"));
        assert_eq!(std::fs::read(src.join("a.txt")).unwrap(), b"new", "never overwritten");
        assert_eq!(q.list().len(), 1, "still held");
    }

    /// The path as it appears inside the manifest's JSON (backslashes doubled on Windows).
    fn json_escaped(p: &Path) -> String {
        p.to_string_lossy().replace('\\', "\\\\")
    }

    #[test]
    fn a_tampered_manifest_cannot_restore_outside_or_read_elsewhere() {
        let (t, q, src) = fixture();
        std::fs::write(src.join("a.txt"), b"x").unwrap();
        let mut b = q.begin("q-3", Timestamp(0)).unwrap();
        b.stage(&src.join("a.txt"), 1).unwrap();
        b.finish();
        let manifest = t.path().join("quarantine/q-3/manifest.json");
        let text = std::fs::read_to_string(&manifest).unwrap();
        let dotdot = src.join("..").join("escaped.txt");
        std::fs::write(&manifest, text.replace(&json_escaped(&src.join("a.txt")), &json_escaped(&dotdot))).unwrap();
        let r = q.restore("q-3").unwrap();
        assert!(r.failed[0].reason.contains("plain full path"), "{:?}", r.failed);
        assert!(!t.path().join("escaped.txt").exists());
        std::fs::write(&manifest, text.replace("\"stored\": \"0\"", "\"stored\": \"../../src\"")).unwrap();
        assert!(q.restore("q-3").unwrap().failed[0].reason.contains("damaged"));
        std::fs::write(&manifest, "{ not json").unwrap();
        assert!(q.list().is_empty());
        assert!(q.restore("q-3").is_err());
        assert!(q.restore("../x").is_err());
        assert!(q.begin("a/b", Timestamp(0)).is_err());
    }

    #[test]
    fn expiry_deletes_for_good_and_ignores_the_unexpired() {
        let (t, q, src) = fixture();
        for (id, at) in [("q-old", 0), ("q-new", 6 * DAY)] {
            std::fs::write(src.join(id), b"x").unwrap();
            let mut b = q.begin(id, Timestamp(at)).unwrap();
            b.stage(&src.join(id), 1).unwrap();
            b.finish();
        }
        assert_eq!(q.expire(Timestamp(7 * DAY)), 1);
        assert!(!t.path().join("quarantine/q-old").exists());
        assert!(t.path().join("quarantine/q-new").exists());
        // A leftover with no manifest goes once it is old enough.
        std::fs::create_dir_all(t.path().join("quarantine/orphan")).unwrap();
        assert_eq!(q.expire(Timestamp(0)), 0);
        assert_eq!(q.expire(Timestamp(i64::MAX / 2)), 2);
        assert_eq!(Quarantine::new(t.path().join("nope")).expire(Timestamp(0)), 0);
    }

    #[test]
    fn an_empty_batch_leaves_nothing_and_a_failed_move_is_not_recorded() {
        let (t, q, src) = fixture();
        let b = q.begin("q-4", Timestamp(0)).unwrap();
        assert_eq!(b.finish(), None);
        assert!(!t.path().join("quarantine/q-4").exists());
        let mut b = q.begin("q-5", Timestamp(0)).unwrap();
        assert!(b.stage(&src.join("missing"), 1).is_err());
        assert!(b.is_empty());
        b.finish();
        assert!(q.list().is_empty());
    }
}
