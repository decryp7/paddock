//! Supervising the runners: the loop that starts each runner with its log redirected, classifies log lines,
//! applies the stall rule and the restart policy, and drives updates (ADR 0001, 0002).
//!
//! Only configuration loading is built so far; [`run`] reports that supervision is not implemented yet.

use crate::error::Error;
use crate::paths::{CONFIG_BACKUP, CONFIG_FILE};
use pitcrew_core::config::{self, Loaded, MAX_CONFIG_BYTES};
use std::path::Path;

/// Reads a text file of at most [`MAX_CONFIG_BYTES`] (+1, so the core sees that it is too large); `None` when it
/// does not exist.
fn read_bounded(path: &Path) -> Result<Option<String>, Error> {
    use std::io::Read;
    let file = match std::fs::File::open(path) {
        Ok(f) => f,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(source) => {
            return Err(Error::Io {
                path: path.display().to_string(),
                source,
            })
        }
    };
    let mut bytes = Vec::new();
    file.take(MAX_CONFIG_BYTES as u64 + 1)
        .read_to_end(&mut bytes)
        .map_err(|source| Error::Io {
            path: path.display().to_string(),
            source,
        })?;
    Ok(Some(String::from_utf8_lossy(&bytes).into_owned()))
}

/// Loads the configuration from `dir` (never fails on damaged contents; see `pitcrew_core::config`).
pub fn load_config(dir: &Path) -> Result<Loaded, Error> {
    let primary = read_bounded(&dir.join(CONFIG_FILE))?;
    let backup = read_bounded(&dir.join(CONFIG_BACKUP))?;
    Ok(config::load(primary.as_deref(), backup.as_deref()))
}

/// Runs the supervision loop until the service is asked to stop.
pub fn run(dir: &Path) -> Result<(), Error> {
    let loaded = load_config(dir)?;
    for p in &loaded.problems {
        eprintln!("pitcrew-service: configuration: {p}");
    }
    Err(Error::NotImplemented("Supervising runners"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use pitcrew_core::config::LoadSource;

    fn temp_dir(name: &str) -> std::path::PathBuf {
        let d = std::env::temp_dir().join(format!("pitcrew-test-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn a_missing_config_loads_the_defaults() {
        let d = temp_dir("missing");
        let l = load_config(&d).unwrap();
        assert_eq!(l.source, LoadSource::Defaults);
        assert!(l.needs_save);
        assert!(matches!(run(&d), Err(Error::NotImplemented(_))));
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn a_corrupt_config_falls_back_to_its_backup() {
        let d = temp_dir("corrupt");
        std::fs::write(d.join(CONFIG_FILE), [0xff, 0xfe, b'[', b'[']).unwrap();
        std::fs::write(d.join(CONFIG_BACKUP), "schema_version = 1\n").unwrap();
        let l = load_config(&d).unwrap();
        assert_eq!(l.source, LoadSource::Backup);
        assert!(l.set_aside_primary);
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn a_huge_config_is_refused_without_reading_it_all() {
        let d = temp_dir("huge");
        std::fs::write(d.join(CONFIG_FILE), vec![b'#'; MAX_CONFIG_BYTES * 2]).unwrap();
        let l = load_config(&d).unwrap();
        assert!(l.set_aside_primary);
        let _ = std::fs::remove_dir_all(&d);
    }
}
