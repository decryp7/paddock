//! `guard`: the per-user copy guard (ADR 0053 section 5), a pure decision over an [`InstallFacts`] port, and the probe the
//! per-machine app runs for its own account at start (section 6.2, [`per_user_copy`]).
//!
//! - Only on a **fresh install**: when `HKLM\...\Uninstall\<product>` exists the guard never runs.
//! - Blocks only for two accounts, the installer's and the **desktop user** (the owner of the installer session's shell
//!   process), and only on the uninstall key in that account's hive. A file-only hit and any other account's hit are logged
//!   and do not block.
//! - The scan for the log walks every `HKU` subkey except `_Classes` hives and the well-known SIDs, reading each user's
//!   Local AppData from that hive's shell-folder settings, never from the installer's own environment.
//! - When the blocking hit is the desktop user's own key and the installer had `/UPDATE` and `/R`, the per-user copy is
//!   started again (unelevated, as the desktop user): [`Decision::Blocked::restart`] names the program.

use crate::log::Sink;

/// Tauri's per-user install puts the program in `<Local AppData>\<product>\<main>`.
pub const HIVE_SKIP_SUFFIX: &str = "_Classes";
/// Accounts whose hives are never scanned.
pub const WELL_KNOWN: [&str; 4] = [".DEFAULT", "S-1-5-18", "S-1-5-19", "S-1-5-20"];

/// An uninstall key found in a user hive.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UninstallEntry {
    /// The `UninstallString` value, when present.
    pub uninstall_string: Option<String>,
}

/// What the guard can look at (the registry, tokens and files in the program).
pub trait InstallFacts {
    /// `HKLM\Software\Microsoft\Windows\CurrentVersion\Uninstall\<product>` exists.
    fn machine_key_exists(&self) -> bool;
    /// SID of the account running the installer.
    fn installer_account(&self) -> Option<String>;
    /// SID of the desktop user (owner of the shell process of the installer's session).
    fn desktop_user(&self) -> Option<String>;
    /// The names of the subkeys of `HKU`.
    fn hku_subkeys(&self) -> Vec<String>;
    /// The uninstall key under `HKU\<sid>`.
    fn uninstall_key(&self, sid: &str) -> Option<UninstallEntry>;
    /// The account's Local AppData folder, from its hive (see [`expand_local_app_data`]).
    fn local_app_data(&self, sid: &str) -> Option<String>;
    /// Whether the file exists.
    fn file_exists(&self, path: &str) -> bool;
    /// The account's display name, for the message only (never logged).
    fn account_name(&self, sid: &str) -> Option<String>;
}

/// The result of the guard.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Decision {
    /// An HKLM key exists: an update or reinstall, the guard does not run.
    NotFresh,
    /// Nothing blocks (other hits, if any, were logged).
    Proceed,
    /// A per-user copy blocks the install.
    Blocked {
        /// The message for the installer.
        message: String,
        /// The per-user program to start again as the desktop user (`/UPDATE /R`, the desktop user's own hit, file present).
        restart: Option<String>,
    },
}

/// The hives to scan: every name except `_Classes` hives and the well-known SIDs, in order, without duplicates.
pub fn scan_accounts(names: &[String]) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for name in names {
        let lower = name.to_ascii_lowercase();
        if name.is_empty()
            || lower.ends_with(&HIVE_SKIP_SUFFIX.to_ascii_lowercase())
            || WELL_KNOWN.iter().any(|w| w.eq_ignore_ascii_case(name))
            || out.iter().any(|o| o.eq_ignore_ascii_case(name))
        {
            continue;
        }
        out.push(name.clone());
    }
    out
}

/// Expands a `User Shell Folders\Local AppData` value with the account's profile folder (`ProfileImagePath`). `None` when
/// it still has an unexpanded variable afterwards.
pub fn expand_local_app_data(raw: &str, profile: &str) -> Option<String> {
    let raw = raw.trim();
    if raw.is_empty() {
        return None;
    }
    let token = "%USERPROFILE%";
    let expanded = match raw.to_ascii_lowercase().find(&token.to_ascii_lowercase()) {
        Some(at) => format!("{}{}{}", &raw[..at], profile.trim_end_matches('\\'), &raw[at + token.len()..]),
        None => raw.to_string(),
    };
    (!expanded.contains('%')).then_some(expanded)
}

