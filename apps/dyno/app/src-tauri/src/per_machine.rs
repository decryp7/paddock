//! Dyno's side of the per-machine Windows install (family ADR 0053 section 4): the updater's hand-off to
//! `$INSTDIR\paddock-setup.exe apply-update`. The contract, the installer lock and the install folder are
//! `paddock_setup::wiring`, one implementation for every app that adopts the install (rule 55); only the administrator check,
//! which needs the Tauri adapter of `paddock-updater`, is here.

#[cfg(windows)]
pub use paddock_setup::wiring::{install_dir, SetupLock};
pub use paddock_setup::wiring::{install_dir_ok, reparse_above, SetupFacts};

/// Whether this account is an administrator (also with a filtered token).
#[cfg(windows)]
pub fn administrator() -> bool {
    paddock_updater::tauri::is_administrator()
}

#[cfg(test)]
mod tests {
    fn macro_body<'a>(hooks: &'a str, name: &str) -> &'a str {
        hooks
            .split(&format!("!macro {name}"))
            .nth(1)
            .and_then(|m| m.split("!macroend").next())
            .unwrap_or_else(|| panic!("no macro {name}"))
    }

    /// Review of B6b (finding A): the uninstaller runs elevated from wherever it was started, so it runs no program of `$INSTDIR`
    /// (the agent's stop, the helper's uninstall) unless the include trusts the folder and `check-dir` accepts it.
    #[test]
    fn the_uninstaller_runs_no_program_of_its_folder_unless_the_folder_is_accepted() {
        let hooks = include_str!("../windows/installer-hooks.nsh");
        let pre = macro_body(hooks, "NSIS_HOOK_PREUNINSTALL");
        let asked = pre.find("PADDOCK_DIR_ACCEPTED").expect("PREUNINSTALL asks the include");
        let stop = pre.find("_DYNO_GRACEFUL_STOP").expect("the graceful stop is called");
        assert!(asked < stop, "the question comes before the first program is run: {pre}");
        let helper = pre.find("dyno-helper.exe").expect("the helper's uninstall");
        assert!(
            asked < helper && pre[..helper].rfind("$DynoDirOk == 1").is_some(),
            "the helper's uninstall is guarded: {pre}"
        );
        let graceful = macro_body(hooks, "_DYNO_GRACEFUL_STOP");
        let (un, installer) = graceful.split_once("!else").expect("an uninstaller branch and an installer branch");
        let at_guard = un.find("$DynoDirOk == 1").expect("guarded in the uninstaller");
        assert!(at_guard < un.find("dyno-agent.exe").unwrap(), "{un}");
        assert!(
            !installer.contains("DynoDirOk"),
            "the installer's PREINSTALL has passed PADDOCK_CHECK_DIR: {installer}"
        );
        assert!(!pre.contains("dyno-agent.exe"), "no direct run of the agent in PREUNINSTALL: {pre}");
    }
}
