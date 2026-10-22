//! The environment a runner starts with (pure parts).
//!
//! launchd (macOS) and the systemd user manager (Linux) start the Pitcrew service with a minimal `PATH`
//! (`/usr/bin:/bin:/usr/sbin:/sbin`), and the runners — and so every job step — inherit it: `node`, `cargo` or
//! Homebrew tools are not found. The service therefore asks the user's login shell for the `PATH` it would give an
//! interactive terminal and passes that to the runners; this module holds the parts that need no I/O: reading the
//! shell's answer, the fallback when the shell does not answer, merging, and validating the per-runner `env` table of
//! `pitcrew.toml`.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

/// Marker printed before the `PATH` so that banners and `.zshrc` noise on standard output are ignored.
pub const PATH_BEGIN: &str = "__PITCREW_PATH_BEGIN__";
/// Marker printed after the `PATH`.
pub const PATH_END: &str = "__PITCREW_PATH_END__";
/// Longest `PATH` Pitcrew accepts from a shell or a configuration file.
pub const MAX_PATH_LEN: usize = 8192;
/// Most entries in a `PATH` (further ones are dropped).
pub const MAX_PATH_ENTRIES: usize = 128;
/// Most variables a runner may override.
pub const MAX_ENV_VARS: usize = 32;
/// Longest variable name.
pub const MAX_ENV_NAME: usize = 64;
/// Longest variable value.
pub const MAX_ENV_VALUE: usize = 4096;

/// The script the login shell runs: prints its `PATH` between markers.
pub fn probe_script() -> String {
    format!("printf '%s' \"{PATH_BEGIN}$PATH{PATH_END}\"")
}

/// Takes the `PATH` out of a login shell's output (the text between the last pair of markers) and cleans it with
/// [`sanitize_path`]. `None` if the shell printed no usable `PATH`.
pub fn parse_probe_output(stdout: &str) -> Option<String> {
    let after = stdout.rsplit_once(PATH_BEGIN)?.1;
    let path = after.split_once(PATH_END)?.0;
    let cleaned = sanitize_path(path);
    (!cleaned.is_empty()).then_some(cleaned)
}

/// Keeps the absolute entries of a `:`-separated `PATH`, once each, in order, bounded in count and length. Entries
/// with control characters are dropped.
pub fn sanitize_path(path: &str) -> String {
    let mut seen: Vec<&str> = Vec::new();
    let mut len = 0usize;
    for entry in path.split(':') {
        if !entry.starts_with('/') || entry.chars().any(char::is_control) || seen.contains(&entry) {
            continue;
        }
        if seen.len() >= MAX_PATH_ENTRIES || len + entry.len() + 1 > MAX_PATH_LEN {
            break;
        }
        len += entry.len() + 1;
        seen.push(entry);
    }
    seen.join(":")
}

/// The newest `vMAJOR.MINOR.PATCH` among the folder names of `~/.nvm/versions/node`.
pub fn newest_node_version<'a>(names: impl IntoIterator<Item = &'a str>) -> Option<&'a str> {
    names
        .into_iter()
        .filter_map(|n| {
            let nums: Vec<u64> = n.strip_prefix('v')?.split('.').map(|p| p.parse().ok()).collect::<Option<_>>()?;
            let [major, minor, patch] = nums[..] else { return None };
            Some((major, minor, patch, n))
        })
        .max()
        .map(|(_, _, _, n)| n)
}

/// What the machine offers when the login shell gives no `PATH`.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct FallbackInputs {
    /// The user's home folder.
    pub home: PathBuf,
    /// The `bin` folder of the newest Node installed with nvm, if any.
    pub nvm_node_bin: Option<PathBuf>,
    /// The `PATH` the service itself got from the OS.
    pub inherited: String,
}

