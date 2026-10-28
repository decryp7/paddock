//! The settings file: `{ "version": 1, "sections": { "<id>": { "<key>": value } } }`, read once at start-up and written
//! atomically after each change.
//!
//! - **Versioned**: [`FORMAT_VERSION`]. A file written by a newer app version is not read (its fields may mean something
//!   else) and is moved aside as `settings.json.v<N>`, so a downgrade never overwrites it.
//! - **Tolerant**: a missing file means defaults; a damaged one (not JSON, the wrong shape, larger than
//!   [`MAX_FILE_BYTES`]) means defaults too and is kept aside as `settings.json.corrupt` so nothing is lost silently. Each
//!   case gives one [`LoadProblem`] for the app to report once. Single values that fail their field's validation are
//!   replaced by the default when the section is registered (`sanitize`), never the whole file.
//! - **Atomic**: written to a temporary file in the same folder, flushed to disk, then renamed over the old one.

use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::BTreeMap;
use std::io::{Read, Write};
use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};

/// The settings file's usual name in the app's config folder.
pub const FILE_NAME: &str = "settings.json";
/// The format this crate writes and reads.
pub const FORMAT_VERSION: u32 = 1;
/// Largest settings file read; a larger one is not a settings file.
pub const MAX_FILE_BYTES: u64 = 1_000_000;
/// How many damaged copies are kept beside the file (`.corrupt`, `.corrupt.1`, ...); the oldest slot is reused.
const MAX_KEPT: u32 = 5;

/// The file's content: raw values per section. Sections nobody registers (a feature that is off in this build) are kept
/// as they are and written back unchanged.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SettingsFile {
    #[serde(default)]
    pub version: u32,
    #[serde(default)]
    pub sections: BTreeMap<String, Value>,
}

/// Why the file was not used.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
#[cfg_attr(test, derive(ts_rs::TS))]
pub enum LoadProblemKind {
    /// Not a settings file (kept aside as `.corrupt`).
    Damaged,
    /// Written by a newer version of the app (kept aside as `.v<N>`).
    Newer,
    /// It could not be read (permissions, a folder in its place); left where it is.
    Unreadable,
}

/// What went wrong reading the file: the app tells the user once (a notification), and the Settings window shows it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct LoadProblem {
    pub kind: LoadProblemKind,
    /// For the user: what happened and what was done about it.
    pub message: String,
    /// The file name the damaged or newer file was kept as, if it was moved.
    pub kept_as: Option<String>,
}

/// The file as read: its content (defaults when it could not be used) and the problem, if any.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Loaded {
    pub file: SettingsFile,
    pub problem: Option<LoadProblem>,
}

/// Reads the settings file at `path`. Never fails: every problem gives defaults and a [`LoadProblem`].
pub fn load(path: &Path) -> Loaded {
    match read(path) {
        Ok(None) => Loaded::default(),
        Ok(Some(file)) if file.version > FORMAT_VERSION => {
            let aside = path.with_file_name(format!("{}.v{}", file_name(path), file.version));
            let kept = std::fs::rename(path, &aside)
                .ok()
                .map(|()| file_name(&aside));
            let message = format!(
                "The settings were written by a newer version of the app (format {}). Defaults are in use{}.",
                file.version,
                kept.as_ref()
                    .map(|k| format!("; the file was kept as {k}"))
                    .unwrap_or_default()
            );
            problem(LoadProblemKind::Newer, message, kept)
        }
        Ok(Some(file)) => Loaded {
            file,
            problem: None,
        },
        Err(Failure::Damaged(why)) => {
            let kept = keep_aside(path);
            let message = format!(
                "The settings file could not be read ({why}). Defaults are in use{}.",
                kept.as_ref()
                    .map(|k| format!("; the damaged file was kept as {k}"))
                    .unwrap_or_default()
            );
            problem(LoadProblemKind::Damaged, message, kept)
        }
        Err(Failure::Unreadable(why)) => problem(
            LoadProblemKind::Unreadable,
            format!(
                "The settings file could not be opened ({why}). Defaults are in use and changes may not be saved."
            ),
            None,
        ),
    }
}

fn problem(kind: LoadProblemKind, message: String, kept_as: Option<String>) -> Loaded {
    Loaded {
        file: SettingsFile::default(),
        problem: Some(LoadProblem {
            kind,
            message,
            kept_as,
        }),
    }
}

enum Failure {
    Damaged(String),
    Unreadable(String),
}

