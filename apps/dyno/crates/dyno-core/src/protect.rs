//! The protected list: processes Dyno never acts on, neither by a rule nor by a click (ADR 0004).
//!
//! Built in: the OS's critical processes (ending or suspending them crashes or hangs the machine), the antivirus
//! (tamper protection fights back, ADR 0004), the audio engine (throttling it makes sound stutter), and Dyno itself.
//! The user adds executable names or full paths; matching is case-insensitive on the file name or the whole path.

use crate::grouping::file_name;
use crate::process::{Pid, ProcessSample};

/// Critical Windows processes (lower-case file names).
pub const WINDOWS_PROTECTED: &[&str] = &[
    "system",
    "registry",
    "secure system",
    "memory compression",
    "smss.exe",
    "csrss.exe",
    "wininit.exe",
    "winlogon.exe",
    "services.exe",
    "lsass.exe",
    "lsaiso.exe",
    "svchost.exe",
    "dwm.exe",
    "fontdrvhost.exe",
    "audiodg.exe",
    "msmpeng.exe",
    "mpdefendercoreservice.exe",
    "nissrv.exe",
    "securityhealthservice.exe",
    "sgrmbroker.exe",
    "wudfhost.exe",
    "spoolsv.exe",
];

/// Critical macOS processes.
pub const MACOS_PROTECTED: &[&str] = &[
    "kernel_task",
    "launchd",
    "windowserver",
    "loginwindow",
    "coreaudiod",
    "logd",
    "securityd",
    "opendirectoryd",
    "syspolicyd",
    "xprotect",
    "trustd",
    "configd",
    "powerd",
    "endpointsecurityd",
];

/// Dyno's own executables (the app, its agent, the helper and plugins are protected too).
pub const DYNO_PROTECTED: &[&str] = &[
    "dyno.exe",
    "dyno",
    "dyno-app.exe",
    "dyno-app",
    "dyno-agent.exe",
    "dyno-agent",
    "dyno-helper.exe",
    "dyno-helper",
];

/// Every built-in protected name (both OSes; names don't collide).
pub fn built_in() -> Vec<String> {
    WINDOWS_PROTECTED
        .iter()
        .chain(MACOS_PROTECTED)
        .chain(DYNO_PROTECTED)
        .map(|s| (*s).to_string())
        .collect()
}

/// Why a process is protected, or `None` when Dyno may act on it.
pub struct Protection<'a> {
    user: &'a [String],
    own_pids: &'a [Pid],
}

impl<'a> Protection<'a> {
    /// The built-in list plus the user's entries; `own_pids` are Dyno's own processes.
    pub fn new(user: &'a [String], own_pids: &'a [Pid]) -> Self {
        Protection { user, own_pids }
    }

    /// Why `p` is protected (a sentence for the UI), or `None`.
    pub fn reason(&self, p: &ProcessSample) -> Option<String> {
        if p.pid <= 4 {
            return Some("This is part of the operating system kernel.".into());
        }
        if self.own_pids.contains(&p.pid) {
            return Some("This is Dyno itself.".into());
        }
        let name = p.name.to_lowercase();
        let exe_name = p.exe.as_deref().map(|e| file_name(e).to_lowercase());
        let is = |list: &[&str]| list.iter().any(|n| *n == name || exe_name.as_deref() == Some(*n));
        if is(WINDOWS_PROTECTED) || is(MACOS_PROTECTED) {
            return Some(format!(
                "{} is critical to the operating system or its security; Dyno never touches it.",
                p.name
            ));
        }
        if is(DYNO_PROTECTED) {
            return Some("This is part of Dyno.".into());
        }
        let exe = p.exe.as_deref().map(|e| e.replace('\\', "/").to_lowercase());
        let user_hit = self.user.iter().any(|entry| {
            let e = entry.trim().replace('\\', "/").to_lowercase();
            !e.is_empty() && (e == name || exe_name.as_deref() == Some(e.as_str()) || exe.as_deref() == Some(e.as_str()))
        });
        user_hit.then(|| format!("You protected {}.", p.name))
    }

    /// Whether Dyno may act on `p`.
    pub fn allows(&self, p: &ProcessSample) -> bool {
        self.reason(p).is_none()
    }
}

/// Normalises a user's protected-list entry, refusing empty or overlong ones (at most 260 characters).
pub fn parse_entry(entry: &str) -> Result<String, String> {
    let e = entry.trim();
    if e.is_empty() {
        Err("Enter an executable name such as game.exe, or a full path.".into())
    } else if e.chars().count() > 260 {
        Err("That entry is too long (at most 260 characters).".into())
    } else {
        Ok(e.to_string())
    }
}

/// The most entries a user's protected list may hold.
pub const MAX_USER_ENTRIES: usize = 500;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::process::fixtures::proc;

    #[test]
    fn system_processes_dyno_and_the_kernel_are_protected() {
        let user: Vec<String> = vec![];
        let p = Protection::new(&user, &[999]);
        assert!(p.reason(&proc(4, None, "System", 0.0, 1)).unwrap().contains("kernel"));
        assert!(p.reason(&proc(500, None, "csrss.exe", 0.0, 1)).unwrap().contains("critical"));
        assert!(p.reason(&proc(501, None, "MsMpEng.exe", 0.0, 1)).is_some());
        assert!(p.reason(&proc(502, None, "WindowServer", 0.0, 1)).is_some());
        assert_eq!(p.reason(&proc(999, None, "x", 0.0, 1)).unwrap(), "This is Dyno itself.");
        assert_eq!(
            p.reason(&proc(503, None, "dyno-agent.exe", 0.0, 1)).unwrap(),
            "This is part of Dyno."
        );
        assert!(p.allows(&proc(600, None, "chrome.exe", 0.0, 1)));
        assert!(built_in().contains(&"lsass.exe".to_string()));
    }

    #[test]
    fn user_entries_match_names_and_paths() {
        let user = vec!["Game.exe".to_string(), "c:/tools/build.exe".to_string(), "  ".to_string()];
        let p = Protection::new(&user, &[]);
        let mut game = proc(700, None, "game.exe", 0.0, 1);
        game.exe = Some("D:\\Games\\GAME.EXE".into());
        assert_eq!(p.reason(&game).unwrap(), "You protected game.exe.");
        let mut build = proc(701, None, "build.exe", 0.0, 1);
        build.exe = Some("C:\\Tools\\build.exe".into());
        assert!(!p.allows(&build));
        let mut other = proc(702, None, "build.exe", 0.0, 1);
        other.exe = Some("C:\\Other\\build.exe".into());
        assert!(p.allows(&other));
        let mut no_exe = proc(703, None, "game.exe", 0.0, 1);
        no_exe.exe = None;
        assert!(!p.allows(&no_exe));
    }

    #[test]
    fn entries_are_parsed() {
        assert_eq!(parse_entry("  x.exe "), Ok("x.exe".into()));
        assert!(parse_entry(" ").is_err());
        assert!(parse_entry(&"a".repeat(261)).is_err());
        const { assert!(MAX_USER_ENTRIES >= 100) };
    }
}
