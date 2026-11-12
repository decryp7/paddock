//! The owner record: a small `.paddock-owner.toml` in every folder the tooling makes, the source of truth for who the folder
//! belongs to. It lives next to the data, so it survives a move or a copy; the SQLite index ([`super::index`]) is only a
//! cache that can be rebuilt from the records.

use crate::worktree::note::{format_utc, now_secs, parse_utc};
use std::path::{Path, PathBuf};

/// The record's file name.
pub const FILE: &str = ".paddock-owner.toml";
/// A touch within this many seconds of the last one is skipped (the record is rewritten at most once a minute).
pub const TOUCH_EVERY_SECS: u64 = 60;

/// What the folder is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    /// A git worktree made by `./dev worktree add`.
    Worktree,
    /// A build folder (`CARGO_TARGET_DIR`).
    Build,
    /// A CI slot's kept folder (the CI kit sweeps these itself; `clean` never removes them).
    Slot,
    /// Review or test scratch made by a tool.
    Scratch,
}

impl Kind {
    /// The word stored in the record and shown in the report.
    pub fn word(self) -> &'static str {
        match self {
            Kind::Worktree => "worktree",
            Kind::Build => "build",
            Kind::Slot => "slot",
            Kind::Scratch => "scratch",
        }
    }

    /// Reads the word back.
    pub fn parse(word: &str) -> Option<Kind> {
        Some(match word {
            "worktree" => Kind::Worktree,
            "build" => Kind::Build,
            "slot" => Kind::Slot,
            "scratch" => Kind::Scratch,
            _ => return None,
        })
    }
}

/// The contents of `.paddock-owner.toml`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Record {
    /// What the folder is.
    pub kind: Kind,
    /// Who made it: an agent id (`implementer:task`) or a job (`ci:<run>/<job>`).
    pub owner: String,
    /// The branch it was made for (empty when none).
    pub branch: String,
    /// The head commit when it was made or last recorded (empty when none).
    pub head: String,
    /// Seconds since the epoch.
    pub created: u64,
    /// Seconds since the epoch; [`touch`] moves it.
    pub last_used: u64,
    /// For a worktree, its git common dir (the repository it belongs to).
    pub repo: String,
    /// For a build folder, the worktree it serves (empty when none).
    pub link: String,
}

impl Record {
    /// A new record, created and last used now.
    pub fn new(kind: Kind, owner: &str) -> Record {
        let now = now_secs();
        Record {
            kind,
            owner: owner.to_string(),
            branch: String::new(),
            head: String::new(),
            created: now,
            last_used: now,
            repo: String::new(),
            link: String::new(),
        }
    }

    /// The file text.
    pub fn render(&self) -> String {
        let q = |s: &str| {
            let mut out = String::from("\"");
            for c in s.chars() {
                match c {
                    '"' => out.push_str("\\\""),
                    '\\' => out.push_str("\\\\"),
                    '\n' => out.push_str("\\n"),
                    '\r' => out.push_str("\\r"),
                    '\t' => out.push_str("\\t"),
                    c if c.is_control() => out.push(' '),
                    c => out.push(c),
                }
            }
            out.push('"');
            out
        };
        format!(
            "# Written by `./dev` (disk tracking). Safe to delete; `./dev disk adopt` can write it again.\n\
             version = 1\nkind = {}\nowner = {}\nbranch = {}\nhead = {}\ncreated = {}\nlast_used = {}\nrepo = {}\nlink = {}\n",
            q(self.kind.word()),
            q(&self.owner),
            q(&self.branch),
            q(&self.head),
            q(&format_utc(self.created)),
            q(&format_utc(self.last_used)),
            q(&self.repo),
            q(&self.link),
        )
    }

    /// Reads a record from its text; the error says what is wrong.
    pub fn parse(text: &str) -> Result<Record, String> {
        let table: toml::Table = text.parse().map_err(|e| format!("not TOML: {e}"))?;
        let s = |k: &str| table.get(k).and_then(|v| v.as_str()).unwrap_or("").to_string();
        let kind = Kind::parse(&s("kind")).ok_or_else(|| format!("unknown kind `{}`", s("kind")))?;
        let time = |k: &str| parse_utc(&s(k)).ok_or_else(|| format!("`{k}` is not a UTC time"));
        Ok(Record {
            kind,
            owner: s("owner"),
            branch: s("branch"),
            head: s("head"),
            created: time("created")?,
            last_used: time("last_used")?,
            repo: s("repo"),
            link: s("link"),
        })
    }
}

/// The record file of a folder.
pub fn path_of(dir: &Path) -> PathBuf {
    dir.join(FILE)
}