/// A sane `PATH` without the shell: nvm's Node, cargo, Homebrew (Apple Silicon and Intel), the usual system folders,
/// then whatever the service inherited. Only entries for which `exists` is true are kept, except the system folders
/// every Unix has.
pub fn fallback_path(inputs: &FallbackInputs, exists: impl Fn(&Path) -> bool) -> String {
    let mut dirs: Vec<PathBuf> = Vec::new();
    if let Some(n) = &inputs.nvm_node_bin {
        dirs.push(n.clone());
    }
    // Unix paths (the login environment is macOS and Linux only), joined with `/` whatever the host.
    let home = inputs.home.to_string_lossy();
    let home = home.trim_end_matches('/');
    dirs.push(format!("{home}/.cargo/bin").into());
    dirs.push(format!("{home}/.local/bin").into());
    dirs.push("/opt/homebrew/bin".into());
    dirs.push("/opt/homebrew/sbin".into());
    dirs.push("/usr/local/bin".into());
    dirs.retain(|d| exists(d));
    let mut all: Vec<String> = dirs.iter().map(|d| d.display().to_string()).collect();
    all.extend(["/usr/bin", "/bin", "/usr/sbin", "/sbin"].map(String::from));
    all.extend(inputs.inherited.split(':').map(String::from));
    sanitize_path(&all.join(":"))
}

/// Why a per-runner `env` table was refused.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum EnvError {
    /// Too many variables.
    #[error("env has more than {MAX_ENV_VARS} variables")]
    TooMany,
    /// A name that is not a plain identifier.
    #[error("env name {0:?} must be 1-{MAX_ENV_NAME} letters, digits or underscores, not starting with a digit")]
    BadName(String),
    /// A value that is too long or has a control character or NUL.
    #[error("the value of env {0} must be at most {MAX_ENV_VALUE} characters without control characters")]
    BadValue(String),
    /// A name Pitcrew sets itself or that would let a job hijack the runner.
    #[error("env {0} cannot be overridden")]
    Reserved(String),
}

/// Checks a per-runner environment override (`[runners.env]` in `pitcrew.toml`).
pub fn validate_env(env: &BTreeMap<String, String>) -> Result<(), EnvError> {
    if env.len() > MAX_ENV_VARS {
        return Err(EnvError::TooMany);
    }
    for (k, v) in env {
        let mut chars = k.chars();
        let first_ok = chars.next().is_some_and(|c| c.is_ascii_alphabetic() || c == '_');
        if !first_ok || k.len() > MAX_ENV_NAME || !chars.all(|c| c.is_ascii_alphanumeric() || c == '_') {
            return Err(EnvError::BadName(k.clone()));
        }
        if v.len() > MAX_ENV_VALUE || v.chars().any(char::is_control) {
            return Err(EnvError::BadValue(k.clone()));
        }
        if k.starts_with("PITCREW_") || k.starts_with("GITEA_RUNNER_") {
            return Err(EnvError::Reserved(k.clone()));
        }
    }
    Ok(())
}

