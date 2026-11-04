//! The `.vmx` of a VMware Fusion guest: parsed, checked and edited here and nowhere else (ADR 0054 §1.5).
//!
//! An edit parses the file into lines, changes only the keys it owns and keeps every other line as it was; the result is
//! written to a temporary file beside the `.vmx` and renamed over it ([`write_atomic`]). A backup copy is never restored: an
//! old `.vmx` names redo logs a later snapshot revert has renumbered, and every revert then fails with "A file was not
//! found". A `.vmx` that does not parse, repeats a key or names a missing disk is a preflight problem for a person to fix.

use std::collections::BTreeMap;
use std::io::Write;
use std::path::Path;

/// One line of a `.vmx`.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Line {
    /// `key = "value"`.
    Entry { key: String, value: String },
    /// A comment, a blank line, or anything that is not an entry; kept as it was.
    Other(String),
}

/// A parsed `.vmx`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Vmx {
    lines: Vec<Line>,
}

/// What a start needs from a `.vmx`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Check {
    /// Entry lines found.
    pub entries: usize,
    /// Keys that appear more than once.
    pub duplicates: Vec<String>,
    /// Lines that are neither an entry, a comment nor blank.
    pub malformed: Vec<usize>,
    /// The file name of the boot disk (`nvme0:0.fileName`), if the file names one.
    pub disk: Option<String>,
    /// The boot disk is among the files beside the `.vmx`.
    pub disk_exists: bool,
}

/// The prefix of the keys that switch the built-in VNC server on.
const VNC_PREFIX: &str = "RemoteDisplay.vnc.";
/// The key that names the boot disk (or its newest redo log).
const DISK_KEY: &str = "nvme0:0.fileName";

fn parse_entry(line: &str) -> Option<(String, String)> {
    let (key, rest) = line.split_once('=')?;
    let key = key.trim();
    if key.is_empty() || !key.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '.' | ':' | '-')) {
        return None;
    }
    let rest = rest.trim();
    let value = rest.strip_prefix('"')?.strip_suffix('"')?;
    Some((key.to_string(), value.to_string()))
}

impl Vmx {
    /// Parses `.vmx` text. Never fails: lines that are not entries are kept and reported by [`Vmx::check`].
    pub fn parse(text: &str) -> Vmx {
        let lines = text
            .lines()
            .map(|l| match parse_entry(l) {
                Some((key, value)) => Line::Entry { key, value },
                None => Line::Other(l.to_string()),
            })
            .collect();
        Vmx { lines }
    }

    /// The text of the file, one line per line, ending in a newline.
    pub fn render(&self) -> String {
        let mut out = String::new();
        for l in &self.lines {
            match l {
                Line::Entry { key, value } => out.push_str(&format!("{key} = \"{value}\"\n")),
                Line::Other(text) => {
                    out.push_str(text);
                    out.push('\n');
                }
            }
        }
        out
    }

    /// The value of `key` (the last entry, as VMware reads it).
    pub fn get(&self, key: &str) -> Option<&str> {
        self.lines.iter().rev().find_map(|l| match l {
            Line::Entry { key: k, value } if k == key => Some(value.as_str()),
            _ => None,
        })
    }

    /// Whether any key starts with `prefix` (for example `encryption.`).
    pub fn has_prefix(&self, prefix: &str) -> bool {
        self.lines
            .iter()
            .any(|l| matches!(l, Line::Entry { key, .. } if key.starts_with(prefix)))
    }

    /// Sets `key`: replaces its first entry and drops repeats, or appends it.
    pub fn set(&mut self, key: &str, value: &str) {
        let mut done = false;
        self.lines.retain_mut(|l| match l {
            Line::Entry { key: k, value: v } if k == key => {
                if done {
                    false
                } else {
                    *v = value.to_string();
                    done = true;
                    true
                }
            }
            _ => true,
        });
        if !done {
            self.lines.push(Line::Entry {
                key: key.to_string(),
                value: value.to_string(),
            });
        }
    }

    /// Removes every entry whose key starts with `prefix`.
    pub fn remove_prefix(&mut self, prefix: &str) {
        self.lines
            .retain(|l| !matches!(l, Line::Entry { key, .. } if key.starts_with(prefix)));
    }

