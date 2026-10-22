//! Small JSON state files (settings, rules, tuning state): written atomically with a backup, read tolerantly
//! (the file, else its backup, else the defaults).

use serde::de::DeserializeOwned;
use serde::Serialize;
use std::io::Write;
use std::path::{Path, PathBuf};

/// The largest state file read.
pub const MAX_BYTES: u64 = 4 * 1024 * 1024;

fn backup_of(path: &Path) -> PathBuf {
    let mut s = path.as_os_str().to_owned();
    s.push(".bak");
    PathBuf::from(s)
}

/// Writes `value` as pretty JSON: to a temp file next to `path`, flushed, then renamed over it; the previous file
/// becomes `<path>.bak`.
pub fn save<T: Serialize>(path: &Path, value: &T) -> std::io::Result<()> {
    let text = serde_json::to_vec_pretty(value).map_err(std::io::Error::other)?;
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let mut tmp_name = path.as_os_str().to_owned();
    tmp_name.push(".tmp");
    let tmp = PathBuf::from(tmp_name);
    {
        let mut f = std::fs::File::create(&tmp)?;
        f.write_all(&text)?;
        f.sync_all()?;
    }
    if path.exists() {
        let _ = std::fs::copy(path, backup_of(path));
    }
    std::fs::rename(&tmp, path)
}

fn read_text(path: &Path) -> Option<String> {
    let meta = std::fs::metadata(path).ok()?;
    if meta.len() > MAX_BYTES {
        return None;
    }
    std::fs::read_to_string(path).ok()
}

/// Reads a state file with `parse` (which returns the value and an optional warning). Missing file: `None` with no
/// warning. Damaged file: the backup if it parses cleanly, else what `parse` made of the damaged file.
pub fn load<T>(path: &Path, parse: impl Fn(&str) -> (T, Option<String>)) -> (Option<T>, Option<String>) {
    let Some(text) = read_text(path) else {
        return match read_text(&backup_of(path)) {
            Some(b) => {
                let (v, _) = parse(&b);
                (Some(v), Some(format!("{} was missing; restored from its backup.", path.display())))
            }
            None => (None, None),
        };
    };
    let (v, warning) = parse(&text);
    if warning.is_none() {
        return (Some(v), None);
    }
    if let Some(b) = read_text(&backup_of(path)) {
        if let (bv, None) = parse(&b) {
            return (Some(bv), Some(format!("{} was damaged; restored from its backup.", path.display())));
        }
    }
    (Some(v), warning)
}

/// A JSON parser for [`load`] for types with defaults: damage gives the defaults and a warning.
pub fn json_or_default<T: DeserializeOwned + Default>(text: &str) -> (T, Option<String>) {
    match serde_json::from_str(text) {
        Ok(v) => (v, None),
        Err(e) => (T::default(), Some(format!("damaged ({e}); using the defaults"))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Debug, Default, PartialEq, serde::Serialize, serde::Deserialize)]
    struct S {
        n: u32,
    }

    #[test]
    fn saves_atomically_and_keeps_a_backup() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("sub").join("s.json");
        assert_eq!(load(&p, json_or_default::<S>), (None, None));
        save(&p, &S { n: 1 }).unwrap();
        save(&p, &S { n: 2 }).unwrap();
        assert_eq!(load(&p, json_or_default::<S>).0, Some(S { n: 2 }));
        assert!(backup_of(&p).exists());
        std::fs::write(&p, "{broken").unwrap();
        let (v, w) = load(&p, json_or_default::<S>);
        assert_eq!(v, Some(S { n: 1 }));
        assert!(w.unwrap().contains("restored"));
        std::fs::remove_file(&p).unwrap();
        let (v, w) = load(&p, json_or_default::<S>);
        assert_eq!(v, Some(S { n: 1 }));
        assert!(w.unwrap().contains("missing"));
        std::fs::write(&p, "{broken").unwrap();
        std::fs::write(backup_of(&p), "{also broken").unwrap();
        let (v, w) = load(&p, json_or_default::<S>);
        assert_eq!(v, Some(S::default()));
        assert!(w.unwrap().contains("damaged"));
    }
}
