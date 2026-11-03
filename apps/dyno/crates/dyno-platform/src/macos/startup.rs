//! The startup manager on macOS, listed only: launch agents in `~/Library/LaunchAgents` and `/Library/LaunchAgents`, and the
//! login items and app-registered agents of System Settings > Login Items & Extensions (`sfltool dumpbtm`).
//! Enabled/disabled for plists is launchd's own override database (`launchctl print-disabled`). `launchctl` and `sfltool` sit
//! behind [`Launchctl`] and [`BtmSource`] so tests never touch the real user's launchd.
//!
//! A source that cannot be read is never an empty list: it is logged with the raw reason, and when nothing at all could be read the
//! error reaches the user with what would help.

use crate::parse::btm::{parse_dumpbtm, BtmItem, BtmKind};
use crate::parse::launchd::{parse_launch_agent, parse_print_disabled, LaunchAgent};
use dyno_core::ports::{PlatformError, StartupItems};
use dyno_core::startup::{exe_of_command, ImpactRating, StartupEntry, StartupImpact, StartupLocation, StartupState};
use std::path::{Path, PathBuf};
use std::time::Duration;

/// Most plist files read per folder.
const MAX_AGENTS: usize = 2_000;

/// The launchd operations the startup manager needs.
pub trait Launchctl: Send {
    /// `launchctl print-disabled gui/<uid>` output.
    fn print_disabled(&mut self) -> Result<String, PlatformError>;
}

/// The Background Task Management database.
pub trait BtmSource: Send {
    /// `sfltool dumpbtm` output.
    fn dump(&mut self) -> Result<String, PlatformError>;
}

/// The real `sfltool`.
pub struct RealBtm;

impl BtmSource for RealBtm {
    fn dump(&mut self) -> Result<String, PlatformError> {
        let mut cmd = crate::process::command("/usr/bin/sfltool");
        cmd.arg("dumpbtm");
        let out = crate::process::run_with_timeout(cmd, Duration::from_secs(20))?;
        btm_output(
            out.status.code(),
            &String::from_utf8_lossy(&out.stdout),
            &String::from_utf8_lossy(&out.stderr),
        )
    }
}

/// Turns the tool's exit code and text into the dump or a typed error. Split from [`RealBtm`] so the refusals are tested.
pub(crate) fn btm_output(code: Option<i32>, stdout: &str, stderr: &str) -> Result<String, PlatformError> {
    if code == Some(0) && !stdout.trim().is_empty() {
        return Ok(stdout.to_string());
    }
    let said = format!("{stderr}\n{stdout}");
    let first = said.lines().map(str::trim).find(|l| !l.is_empty()).unwrap_or("it printed nothing");
    let lower = said.to_lowercase();
    if ["root", "privilege", "not permitted", "authoriz", "administrator", "sudo"]
        .iter()
        .any(|w| lower.contains(w))
    {
        return Err(PlatformError::AccessDenied(format!(
            "Login items need administrator rights to read (sfltool said: {first}). The privileged helper, a later release, can show them."
        )));
    }
    let exit = code.map_or("no exit code".to_string(), |c| format!("exit {c}"));
    Err(PlatformError::Failed(format!(
        "sfltool dumpbtm gave nothing usable ({exit}): {first}"
    )))
}

/// The real `launchctl` for the current user's GUI domain.
pub struct RealLaunchctl {
    domain: String,
}

impl RealLaunchctl {
    /// For the current user.
    pub fn new() -> Self {
        // SAFETY: getuid has no preconditions.
        let uid = unsafe { libc::getuid() };
        RealLaunchctl {
            domain: format!("gui/{uid}"),
        }
    }

    fn run(&self, args: &[&str]) -> Result<String, PlatformError> {
        let mut cmd = crate::process::command("/bin/launchctl");
        cmd.args(args);
        let out = crate::process::run_with_timeout(cmd, Duration::from_secs(15))?;
        if out.status.success() {
            Ok(String::from_utf8_lossy(&out.stdout).into_owned())
        } else {
            let err = String::from_utf8_lossy(&out.stderr);
            Err(PlatformError::Failed(format!(
                "launchctl said: {}",
                err.lines().next().unwrap_or("it failed").trim()
            )))
        }
    }
}

impl Default for RealLaunchctl {
    fn default() -> Self {
        Self::new()
    }
}