/// Reads a folder's record: `Ok(None)` when it has none, `Err` when the file is unreadable or malformed (the folder then
/// counts as an orphan, and the report says why).
pub fn read(dir: &Path) -> Result<Option<Record>, String> {
    match std::fs::read_to_string(path_of(dir)) {
        Ok(text) => Record::parse(&text).map(Some),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(format!("cannot read {}: {e}", path_of(dir).display())),
    }
}

/// Writes a folder's record atomically (a temp file, then rename).
pub fn write(dir: &Path, r: &Record) -> Result<(), String> {
    let target = path_of(dir);
    let tmp = dir.join(format!("{FILE}.tmp{}", std::process::id()));
    std::fs::write(&tmp, r.render()).map_err(|e| format!("cannot write {}: {e}", tmp.display()))?;
    std::fs::rename(&tmp, &target).map_err(|e| {
        let _ = std::fs::remove_file(&tmp);
        format!("cannot write {}: {e}", target.display())
    })
}

/// Marks the folder as used now. A folder without a record, or one touched in the last minute, is left alone. Returns whether
/// the record was rewritten.
pub fn touch(dir: &Path) -> bool {
    let Ok(Some(mut r)) = read(dir) else { return false };
    let now = now_secs();
    if now.saturating_sub(r.last_used) < TOUCH_EVERY_SECS {
        return false;
    }
    r.last_used = now;
    write(dir, &r).is_ok()
}

/// Makes git ignore the record in every worktree of the repository, so it never shows as an untracked file: one line in
/// `<common-dir>/info/exclude`.
pub fn exclude_in_repo(common_dir: &Path) -> Result<(), String> {
    let file = common_dir.join("info").join("exclude");
    let line = format!("/{FILE}");
    let current = std::fs::read_to_string(&file).unwrap_or_default();
    if current.lines().any(|l| l.trim() == line) {
        return Ok(());
    }
    if let Some(parent) = file.parent() {
        std::fs::create_dir_all(parent).map_err(|e| format!("cannot create {}: {e}", parent.display()))?;
    }
    let mut text = current;
    if !text.is_empty() && !text.ends_with('\n') {
        text.push('\n');
    }
    text.push_str(&line);
    text.push('\n');
    std::fs::write(&file, text).map_err(|e| format!("cannot write {}: {e}", file.display()))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> Record {
        Record {
            kind: Kind::Worktree,
            owner: "implementer:dev-disk".into(),
            branch: "feat/\"x\"".into(),
            head: "abc123".into(),
            created: 1_700_000_000,
            last_used: 1_700_000_500,
            repo: "/r/.git".into(),
            link: String::new(),
        }
    }

    #[test]
    fn a_record_survives_its_own_text_including_quotes_and_backslashes() {
        let mut r = sample();
        r.owner = "a\\b\"c".into();
        assert_eq!(Record::parse(&r.render()).unwrap(), r);
    }

    #[test]
    fn a_malformed_record_is_an_error_not_an_empty_record() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(path_of(dir.path()), "kind = \"nonsense\"\n").unwrap();
        assert!(read(dir.path()).unwrap_err().contains("unknown kind"));
        std::fs::write(path_of(dir.path()), "= = =").unwrap();
        assert!(read(dir.path()).unwrap_err().contains("not TOML"));
    }

    #[test]
    fn a_folder_without_a_record_has_none() {
        let dir = tempfile::tempdir().unwrap();
        assert_eq!(read(dir.path()).unwrap(), None);
        assert!(!touch(dir.path()), "nothing to touch");
    }

    #[test]
    fn touch_moves_last_used_but_at_most_once_a_minute() {
        let dir = tempfile::tempdir().unwrap();
        let mut r = sample();
        write(dir.path(), &r).unwrap();
        assert!(touch(dir.path()));
        let after = read(dir.path()).unwrap().unwrap();
        assert!(after.last_used > r.last_used);
        assert_eq!(after.created, r.created, "created never moves");
        assert!(!touch(dir.path()), "a second touch inside a minute is skipped");
        r.last_used = after.last_used;
        assert_eq!(read(dir.path()).unwrap().unwrap(), r);
    }

    #[test]
    fn the_exclude_line_is_added_once() {
        let dir = tempfile::tempdir().unwrap();
        exclude_in_repo(dir.path()).unwrap();
        exclude_in_repo(dir.path()).unwrap();
        let text = std::fs::read_to_string(dir.path().join("info/exclude")).unwrap();
        assert_eq!(text.matches(FILE).count(), 1);
    }
}