/// `Ok(None)` when there is no file.
fn read(path: &Path) -> Result<Option<SettingsFile>, Failure> {
    let meta = match std::fs::metadata(path) {
        Ok(m) => m,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(Failure::Unreadable(e.to_string())),
    };
    if !meta.is_file() {
        return Err(Failure::Unreadable("it is not a file".into()));
    }
    let mut bytes = Vec::new();
    let opened = std::fs::File::open(path).map_err(|e| Failure::Unreadable(e.to_string()))?;
    // Bounded read: the size may change between the metadata and the read.
    opened
        .take(MAX_FILE_BYTES + 1)
        .read_to_end(&mut bytes)
        .map_err(|e| Failure::Unreadable(e.to_string()))?;
    if bytes.len() as u64 > MAX_FILE_BYTES {
        return Err(Failure::Damaged(format!(
            "larger than {}",
            paddock_units::format_whole(MAX_FILE_BYTES, paddock_units::Kind::Storage)
        )));
    }
    let value: Value =
        serde_json::from_slice(&bytes).map_err(|e| Failure::Damaged(format!("not JSON: {e}")))?;
    if !value.is_object() {
        return Err(Failure::Damaged("not a JSON object".into()));
    }
    let file: SettingsFile =
        serde_json::from_value(value).map_err(|e| Failure::Damaged(e.to_string()))?;
    if let Some((id, _)) = file.sections.iter().find(|(_, v)| !v.is_object()) {
        return Err(Failure::Damaged(format!(
            "section \"{}\" is not an object",
            truncate(id)
        )));
    }
    Ok(Some(file))
}

fn truncate(id: &str) -> String {
    id.chars().take(crate::schema::MAX_ID_LEN).collect()
}

fn file_name(path: &Path) -> String {
    path.file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default()
}

/// Moves a damaged file aside to the first free `.corrupt` slot (the last one is replaced when all are taken).
fn keep_aside(path: &Path) -> Option<String> {
    let base = file_name(path);
    let slot = |n: u32| {
        path.with_file_name(if n == 0 {
            format!("{base}.corrupt")
        } else {
            format!("{base}.corrupt.{n}")
        })
    };
    let target = (0..MAX_KEPT)
        .map(slot)
        .find(|p| !p.exists())
        .unwrap_or_else(|| slot(MAX_KEPT - 1));
    std::fs::rename(path, &target)
        .ok()
        .map(|()| file_name(&target))
}

/// Writes `file` (as [`FORMAT_VERSION`]) to `path` atomically: a temporary file next to it, flushed, then renamed over
/// the old one. Creates the folder. On failure the old file is untouched and the temporary file is removed.
pub fn save(path: &Path, file: &SettingsFile) -> std::io::Result<()> {
    let doc = SettingsFile {
        version: FORMAT_VERSION,
        sections: file.sections.clone(),
    };
    let mut bytes = serde_json::to_vec_pretty(&doc).map_err(std::io::Error::other)?;
    bytes.push(b'\n');
    write_atomic(path, &bytes)
}

static NEXT_TMP: AtomicU64 = AtomicU64::new(0);

/// Writes `bytes` to `path` atomically (see [`save`]).
pub fn write_atomic(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    let dir = path.parent().unwrap_or(Path::new(""));
    std::fs::create_dir_all(dir)?;
    let n = NEXT_TMP.fetch_add(1, Ordering::Relaxed);
    let tmp = dir.join(format!(
        ".{}.{}.{n}.tmp",
        file_name(path),
        std::process::id()
    ));
    let result = (|| {
        let mut f = std::fs::File::create(&tmp)?;
        f.write_all(bytes)?;
        f.sync_all()?;
        drop(f);
        std::fs::rename(&tmp, path)?;
        sync_dir(dir);
        Ok(())
    })();
    if result.is_err() {
        let _ = std::fs::remove_file(&tmp);
    }
    result
}