impl Launchctl for RealLaunchctl {
    fn print_disabled(&mut self) -> Result<String, PlatformError> {
        self.run(&["print-disabled", &self.domain])
    }
}

/// Launch agents as startup entries.
pub struct LaunchAgents {
    user_dir: Option<PathBuf>,
    system_dir: Option<PathBuf>,
    launchctl: Box<dyn Launchctl>,
    btm: Box<dyn BtmSource>,
    uid: u32,
}

/// What one launch-agent folder gave.
#[derive(Default)]
struct Folder {
    /// Plist files looked at.
    files: usize,
    /// Files that were not readable or not a launch agent.
    skipped: usize,
    /// Why the folder itself could not be listed (not set when it does not exist).
    unreadable: Option<String>,
}

impl LaunchAgents {
    /// The current user's folders and the real `launchctl`.
    pub fn for_current_user() -> Self {
        let home = std::env::var_os("HOME").filter(|h| !h.is_empty()).map(PathBuf::from);
        LaunchAgents {
            user_dir: home.map(|h| h.join("Library").join("LaunchAgents")),
            system_dir: Some(PathBuf::from("/Library/LaunchAgents")),
            launchctl: Box::new(RealLaunchctl::new()),
            btm: Box::new(RealBtm),
            // SAFETY: getuid has no preconditions.
            uid: unsafe { libc::getuid() },
        }
    }

    /// Explicit folders and launchctl (tests).
    pub fn with(
        user_dir: Option<PathBuf>,
        system_dir: Option<PathBuf>,
        launchctl: Box<dyn Launchctl>,
        btm: Box<dyn BtmSource>,
        uid: u32,
    ) -> Self {
        LaunchAgents {
            user_dir,
            system_dir,
            launchctl,
            btm,
            uid,
        }
    }

    fn read_dir(dir: &Path, location: StartupLocation, disabled: &[(String, bool)], out: &mut Vec<StartupEntry>) -> Folder {
        let mut folder = Folder::default();
        let rd = match std::fs::read_dir(dir) {
            Ok(rd) => rd,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return folder,
            Err(e) => {
                folder.unreadable = Some(e.to_string());
                return folder;
            }
        };
        for e in rd.flatten().take(MAX_AGENTS) {
            let path = e.path();
            if path.extension().and_then(|x| x.to_str()) != Some("plist") {
                continue;
            }
            folder.files += 1;
            let Ok(bytes) = std::fs::read(&path) else {
                folder.skipped += 1;
                continue;
            };
            let Some(agent) = parse_launch_agent(&bytes) else {
                folder.skipped += 1;
                continue;
            };
            if !agent.starts_at_login() {
                continue;
            }
            out.push(entry_of(&agent, location.clone(), disabled));
        }
        folder
    }
}

fn btm_entry(item: &BtmItem) -> StartupEntry {
    let (prefix, command) = match item.kind {
        BtmKind::LoginItem => ("login", item.exe.clone().unwrap_or_default()),
        BtmKind::Agent => ("btm-agent", item.exe.clone().unwrap_or_default()),
    };
    StartupEntry {
        id: format!("{prefix}:{}", item.identifier),
        name: item.name.clone(),
        exe: item.exe.clone(),
        command,
        location: StartupLocation::LoginItem,
        state: if item.enabled {
            StartupState::Enabled
        } else {
            StartupState::Disabled
        },
        publisher: None,
        impact: StartupImpact {
            cpu_secs: 0.0,
            disk_bytes: 0,
            rating: ImpactRating::NotMeasured,
        },
    }
}

fn entry_of(agent: &LaunchAgent, location: StartupLocation, disabled: &[(String, bool)]) -> StartupEntry {
    let override_state = disabled.iter().find(|(l, _)| *l == agent.label).map(|(_, d)| *d);
    let is_disabled = override_state.unwrap_or(agent.disabled);
    let command = agent.command();
    let prefix = if location == StartupLocation::UserLaunchAgent {
        "user"
    } else {
        "system"
    };
    StartupEntry {
        id: format!("agent:{prefix}:{}", agent.label),
        name: agent.label.clone(),
        exe: agent.executable().map(str::to_string).or_else(|| exe_of_command(&command)),
        command,
        location,
        state: if is_disabled {
            StartupState::Disabled
        } else {
            StartupState::Enabled
        },
        publisher: None,
        impact: StartupImpact {
            cpu_secs: 0.0,
            disk_bytes: 0,
            rating: ImpactRating::NotMeasured,
        },
    }
}