    /// Turns the built-in VNC server on for the next start: only the `RemoteDisplay.vnc.*` entries change.
    pub fn with_vnc(mut self, port: u16, password: &str) -> Vmx {
        self.remove_prefix(VNC_PREFIX);
        self.set("RemoteDisplay.vnc.enabled", "TRUE");
        self.set("RemoteDisplay.vnc.port", &port.to_string());
        self.set("RemoteDisplay.vnc.password", password);
        self
    }

    /// Removes the VNC entries.
    pub fn without_vnc(mut self) -> Vmx {
        self.remove_prefix(VNC_PREFIX);
        self
    }

    /// Whether the VNC entries are present.
    pub fn has_vnc(&self) -> bool {
        self.has_prefix(VNC_PREFIX)
    }

    /// If the `.vmx` names a boot disk that is not in `existing` (file names), points it at the newest redo log that is: the
    /// highest number among files named `<base>-NNNNNN.vmdk`. A revert renumbers redo logs, so the name goes stale.
    pub fn fix_disk_name(mut self, base: &str, existing: &[String]) -> Vmx {
        let Some(current) = self.get(DISK_KEY).map(str::to_string) else {
            return self;
        };
        if existing.contains(&current) {
            return self;
        }
        let newest = existing.iter().filter(|f| is_redo_log(base, f)).max();
        if let Some(name) = newest {
            self.set(DISK_KEY, name);
        }
        self
    }

    /// The facts a start depends on.
    pub fn check(&self, existing: &[String]) -> Check {
        let mut counts: BTreeMap<&str, usize> = BTreeMap::new();
        let mut entries = 0;
        let mut malformed = Vec::new();
        for (n, l) in self.lines.iter().enumerate() {
            match l {
                Line::Entry { key, .. } => {
                    entries += 1;
                    *counts.entry(key.as_str()).or_default() += 1;
                }
                Line::Other(t) => {
                    let t = t.trim();
                    if !t.is_empty() && !t.starts_with('#') && !t.starts_with('.') {
                        malformed.push(n + 1);
                    }
                }
            }
        }
        let disk = self.get(DISK_KEY).map(str::to_string);
        Check {
            entries,
            duplicates: counts.into_iter().filter(|(_, n)| *n > 1).map(|(k, _)| k.to_string()).collect(),
            malformed,
            disk_exists: disk.as_ref().is_some_and(|d| existing.iter().any(|f| f == d)),
            disk,
        }
    }
}

/// `<base>-NNNNNN.vmdk`.
fn is_redo_log(base: &str, file: &str) -> bool {
    file.strip_prefix(base)
        .and_then(|r| r.strip_prefix('-'))
        .and_then(|r| r.strip_suffix(".vmdk"))
        .is_some_and(|digits| digits.len() == 6 && digits.bytes().all(|b| b.is_ascii_digit()))
}

/// Whether `key` is a device-presence key such as `sata0:0.present`: the only keys the broker sets on request.
pub fn is_presence_key(key: &str) -> bool {
    let Some(rest) = key.strip_suffix(".present") else { return false };
    let Some((bus, unit)) = rest.split_once(':') else { return false };
    let bus_ok = ["sata", "nvme", "ide", "scsi"].iter().any(|b| {
        bus.strip_prefix(b)
            .is_some_and(|n| !n.is_empty() && n.bytes().all(|c| c.is_ascii_digit()))
    });
    bus_ok && !unit.is_empty() && unit.bytes().all(|c| c.is_ascii_digit())
}