/// Makes the rename durable where the platform needs it (best effort: the data itself is already flushed).
fn sync_dir(dir: &Path) {
    #[cfg(unix)]
    if let Ok(d) = std::fs::File::open(dir) {
        let _ = d.sync_all();
    }
    #[cfg(not(unix))]
    let _ = dir;
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn temp() -> tempfile::TempDir {
        tempfile::tempdir().unwrap()
    }

    #[test]
    fn a_missing_file_is_defaults_without_a_problem() {
        let t = temp();
        assert_eq!(load(&t.path().join(FILE_NAME)), Loaded::default());
    }

    #[test]
    fn a_saved_file_reads_back_and_leaves_no_temporary_files() {
        let t = temp();
        let path = t.path().join("deep/settings.json");
        let mut file = SettingsFile::default();
        file.sections
            .insert("pd.appearance".into(), json!({ "theme": "dark" }));
        save(&path, &file).unwrap();
        let back = load(&path);
        assert_eq!(back.problem, None);
        assert_eq!(back.file.version, FORMAT_VERSION);
        assert_eq!(back.file.sections, file.sections);
        let names: Vec<String> = std::fs::read_dir(path.parent().unwrap())
            .unwrap()
            .flatten()
            .map(|e| e.file_name().to_string_lossy().into_owned())
            .collect();
        assert_eq!(names, ["settings.json"]);
        // An older file without a version reads.
        std::fs::write(&path, r#"{ "sections": { "a": { "x": 1 } } }"#).unwrap();
        assert_eq!(load(&path).file.sections["a"], json!({ "x": 1 }));
    }

    #[test]
    fn a_damaged_file_gives_defaults_one_problem_and_is_kept_aside() {
        let t = temp();
        let path = t.path().join(FILE_NAME);
        for (n, text) in [
            "{ nope",
            "[1, 2]",
            r#"{"version":1,"sections":{"a":3}}"#,
            r#"{"version":1,"other":true}"#,
            r#"{"version":"x"}"#,
        ]
        .iter()
        .enumerate()
        {
            std::fs::write(&path, text).unwrap();
            let loaded = load(&path);
            let p = loaded.problem.expect("a problem");
            assert_eq!(p.kind, LoadProblemKind::Damaged, "{text}");
            assert_eq!(loaded.file, SettingsFile::default());
            let kept = if n == 0 {
                "settings.json.corrupt".to_string()
            } else {
                format!("settings.json.corrupt.{n}")
            };
            assert_eq!(p.kept_as.as_deref(), Some(kept.as_str()));
            assert!(
                p.message.contains("Defaults are in use") && p.message.contains(&kept),
                "{}",
                p.message
            );
            assert!(!path.exists() && t.path().join(&kept).exists());
        }
        // Every slot is taken: the last one is reused.
        std::fs::write(&path, "x").unwrap();
        assert_eq!(
            load(&path).problem.unwrap().kept_as.as_deref(),
            Some("settings.json.corrupt.4")
        );
        std::fs::write(&path, "y").unwrap();
        assert_eq!(
            load(&path).problem.unwrap().kept_as.as_deref(),
            Some("settings.json.corrupt.4")
        );
    }

    #[test]
    fn an_oversized_file_is_damaged() {
        let t = temp();
        let path = t.path().join(FILE_NAME);
        std::fs::write(&path, vec![b' '; (MAX_FILE_BYTES + 1) as usize]).unwrap();
        let p = load(&path).problem.unwrap();
        assert_eq!(p.kind, LoadProblemKind::Damaged);
        assert!(p.message.contains("larger than 1024 KB"));
    }

    #[test]
    fn a_newer_file_is_not_read_and_kept_for_the_newer_app() {
        let t = temp();
        let path = t.path().join(FILE_NAME);
        std::fs::write(
            &path,
            r#"{ "version": 7, "sections": { "a": { "x": 1 } } }"#,
        )
        .unwrap();
        let loaded = load(&path);
        assert_eq!(loaded.file, SettingsFile::default());
        let p = loaded.problem.unwrap();
        assert_eq!(
            (p.kind, p.kept_as.as_deref()),
            (LoadProblemKind::Newer, Some("settings.json.v7"))
        );
        assert!(p.message.contains("format 7"));
        assert!(t.path().join("settings.json.v7").exists());
    }

    #[test]
    fn a_folder_in_place_of_the_file_is_unreadable_and_left_alone() {
        let t = temp();
        let path = t.path().join(FILE_NAME);
        std::fs::create_dir(&path).unwrap();
        let p = load(&path).problem.unwrap();
        assert_eq!((p.kind, p.kept_as), (LoadProblemKind::Unreadable, None));
        assert!(path.is_dir());
        assert!(
            save(&path, &SettingsFile::default()).is_err(),
            "a folder is not replaced"
        );
        let leftovers = std::fs::read_dir(t.path())
            .unwrap()
            .flatten()
            .filter(|e| e.file_name().to_string_lossy().ends_with(".tmp"))
            .count();
        assert_eq!(
            leftovers, 0,
            "the temporary file is removed after a failed write"
        );
    }

    #[cfg(unix)]
    #[test]
    fn an_unreadable_file_is_left_alone() {
        use std::os::unix::fs::PermissionsExt;
        let t = temp();
        let path = t.path().join(FILE_NAME);
        std::fs::write(&path, "{}").unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o000)).unwrap();
        let readable = std::fs::File::open(&path).is_ok(); // root reads anything
        let loaded = load(&path);
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600)).unwrap();
        let kind = loaded.problem.map(|p| p.kind);
        assert!(
            readable || kind == Some(LoadProblemKind::Unreadable),
            "{kind:?}"
        );
        assert!(path.exists(), "an unreadable file is left where it is");
        // A folder that cannot be searched: the metadata itself fails.
        let locked = t.path().join("locked");
        std::fs::create_dir(&locked).unwrap();
        std::fs::set_permissions(&locked, std::fs::Permissions::from_mode(0o000)).unwrap();
        let inner = load(&locked.join(FILE_NAME));
        std::fs::set_permissions(&locked, std::fs::Permissions::from_mode(0o700)).unwrap();
        let kind = inner.problem.map(|p| p.kind);
        assert!(
            readable || kind == Some(LoadProblemKind::Unreadable),
            "{kind:?}"
        );
    }
}
