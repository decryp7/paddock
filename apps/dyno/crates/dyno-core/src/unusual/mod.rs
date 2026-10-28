//! New and unusual programs (WP-07). A transparency tool, not an antivirus: it never says "malware". It keeps a baseline
//! of the programs seen on this machine ([`baseline`]) and explains, in plain words, why a program that is new might be
//! worth a look: first seen, unsigned, running from a temporary or download folder, named like a system process,
//! many children or connections, a new startup entry. This module is the pure scoring; flags never trigger an action.

pub mod baseline;

use crate::grouping::file_name;
use crate::os::Os;
use crate::process::SignatureInfo;
use serde::{Deserialize, Serialize};

pub use baseline::{Baseline, Change, Entry, Learning, Observed, Outcome, StartupObs, Status, UnusualItem, UnusualOverview};

/// A program scoring at least this is called unusual (below it, a new program is just new).
pub const UNUSUAL_SCORE: u32 = 40;
/// A single process with at least this many children is "running many programs".
pub const MANY_CHILDREN: u32 = 50;
/// A program with at least this many open network connections is "talking to many hosts".
pub const MANY_CONNECTIONS: u32 = 40;

/// The user's choices.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct UnusualSettings {
    /// Learn the baseline and list new programs (observe-only; on by default, never acts).
    pub enabled: bool,
    /// Days to learn silently after the first look (0 to 60), so a fresh install does not flag everything it already has.
    pub learning_days: u32,
}

impl Default for UnusualSettings {
    fn default() -> Self {
        UnusualSettings {
            enabled: true,
            learning_days: 7,
        }
    }
}

impl UnusualSettings {
    /// The settings with the learning period in range.
    pub fn clamped(mut self) -> UnusualSettings {
        self.learning_days = self.learning_days.min(60);
        self
    }
}

/// Whether the executable is signed, as far as Dyno has checked.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Signing {
    /// Not checked yet (checking is slow and happens for new programs only).
    #[default]
    Unchecked,
    /// Signed; the signer.
    Signed(String),
    /// Not signed.
    Unsigned,
    /// Signed but the signature does not verify.
    Invalid,
    /// The platform could not tell (so nothing is claimed).
    Unknown,
}

impl From<&SignatureInfo> for Signing {
    fn from(s: &SignatureInfo) -> Signing {
        match s {
            SignatureInfo::Signed(who) => Signing::Signed(who.clone()),
            SignatureInfo::Unsigned => Signing::Unsigned,
            SignatureInfo::Invalid(_) => Signing::Invalid,
            SignatureInfo::Unknown(_) => Signing::Unknown,
        }
    }
}

/// Where a program runs from, when that place is unusual for an installed program.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Location {
    /// A temporary folder.
    Temp,
    /// The Downloads folder.
    Downloads,
    /// A folder every user can write to.
    Shared,
    /// A mounted disk image or removable volume.
    Mounted,
    /// The trash.
    Trash,
    /// Its file was deleted after it started.
    Deleted,
}

impl Location {
    fn text(self) -> &'static str {
        match self {
            Location::Temp => "runs from a temporary folder",
            Location::Downloads => "runs from the Downloads folder",
            Location::Shared => "runs from a folder every user can write to",
            Location::Mounted => "runs from a mounted disk or disk image",
            Location::Trash => "runs from the trash",
            Location::Deleted => "runs from a file that was deleted",
        }
    }
}

/// One reason a program is worth a look.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Reason {
    /// Not seen on this machine before.
    FirstSeen,
    /// Not signed.
    Unsigned,
    /// Signed, but the signature does not verify.
    InvalidSignature,
    /// Runs from an unusual place.
    Location(Location),
    /// Has the name of a system program but does not run from the system folders.
    ImitatesSystem(String),
    /// Its name is one character away from a system program's.
    LooksLikeSystem(String),
    /// One process started this many others.
    ManyChildren(u32),
    /// This many open network connections.
    ManyConnections(u32),
    /// A new entry starts it at login.
    NewStartup,
}

