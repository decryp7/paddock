//! What the helper checks about itself before it serves (ADR 0027 §3): the protected-location rule on the program, the data
//! folder and the configuration, each by the facts of the open object. Pure over [`ProtectedFiles`]: the verdicts are
//! returned for the log and the daemon fails closed when a required one fails.

use crate::config::HelperConfig;
use paddock_service::privileged::{verify_protected, FileKind, ProtectedFiles};
use std::path::{Path, PathBuf};

/// One checked thing.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Verdict {
    /// What it is ("the program", "the data folder", "the configuration").
    pub what: &'static str,
    /// Where.
    pub path: PathBuf,
    /// `Ok` when protected, else why not.
    pub result: Result<(), String>,
}

/// Checks `program`, the data folder and the configuration file.
pub fn verdicts(files: &mut dyn ProtectedFiles, program: &Path, data_dir: &Path, config: &Path) -> Vec<Verdict> {
    [
        ("the program", program, FileKind::File),
        ("the data folder", data_dir, FileKind::Dir),
        ("the configuration", config, FileKind::File),
    ]
    .into_iter()
    .map(|(what, path, kind)| {
        let result = match files.facts(path) {
            Ok(facts) => verify_protected(facts.as_ref(), kind).map_err(|v| v.to_string()),
            Err(e) => Err(format!("it could not be examined: {e}")),
        };
        Verdict {
            what,
            path: path.to_path_buf(),
            result,
        }
    })
    .collect()
}

/// The first failed verdict as the reason the helper will not start.
///
/// # Errors
/// Words for the log and the service manager's output.
pub fn require_all(verdicts: &[Verdict]) -> Result<(), String> {
    match verdicts.iter().find(|v| v.result.is_err()) {
        Some(v) => Err(format!(
            "The helper will not start: {} ({}) is not protected: {}.",
            v.what,
            v.path.display(),
            v.result.as_ref().err().map_or("", String::as_str)
        )),
        None => Ok(()),
    }
}

/// Reads the configuration by handle and applies the rule to what was opened.
///
/// # Errors
/// Why the helper will not start.
pub fn load_config(files: &mut dyn ProtectedFiles, path: &Path) -> Result<HelperConfig, String> {
    let (facts, text) = files
        .read_checked(path, 16 * 1024)
        .map_err(|e| format!("The helper will not start: {} cannot be read: {e}", path.display()))?
        .ok_or_else(|| {
            format!(
                "The helper will not start: {} is missing. Turn the helper on again in Settings > Helper.",
                path.display()
            )
        })?;
    verify_protected(Some(&facts), FileKind::File)
        .map_err(|v| format!("The helper will not start: {} is not protected: {v}.", path.display()))?;
    HelperConfig::parse(&text).map_err(|e| format!("The helper will not start: {e}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use dyno_core::helper_wire::HelperUser;
    use paddock_service::testing::FakeFiles;

    fn disk() -> FakeFiles {
        let f = FakeFiles::new();
        f.put_file("/p/dyno-helper", b"x", 0, 0o755).put_dir("/d", 0, 0o700).put_file(
            "/d/config.json",
            HelperConfig::new(HelperUser::Uid(5), "1.1.0").to_text().as_bytes(),
            0,
            0o600,
        );
        f
    }

    #[test]
    fn a_protected_install_passes_and_the_first_unprotected_thing_stops_the_start() {
        let mut f = disk();
        let v = verdicts(&mut f, Path::new("/p/dyno-helper"), Path::new("/d"), Path::new("/d/config.json"));
        assert_eq!(v.len(), 3);
        assert_eq!(require_all(&v), Ok(()));
        f.put_file("/p/dyno-helper", b"x", 501, 0o755);
        let v = verdicts(&mut f, Path::new("/p/dyno-helper"), Path::new("/d"), Path::new("/d/config.json"));
        let e = require_all(&v).unwrap_err();
        assert!(e.contains("the program") && e.contains("not root"), "{e}");
        let v = verdicts(&mut f, Path::new("/p/missing"), Path::new("/d"), Path::new("/d/config.json"));
        assert!(require_all(&v).unwrap_err().contains("not there"));
    }

    #[test]
    fn the_configuration_is_read_by_handle_and_checked() {
        let mut f = disk();
        assert_eq!(load_config(&mut f, Path::new("/d/config.json")).unwrap().owner, HelperUser::Uid(5));
        assert!(load_config(&mut f, Path::new("/d/none")).unwrap_err().contains("missing"));
        f.put_file("/d/config.json", b"{", 0, 0o600);
        assert!(load_config(&mut f, Path::new("/d/config.json")).unwrap_err().contains("damaged"));
        f.put_file("/d/config.json", b"{}", 501, 0o600);
        assert!(load_config(&mut f, Path::new("/d/config.json"))
            .unwrap_err()
            .contains("not protected"));
        f.fail_with(None);
        let big = vec![b' '; 20_000];
        f.put_file("/d/config.json", &big, 0, 0o600);
        assert!(load_config(&mut f, Path::new("/d/config.json"))
            .unwrap_err()
            .contains("cannot be read"));
    }
}