/// The folder a per-user uninstall string points into (`"C:\...\Dyno\uninstall.exe"` gives `C:\...\Dyno`).
pub fn install_dir_of(uninstall_string: &str) -> Option<String> {
    let program = crate::runvalues::program_of(uninstall_string)?;
    let (parent, _) = program.rsplit_once('\\')?;
    (!parent.is_empty()).then(|| parent.to_string())
}

fn yes(b: bool) -> &'static str {
    if b { "yes" } else { "no" }
}

/// One account of an [`InstallFacts`] as an [`AccountProbe`].
struct SidProbe<'a> {
    facts: &'a dyn InstallFacts,
    sid: &'a str,
}

impl AccountProbe for SidProbe<'_> {
    fn uninstall_key(&self) -> Option<UninstallEntry> {
        self.facts.uninstall_key(self.sid)
    }
    fn local_app_data(&self) -> Option<String> {
        self.facts.local_app_data(self.sid)
    }
    fn file_exists(&self, path: &str) -> bool {
        self.facts.file_exists(path)
    }
}

/// Runs the guard.
pub fn decide(
    facts: &dyn InstallFacts,
    product: &str,
    main: &str,
    update: bool,
    restart: bool,
    log: &mut dyn Sink,
) -> Decision {
    if facts.machine_key_exists() {
        log.line("guard", "not a fresh install (the per-machine uninstall key exists); the per-user guard does not run");
        return Decision::NotFresh;
    }
    let installer = facts.installer_account();
    let desktop = facts.desktop_user();
    let mut sids = scan_accounts(&facts.hku_subkeys());
    for own in [&installer, &desktop].into_iter().flatten() {
        if !sids.iter().any(|s| s.eq_ignore_ascii_case(own)) {
            sids.push(own.clone());
        }
    }
    log.line("guard", &format!("scanning {} account hive(s)", sids.len()));
    let is = |who: &Option<String>, sid: &str| who.as_deref().is_some_and(|w| w.eq_ignore_ascii_case(sid));
    let (mut installer_hit, mut desktop_hit): (Option<PerUserCopy>, Option<PerUserCopy>) = (None, None);
    for sid in &sids {
        let probe = SidProbe { facts, sid };
        let found = per_user_copy(&probe, product, main);
        let (i, d) = (is(&installer, sid), is(&desktop, sid));
        if let PerUserCopy::Present { key, file, .. } = &found {
            let role = match (i, d) {
                (true, true) => "installer+desktop",
                (true, false) => "installer",
                (false, true) => "desktop",
                (false, false) => "other",
            };
            let blocks = (i || d) && *key;
            log.line(
                "guard",
                &format!("hit role={role} key={} file={}{}", yes(*key), yes(*file), if blocks { " (blocks)" } else { " (does not block)" }),
            );
        }
        if i && matches!(found, PerUserCopy::Present { key: true, .. }) {
            installer_hit = Some(found.clone());
        }
        if d && matches!(found, PerUserCopy::Present { key: true, .. }) {
            desktop_hit = Some(found);
        }
    }
    if installer_hit.is_none() && desktop_hit.is_none() {
        log.line("guard", "no per-user copy blocks the install");
        return Decision::Proceed;
    }
    let mut message = format!("Uninstall the older {product} first (Settings \u{203a} Apps), then run this installer again.");
    if desktop_hit.is_some()
        && installer_hit.is_none()
        && let Some(name) = desktop.as_deref().and_then(|d| facts.account_name(d))
    {
        message.push_str(&format!(" It is installed for {name}."));
    }
    let asked = update && restart;
    let restart_target = match (&desktop_hit, asked) {
        (Some(PerUserCopy::Present { dir: Some(dir), .. }), true) => {
            let program = format!(r"{dir}\{main}");
            facts.file_exists(&program).then_some(program)
        }
        _ => None,
    };
    if asked && restart_target.is_none() {
        message.push_str(&format!(" Then start {product} again."));
    }
    log.line("guard", &format!("blocked; restart of the older copy: {}", yes(restart_target.is_some())));
    Decision::Blocked { message, restart: restart_target }
}