impl Reason {
    /// What it adds to the score.
    pub fn weight(&self) -> u32 {
        match self {
            Reason::FirstSeen => 15,
            Reason::Unsigned => 25,
            Reason::InvalidSignature => 50,
            Reason::Location(Location::Temp | Location::Deleted | Location::Downloads) => 40,
            Reason::Location(_) => 30,
            Reason::ImitatesSystem(_) => 70,
            Reason::LooksLikeSystem(_) => 60,
            Reason::ManyChildren(_) | Reason::ManyConnections(_) => 20,
            Reason::NewStartup => 30,
        }
    }

    /// Plain wording; never a verdict. Starts lower-case so it follows "unusual because it".
    pub fn text(&self) -> String {
        match self {
            Reason::FirstSeen => "was not seen on this computer before".into(),
            Reason::Unsigned => "is not signed by any publisher".into(),
            Reason::InvalidSignature => "has a signature that does not check out".into(),
            Reason::Location(l) => l.text().into(),
            Reason::ImitatesSystem(n) => format!("is named like the system program {n} but does not run from the system folders"),
            Reason::LooksLikeSystem(n) => format!("has a name one character away from the system program {n}"),
            Reason::ManyChildren(n) => format!("started {n} other programs"),
            Reason::ManyConnections(n) => format!("has {n} open network connections"),
            Reason::NewStartup => "was added to what starts at login".into(),
        }
    }
}

/// How noteworthy a program is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Level {
    /// New, with nothing else odd about it.
    New,
    /// Worth a look.
    Unusual,
}

/// What is known about one program right now.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Facts<'a> {
    /// Its executable name.
    pub name: &'a str,
    /// Its full path, when readable.
    pub exe: Option<&'a str>,
    /// Whether it is new to this computer.
    pub first_seen: bool,
    /// The signature, as far as checked.
    pub signing: &'a Signing,
    /// The most children one of its processes has.
    pub children: u32,
    /// Its open network connections.
    pub connections: u32,
    /// A startup entry for it appeared since the baseline.
    pub new_startup: bool,
}

/// The reasons and score for one program.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Assessment {
    /// 0 to 100.
    pub score: u32,
    /// Why, most weighty first.
    pub reasons: Vec<Reason>,
}

impl Assessment {
    /// Unusual from the score; new otherwise.
    pub fn level(&self) -> Level {
        if self.score >= UNUSUAL_SCORE {
            Level::Unusual
        } else {
            Level::New
        }
    }
}

/// Scores one program on `os`. Pure: each reason is independent, the score is their weights capped at 100.
pub fn assess(os: Os, f: &Facts<'_>) -> Assessment {
    let mut reasons = Vec::new();
    let path = f.exe.map(normalise_path);
    if f.first_seen {
        reasons.push(Reason::FirstSeen);
    }
    if let Some(p) = &path {
        if let Some(l) = location(os, p) {
            reasons.push(Reason::Location(l));
        }
    }
    let in_system = path.as_deref().is_some_and(|p| in_system_folders(os, p));
    match (&path, system_match(os, f.name)) {
        (Some(_), SystemName::Is(n)) if !in_system => reasons.push(Reason::ImitatesSystem(n)),
        (Some(_), SystemName::Near(n)) if !in_system => reasons.push(Reason::LooksLikeSystem(n)),
        _ => {}
    }
    match f.signing {
        Signing::Unsigned if !in_system => reasons.push(Reason::Unsigned),
        Signing::Invalid => reasons.push(Reason::InvalidSignature),
        _ => {}
    }
    if f.children >= MANY_CHILDREN {
        reasons.push(Reason::ManyChildren(f.children));
    }
    if f.connections >= MANY_CONNECTIONS {
        reasons.push(Reason::ManyConnections(f.connections));
    }
    if f.new_startup {
        reasons.push(Reason::NewStartup);
    }
    reasons.sort_by_key(|r| std::cmp::Reverse(r.weight()));
    Assessment {
        score: reasons.iter().map(Reason::weight).sum::<u32>().min(100),
        reasons,
    }
}

/// The path with forward slashes and lower case, which is how every comparison here is done.
pub fn normalise_path(path: &str) -> String {
    path.replace('\\', "/").to_lowercase()
}

