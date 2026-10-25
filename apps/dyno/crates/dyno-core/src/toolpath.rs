//! Finding the user's tools (G11, ADR 0010 item 9): a service started by launchd or systemd gets a minimal `PATH`, so
//! `restic` or `borg` installed through Homebrew or a version manager is not found. The agent asks the user's login shell
//! for its `PATH` once (bounded, timed out, cached) and searches the per-tool override folders first, then that `PATH`,
//! then the process's own. This module is the pure part: parsing the shell's answer, validating overrides, composing the
//! search list.

/// Printed before the `PATH` so rc-file noise on the shell's output cannot be mistaken for it.
pub const MARKER: &str = "__DYNO_PATH__";
/// Printed before each exported variable.
pub const ENV_MARKER: &str = "__DYNO_ENV__";
/// The only variables read from the login shell besides `PATH`: where a backup tool's repository is and how it unlocks
/// it without a typed password. Passwords themselves (`RESTIC_PASSWORD`, `BORG_PASSPHRASE`) are never read.
pub const ENV_ALLOWLIST: [&str; 6] = [
    "RESTIC_REPOSITORY",
    "RESTIC_REPOSITORY_FILE",
    "RESTIC_PASSWORD_FILE",
    "RESTIC_PASSWORD_COMMAND",
    "BORG_REPO",
    "BORG_PASSCOMMAND",
];
/// The longest `PATH` accepted from a shell.
pub const MAX_PATH_BYTES: usize = 32 * 1024;
/// Most folders in an override.
pub const MAX_DIRS: usize = 16;

/// Whether `dir` is an absolute path by either OS's rules (`/x`, `C:\x`, `C:/x`, `\\server\share`).
pub fn is_absolute_any_os(dir: &str) -> bool {
    let b = dir.as_bytes();
    dir.starts_with('/')
        || dir.starts_with("\\\\")
        || (b.len() >= 3 && b[0].is_ascii_alphabetic() && b[1] == b':' && (b[2] == b'\\' || b[2] == b'/'))
}

/// Checks override folders: few, absolute, plain (no control characters, no `..`, no `PATH` separators inside one entry).
pub fn validate_dirs(dirs: &[String]) -> Result<(), String> {
    if dirs.len() > MAX_DIRS {
        return Err(format!("At most {MAX_DIRS} folders."));
    }
    for d in dirs {
        let bad = d.is_empty()
            || d.len() > 1024
            || d.chars().any(char::is_control)
            || d.contains(';')
            || (d.contains(':') && !d.as_bytes().get(1).is_some_and(|c| *c == b':'))
            || d.replace('\\', "/").split('/').any(|s| s == "..");
        if bad || !is_absolute_any_os(d) {
            return Err(format!(
                "\"{}\" is not a plain full folder path.",
                d.chars().take(60).collect::<String>()
            ));
        }
    }
    Ok(())
}

/// The `PATH` from the shell's output: the text after the last marker on its line.
pub fn parse_marked(output: &str) -> Option<String> {
    let line = output.lines().rev().find_map(|l| l.split_once(MARKER).map(|(_, p)| p.trim()))?;
    (!line.is_empty() && line.len() <= MAX_PATH_BYTES && !line.contains('\0')).then(|| line.to_string())
}

/// The allow-listed variables in a shell's output: lines `__DYNO_ENV__NAME=value`. Anything else is ignored.
pub fn parse_env(output: &str) -> Vec<(String, String)> {
    let mut out: Vec<(String, String)> = Vec::new();
    for line in output.lines() {
        let Some((_, rest)) = line.split_once(ENV_MARKER) else { continue };
        let Some((name, value)) = rest.split_once('=') else { continue };
        let value = value.trim();
        if ENV_ALLOWLIST.contains(&name)
            && !value.is_empty()
            && value.len() <= 1024
            && !value.contains('\0')
            && !out.iter().any(|(n, _)| n == name)
        {
            out.push((name.to_string(), value.to_string()));
        }
    }
    out
}

/// The folders to search, in order: the overrides, the login shell's `PATH`, the process's `PATH`, then well-known
/// places a service's minimal `PATH` misses. Relative entries are dropped (a relative folder would depend on the working
/// directory, which a service does not control); duplicates are kept once.
pub fn compose(overrides: &[String], login_path: Option<&str>, process_path: &str, separator: char, fallbacks: &[&str]) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    let split = |p: &str| -> Vec<String> { p.split(separator).map(str::to_string).collect() };
    let all = overrides
        .iter()
        .cloned()
        .chain(login_path.map(split).unwrap_or_default())
        .chain(split(process_path))
        .chain(fallbacks.iter().map(|s| s.to_string()));
    for d in all {
        if d.is_empty() || !is_absolute_any_os(&d) || out.contains(&d) {
            continue;
        }
        out.push(d);
    }
    out
}

/// Joins folders into a `PATH` value.
pub fn join(dirs: &[String], separator: char) -> String {
    dirs.join(&separator.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn marker_survives_rc_file_noise() {
        let out = "Welcome back!\nnvm: using node 20\n__DYNO_PATH__/opt/homebrew/bin:/usr/bin\n";
        assert_eq!(parse_marked(out).as_deref(), Some("/opt/homebrew/bin:/usr/bin"));
        assert_eq!(parse_marked("no marker"), None);
        assert_eq!(parse_marked("__DYNO_PATH__\n"), None);
        assert_eq!(parse_marked(&format!("__DYNO_PATH__{}", "a".repeat(MAX_PATH_BYTES + 1))), None);
    }

    #[test]
    fn only_allow_listed_variables_are_read() {
        let out = "chatter\n__DYNO_ENV__RESTIC_REPOSITORY=sftp:me@nas:/backup\n__DYNO_ENV__RESTIC_PASSWORD=secret\n__DYNO_ENV__BORG_REPO=\n__DYNO_ENV__RESTIC_REPOSITORY=again\n__DYNO_ENV__NOEQUALS\n";
        let env = parse_env(out);
        assert_eq!(env, [("RESTIC_REPOSITORY".to_string(), "sftp:me@nas:/backup".to_string())]);
        assert!(parse_env(&format!("__DYNO_ENV__BORG_REPO={}", "x".repeat(2000))).is_empty());
    }

    #[test]
    fn overrides_must_be_plain_absolute_folders() {
        assert!(validate_dirs(&["/opt/x/bin".into(), "C:\\Tools".into(), "D:/t".into(), "\\\\srv\\share".into()]).is_ok());
        for bad in ["bin", "", "./x", "/a/../b", "/a;b", "/a\nb", "/a:b", "~/bin"] {
            assert!(validate_dirs(&[bad.into()]).is_err(), "{bad}");
        }
        assert!(validate_dirs(&vec!["/a".to_string(); MAX_DIRS + 1]).is_err());
    }

    #[test]
    fn the_search_list_is_ordered_deduplicated_and_absolute() {
        let l = compose(
            &["/custom".to_string()],
            Some("/opt/homebrew/bin:/usr/bin:relative"),
            "/usr/bin:/bin::",
            ':',
            &["/opt/homebrew/bin", "/usr/local/bin"],
        );
        assert_eq!(l, ["/custom", "/opt/homebrew/bin", "/usr/bin", "/bin", "/usr/local/bin"]);
        assert_eq!(join(&l[..2], ':'), "/custom:/opt/homebrew/bin");
        let w = compose(&[], None, "C:\\Windows;C:\\Windows\\System32", ';', &[]);
        assert_eq!(w.len(), 2);
    }
}
