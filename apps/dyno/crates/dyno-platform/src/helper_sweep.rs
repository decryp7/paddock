//! The helper's sweep of every process (ADR 0027 WP-5): `sysinfo` reads each process once, the owner is classified by id
//! ([`classify_owner`]), and every account's home folder comes from the OS's own account list: `/etc/passwd` on Linux,
//! `dscl . -list /Users NFSHomeDirectory` on macOS, the `ProfileList` registry key (through `reg query`) on Windows. The home
//! parsers are pure and compile everywhere; only allow-listed fields are read, so there is nothing to leak.

use crate::runner::ToolRunner;
use dyno_core::helper::HelperHost;
use dyno_core::helper_processes::{classify_owner, default_first_person, ProcState, ProcessSweeper, RawOwner, RawProcess, Sweep};
use dyno_core::helper_wire::HelperUser;
use dyno_core::ports::PlatformError;
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use sysinfo::{ProcessRefreshKind, ProcessStatus, ProcessesToUpdate, System, UpdateKind, Users};

const TIMEOUT: Duration = Duration::from_secs(10);
const DSCL: &str = "/usr/bin/dscl";
const REG: &str = r"C:\Windows\System32\reg.exe";
const PROFILE_LIST: &str = r"HKLM\SOFTWARE\Microsoft\Windows NT\CurrentVersion\ProfileList";
/// The most account homes read (a bound on a hostile or huge directory).
const MAX_HOMES: usize = 4_096;

/// `name:x:uid:gid:gecos:home:shell` lines to (uid, home); other lines are skipped.
pub fn parse_passwd(text: &str) -> Vec<(HelperUser, String)> {
    text.lines()
        .filter_map(|line| {
            let f: Vec<&str> = line.split(':').collect();
            let uid = f.get(2)?.parse::<u32>().ok()?;
            let home = f.get(5)?.trim();
            (!home.is_empty() && home != "/" && home != "/var/empty" && home != "/nonexistent")
                .then(|| (HelperUser::Uid(uid), home.to_string()))
        })
        .take(MAX_HOMES)
        .collect()
}

/// `dscl . -list /Users NFSHomeDirectory` (name, spaces, home) joined with `dscl . -list /Users UniqueID` (name, spaces, uid).
pub fn parse_dscl_homes(homes: &str, ids: &str) -> Vec<(HelperUser, String)> {
    let split = |line: &str| -> Option<(String, String)> {
        let (name, rest) = line.trim().split_once(char::is_whitespace)?;
        Some((name.to_string(), rest.trim().to_string()))
    };
    let uids: std::collections::HashMap<String, u32> = ids
        .lines()
        .filter_map(split)
        .filter_map(|(n, v)| Some((n, v.parse().ok()?)))
        .collect();
    homes
        .lines()
        .filter_map(split)
        .filter(|(_, home)| !home.is_empty() && home != "/var/empty" && home != "/dev/null")
        .filter_map(|(name, home)| Some((HelperUser::Uid(*uids.get(&name)?), home)))
        .take(MAX_HOMES)
        .collect()
}

/// `reg query ...ProfileList /s` output: a key line ending in the SID, then a `ProfileImagePath    REG_EXPAND_SZ    C:\Users\x` line.
pub fn parse_profile_list(text: &str) -> Vec<(HelperUser, String)> {
    let mut out = Vec::new();
    let mut sid: Option<String> = None;
    for line in text.lines() {
        let t = line.trim();
        if t.to_ascii_uppercase().starts_with("HKEY_") {
            sid = t.rsplit('\\').next().filter(|s| s.starts_with("S-1-")).map(str::to_string);
        } else if let (Some(sid), Some(rest)) = (&sid, t.strip_prefix("ProfileImagePath")) {
            if let Some((_, home)) = rest.trim().split_once(char::is_whitespace) {
                let home = home.trim();
                if !home.is_empty() && out.len() < MAX_HOMES {
                    out.push((HelperUser::Sid(sid.clone()), home.to_string()));
                }
            }
        }
    }
    out
}

/// The machine's first person uid: `UID_MIN` of `/etc/login.defs` on Linux (default 1000), 500 on macOS.
pub fn first_person_uid(host: HelperHost) -> u32 {
    if host == HelperHost::Linux {
        if let Some(n) = std::fs::read_to_string("/etc/login.defs").ok().as_deref().and_then(parse_uid_min) {
            return n;
        }
    }
    default_first_person(host)
}