/// The identity of a program in the baseline: its full path when known, else its name.
pub fn program_key(name: &str, exe: Option<&str>) -> String {
    match exe {
        Some(e) if !e.is_empty() => normalise_path(e),
        _ => name.to_lowercase(),
    }
}

/// The unusual place `path` (already normalised) is in on `os`, if any. Pure and OS-explicit, so a Windows path is judged
/// by Windows rules on any host.
pub fn location(os: Os, path: &str) -> Option<Location> {
    if path.ends_with(" (deleted)") || path.contains("(deleted)/") {
        return Some(Location::Deleted);
    }
    let has = |needles: &[&str]| needles.iter().any(|n| path.contains(n));
    match os {
        Os::Windows => {
            if has(&["$recycle.bin/"]) {
                Some(Location::Trash)
            } else if has(&["/appdata/local/temp/", "/windows/temp/"]) || (path.contains("/temp/") && path.contains("/users/")) {
                Some(Location::Temp)
            } else if has(&["/downloads/"]) {
                Some(Location::Downloads)
            } else if has(&["/users/public/"]) {
                Some(Location::Shared)
            } else {
                None
            }
        }
        Os::Macos => {
            if has(&["/.trash/"]) {
                Some(Location::Trash)
            } else if path.starts_with("/tmp/") || has(&["/private/tmp/", "/var/folders/", "/private/var/folders/"]) {
                Some(Location::Temp)
            } else if has(&["/downloads/"]) {
                Some(Location::Downloads)
            } else if path.starts_with("/volumes/") && !path.starts_with("/volumes/macintosh hd/") {
                Some(Location::Mounted)
            } else if path.starts_with("/users/shared/") {
                Some(Location::Shared)
            } else {
                None
            }
        }
        Os::Linux => {
            if has(&["/.local/share/trash/"]) {
                Some(Location::Trash)
            } else if path.starts_with("/tmp/") || path.starts_with("/var/tmp/") || path.starts_with("/dev/shm/") {
                Some(Location::Temp)
            } else if has(&["/downloads/"]) {
                Some(Location::Downloads)
            } else if path.starts_with("/media/") || path.starts_with("/mnt/") {
                Some(Location::Mounted)
            } else {
                None
            }
        }
    }
}

/// Whether `path` (normalised) is in the folders the OS keeps its own programs in.
fn in_system_folders(os: Os, path: &str) -> bool {
    let starts = |roots: &[&str]| roots.iter().any(|r| path.starts_with(r));
    match os {
        // Drive letters differ, so the root is matched after "x:".
        Os::Windows => {
            let rest = path.split_once(":/").map_or(path, |(_, r)| r);
            [
                "windows/",
                "program files/",
                "program files (x86)/",
                "programdata/microsoft/windows defender/",
            ]
            .iter()
            .any(|r| rest.starts_with(r))
        }
        Os::Macos => starts(&["/system/", "/usr/", "/sbin/", "/bin/", "/library/apple/"]),
        Os::Linux => starts(&["/usr/", "/bin/", "/sbin/", "/lib/", "/lib64/", "/opt/", "/snap/"]),
    }
}

enum SystemName {
    None,
    Is(String),
    Near(String),
}

const WINDOWS_EXTRA: &[&str] = &[
    "explorer.exe",
    "taskhostw.exe",
    "conhost.exe",
    "runtimebroker.exe",
    "searchindexer.exe",
    "sihost.exe",
    "ctfmon.exe",
    "taskmgr.exe",
    "dllhost.exe",
    "wmiprvse.exe",
];
const LINUX_SYSTEM: &[&str] = &[
    "systemd",
    "sshd",
    "dbus-daemon",
    "cron",
    "rsyslogd",
    "networkmanager",
    "systemd-journald",
    "systemd-logind",
];