/// One account, for the app's own start-up probe.
pub trait AccountProbe {
    /// The uninstall key in this account's `HKCU`.
    fn uninstall_key(&self) -> Option<UninstallEntry>;
    /// This account's Local AppData.
    fn local_app_data(&self) -> Option<String>;
    /// Whether the file exists.
    fn file_exists(&self, path: &str) -> bool;
}

/// A leftover per-user copy in one account.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PerUserCopy {
    /// Neither the key nor the file.
    None,
    /// The key and/or the program file is there.
    Present {
        /// The uninstall key exists.
        key: bool,
        /// `<Local AppData>\<product>\<main>` exists.
        file: bool,
        /// The folder of the old copy, when known (from the file's place or the uninstall string).
        dir: Option<String>,
    },
}

/// The probe of section 6.2 (the same one the guard uses per account).
pub fn per_user_copy(probe: &dyn AccountProbe, product: &str, main: &str) -> PerUserCopy {
    let entry = probe.uninstall_key();
    let file_dir = probe.local_app_data().map(|l| format!(r"{}\{product}", l.trim_end_matches('\\')));
    let file = file_dir.as_ref().is_some_and(|d| probe.file_exists(&format!(r"{d}\{main}")));
    let key = entry.is_some();
    if !key && !file {
        return PerUserCopy::None;
    }
    let dir = entry.and_then(|e| e.uninstall_string).and_then(|s| install_dir_of(&s)).or(file.then_some(file_dir).flatten());
    PerUserCopy::Present { key, file, dir }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::{BTreeMap, BTreeSet};

    const INSTALLER: &str = "S-1-5-21-1-1-1-1001";
    const DESKTOP: &str = "S-1-5-21-1-1-1-1002";
    const OTHER: &str = "S-1-5-21-1-1-1-1003";

    #[derive(Default)]
    struct Fake {
        machine: bool,
        installer: Option<String>,
        desktop: Option<String>,
        hives: Vec<String>,
        keys: BTreeMap<String, UninstallEntry>,
        lad: BTreeMap<String, String>,
        files: BTreeSet<String>,
    }
    impl InstallFacts for Fake {
        fn machine_key_exists(&self) -> bool {
            self.machine
        }
        fn installer_account(&self) -> Option<String> {
            self.installer.clone()
        }
        fn desktop_user(&self) -> Option<String> {
            self.desktop.clone()
        }
        fn hku_subkeys(&self) -> Vec<String> {
            self.hives.clone()
        }
        fn uninstall_key(&self, sid: &str) -> Option<UninstallEntry> {
            self.keys.get(sid).cloned()
        }
        fn local_app_data(&self, sid: &str) -> Option<String> {
            self.lad.get(sid).cloned()
        }
        fn file_exists(&self, path: &str) -> bool {
            self.files.contains(path)
        }
        fn account_name(&self, sid: &str) -> Option<String> {
            Some(format!("name-of-{}", sid.rsplit('-').next().unwrap()))
        }
    }
    #[derive(Default)]
    struct Lines(Vec<String>);
    impl Sink for Lines {
        fn line(&mut self, phase: &str, message: &str) {
            self.0.push(format!("{phase}: {message}"));
        }
    }

    fn entry(dir: &str) -> UninstallEntry {
        UninstallEntry { uninstall_string: Some(format!("\"{dir}\\uninstall.exe\"")) }
    }
    fn base() -> Fake {
        let mut f = Fake {
            installer: Some(INSTALLER.into()),
            desktop: Some(DESKTOP.into()),
            hives: vec![".DEFAULT".into(), INSTALLER.into(), format!("{INSTALLER}_Classes"), DESKTOP.into(), OTHER.into(), "S-1-5-18".into()],
            ..Fake::default()
        };
        for (sid, user) in [(INSTALLER, "inst"), (DESKTOP, "desk"), (OTHER, "oth")] {
            f.lad.insert(sid.into(), format!(r"C:\Users\{user}\AppData\Local"));
        }
        f
    }
    fn run(f: &Fake, update: bool, restart: bool) -> (Decision, Vec<String>) {
        let mut log = Lines::default();
        let d = decide(f, "Dyno", "dyno.exe", update, restart, &mut log);
        (d, log.0)
    }

    #[test]
    fn scan_skips_classes_and_well_known() {
        let names: Vec<String> = [".DEFAULT", "S-1-5-18", "S-1-5-19", "S-1-5-20", "S-1-5-21-1-2-3-1001", "S-1-5-21-1-2-3-1001_Classes", "s-1-5-21-1-2-3-1002_CLASSES", "S-1-5-21-1-2-3-1001", ".default"]
            .into_iter()
            .map(String::from)
            .collect();
        assert_eq!(scan_accounts(&names), vec!["S-1-5-21-1-2-3-1001"]);
    }

    #[test]
    fn local_app_data_is_expanded_from_the_profile() {
        assert_eq!(expand_local_app_data(r"%USERPROFILE%\AppData\Local", r"C:\Users\x").as_deref(), Some(r"C:\Users\x\AppData\Local"));
        assert_eq!(expand_local_app_data(r"%userprofile%\AppData\Local", r"C:\Users\x\").as_deref(), Some(r"C:\Users\x\AppData\Local"));
        assert_eq!(expand_local_app_data(r"D:\Data\Local", r"C:\Users\x").as_deref(), Some(r"D:\Data\Local"));
        assert_eq!(expand_local_app_data(r"%SomethingElse%\Local", r"C:\Users\x"), None);
        assert_eq!(expand_local_app_data("", r"C:\Users\x"), None);
    }

    #[test]
    fn install_dir_from_uninstall_string() {
        assert_eq!(install_dir_of(r#""C:\Users\x\AppData\Local\Dyno\uninstall.exe""#).as_deref(), Some(r"C:\Users\x\AppData\Local\Dyno"));
        assert_eq!(install_dir_of(r#""C:\Users\x\AppData\Local\Dyno\uninstall.exe" /S"#).as_deref(), Some(r"C:\Users\x\AppData\Local\Dyno"));
        assert_eq!(install_dir_of(r"C:\A\B\uninstall.exe").as_deref(), Some(r"C:\A\B"));
        assert_eq!(install_dir_of("uninstall.exe"), None);
        assert_eq!(install_dir_of(""), None);
    }

    #[test]
    fn a_per_machine_key_means_not_fresh() {
        let mut f = base();
        f.machine = true;
        f.keys.insert(INSTALLER.into(), entry(r"C:\Users\inst\AppData\Local\Dyno"));
        let (d, log) = run(&f, false, false);
        assert_eq!(d, Decision::NotFresh);
        assert!(log.iter().any(|l| l.contains("not a fresh install")));
    }

    #[test]
    fn nothing_found_proceeds() {
        let (d, _) = run(&base(), false, false);
        assert_eq!(d, Decision::Proceed);
    }

    #[test]
    fn the_installer_accounts_key_blocks() {
        let mut f = base();
        f.keys.insert(INSTALLER.into(), entry(r"C:\Users\inst\AppData\Local\Dyno"));
        let (d, _) = run(&f, false, false);
        let Decision::Blocked { message, restart } = d else { panic!("{d:?}") };
        assert_eq!(message, "Uninstall the older Dyno first (Settings \u{203a} Apps), then run this installer again.");
        assert_eq!(restart, None);
    }

    #[test]
    fn the_desktop_users_key_blocks_and_names_the_account() {
        let mut f = base();
        f.keys.insert(DESKTOP.into(), entry(r"C:\Users\desk\AppData\Local\Dyno"));
        let (d, _) = run(&f, false, false);
        let Decision::Blocked { message, .. } = d else { panic!("{d:?}") };
        assert_eq!(message, "Uninstall the older Dyno first (Settings \u{203a} Apps), then run this installer again. It is installed for name-of-1002.");
    }

    #[test]
    fn both_keys_do_not_add_the_account_sentence() {
        let mut f = base();
        f.keys.insert(INSTALLER.into(), entry(r"C:\Users\inst\AppData\Local\Dyno"));
        f.keys.insert(DESKTOP.into(), entry(r"C:\Users\desk\AppData\Local\Dyno"));
        let Decision::Blocked { message, .. } = run(&f, false, false).0 else { panic!() };
        assert!(!message.contains("It is installed for"));
    }

    #[test]
    fn same_account_for_installer_and_desktop() {
        let mut f = base();
        f.desktop = Some(INSTALLER.into());
        f.keys.insert(INSTALLER.into(), entry(r"C:\Users\inst\AppData\Local\Dyno"));
        let Decision::Blocked { message, .. } = run(&f, false, false).0 else { panic!() };
        assert!(!message.contains("It is installed for"));
    }

    #[test]
    fn another_accounts_key_is_logged_and_does_not_block() {
        let mut f = base();
        f.keys.insert(OTHER.into(), entry(r"C:\Users\oth\AppData\Local\Dyno"));
        let (d, log) = run(&f, false, false);
        assert_eq!(d, Decision::Proceed);
        assert!(log.iter().any(|l| l.contains("role=other") && l.contains("key=yes") && l.contains("does not block")), "{log:?}");
    }

    #[test]
    fn a_file_only_hit_does_not_block() {
        let mut f = base();
        f.files.insert(r"C:\Users\inst\AppData\Local\Dyno\dyno.exe".into());
        f.files.insert(r"C:\Users\desk\AppData\Local\Dyno\dyno.exe".into());
        let (d, log) = run(&f, false, false);
        assert_eq!(d, Decision::Proceed);
        assert!(log.iter().any(|l| l.contains("role=installer") && l.contains("key=no") && l.contains("file=yes")), "{log:?}");
    }

    #[test]
    fn the_log_carries_no_account_names_or_paths() {
        let mut f = base();
        f.keys.insert(DESKTOP.into(), entry(r"C:\Users\desk\AppData\Local\Dyno"));
        f.files.insert(r"C:\Users\desk\AppData\Local\Dyno\dyno.exe".into());
        let (_, log) = run(&f, true, true);
        for l in &log {
            assert!(!l.contains("Users") && !l.contains("name-of") && !l.contains("S-1-5-21"), "{l}");
        }
    }

    #[test]
    fn classes_hives_and_well_known_sids_are_not_scanned() {
        let mut f = base();
        f.hives = vec![format!("{OTHER}_Classes"), "S-1-5-18".into(), ".DEFAULT".into()];
        f.keys.insert(format!("{OTHER}_Classes"), entry(r"C:\x\Dyno"));
        f.keys.insert("S-1-5-18".into(), entry(r"C:\x\Dyno"));
        let (d, log) = run(&f, false, false);
        assert_eq!(d, Decision::Proceed);
        assert!(!log.iter().any(|l| l.contains("key=yes")));
    }

    #[test]
    fn the_installer_account_is_checked_even_when_its_hive_is_not_listed() {
        let mut f = base();
        f.hives = vec![];
        f.keys.insert(INSTALLER.into(), entry(r"C:\Users\inst\AppData\Local\Dyno"));
        assert!(matches!(run(&f, false, false).0, Decision::Blocked { .. }));
    }

    #[test]
    fn restart_only_for_the_desktop_users_own_key_with_update_and_restart() {
        let mut f = base();
        let dir = r"C:\Users\desk\AppData\Local\Dyno";
        f.keys.insert(DESKTOP.into(), entry(dir));
        f.files.insert(format!(r"{dir}\dyno.exe"));
        let Decision::Blocked { message, restart } = run(&f, true, true).0 else { panic!() };
        assert_eq!(restart.as_deref(), Some(format!(r"{dir}\dyno.exe").as_str()));
        assert!(!message.ends_with("Then start Dyno again."));
        // /UPDATE without /R, or /R without /UPDATE: nothing started.
        assert_eq!(match run(&f, true, false).0 { Decision::Blocked { restart, .. } => restart, d => panic!("{d:?}") }, None);
        assert_eq!(match run(&f, false, true).0 { Decision::Blocked { restart, .. } => restart, d => panic!("{d:?}") }, None);
    }

    #[test]
    fn restart_needs_the_program_file() {
        let mut f = base();
        f.keys.insert(DESKTOP.into(), entry(r"C:\Users\desk\AppData\Local\Dyno"));
        let Decision::Blocked { message, restart } = run(&f, true, true).0 else { panic!() };
        assert_eq!(restart, None);
        assert!(message.ends_with("Then start Dyno again."), "{message}");
    }

    #[test]
    fn no_restart_when_only_the_installer_account_has_the_key() {
        let mut f = base();
        let dir = r"C:\Users\inst\AppData\Local\Dyno";
        f.keys.insert(INSTALLER.into(), entry(dir));
        f.files.insert(format!(r"{dir}\dyno.exe"));
        let Decision::Blocked { message, restart } = run(&f, true, true).0 else { panic!() };
        assert_eq!(restart, None);
        assert!(message.ends_with("Then start Dyno again."));
    }

    #[test]
    fn restart_when_both_have_keys_uses_the_desktop_users() {
        let mut f = base();
        let d1 = r"C:\Users\inst\AppData\Local\Dyno";
        let d2 = r"C:\Users\desk\AppData\Local\Dyno";
        f.keys.insert(INSTALLER.into(), entry(d1));
        f.keys.insert(DESKTOP.into(), entry(d2));
        f.files.insert(format!(r"{d1}\dyno.exe"));
        f.files.insert(format!(r"{d2}\dyno.exe"));
        let Decision::Blocked { restart, .. } = run(&f, true, true).0 else { panic!() };
        assert_eq!(restart.as_deref(), Some(format!(r"{d2}\dyno.exe").as_str()));
    }

    struct Probe {
        key: Option<UninstallEntry>,
        lad: Option<String>,
        files: Vec<String>,
    }
    impl AccountProbe for Probe {
        fn uninstall_key(&self) -> Option<UninstallEntry> {
            self.key.clone()
        }
        fn local_app_data(&self) -> Option<String> {
            self.lad.clone()
        }
        fn file_exists(&self, path: &str) -> bool {
            self.files.iter().any(|f| f == path)
        }
    }

    #[test]
    fn per_user_copy_cases() {
        let none = Probe { key: None, lad: Some(r"C:\L".into()), files: vec![] };
        assert_eq!(per_user_copy(&none, "Dyno", "dyno.exe"), PerUserCopy::None);
        let both = Probe { key: Some(entry(r"C:\L\Dyno")), lad: Some(r"C:\L".into()), files: vec![r"C:\L\Dyno\dyno.exe".into()] };
        assert_eq!(per_user_copy(&both, "Dyno", "dyno.exe"), PerUserCopy::Present { key: true, file: true, dir: Some(r"C:\L\Dyno".into()) });
        let file_only = Probe { key: None, lad: Some(r"C:\L".into()), files: vec![r"C:\L\Dyno\dyno.exe".into()] };
        assert_eq!(per_user_copy(&file_only, "Dyno", "dyno.exe"), PerUserCopy::Present { key: false, file: true, dir: Some(r"C:\L\Dyno".into()) });
        let key_only = Probe { key: Some(entry(r"C:\Other\Dyno")), lad: None, files: vec![] };
        assert_eq!(per_user_copy(&key_only, "Dyno", "dyno.exe"), PerUserCopy::Present { key: true, file: false, dir: Some(r"C:\Other\Dyno".into()) });
    }
}