/// `UID_MIN   1000` from `login.defs` text; comments and other keys are skipped.
pub fn parse_uid_min(text: &str) -> Option<u32> {
    text.lines().find_map(|l| {
        let mut w = l.split_whitespace();
        (w.next() == Some("UID_MIN")).then(|| w.next()?.parse().ok()).flatten()
    })
}

fn state(s: ProcessStatus) -> ProcState {
    match s {
        ProcessStatus::Run => ProcState::Running,
        ProcessStatus::Sleep | ProcessStatus::Idle | ProcessStatus::UninterruptibleDiskSleep => ProcState::Sleeping,
        ProcessStatus::Stop | ProcessStatus::Tracing => ProcState::Stopped,
        _ => ProcState::Other,
    }
}

/// The real sweep: `sysinfo` for the processes, the OS's account list for the homes.
pub struct SystemSweeper {
    host: HelperHost,
    runner: Box<dyn ToolRunner>,
}

impl SystemSweeper {
    /// A sweeper for `host` reading its account list through `runner`.
    pub fn new(host: HelperHost, runner: Box<dyn ToolRunner>) -> SystemSweeper {
        SystemSweeper { host, runner }
    }

    /// Every account's home folder. An unreadable list is an error: with an incomplete list other accounts' paths would not be
    /// redacted, so no answer is better than a leaking one.
    pub fn homes(&self) -> Result<Vec<(HelperUser, String)>, PlatformError> {
        match self.host {
            HelperHost::Linux => {
                let text = std::fs::read_to_string("/etc/passwd")
                    .map_err(|e| PlatformError::Failed(format!("The account list /etc/passwd could not be read: {e}")))?;
                Ok(parse_passwd(&text))
            }
            HelperHost::Macos => {
                let list = |attr: &str| {
                    self.runner
                        .run(DSCL, &[".".into(), "-list".into(), "/Users".into(), attr.into()], TIMEOUT)
                };
                Ok(parse_dscl_homes(&list("NFSHomeDirectory")?, &list("UniqueID")?))
            }
            HelperHost::Windows => {
                let args = ["query".to_string(), PROFILE_LIST.to_string(), "/s".to_string()];
                Ok(parse_profile_list(&self.runner.run(REG, &args, TIMEOUT)?))
            }
        }
    }
}

#[cfg(unix)]
fn user_of(u: &sysinfo::Uid) -> HelperUser {
    HelperUser::Uid(**u)
}
#[cfg(windows)]
fn user_of(u: &sysinfo::Uid) -> HelperUser {
    HelperUser::Sid(u.to_string())
}

