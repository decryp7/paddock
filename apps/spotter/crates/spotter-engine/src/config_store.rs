//! `spotter.toml` on disk: read tolerantly (falling back to the backup, then to the defaults, keeping the damaged
//! file as `.corrupt`) and written atomically (temporary file, flush, backup of the previous file, rename).
use crate::error::{EngineError, Result};
use spotter_core::config::{self, ConfigDoc};
use spotter_core::limits::MAX_CONFIG_BYTES;
use std::io::Write;
use std::path::{Path, PathBuf};

/// What loading the configuration found.
#[derive(Debug)]
pub struct LoadedConfig {
    /// The configuration (the defaults when nothing could be read).
    pub doc: ConfigDoc,
    /// Problems that were worked around.
    pub warnings: Vec<String>,
    /// It came from the old Release Monitor's format; the caller writes it back in the new one.
    pub migrated: bool,
}

fn sibling(path: &Path, suffix: &str) -> PathBuf {
    let mut name = path.file_name().map(|n| n.to_os_string()).unwrap_or_default();
    name.push(suffix);
    path.with_file_name(name)
}

fn read_limited(path: &Path) -> std::io::Result<Option<String>> {
    match std::fs::metadata(path) {
        Ok(m) if m.len() > (MAX_CONFIG_BYTES as u64) * 2 => Ok(Some("x".repeat(MAX_CONFIG_BYTES + 1))),
        Ok(_) => std::fs::read_to_string(path).map(Some),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(e),
    }
}

/// Loads the configuration at `path`.
pub fn load(path: &Path) -> LoadedConfig {
    let fresh = |warnings: Vec<String>| LoadedConfig {
        doc: ConfigDoc::default(),
        warnings,
        migrated: false,
    };
    let text = match read_limited(path) {
        Ok(Some(t)) => t,
        Ok(None) => return fresh(Vec::new()),
        Err(e) => {
            return fresh(vec![format!(
                "The configuration could not be read ({e}); Spotter starts with the defaults."
            )])
        }
    };
    match config::parse(&text) {
        Ok(l) => LoadedConfig {
            doc: l.doc,
            warnings: l.warnings,
            migrated: l.migrated,
        },
        Err(why) => {
            let mut warnings = vec![why];
            let kept = sibling(path, ".corrupt");
            let backup = sibling(path, ".bak");
            if std::fs::rename(path, &kept).is_ok() {
                warnings.push(format!("The unreadable file was kept as {}.", kept.display()));
            }
            if let Ok(Some(b)) = read_limited(&backup) {
                if let Ok(l) = config::parse(&b) {
                    warnings.push("Spotter restored your last good configuration from the backup.".into());
                    warnings.extend(l.warnings);
                    return LoadedConfig {
                        doc: l.doc,
                        warnings,
                        migrated: false,
                    };
                }
            }
            warnings.push("Spotter starts with the defaults.".into());
            fresh(warnings)
        }
    }
}