/// Writes `text` to `path` through a temporary file beside it and a rename, so a reader never sees half a file.
pub fn write_atomic(path: &Path, text: &str) -> std::io::Result<()> {
    let mut tmp = path.as_os_str().to_owned();
    tmp.push(format!(".{}.tmp", std::process::id()));
    let tmp = std::path::PathBuf::from(tmp);
    let result = std::fs::File::create(&tmp)
        .and_then(|mut f| f.write_all(text.as_bytes()).and_then(|()| f.sync_all()))
        .and_then(|()| std::fs::rename(&tmp, path));
    if result.is_err() {
        paddock_proc::cleanup::remove_file(&tmp);
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = ".encoding = \"UTF-8\"\n# a comment\n\nnvme0:0.fileName = \"paddock-win11-test-000003.vmdk\"\ndisplayName = \"paddock-win11-test\"\n";

    fn files(names: &[&str]) -> Vec<String> {
        names.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn a_parse_and_render_keeps_every_line() {
        assert_eq!(Vmx::parse(SAMPLE).render(), SAMPLE);
    }

    #[test]
    fn vnc_lines_are_added_and_stripped_without_touching_others() {
        let on = Vmx::parse(SAMPLE).with_vnc(5995, "pw");
        assert!(on.has_vnc());
        assert_eq!(on.get("RemoteDisplay.vnc.port"), Some("5995"));
        let off = on.without_vnc();
        assert!(!off.has_vnc());
        assert_eq!(off.render(), SAMPLE);
    }

    #[test]
    fn vnc_on_twice_leaves_one_set_of_lines() {
        let v = Vmx::parse(SAMPLE).with_vnc(5995, "a").with_vnc(5996, "b");
        assert_eq!(v.render().matches("RemoteDisplay.vnc.port").count(), 1);
        assert_eq!(v.get("RemoteDisplay.vnc.port"), Some("5996"));
    }

    #[test]
    fn a_missing_redo_log_is_replaced_by_the_newest_existing_one() {
        let existing = files(&[
            "paddock-win11-test-000002.vmdk",
            "paddock-win11-test-000010.vmdk",
            "paddock-win11-test-000009.vmdk",
            "x.vmx",
        ]);
        let fixed = Vmx::parse(SAMPLE).fix_disk_name("paddock-win11-test", &existing);
        assert_eq!(fixed.get("nvme0:0.fileName"), Some("paddock-win11-test-000010.vmdk"));
    }

    #[test]
    fn an_existing_disk_is_left_alone_and_no_candidate_changes_nothing() {
        let existing = files(&["paddock-win11-test-000003.vmdk", "paddock-win11-test-000010.vmdk"]);
        assert_eq!(Vmx::parse(SAMPLE).fix_disk_name("paddock-win11-test", &existing).render(), SAMPLE);
        let none = files(&["other.vmdk"]);
        assert_eq!(Vmx::parse(SAMPLE).fix_disk_name("paddock-win11-test", &none).render(), SAMPLE);
    }

    #[test]
    fn check_reports_duplicates_malformed_lines_and_a_missing_disk() {
        let text = "a = \"1\"\na = \"2\"\nthis is not an entry\nnvme0:0.fileName = \"gone.vmdk\"\n";
        let c = Vmx::parse(text).check(&files(&["here.vmdk"]));
        assert_eq!(c.duplicates, vec!["a".to_string()]);
        assert_eq!(c.malformed, vec![3]);
        assert_eq!((c.disk.as_deref(), c.disk_exists), (Some("gone.vmdk"), false));
        assert_eq!(c.entries, 3);
    }

    #[test]
    fn check_is_clean_for_a_good_file() {
        let c = Vmx::parse(SAMPLE).check(&files(&["paddock-win11-test-000003.vmdk"]));
        assert!(c.duplicates.is_empty() && c.malformed.is_empty() && c.disk_exists);
    }

    #[test]
    fn set_replaces_the_first_entry_and_drops_repeats() {
        let mut v = Vmx::parse("k = \"1\"\nk = \"2\"\nz = \"3\"\n");
        v.set("k", "9");
        assert_eq!(v.render(), "k = \"9\"\nz = \"3\"\n");
    }

    #[test]
    fn encryption_keys_are_detected_by_prefix() {
        assert!(Vmx::parse("encryption.keySafe = \"x\"\n").has_prefix("encryption."));
        assert!(!Vmx::parse(SAMPLE).has_prefix("encryption."));
    }

    #[test]
    fn only_device_presence_keys_can_be_set() {
        for ok in ["sata0:0.present", "sata0:1.present", "nvme0:0.present", "ide1:0.present"] {
            assert!(is_presence_key(ok), "{ok}");
        }
        for bad in [
            "sata0:0.fileName",
            "memsize",
            "sata:0.present",
            "sata0:.present",
            "RemoteDisplay.vnc.enabled",
            "x.present",
            "sata0:0.present.x",
        ] {
            assert!(!is_presence_key(bad), "{bad}");
        }
    }

    #[test]
    fn write_atomic_replaces_the_file_and_leaves_no_temporary() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("a.vmx");
        std::fs::write(&p, "old").unwrap();
        write_atomic(&p, "new\n").unwrap();
        assert_eq!(std::fs::read_to_string(&p).unwrap(), "new\n");
        assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 1);
    }
}
