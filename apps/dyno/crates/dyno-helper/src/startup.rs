//! What the helper checks about itself before it serves (ADR 0027 §3): the protected-location rule on the program, the data
//! folder and the configuration, each by the facts of the open object. Pure over [`ProtectedFiles`]: the verdicts are
//! returned for the log and the daemon fails closed when a required one fails.

use crate::config::HelperConfig;
use paddock_service::privileged::trust::{parent_of, verify_chain, Style, TrustPort};
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

/// Checks `program`, the data folder and the configuration file, and each of `others` (the policy, the audit log, a pending
/// `config.json.new`) that is there: a file a user could have planted in an unprotected data folder, or a link, is not
/// trusted just because it is not read yet.
pub fn verdicts(
    files: &mut dyn ProtectedFiles,
    trust: &mut dyn TrustPort,
    program: &Path,
    data_dir: &Path,
    config: &Path,
    others: &[(&'static str, &Path)],
) -> Vec<Verdict> {
    let mut items = vec![
        ("the program", program, FileKind::File, true),
        ("the data folder", data_dir, FileKind::Dir, true),
        ("the configuration", config, FileKind::File, true),
    ];
    for (what, path) in others {
        // Only what is there: a missing optional file is nothing to distrust.
        if files.facts(path).ok().flatten().is_some() {
            items.push((*what, *path, FileKind::File, false));
        }
    }
    items
        .into_iter()
        .map(|(what, path, kind, _)| {
            let mut result = match files.facts(path) {
                Ok(facts) => verify_protected(facts.as_ref(), kind).map_err(|v| v.to_string()),
                Err(e) => Err(format!("it could not be examined: {e}")),
            };
            // Windows has no owner or mode in the facts: the owner and DACL of every component of the path decide (the data
            // folder and the configuration inside the data folder's protected root, the program inside its own folder).
            if result.is_ok() && trust.style() == Style::Windows {
                let text = path.to_string_lossy();
                let root = if what == "the program" {
                    parent_of(&text, Style::Windows).unwrap_or_default()
                } else {
                    data_dir.to_string_lossy().into_owned()
                };
                result = verify_chain(trust, &text, kind, Some(&root), false).map_err(|u| format!("{} ({})", u.reason, u.path));
            }
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
    use paddock_service::privileged::trust_fake::*;
    use paddock_service::testing::FakeFiles;

    fn unix_port() -> FakePort {
        FakePort {
            style: Style::Unix,
            nodes: Default::default(),
            links: Default::default(),
            calls: vec![],
        }
    }

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
        let v = verdicts(
            &mut f,
            &mut unix_port(),
            Path::new("/p/dyno-helper"),
            Path::new("/d"),
            Path::new("/d/config.json"),
            &[],
        );
        assert_eq!(v.len(), 3);
        assert_eq!(require_all(&v), Ok(()));
        f.put_file("/p/dyno-helper", b"x", 501, 0o755);
        let v = verdicts(
            &mut f,
            &mut unix_port(),
            Path::new("/p/dyno-helper"),
            Path::new("/d"),
            Path::new("/d/config.json"),
            &[],
        );
        let e = require_all(&v).unwrap_err();
        assert!(e.contains("the program") && e.contains("not root"), "{e}");
        let v = verdicts(
            &mut f,
            &mut unix_port(),
            Path::new("/p/missing"),
            Path::new("/d"),
            Path::new("/d/config.json"),
            &[],
        );
        assert!(require_all(&v).unwrap_err().contains("not there"));
    }

    #[test]
    fn the_policy_the_audit_log_and_a_pending_configuration_are_checked_when_they_are_there() {
        let mut f = disk();
        let (policy, audit, pending) = (
            Path::new("/d/policy.toml"),
            Path::new("/d/audit.jsonl"),
            Path::new("/d/config.json.new"),
        );
        let others = [
            ("the policy file", policy),
            ("the audit log", audit),
            ("the pending configuration", pending),
        ];
        let run = |f: &mut FakeFiles| {
            verdicts(
                f,
                &mut unix_port(),
                Path::new("/p/dyno-helper"),
                Path::new("/d"),
                Path::new("/d/config.json"),
                &others,
            )
        };
        assert_eq!(run(&mut f).len(), 3, "files that are not there are not checked");
        f.put_file("/d/audit.jsonl", b"{}", 0, 0o600)
            .put_file("/d/config.json.new", b"{}", 501, 0o600);
        let v = run(&mut f);
        assert_eq!(v.len(), 5);
        let e = require_all(&v).unwrap_err();
        assert!(e.contains("the pending configuration") && e.contains("not root"), "{e}");
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

#[cfg(test)]
mod windows_tests {
    use super::*;
    use paddock_service::privileged::trust_fake::*;
    use paddock_service::testing::FakeFiles;

    const DATA: &str = "C:\\ProgramData\\Dyno\\Helper";

    #[test]
    fn the_data_folder_the_unfixed_helper_left_is_not_reported_protected() {
        // The log of the VM said: what=the data folder protected=true, for the real SDDL below (Users may add files).
        let files = FakeFiles::windows();
        files.put_file("C:\\Program Files\\Dyno Helper\\dyno-helper.exe", b"x", 0, 0o755);
        files
            .put_dir(DATA, 0, 0o700)
            .put_file("C:\\ProgramData\\Dyno\\Helper\\config.json", b"{}", 0, 0o600);
        let mut files = files;
        let mut m = windows_machine(|_| locked());
        m.put("C:\\Program Files", from_sddl(FileKind::Dir, SDDL_PROGRAM_FILES))
            .put("C:\\Program Files\\Dyno Helper", from_sddl(FileKind::Dir, SDDL_PROGRAM_FILES_DYNO))
            .put(
                "C:\\Program Files\\Dyno Helper\\dyno-helper.exe",
                from_sddl(FileKind::File, SDDL_PROGRAM_FILES_DYNO),
            )
            .put(DATA, from_sddl(FileKind::Dir, SDDL_DYNO_HELPER_UNFIXED))
            .put("C:\\ProgramData\\Dyno\\Helper\\config.json", file(locked()));
        let program = Path::new("C:\\Program Files\\Dyno Helper\\dyno-helper.exe");
        let config = Path::new("C:\\ProgramData\\Dyno\\Helper\\config.json");
        let v = verdicts(&mut files, &mut m, program, Path::new(DATA), config, &[]);
        let folder = v.iter().find(|v| v.what == "the data folder").unwrap();
        assert!(folder.result.as_ref().unwrap_err().contains("add file"), "{folder:?}");
        assert!(v.iter().find(|v| v.what == "the program").unwrap().result.is_ok(), "{v:?}");
        assert!(require_all(&v).unwrap_err().contains("not protected"));
        // Locked, it passes.
        m.put(DATA, locked());
        assert_eq!(
            require_all(&verdicts(&mut files, &mut m, program, Path::new(DATA), config, &[])),
            Ok(())
        );
    }
}