/// Critical Windows processes (the OS's own) (lower-case file names).
const WINDOWS_PROTECTED: &[&str] = &[
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
const MACOS_PROTECTED: &[&str] = &[
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

/// The system program `name` is or nearly is on `os`.
fn system_match(os: Os, name: &str) -> SystemName {
    let lower = name.to_lowercase();
    let names: Vec<&str> = match os {
        // Windows names without a path ("System", "Registry") cannot be judged by where they run from.
        Os::Windows => WINDOWS_PROTECTED
            .iter()
            .chain(WINDOWS_EXTRA)
            .copied()
            .filter(|n| n.ends_with(".exe"))
            .collect(),
        Os::Macos => MACOS_PROTECTED.to_vec(),
        Os::Linux => LINUX_SYSTEM.to_vec(),
    };
    if names.contains(&lower.as_str()) {
        return SystemName::Is(name.to_string());
    }
    if lower.chars().count() >= 6 {
        if let Some(n) = names.iter().find(|n| n.chars().count() >= 6 && within_one_edit(&lower, n)) {
            return SystemName::Near((*n).to_string());
        }
    }
    SystemName::None
}

/// Whether `a` and `b` differ by exactly one insertion, deletion or substitution.
pub fn within_one_edit(a: &str, b: &str) -> bool {
    let (a, b): (Vec<char>, Vec<char>) = (a.chars().collect(), b.chars().collect());
    if a == b || a.len().abs_diff(b.len()) > 1 {
        return false;
    }
    let (short, long) = if a.len() <= b.len() { (&a, &b) } else { (&b, &a) };
    let prefix = short.iter().zip(long.iter()).take_while(|(x, y)| x == y).count();
    if short.len() == long.len() {
        short[prefix + 1..] == long[prefix + 1..]
    } else {
        short[prefix..] == long[prefix + 1..]
    }
}

/// The executable's file name from a path (either separator).
pub fn exe_name(path: &str) -> &str {
    file_name(path)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn facts<'a>(name: &'a str, exe: Option<&'a str>, signing: &'a Signing) -> Facts<'a> {
        Facts {
            name,
            exe,
            first_seen: false,
            signing,
            children: 0,
            connections: 0,
            new_startup: false,
        }
    }

    fn reasons(os: Os, f: &Facts<'_>) -> Vec<Reason> {
        assess(os, f).reasons
    }

    #[test]
    fn an_ordinary_installed_program_has_no_reasons() {
        let s = Signing::Signed("Example Ltd".into());
        let f = facts("app.exe", Some("C:\\Program Files\\Example\\app.exe"), &s);
        let a = assess(Os::Windows, &f);
        assert!(a.reasons.is_empty());
        assert_eq!(a.score, 0);
        assert_eq!(a.level(), Level::New);
    }

    #[test]
    fn first_seen_alone_is_new_not_unusual() {
        let s = Signing::Unchecked;
        let mut f = facts("tool", Some("/Applications/Tool.app/Contents/MacOS/Tool"), &s);
        f.first_seen = true;
        let a = assess(Os::Macos, &f);
        assert_eq!(a.reasons, vec![Reason::FirstSeen]);
        assert_eq!(a.level(), Level::New);
        assert_eq!(a.score, 15);
    }

    #[test]
    fn unusual_locations_per_os() {
        assert_eq!(location(Os::Windows, "c:/users/ann/appdata/local/temp/x.exe"), Some(Location::Temp));
        assert_eq!(location(Os::Windows, "c:/windows/temp/x.exe"), Some(Location::Temp));
        assert_eq!(location(Os::Windows, "d:/users/ann/downloads/x.exe"), Some(Location::Downloads));
        assert_eq!(location(Os::Windows, "c:/users/public/x.exe"), Some(Location::Shared));
        assert_eq!(location(Os::Windows, "c:/$recycle.bin/s-1/x.exe"), Some(Location::Trash));
        assert_eq!(location(Os::Windows, "c:/program files/a/a.exe"), None);
        assert_eq!(location(Os::Macos, "/private/var/folders/ab/cd/t/x"), Some(Location::Temp));
        assert_eq!(location(Os::Macos, "/tmp/x"), Some(Location::Temp));
        assert_eq!(
            location(Os::Macos, "/users/ann/downloads/a.app/contents/macos/a"),
            Some(Location::Downloads)
        );
        assert_eq!(
            location(Os::Macos, "/volumes/installer/a.app/contents/macos/a"),
            Some(Location::Mounted)
        );
        assert_eq!(location(Os::Macos, "/volumes/macintosh hd/applications/a"), None);
        assert_eq!(location(Os::Macos, "/users/shared/x"), Some(Location::Shared));
        assert_eq!(location(Os::Macos, "/users/ann/.trash/x"), Some(Location::Trash));
        assert_eq!(location(Os::Macos, "/applications/safari.app/contents/macos/safari"), None);
        assert_eq!(location(Os::Linux, "/dev/shm/x"), Some(Location::Temp));
        assert_eq!(location(Os::Linux, "/var/tmp/x"), Some(Location::Temp));
        assert_eq!(location(Os::Linux, "/media/ann/usb/x"), Some(Location::Mounted));
        assert_eq!(location(Os::Linux, "/home/ann/.local/share/trash/files/x"), Some(Location::Trash));
        assert_eq!(location(Os::Linux, "/usr/bin/x"), None);
        assert_eq!(location(Os::Linux, "/usr/bin/x (deleted)"), Some(Location::Deleted));
        // A Windows path is judged by Windows rules whatever the host.
        assert_eq!(
            location(Os::Windows, &normalise_path("C:\\Users\\Ann\\AppData\\Local\\Temp\\a.exe")),
            Some(Location::Temp)
        );
    }

    #[test]
    fn a_system_name_in_the_wrong_place_is_flagged() {
        let s = Signing::Unchecked;
        let bad = facts("svchost.exe", Some("C:\\Users\\Ann\\AppData\\Local\\Temp\\svchost.exe"), &s);
        let r = reasons(Os::Windows, &bad);
        assert!(r.contains(&Reason::ImitatesSystem("svchost.exe".into())));
        assert!(r.contains(&Reason::Location(Location::Temp)));
        assert_eq!(assess(Os::Windows, &bad).level(), Level::Unusual);
        let ok = facts("svchost.exe", Some("C:\\Windows\\System32\\svchost.exe"), &s);
        assert!(reasons(Os::Windows, &ok).is_empty());
        let mac = facts("launchd", Some("/users/ann/launchd"), &s);
        assert!(reasons(Os::Macos, &mac).contains(&Reason::ImitatesSystem("launchd".into())));
        let mac_ok = facts("launchd", Some("/sbin/launchd"), &s);
        assert!(reasons(Os::Macos, &mac_ok).is_empty());
        let linux = facts("sshd", Some("/home/ann/sshd"), &s);
        assert!(reasons(Os::Linux, &linux).contains(&Reason::ImitatesSystem("sshd".into())));
        // Without a path nothing can be said about where it runs from.
        assert!(reasons(Os::Windows, &facts("svchost.exe", None, &s)).is_empty());
    }

    #[test]
    fn lookalike_names_are_one_character_away() {
        let s = Signing::Unchecked;
        for fake in ["svch0st.exe", "svchosts.exe", "svchst.exe"] {
            let f = facts(fake, Some("C:\\Users\\Ann\\x\\a.exe"), &s);
            assert!(
                reasons(Os::Windows, &f).contains(&Reason::LooksLikeSystem("svchost.exe".into())),
                "{fake}"
            );
        }
        // Two edits away, or too short to compare, is not a lookalike.
        assert!(!reasons(Os::Windows, &facts("svhst.exe", Some("C:\\x\\a.exe"), &s))
            .iter()
            .any(|r| matches!(r, Reason::LooksLikeSystem(_))));
        assert!(!reasons(Os::Macos, &facts("logs", Some("/users/a/logs"), &s))
            .iter()
            .any(|r| matches!(r, Reason::LooksLikeSystem(_))));
        assert!(within_one_edit("abc", "abd"));
        assert!(within_one_edit("abc", "abcd"));
        assert!(within_one_edit("abcd", "acd"));
        assert!(!within_one_edit("abc", "abc"));
        assert!(!within_one_edit("abc", "xyz"));
        assert!(!within_one_edit("abc", "abcde"));
        assert!(!within_one_edit("ab", "ba"));
    }

    #[test]
    fn signatures() {
        let unsigned = Signing::Unsigned;
        let in_home = facts("tool", Some("/Users/ann/tool"), &unsigned);
        assert!(reasons(Os::Macos, &in_home).contains(&Reason::Unsigned));
        // Unsigned programs inside the OS folders are normal there (many system tools are not individually signed).
        let sys = facts("tool", Some("/usr/local/bin/tool"), &unsigned);
        assert!(!reasons(Os::Macos, &sys).contains(&Reason::Unsigned));
        let invalid = Signing::Invalid;
        assert!(reasons(Os::Macos, &facts("tool", Some("/Applications/Tool.app/x"), &invalid)).contains(&Reason::InvalidSignature));
        // An unknown or unchecked signature is never held against a program.
        for s in [Signing::Unknown, Signing::Unchecked] {
            assert!(reasons(Os::Macos, &facts("tool", Some("/Users/ann/tool"), &s)).is_empty());
        }
        assert_eq!(Signing::from(&SignatureInfo::Signed("A".into())), Signing::Signed("A".into()));
        assert_eq!(Signing::from(&SignatureInfo::Unsigned), Signing::Unsigned);
        assert_eq!(Signing::from(&SignatureInfo::Invalid("x".into())), Signing::Invalid);
        assert_eq!(Signing::from(&SignatureInfo::Unknown("x".into())), Signing::Unknown);
    }

    #[test]
    fn behaviour_reasons_have_thresholds() {
        let s = Signing::Unchecked;
        let mut f = facts("tool", Some("/Applications/T.app/t"), &s);
        f.children = MANY_CHILDREN - 1;
        f.connections = MANY_CONNECTIONS - 1;
        assert!(reasons(Os::Macos, &f).is_empty());
        f.children = MANY_CHILDREN;
        f.connections = MANY_CONNECTIONS;
        f.new_startup = true;
        let a = assess(Os::Macos, &f);
        assert_eq!(a.reasons.len(), 3);
        assert_eq!(a.reasons[0], Reason::NewStartup);
        assert_eq!(a.score, 70);
        assert_eq!(a.level(), Level::Unusual);
    }

    #[test]
    fn the_score_is_capped_and_wording_is_never_a_verdict() {
        let s = Signing::Invalid;
        let mut f = facts("svchost.exe", Some("C:\\Users\\a\\AppData\\Local\\Temp\\svchost.exe"), &s);
        f.first_seen = true;
        f.new_startup = true;
        f.children = 99;
        let a = assess(Os::Windows, &f);
        assert_eq!(a.score, 100);
        for r in &a.reasons {
            let t = r.text().to_lowercase();
            for banned in ["malware", "virus", "trojan", "malicious", "infected", "dangerous"] {
                assert!(!t.contains(banned), "{t}");
            }
        }
        assert!(Reason::LooksLikeSystem("svchost.exe".into()).text().contains("one character away"));
        assert!(Reason::ManyConnections(77).text().contains("77"));
        for l in [
            Location::Temp,
            Location::Downloads,
            Location::Shared,
            Location::Mounted,
            Location::Trash,
            Location::Deleted,
        ] {
            assert!(Reason::Location(l).text().starts_with("runs from"));
        }
    }

    #[test]
    fn keys_prefer_the_path() {
        assert_eq!(program_key("A.exe", Some("C:\\X\\A.exe")), "c:/x/a.exe");
        assert_eq!(program_key("A.exe", None), "a.exe");
        assert_eq!(program_key("A.exe", Some("")), "a.exe");
        assert_eq!(exe_name("C:\\x\\a.exe"), "a.exe");
    }

    #[test]
    fn settings_are_clamped() {
        assert_eq!(
            UnusualSettings {
                enabled: true,
                learning_days: 999
            }
            .clamped()
            .learning_days,
            60
        );
        assert!(UnusualSettings::default().enabled);
        assert_eq!(UnusualSettings::default().learning_days, 7);
    }
}
