//! Finding a program the way the job would: on the job's own `PATH`, with Windows' spelling of the variable (`Path`) and its program
//! extensions, never through the parent's `PATH`. The platform is a parameter so that every OS tests both behaviours.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

/// The value of `key` in `vars`: exact on Unix, ignoring case on Windows (the real variable is usually `Path`, and `Env` keeps the
/// spelling the process had). An exact match wins over a differently-cased one.
pub fn lookup<'a>(vars: &'a BTreeMap<String, String>, key: &str, windows: bool) -> Option<&'a str> {
    let found = vars.get(key).or_else(|| {
        windows
            .then(|| vars.iter().find(|(k, _)| k.eq_ignore_ascii_case(key)).map(|(_, v)| v))
            .flatten()
    });
    found.map(String::as_str).filter(|v| !v.is_empty())
}

/// `vars` with one key per variable. On Windows `PATH` and `Path` are one variable, and a map that holds both would give `get`, `set` and
/// a child's environment three answers; the key kept is the first in order and its value is the one the process itself reports for
/// it (`os_value`), falling back to the value it had. Elsewhere `vars` is returned as it is.
pub fn normalise(vars: BTreeMap<String, String>, windows: bool, os_value: &dyn Fn(&str) -> Option<String>) -> BTreeMap<String, String> {
    if !windows {
        return vars;
    }
    let mut out: BTreeMap<String, String> = BTreeMap::new();
    let mut seen: Vec<String> = Vec::new();
    for (k, v) in &vars {
        let lower = k.to_ascii_lowercase();
        if seen.contains(&lower) {
            continue;
        }
        seen.push(lower);
        out.insert(k.clone(), os_value(k).unwrap_or_else(|| v.clone()));
    }
    out
}

/// The extensions a bare program name may take on Windows, in `PATHEXT` order, limited to the ones a job's npm and node come as
/// (`.exe`, `.cmd`); without a `PATHEXT` the order is `.exe`, `.cmd`.
pub fn extensions(pathext: Option<&str>) -> Vec<String> {
    let listed: Vec<String> = pathext
        .unwrap_or("")
        .split(';')
        .map(|e| e.trim().to_ascii_lowercase())
        .filter(|e| e == ".exe" || e == ".cmd")
        .collect();
    if listed.is_empty() {
        vec![".exe".into(), ".cmd".into()]
    } else {
        listed
    }
}

/// The first `name` (with each of `extensions` on Windows) in an absolute folder of `path`, in order. Empty and relative entries are
/// ignored: a relative one would be read against the folder the probe runs in, which is the tagged tree. On Unix the file must be
/// executable.
pub fn find_program(path: &str, name: &str, windows: bool, extensions: &[String]) -> Option<PathBuf> {
    for dir in std::env::split_paths(path) {
        if dir.as_os_str().is_empty() || !dir.is_absolute() {
            continue;
        }
        let candidates: Vec<PathBuf> = if windows {
            extensions.iter().map(|e| dir.join(format!("{name}{e}"))).collect()
        } else {
            vec![dir.join(name)]
        };
        if let Some(found) = candidates.into_iter().find(|p| p.is_file() && (windows || is_executable(p))) {
            return Some(found);
        }
    }
    None
}

#[cfg(unix)]
fn is_executable(p: &Path) -> bool {
    use std::os::unix::fs::PermissionsExt;
    std::fs::metadata(p).is_ok_and(|m| m.permissions().mode() & 0o111 != 0)
}