/// Writes the configuration atomically, keeping the previous file as `.bak`.
pub fn save(path: &Path, doc: &ConfigDoc) -> Result<()> {
    let fail = |e: std::io::Error| {
        EngineError::Internal(format!(
            "The configuration could not be saved to {} ({e}). Check that the folder exists and is writable.",
            path.display()
        ))
    };
    let tmp = sibling(path, ".tmp");
    let mut f = std::fs::File::create(&tmp).map_err(fail)?;
    f.write_all(config::render(doc).as_bytes()).map_err(fail)?;
    f.sync_all().map_err(fail)?;
    drop(f);
    if path.exists() {
        // A failed backup does not stop the save; the new file is what matters.
        let _ = std::fs::copy(path, sibling(path, ".bak"));
    }
    std::fs::rename(&tmp, path).map_err(|e| {
        let _ = std::fs::remove_file(&tmp);
        fail(e)
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use spotter_core::source::SourceConfig;
    use spotter_core::watch::WatchSpec;

    fn doc(name: &str) -> ConfigDoc {
        let mut d = ConfigDoc::default();
        d.watches.push(WatchSpec {
            id: "a".into(),
            name: name.into(),
            source: SourceConfig::new("folder", &[("path", "/x")]),
            versions: "semver".into(),
            version_pattern: None,
            include_prereleases: false,
            name_filter: String::new(),
            interval_secs: 60,
            notify: Default::default(),
            actions: Vec::new(),
            channels: Vec::new(),
            version_filter: Default::default(),
            allow_local: false,
            paused: false,
            tracking: Default::default(),
        });
        d
    }

    #[test]
    fn saving_is_atomic_and_keeps_a_backup() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("spotter.toml");
        assert!(load(&p).doc.watches.is_empty(), "a missing file is the defaults, silently");
        assert!(load(&p).warnings.is_empty());
        save(&p, &doc("one")).unwrap();
        save(&p, &doc("two")).unwrap();
        assert_eq!(load(&p).doc.watches[0].name, "two");
        let bak = std::fs::read_to_string(dir.path().join("spotter.toml.bak")).unwrap();
        assert!(bak.contains("one"));
        assert!(!dir.path().join("spotter.toml.tmp").exists());
    }

    #[test]
    fn a_damaged_file_is_kept_and_the_backup_restored() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("spotter.toml");
        save(&p, &doc("good")).unwrap();
        save(&p, &doc("good")).unwrap();
        std::fs::write(&p, "this is = = not toml").unwrap();
        let l = load(&p);
        assert_eq!(l.doc.watches[0].name, "good");
        assert!(l.warnings.iter().any(|w| w.contains("restored")), "{:?}", l.warnings);
        assert!(dir.path().join("spotter.toml.corrupt").exists());
        std::fs::write(&p, "= = =").unwrap();
        std::fs::remove_file(dir.path().join("spotter.toml.bak")).unwrap();
        let l = load(&p);
        assert!(l.doc.watches.is_empty() && l.warnings.last().unwrap().contains("defaults"));
    }

    #[test]
    fn oversize_and_unreadable_files_are_defaults_with_a_warning() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("spotter.toml");
        std::fs::write(&p, vec![b'#'; MAX_CONFIG_BYTES * 2 + 10]).unwrap();
        assert!(load(&p).warnings[0].contains("larger"));
        let d = dir.path().join("a-folder.toml");
        std::fs::create_dir(&d).unwrap();
        assert!(load(&d).warnings[0].contains("could not be read"));
    }

    #[test]
    fn the_old_release_monitor_format_is_migrated() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("spotter.toml");
        std::fs::write(
            &p,
            "path = \"/drops\"\nfile_regex = \".*\\\\.zip\"\ninterval_seconds = 30\nnaggy = true\n",
        )
        .unwrap();
        let l = load(&p);
        assert!(l.migrated);
        assert_eq!(l.doc.watches[0].versions, "build-number");
        assert!(l.doc.watches[0].notify.nag);
    }

    #[test]
    fn saving_into_a_missing_folder_says_what_to_fix() {
        let dir = tempfile::tempdir().unwrap();
        let e = save(&dir.path().join("nope").join("spotter.toml"), &ConfigDoc::default()).unwrap_err();
        assert!(e.to_string().contains("writable"));
    }

    #[test]
    fn a_path_below_a_file_is_a_read_warning_and_a_damaged_backup_is_not_trusted() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("plain");
        std::fs::write(&file, "x").unwrap();
        let l = load(&file.join("spotter.toml"));
        assert!(l.warnings[0].contains("could not be read"), "{:?}", l.warnings);
        let p = dir.path().join("spotter.toml");
        std::fs::write(&p, "= = =").unwrap();
        std::fs::write(dir.path().join("spotter.toml.bak"), "also = = broken").unwrap();
        let l = load(&p);
        assert!(l.doc.watches.is_empty());
        assert!(l.warnings.iter().all(|w| !w.contains("restored")), "{:?}", l.warnings);
        assert!(l.warnings.last().unwrap().contains("defaults"));
    }

    #[test]
    fn a_save_that_cannot_replace_the_file_leaves_no_temporary_file() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("spotter.toml");
        // A folder where the file belongs: the final rename fails.
        std::fs::create_dir_all(p.join("inside")).unwrap();
        let e = save(&p, &doc("x")).unwrap_err();
        assert!(e.to_string().contains("could not be saved"), "{e}");
        assert!(!dir.path().join("spotter.toml.tmp").exists());
    }
}