impl ProcessSweeper for SystemSweeper {
    fn sweep(&self) -> Result<Sweep, PlatformError> {
        let homes = self.homes()?;
        let mut sys = System::new().map_err(|why| {
            log::warn!("the OS process table is unavailable (sysinfo): {why}; the sweep is refused");
            PlatformError::Failed(format!("The OS process table could not be read ({why})."))
        })?;
        let what = ProcessRefreshKind::nothing()
            .with_cpu()
            .with_memory()
            .with_disk_usage()
            .with_user(UpdateKind::Always)
            .with_exe(UpdateKind::Always);
        sys.refresh_processes_specifics(ProcessesToUpdate::All, true, what);
        let users = Users::new_with_refreshed_list().map_err(|why| {
            log::warn!("the user list is unavailable (sysinfo): {why}; the sweep is refused");
            PlatformError::Failed(format!("The OS user list could not be read ({why})."))
        })?;
        let first_person = first_person_uid(self.host);
        let processes = sys
            .processes()
            .values()
            .map(|p| {
                let name = p.user_id().and_then(|u| users.get_user_by_id(u)).map(|u| u.name().to_string());
                let owner = match p.user_id() {
                    Some(u) => classify_owner(&user_of(u), name.as_deref(), first_person),
                    None => RawOwner::Unknown,
                };
                let disk = p.disk_usage();
                RawProcess {
                    pid: p.pid().as_u32(),
                    started: i64::try_from(p.start_time()).ok().filter(|s| *s > 0),
                    parent: p.parent().map(|p| p.as_u32()),
                    name: p.name().to_string_lossy().into_owned(),
                    path: p.exe().map(|e| e.to_string_lossy().into_owned()),
                    owner,
                    state: state(p.status()),
                    threads: p.tasks().map(|t| u32::try_from(t.len()).unwrap_or(u32::MAX)),
                    cpu_time_ms: Some(p.accumulated_cpu_time()),
                    memory_bytes: Some(p.memory()),
                    disk_read_bytes: Some(disk.total_read_bytes),
                    disk_written_bytes: Some(disk.total_written_bytes),
                }
            })
            .collect();
        let taken_at = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_or(0, |d| i64::try_from(d.as_secs()).unwrap_or(0));
        Ok(Sweep {
            processes,
            homes,
            taken_at,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::runner::{Tool, ToolOutput};

    struct Fake(Vec<(&'static str, &'static str)>);
    impl ToolRunner for Fake {
        fn run_tool(&self, t: &Tool, _: Duration) -> Result<ToolOutput, PlatformError> {
            let key = t.args.join(" ");
            self.0
                .iter()
                .find(|(k, _)| key.contains(k))
                .map(|(_, v)| ToolOutput {
                    ok: true,
                    stdout: (*v).to_string(),
                    error: String::new(),
                })
                .ok_or_else(|| PlatformError::Failed(format!("unscripted {key}")))
        }
    }

    #[test]
    fn uid_min_is_read_from_login_defs_text() {
        assert_eq!(parse_uid_min("# UID_MIN 5\nUID_MIN\t\t500\nUID_MAX 60000\n"), Some(500));
        assert_eq!(parse_uid_min("UID_MAX 60000\n"), None);
        assert_eq!(parse_uid_min("UID_MIN lots\n"), None);
    }

    #[test]
    fn passwd_lines_give_uid_and_home_and_skip_the_rest() {
        let text = "root:x:0:0:root:/root:/bin/bash\nana:x:1000:1000:Ana,,,:/home/ana:/bin/bash\nnobody:x:65534:65534::/nonexistent:/usr/sbin/nologin\nbroken\n";
        assert_eq!(
            parse_passwd(text),
            [(HelperUser::Uid(0), "/root".into()), (HelperUser::Uid(1000), "/home/ana".into())]
        );
    }

    #[test]
    fn dscl_output_joins_homes_with_ids() {
        let homes = "_mdnsresponder          /var/empty\nana                     /Users/ana\nben                     /Users/Ben Smith\n";
        let ids = "_mdnsresponder          65\nana                     501\nben                     502\n";
        assert_eq!(
            parse_dscl_homes(homes, ids),
            [
                (HelperUser::Uid(501), "/Users/ana".into()),
                (HelperUser::Uid(502), "/Users/Ben Smith".into())
            ]
        );
    }

    #[test]
    fn the_profile_list_gives_a_home_per_sid() {
        let text = "\nHKEY_LOCAL_MACHINE\\SOFTWARE\\Microsoft\\Windows NT\\CurrentVersion\\ProfileList\\S-1-5-18\n    ProfileImagePath    REG_EXPAND_SZ    %systemroot%\\system32\\config\\systemprofile\n\nHKEY_LOCAL_MACHINE\\SOFTWARE\\Microsoft\\Windows NT\\CurrentVersion\\ProfileList\\S-1-5-21-1-2-3-1001\n    Flags    REG_DWORD    0x0\n    ProfileImagePath    REG_EXPAND_SZ    C:\\Users\\Ana Lee\n";
        let got = parse_profile_list(text);
        assert_eq!(got.len(), 2);
        assert_eq!(got[1], (HelperUser::Sid("S-1-5-21-1-2-3-1001".into()), r"C:\Users\Ana Lee".into()));
    }

    #[test]
    fn homes_come_from_the_hosts_own_list_and_a_failing_list_is_an_error() {
        let mac = SystemSweeper::new(
            HelperHost::Macos,
            Box::new(Fake(vec![("NFSHomeDirectory", "ana /Users/ana\n"), ("UniqueID", "ana 501\n")])),
        );
        assert_eq!(mac.homes().unwrap(), [(HelperUser::Uid(501), "/Users/ana".into())]);
        let win = SystemSweeper::new(HelperHost::Windows, Box::new(Fake(vec![])));
        assert!(win.homes().is_err());
        assert!(win.sweep().is_err(), "no sweep without the account list");
    }
}