#[cfg(not(unix))]
fn is_executable(_: &Path) -> bool {
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    fn vars(pairs: &[(&str, &str)]) -> BTreeMap<String, String> {
        pairs.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect()
    }

    fn touch(dir: &Path, name: &str, mode: u32) {
        let p = dir.join(name);
        std::fs::write(&p, "x").unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&p, std::fs::Permissions::from_mode(mode)).unwrap();
        }
        let _ = mode;
    }

    fn joined(dirs: &[&Path]) -> String {
        std::env::join_paths(dirs).unwrap().to_string_lossy().into_owned()
    }

    #[test]
    fn windows_finds_the_variable_under_any_case_and_unix_only_under_its_own() {
        let v = vars(&[("Path", "C:\\a"), ("PATHEXT", ".EXE;.CMD")]);
        assert_eq!(lookup(&v, "PATH", true), Some("C:\\a"));
        assert_eq!(lookup(&v, "PATH", false), None);
        assert_eq!(
            lookup(&vars(&[("PATH", "/x"), ("Path", "/y")]), "PATH", true),
            Some("/x"),
            "an exact key wins"
        );
        assert_eq!(lookup(&vars(&[("Path", "")]), "PATH", true), None, "empty is unset");
    }

    #[test]
    fn a_path_under_the_key_path_finds_a_fake_npm_cmd_with_windows_semantics() {
        let d = tempfile::tempdir().unwrap();
        touch(d.path(), "npm.cmd", 0o644);
        let env = vars(&[("Path", d.path().to_str().unwrap())]);
        let path = lookup(&env, "PATH", true).expect("Path is read as PATH");
        let found = find_program(path, "npm", true, &extensions(None));
        assert_eq!(found, Some(d.path().join("npm.cmd")));
    }

    #[test]
    fn exe_counts_as_well_as_cmd_and_pathext_orders_them() {
        let d = tempfile::tempdir().unwrap();
        touch(d.path(), "npm.cmd", 0o644);
        touch(d.path(), "npm.exe", 0o644);
        let p = joined(&[d.path()]);
        assert_eq!(
            find_program(&p, "npm", true, &extensions(Some(".COM;.EXE;.CMD"))),
            Some(d.path().join("npm.exe"))
        );
        assert_eq!(
            find_program(&p, "npm", true, &extensions(Some(".CMD;.EXE"))),
            Some(d.path().join("npm.cmd"))
        );
        assert_eq!(find_program(&p, "npm", true, &extensions(None)), Some(d.path().join("npm.exe")));
        let only_exe = tempfile::tempdir().unwrap();
        touch(only_exe.path(), "node.exe", 0o644);
        assert_eq!(
            find_program(&joined(&[only_exe.path()]), "node", true, &extensions(None)),
            Some(only_exe.path().join("node.exe"))
        );
    }

    #[cfg(unix)]
    #[test]
    fn on_unix_the_file_needs_its_executable_bit_and_the_search_goes_on() {
        let (a, b) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
        touch(a.path(), "npm", 0o644);
        touch(b.path(), "npm", 0o755);
        let p = joined(&[a.path(), b.path()]);
        assert_eq!(find_program(&p, "npm", false, &[]), Some(b.path().join("npm")));
        assert_eq!(find_program(&joined(&[a.path()]), "npm", false, &[]), None);
    }

    #[test]
    fn empty_and_relative_entries_are_ignored() {
        // The tests run in the crate folder, which holds `Cargo.toml`: with `.toml` as the extension, a relative entry that was read
        // against the current folder would find it. Nothing is written anywhere.
        let toml = [".toml".to_string()];
        assert!(
            Path::new("Cargo.toml").is_file(),
            "the control: the current folder holds Cargo.toml"
        );
        let sep = if cfg!(windows) { ";" } else { ":" };
        for entry in ["", ".", "src/.."] {
            let p = format!("{entry}{sep}{entry}");
            assert_eq!(find_program(&p, "Cargo", true, &toml), None, "{entry:?}");
        }
        // An absolute entry after them is still found.
        let d = tempfile::tempdir().unwrap();
        touch(d.path(), "npm.cmd", 0o755);
        touch(d.path(), "npm", 0o755);
        let p = format!("{sep}.{sep}{}", d.path().display());
        let name = if cfg!(windows) {
            d.path().join("npm.cmd")
        } else {
            d.path().join("npm")
        };
        assert_eq!(find_program(&p, "npm", cfg!(windows), &extensions(None)), Some(name));
    }

    #[test]
    fn a_map_with_path_and_path_is_one_variable_with_the_value_the_process_reports() {
        let both = vars(&[("PATH", "/x"), ("Path", "/y"), ("HOME", "/h")]);
        let os = |k: &str| {
            if k.eq_ignore_ascii_case("path") {
                Some("/y".to_string())
            } else {
                None
            }
        };
        let n = normalise(both.clone(), true, &os);
        assert_eq!(n.len(), 2, "one key for the path variable, one for HOME: {n:?}");
        assert_eq!(lookup(&n, "PATH", true), Some("/y"));
        assert_eq!(lookup(&n, "Path", true), Some("/y"));
        assert_eq!(lookup(&n, "HOME", true), Some("/h"));
        assert_eq!(normalise(both.clone(), false, &os), both, "Unix keeps every key");
        // No answer from the process: the kept key's own value stays.
        assert_eq!(normalise(both, true, &|_| None).get("PATH").map(String::as_str), Some("/x"));
    }
}
