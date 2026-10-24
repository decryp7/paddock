//! What a background program does first when the OS started it (R05): launchd and the systemd user manager give it a
//! minimal `PATH`, so tools in Homebrew, nvm or a venv are not found. It asks the user's login shell for the `PATH` a
//! terminal has (`paddock_proc::login_path`: bounded, with a timeout) and merges it in front, once, before its threads
//! read the environment. A program started by hand already has the terminal's `PATH` and is left alone.

use crate::watch::is_supervised;

/// The `PATH` to use: the login shell's in front of the inherited one, when supervised and a shell answered. Pure.
pub fn merged_path(supervised: bool, login: Option<&str>, inherited: &str) -> Option<String> {
    if !supervised {
        return None;
    }
    let login = login.filter(|l| !l.is_empty())?;
    Some(paddock_proc::merge_path(login, inherited, ':'))
}

/// The `PATH` a supervised program should set at start (`None`: keep the inherited one; always on Windows, where the
/// service gets the user's environment block). `var` is the spec's `supervised_env`. The caller sets it once, before it
/// starts threads: `if let Some(p) = supervised_path("PITWALL_SERVICE") { unsafe { std::env::set_var("PATH", p) } }`.
pub fn supervised_path(var: &str) -> Option<String> {
    path_for(
        !cfg!(windows) && is_supervised(var),
        paddock_proc::login_path,
        &std::env::var("PATH").unwrap_or_default(),
    )
}

/// [`supervised_path`] with the login shell injected: the shell is only asked when supervised.
pub fn path_for(
    supervised: bool,
    login: impl FnOnce() -> Option<String>,
    inherited: &str,
) -> Option<String> {
    if !supervised {
        return None;
    }
    merged_path(true, login().as_deref(), inherited)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_login_path_comes_first_only_when_supervised() {
        assert_eq!(
            merged_path(true, Some("/opt/homebrew/bin:/usr/bin"), "/usr/bin:/bin").as_deref(),
            Some("/opt/homebrew/bin:/usr/bin:/bin")
        );
        assert_eq!(
            merged_path(false, Some("/opt/homebrew/bin"), "/usr/bin"),
            None
        );
        assert_eq!(
            merged_path(true, None, "/usr/bin"),
            None,
            "no shell answered"
        );
        assert_eq!(merged_path(true, Some(""), "/usr/bin"), None);
        assert_eq!(
            path_for(false, || panic!("the shell is not asked"), "/usr/bin"),
            None
        );
        assert_eq!(
            path_for(true, || Some("/opt/bin".into()), "/usr/bin").as_deref(),
            Some("/opt/bin:/usr/bin")
        );
        assert_eq!(
            supervised_path("PADDOCK_SERVICE_TEST_SURELY_UNSET_VARIABLE"),
            None,
            "started by hand"
        );
    }
}