impl StartupItems for LaunchAgents {
    fn list(&mut self) -> Result<Vec<StartupEntry>, PlatformError> {
        let started = std::time::Instant::now();
        // Without launchctl's answer, the plist's own Disabled key is all we know.
        let disabled = match self.launchctl.print_disabled() {
            Ok(t) => parse_print_disabled(&t),
            Err(e) => {
                log::warn!(Error = e.to_string().as_str(); "Startup items: launchctl print-disabled failed; the plists' own Disabled keys are used");
                Vec::new()
            }
        };
        let mut out = Vec::new();
        let mut problems: Vec<PlatformError> = Vec::new();
        for (dir, location, what) in [
            (&self.user_dir, StartupLocation::UserLaunchAgent, "user launch agents"),
            (&self.system_dir, StartupLocation::SystemLaunchAgent, "system launch agents"),
        ] {
            let Some(d) = dir else { continue };
            let before = out.len();
            let f = Self::read_dir(d, location, &disabled, &mut out);
            log::info!(Source = what, Files = f.files, Skipped = f.skipped, Listed = out.len() - before; "Startup items: launch agent folder read");
            if let Some(why) = f.unreadable {
                log::warn!(Source = what, Error = why.as_str(); "Startup items: a launch agent folder could not be read");
                problems.push(PlatformError::Failed(format!("The {what} folder could not be read: {why}")));
            }
            if f.skipped > 0 {
                log::warn!(Source = what, Skipped = f.skipped; "Startup items: some plist files were unreadable or not launch agents");
            }
        }
        let before = out.len();
        match self.btm.dump().and_then(|t| {
            parse_dumpbtm(&t, self.uid).map_err(|e| {
                let crate::parse::btm::BtmError::NoRecords { first_line } = e;
                PlatformError::Failed(format!("sfltool dumpbtm printed no records (first line: {first_line})"))
            })
        }) {
            Ok(items) => {
                out.extend(items.iter().map(btm_entry));
                log::info!(Listed = out.len() - before; "Startup items: login items read from the Background Task Management database");
            }
            Err(e) => {
                log::warn!(Error = e.to_string().as_str(); "Startup items: login items could not be read from the Background Task Management database");
                problems.push(e);
            }
        }
        log::info!(Entries = out.len(), Problems = problems.len(), Ms = started.elapsed().as_millis() as u64; "Startup items listed");
        // Never an empty list that hides a failed read: with nothing read and something unreadable, the reason is the answer.
        if out.is_empty() {
            if let Some(first) = problems.into_iter().next() {
                return Err(first);
            }
        }
        out.sort_by_key(|e| e.name.to_lowercase());
        Ok(out)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Clone, Default)]
    struct Fake {
        disabled_text: String,
    }
    impl Launchctl for Fake {
        fn print_disabled(&mut self) -> Result<String, PlatformError> {
            Ok(self.disabled_text.clone())
        }
    }

    /// A BTM source with a fixed answer.
    struct FakeBtm(Result<String, PlatformError>);
    impl BtmSource for FakeBtm {
        fn dump(&mut self) -> Result<String, PlatformError> {
            self.0.clone()
        }
    }

    const UID: u32 = 501;
    /// Invented, same-shape dump: one login item of user 501.
    const DUMP: &str = " Records for UID 501 : AAAA\n\n Items:\n\n #1:\n   Name: Example Sync\n   Type: login item (0x4)\n\
   Disposition: [enabled, allowed, notified] (0xb)\n   Identifier: 4.com.example.sync\n   Executable Path: /Applications/Example Sync.app/x\n";

    fn plist(label: &str, run_at_load: bool) -> String {
        format!(
            "<?xml version=\"1.0\"?><plist version=\"1.0\"><dict><key>Label</key><string>{label}</string><key>ProgramArguments</key>\
<array><string>/Applications/X.app/x</string></array><key>RunAtLoad</key><{}/></dict></plist>",
            run_at_load
        )
    }

    fn agents(user: Option<PathBuf>, system: Option<PathBuf>, btm: Result<String, PlatformError>) -> LaunchAgents {
        LaunchAgents::with(user, system, Box::new(Fake::default()), Box::new(FakeBtm(btm)), UID)
    }

    fn setup() -> (tempfile::TempDir, LaunchAgents) {
        let dir = tempfile::tempdir().unwrap();
        let user = dir.path().join("user");
        let system = dir.path().join("system");
        std::fs::create_dir_all(&user).unwrap();
        std::fs::create_dir_all(&system).unwrap();
        std::fs::write(user.join("a.plist"), plist("com.example.a", true)).unwrap();
        std::fs::write(user.join("b.plist"), plist("com.example.b", true)).unwrap();
        std::fs::write(user.join("manual.plist"), plist("com.example.manual", false)).unwrap();
        std::fs::write(user.join("junk.plist"), b"not a plist").unwrap();
        std::fs::write(user.join("readme.txt"), b"x").unwrap();
        std::fs::write(system.join("s.plist"), plist("com.vendor.s", true)).unwrap();
        let fake = Fake {
            disabled_text: "disabled services = {\n\t\"com.example.b\" => disabled\n\t\"com.example.a\" => enabled\n}".into(),
        };
        let la = LaunchAgents::with(Some(user), Some(system), Box::new(fake), Box::new(FakeBtm(Ok(DUMP.into()))), UID);
        (dir, la)
    }

    #[test]
    fn lists_login_agents_with_launchds_disabled_state_and_login_items() {
        let (_d, mut la) = setup();
        let list = la.list().unwrap();
        let names: Vec<&str> = list.iter().map(|e| e.name.as_str()).collect();
        assert_eq!(names, vec!["com.example.a", "com.example.b", "com.vendor.s", "Example Sync"]);
        assert_eq!(list[0].state, StartupState::Enabled);
        assert_eq!(list[1].state, StartupState::Disabled);
        assert_eq!(list[2].location, StartupLocation::SystemLaunchAgent);
        assert_eq!(list[0].exe.as_deref(), Some("/Applications/X.app/x"));
        assert_eq!(
            (list[3].location.clone(), list[3].state),
            (StartupLocation::LoginItem, StartupState::Enabled)
        );
    }

    #[test]
    fn missing_folders_and_no_login_items_are_a_genuinely_empty_list() {
        let none = "Records for UID 501 : A\n Items:\n".to_string();
        let mut la = agents(None, Some(PathBuf::from("/dyno/none")), Ok(none));
        assert!(la.list().unwrap().is_empty());
    }

    /// The reported bug: when nothing could be read, the list said "no startup items" instead of why.
    #[test]
    fn nothing_readable_is_an_error_with_the_reason_never_an_empty_list() {
        let mut la = agents(
            None,
            Some(PathBuf::from("/dyno/none")),
            Err(PlatformError::AccessDenied("needs admin".into())),
        );
        assert_eq!(la.list(), Err(PlatformError::AccessDenied("needs admin".into())));
        let mut la = agents(None, None, Ok("sfltool: usage".into()));
        match la.list() {
            Err(PlatformError::Failed(m)) => assert!(m.contains("sfltool: usage"), "{m}"),
            other => panic!("expected the reason, got {other:?}"),
        }
    }

    #[test]
    fn one_source_failing_keeps_what_the_other_gave() {
        let (_d, mut la) = setup();
        la.btm = Box::new(FakeBtm(Err(PlatformError::AccessDenied("needs admin".into()))));
        assert_eq!(la.list().unwrap().len(), 3);
    }

    #[test]
    fn sfltool_refusals_say_what_would_help() {
        assert!(matches!(btm_output(Some(0), "text", ""), Ok(t) if t == "text"));
        assert!(
            matches!(btm_output(Some(1), "", "sfltool: must be run as root"), Err(PlatformError::AccessDenied(m)) if m.contains("helper"))
        );
        assert!(matches!(btm_output(Some(0), "  \n", ""), Err(PlatformError::Failed(m)) if m.contains("exit 0")));
        assert!(matches!(btm_output(None, "", "boom"), Err(PlatformError::Failed(m)) if m.contains("no exit code") && m.contains("boom")));
    }

    #[test]
    fn this_macs_startup_items_list_without_changing_anything() {
        let mut la = LaunchAgents::for_current_user();
        // Either a list or a reason; never a silent empty list caused by a failed read.
        if let Ok(list) = la.list() {
            assert!(list.iter().all(|e| !e.id.is_empty() && !e.name.is_empty()));
        }
    }
}