/// The variables to start a runner with: the base ones (`PATH` from the login shell) overridden by the runner's own
/// `env`. Later entries win.
pub fn runner_environment(base_path: Option<&str>, overrides: &BTreeMap<String, String>) -> Vec<(String, String)> {
    let mut out: BTreeMap<String, String> = BTreeMap::new();
    if let Some(p) = base_path.filter(|p| !p.is_empty()) {
        out.insert("PATH".into(), p.to_string());
    }
    out.extend(overrides.iter().map(|(k, v)| (k.clone(), v.clone())));
    out.into_iter().collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn map(pairs: &[(&str, &str)]) -> BTreeMap<String, String> {
        pairs.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect()
    }

    #[test]
    fn the_path_is_read_between_the_markers_ignoring_noise() {
        let out = format!("Welcome to zsh\nnvm loaded\n{PATH_BEGIN}/opt/homebrew/bin:/usr/bin:/opt/homebrew/bin:rel:/bin{PATH_END}bye");
        assert_eq!(parse_probe_output(&out).unwrap(), "/opt/homebrew/bin:/usr/bin:/bin");
        assert_eq!(parse_probe_output("no markers"), None);
        assert_eq!(parse_probe_output(&format!("{PATH_BEGIN}/usr/bin")), None);
        assert_eq!(parse_probe_output(&format!("{PATH_BEGIN}{PATH_END}")), None);
        assert_eq!(parse_probe_output(&format!("{PATH_BEGIN}relative:also{PATH_END}")), None);
        // The last pair wins: an rc file that echoes the markers cannot hide the real answer.
        let twice = format!("{PATH_BEGIN}/a{PATH_END}{PATH_BEGIN}/b{PATH_END}");
        assert_eq!(parse_probe_output(&twice).unwrap(), "/b");
    }

    #[test]
    fn sanitizing_drops_bad_entries_and_bounds_the_size() {
        assert_eq!(sanitize_path("/a::/b:/a:rel:/c\n:/d"), "/a:/b:/d");
        let many = (0..500).map(|i| format!("/d{i}")).collect::<Vec<_>>().join(":");
        assert_eq!(sanitize_path(&many).split(':').count(), MAX_PATH_ENTRIES);
        let long = format!("/{}:/b", "x".repeat(MAX_PATH_LEN));
        assert_eq!(sanitize_path(&long), "");
    }

    #[test]
    fn the_newest_nvm_node_is_chosen_numerically() {
        let names = ["v9.0.0", "v22.1.0", "v22.10.1", "v22.9.9", "system", "v1.2", "v1.2.3.4"];
        assert_eq!(newest_node_version(names), Some("v22.10.1"));
        assert_eq!(newest_node_version(["x"]), None);
        assert_eq!(newest_node_version([]), None);
    }

    #[test]
    fn the_fallback_has_homebrew_cargo_nvm_and_the_system_folders() {
        let inputs = FallbackInputs {
            home: "/Users/ci".into(),
            nvm_node_bin: Some("/Users/ci/.nvm/versions/node/v22.1.0/bin".into()),
            inherited: "/usr/bin:/custom/bin".into(),
        };
        let have = |p: &Path| {
            [
                "/opt/homebrew/bin",
                "/Users/ci/.cargo/bin",
                "/Users/ci/.nvm/versions/node/v22.1.0/bin",
            ]
            .iter()
            .any(|d| Path::new(d) == p)
        };
        assert_eq!(
            fallback_path(&inputs, have),
            "/Users/ci/.nvm/versions/node/v22.1.0/bin:/Users/ci/.cargo/bin:/opt/homebrew/bin:/usr/bin:/bin:/usr/sbin:/sbin:/custom/bin"
        );
        // Nothing installed: just the system folders and the inherited ones.
        assert_eq!(
            fallback_path(&FallbackInputs::default(), |_| false),
            "/usr/bin:/bin:/usr/sbin:/sbin"
        );
    }

    #[test]
    fn env_overrides_are_validated() {
        assert_eq!(validate_env(&map(&[("PATH", "/x"), ("NODE_ENV", "ci"), ("_a1", "")])), Ok(()));
        assert_eq!(validate_env(&map(&[("1A", "x")])), Err(EnvError::BadName("1A".into())));
        assert_eq!(validate_env(&map(&[("A=B", "x")])), Err(EnvError::BadName("A=B".into())));
        assert_eq!(validate_env(&map(&[("", "x")])), Err(EnvError::BadName(String::new())));
        assert_eq!(validate_env(&map(&[("A", "x\ny")])), Err(EnvError::BadValue("A".into())));
        assert_eq!(validate_env(&map(&[("A", "x\0")])), Err(EnvError::BadValue("A".into())));
        assert!(matches!(
            validate_env(&map(&[("A", &"v".repeat(MAX_ENV_VALUE + 1))])),
            Err(EnvError::BadValue(_))
        ));
        assert_eq!(
            validate_env(&map(&[("PITCREW_X", "1")])),
            Err(EnvError::Reserved("PITCREW_X".into()))
        );
        assert!(matches!(
            validate_env(&map(&[("GITEA_RUNNER_TOKEN", "1")])),
            Err(EnvError::Reserved(_))
        ));
        let too_many: BTreeMap<String, String> = (0..=MAX_ENV_VARS).map(|i| (format!("V{i}"), String::new())).collect();
        assert_eq!(validate_env(&too_many), Err(EnvError::TooMany));
        assert!(validate_env(&map(&[("PATH", "x")])).is_ok());
        assert!(EnvError::TooMany.to_string().contains("32"));
    }

    #[test]
    fn overrides_win_over_the_login_path() {
        assert_eq!(
            runner_environment(Some("/a"), &BTreeMap::new()),
            vec![("PATH".to_string(), "/a".to_string())]
        );
        assert_eq!(
            runner_environment(Some("/a"), &map(&[("PATH", "/b"), ("X", "1")])),
            vec![("PATH".into(), "/b".into()), ("X".into(), "1".into())]
        );
        assert!(runner_environment(None, &BTreeMap::new()).is_empty());
        assert!(runner_environment(Some(""), &BTreeMap::new()).is_empty());
        assert!(probe_script().contains(PATH_BEGIN));
    }
}
