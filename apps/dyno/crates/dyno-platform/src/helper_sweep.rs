//! The helper's sweep of every process (ADR 0027 WP-5): `sysinfo` reads each process once, the owner is classified by id
//! ([`classify_owner`]), and every account's home folder comes from the OS's own account list: `/etc/passwd` on Linux,
//! `dscl . -list /Users NFSHomeDirectory` on macOS, the `ProfileList` registry key (through `reg query`) on Windows. The home
//! parsers are pure and compile everywhere; only allow-listed fields are read, so there is nothing to leak.

use crate::runner::ToolRunner;
use dyno_core::helper::HelperHost;
use dyno_core::helper_processes::{classify_owner, default_first_person, ProcState, ProcessSweeper, RawOwner, RawProcess, Sweep};
use dyno_core::helper_wire::HelperUser;
use dyno_core::ports::PlatformError;
use std::path::PathBuf;
use std::sync::Mutex;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
use sysinfo::{ProcessRefreshKind, ProcessStatus, ProcessesToUpdate, System, UpdateKind, Users};

const TIMEOUT: Duration = Duration::from_secs(10);
const DSCL: &str = "/usr/bin/dscl";
const REG: &str = r"C:\Windows\System32\reg.exe";
const PROFILE_LIST: &str = r"HKLM\SOFTWARE\Microsoft\Windows NT\CurrentVersion\ProfileList";
/// The most account homes read (a bound on a hostile or huge directory).
const MAX_HOMES: usize = 4_096;
/// How long the account list is reused at most (ADR 0108 DY-2), the outer bound of the cache. A sweep that finds a person's
/// process whose account the kept list does not name reads the list again at once ([`SystemSweeper::sweep`]), so a new account's
/// own processes are always redacted for others; on macOS a change of the account database's folder also ends the cache. What
/// this bounds is the rare rest: a path in the home of an account that has no process of its own yet, made within this time.
pub const HOMES_TTL: Duration = Duration::from_secs(60);
/// How long an owner still missing after a fresh read is not looked for again: a safety net for kinds `classify_owner` does not
/// know, short so an account in its first-logon race is looked for again within seconds, not after the whole TTL.
pub const NO_HOME_TTL: Duration = Duration::from_secs(5);
/// The local account database of macOS (one plist per account): its modification time changes when an account is added or
/// removed. The helper runs as root and can read it; when it cannot, [`HOMES_TTL`] alone bounds the cache.
const DSLOCAL_USERS: &str = "/var/db/dslocal/nodes/Default/users";

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
    account_db: PathBuf,
    homes: Mutex<Option<CachedHomes>>,
}

/// The account list as last read: when, the account database's modification time then, and the list.
struct CachedHomes {
    at: Instant,
    db_modified: Option<SystemTime>,
    homes: Vec<(HelperUser, String)>,
    /// Owners this list was read again for and still did not name, with when: a safety net for kinds `classify_owner` does not
    /// know (the known homeless kinds are services there), so they do not force a read on every sweep, for [`NO_HOME_TTL`] only.
    no_home: Vec<(HelperUser, Instant)>,
}

impl SystemSweeper {
    /// A sweeper for `host` reading its account list through `runner`.
    pub fn new(host: HelperHost, runner: Box<dyn ToolRunner>) -> SystemSweeper {
        SystemSweeper {
            host,
            runner,
            account_db: PathBuf::from(DSLOCAL_USERS),
            homes: Mutex::new(None),
        }
    }

    /// The same, watching `dir` instead of macOS' account database for changes (tests).
    #[cfg(test)]
    pub(crate) fn with_account_db(mut self, dir: &std::path::Path) -> SystemSweeper {
        self.account_db = dir.to_path_buf();
        self
    }

    /// Every account's home folder. An unreadable list is an error: with an incomplete list other accounts' paths would not be
    /// redacted, so no answer is better than a leaking one.
    pub fn homes(&self) -> Result<Vec<(HelperUser, String)>, PlatformError> {
        self.homes_at(Instant::now())
    }

    /// [`SystemSweeper::homes`] at `now`: on macOS and Windows, where the list costs program starts (`dscl` twice, `reg`), the
    /// last list is reused for [`HOMES_TTL`] while the account database is unchanged (ADR 0108 DY-2); Linux reads
    /// `/etc/passwd` each time (a file read). A failed read is not kept.
    fn homes_at(&self, now: Instant) -> Result<Vec<(HelperUser, String)>, PlatformError> {
        if self.host == HelperHost::Linux {
            return self.read_homes();
        }
        let db_modified = std::fs::metadata(&self.account_db).and_then(|m| m.modified()).ok();
        let cache = || self.homes.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        if let Some(c) = cache().as_ref() {
            if now.saturating_duration_since(c.at) < HOMES_TTL && c.db_modified == db_modified {
                return Ok(c.homes.clone());
            }
        }
        self.read_and_keep(now)
    }

    /// The account list for a sweep whose people's accounts are `owners`: the kept one when it names every owner, else read
    /// again once, past the cache (an account made since it was read, which Windows cannot see in an account database and macOS
    /// misses when a record is edited in place). Paths in an owner's home that the list does not name would reach another
    /// account unredacted.
    fn homes_for(&self, owners: &[&HelperUser], now: Instant) -> Result<Vec<(HelperUser, String)>, PlatformError> {
        let homes = self.homes_at(now)?;
        if self.host == HelperHost::Linux {
            return Ok(homes);
        }
        let known_without = |o: &HelperUser| {
            self.homes
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .as_ref()
                .is_some_and(|c| {
                    c.no_home
                        .iter()
                        .any(|(u, at)| u == o && now.saturating_duration_since(*at) < NO_HOME_TTL)
                })
        };
        let missing: Vec<&HelperUser> = owners
            .iter()
            .copied()
            .filter(|o| !homes.iter().any(|(u, _)| u == *o) && !known_without(o))
            .collect();
        if missing.is_empty() {
            return Ok(homes);
        }
        log::info!(missing = missing.len(); "Helper: a process owner is not in the kept account list; it is read again");
        let homes = self.read_and_keep(now)?;
        let still: Vec<HelperUser> = missing
            .into_iter()
            .filter(|o| !homes.iter().any(|(u, _)| u == *o))
            .cloned()
            .collect();
        if !still.is_empty() {
            log::info!(owners = still.len(); "Helper: process owners with no home in the account list; not read again for them");
            if let Some(c) = self.homes.lock().unwrap_or_else(std::sync::PoisonError::into_inner).as_mut() {
                c.no_home.retain(|(u, _)| !still.contains(u));
                c.no_home.extend(still.into_iter().map(|u| (u, now)));
                c.no_home.truncate(MAX_HOMES);
            }
        }
        Ok(homes)
    }

    fn read_and_keep(&self, now: Instant) -> Result<Vec<(HelperUser, String)>, PlatformError> {
        let db_modified = std::fs::metadata(&self.account_db).and_then(|m| m.modified()).ok();
        let homes = self.read_homes()?;
        log::debug!(accounts = homes.len(); "Helper: the account list was read");
        *self.homes.lock().unwrap_or_else(std::sync::PoisonError::into_inner) = Some(CachedHomes {
            at: now,
            db_modified,
            homes: homes.clone(),
            no_home: Vec::new(),
        });
        Ok(homes)
    }

    fn read_homes(&self) -> Result<Vec<(HelperUser, String)>, PlatformError> {
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
        let processes: Vec<RawProcess> = sys
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
        // The account list after the processes, so it can be checked to name every person who owns one.
        let owners: Vec<&HelperUser> = {
            let mut v: Vec<&HelperUser> = processes
                .iter()
                .filter_map(|p| match &p.owner {
                    RawOwner::Account { id, .. } => Some(id),
                    _ => None,
                })
                .collect();
            v.sort_by_key(|u| format!("{u:?}"));
            v.dedup();
            v
        };
        let homes = self.homes_for(&owners, Instant::now())?;
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

    /// DY-2 (ADR 0108, guard D): the account list is read once, not twice per sweep (every page of the Processes list and both
    /// sweeps of a Traffic answer took one): `dscl` starts twice on the first read, then not again until the list may have changed.
    #[test]
    fn the_account_list_is_read_once_and_again_when_it_may_have_changed() {
        use crate::runner::Counted;
        use paddock_proc::starts::{assert_starts, at_most_or_none};
        let dscl = || Counted(Fake(vec![("NFSHomeDirectory", "ana /Users/ana\n"), ("UniqueID", "ana 501\n")]));
        let db = paddock_testkit::private_tempdir().unwrap();
        let mac = SystemSweeper::new(HelperHost::Macos, Box::new(dscl())).with_account_db(db.path());
        let start = Instant::now();
        let first = assert_starts(2, || mac.homes_at(start)).unwrap();
        let again = assert_starts(at_most_or_none(0, "the list is cached"), || {
            mac.homes_at(start + Duration::from_secs(5))
        })
        .unwrap();
        assert_eq!(again, first);
        // An account added or removed changes the account database's folder: read again at once.
        std::fs::create_dir(db.path().join("new-account")).unwrap();
        let mtime = std::fs::metadata(db.path()).unwrap().modified().unwrap();
        let bumped = filetime_plus(mtime);
        set_mtime(db.path(), bumped);
        assert_starts(2, || mac.homes_at(start + Duration::from_secs(6))).unwrap();
        assert_starts(at_most_or_none(0, "the list is cached"), || {
            mac.homes_at(start + Duration::from_secs(7))
        })
        .unwrap();
        // And at the latest after HOMES_TTL.
        assert_starts(2, || mac.homes_at(start + Duration::from_secs(7) + HOMES_TTL)).unwrap();
        // A failed read is not cached.
        let failing = SystemSweeper::new(HelperHost::Macos, Box::new(Counted(Fake(vec![])))).with_account_db(db.path());
        assert_starts(1, || failing.homes_at(start)).unwrap_err();
        assert_starts(1, || failing.homes_at(start)).unwrap_err();
    }

    /// A dscl whose list changes between reads: the first pair of runs lists `before`, every later pair `after` (an account
    /// made in between).
    struct Changing {
        runs: std::sync::atomic::AtomicUsize,
        before: String,
        after: String,
    }
    impl ToolRunner for Changing {
        fn run_tool(&self, t: &Tool, _: Duration) -> Result<ToolOutput, PlatformError> {
            let _counted = crate::process::command(&t.program);
            let n = self.runs.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            let list = if n < 2 { &self.before } else { &self.after };
            let homes = t.args.iter().any(|a| a == "NFSHomeDirectory");
            let text = list
                .lines()
                .map(|l| {
                    let (name, uid) = l.split_once(' ').unwrap();
                    if homes {
                        format!("{name} /Users/{name}\n")
                    } else {
                        format!("{name} {uid}\n")
                    }
                })
                .collect();
            Ok(ToolOutput {
                ok: true,
                stdout: text,
                error: String::new(),
            })
        }
    }

    /// P1 of the DY-2 review (redaction): a person whose account is not in the kept list (made since it was read; Windows has
    /// no account database to watch, and macOS misses an edit in place) makes the list be read again at once, within the TTL,
    /// so their paths are never shown to another account unredacted.
    #[test]
    fn an_owner_missing_from_the_kept_list_reads_it_again_at_once() {
        use paddock_proc::starts::{assert_starts, at_most_or_none};
        let db = paddock_testkit::private_tempdir().unwrap();
        let mac = SystemSweeper::new(
            HelperHost::Macos,
            Box::new(Changing {
                runs: Default::default(),
                before: "ana 501".into(),
                after: "ana 501\nnew 502".into(),
            }),
        )
        .with_account_db(db.path());
        let ana = HelperUser::Uid(501);
        let new = HelperUser::Uid(502);
        let start = Instant::now();
        assert_starts(2, || mac.homes_for(&[&ana], start)).unwrap();
        assert_starts(at_most_or_none(0, "every owner is in the kept list"), || {
            mac.homes_for(&[&ana], start)
        })
        .unwrap();
        let homes = assert_starts(2, || mac.homes_for(&[&ana, &new], start + Duration::from_secs(1))).unwrap();
        assert!(homes.iter().any(|(u, h)| *u == new && h == "/Users/new"), "{homes:?}");
        // The new list is kept: the next sweep with both is free.
        assert_starts(at_most_or_none(0, "the re-read list is kept"), || {
            mac.homes_for(&[&ana, &new], start + Duration::from_secs(2))
        })
        .unwrap();
        // An owner still missing after a fresh read (a Hyper-V or IIS AppPool account, `nobody`) is remembered for the life of
        // that list: one re-read, then none (second review P2-b).
        let ghost = HelperUser::Uid(777);
        assert_starts(2, || mac.homes_for(&[&ghost], start + Duration::from_secs(3))).unwrap();
        assert_starts(at_most_or_none(0, "the owner is known to have no home in this list"), || {
            mac.homes_for(&[&ghost], start + Duration::from_secs(4))
        })
        .unwrap();
        // Only for NO_HOME_TTL, a safety net: an account in its first-logon race is looked for again soon (third review).
        assert_starts(2, || {
            mac.homes_for(&[&ghost], start + Duration::from_secs(3) + NO_HOME_TTL + Duration::from_millis(1))
        })
        .unwrap();
    }

    /// A `reg` whose second answer adds a profile (an account made in between).
    struct ChangingReg(std::sync::atomic::AtomicUsize);
    impl ToolRunner for ChangingReg {
        fn run_tool(&self, t: &Tool, _: Duration) -> Result<ToolOutput, PlatformError> {
            let _counted = crate::process::command(&t.program);
            let n = self.0.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            let key = |sid: &str, home: &str| {
                format!("HKEY_LOCAL_MACHINE\\SOFTWARE\\Microsoft\\Windows NT\\CurrentVersion\\ProfileList\\{sid}\n    ProfileImagePath    REG_EXPAND_SZ    {home}\n\n")
            };
            let mut text = key("S-1-5-21-1-2-3-1001", r"C:\Users\Ana");
            if n > 0 {
                text.push_str(&key("S-1-5-21-1-2-3-1002", r"C:\Users\New"));
            }
            Ok(ToolOutput {
                ok: true,
                stdout: text,
                error: String::new(),
            })
        }
    }

    /// Second review P2-a: the P1 fix on a Windows host, where no account database tells the cache it is stale: an owner with a
    /// new profile's SID forces exactly one `reg` run, and its home is in the list. Pure, so it runs on every OS.
    #[test]
    fn a_windows_owner_missing_from_the_kept_profile_list_reads_it_again_once() {
        use paddock_proc::starts::{assert_starts, at_most_or_none};
        let win = SystemSweeper::new(HelperHost::Windows, Box::new(ChangingReg(Default::default())));
        let ana = HelperUser::Sid("S-1-5-21-1-2-3-1001".into());
        let new = HelperUser::Sid("S-1-5-21-1-2-3-1002".into());
        let start = Instant::now();
        assert_starts(1, || win.homes_for(&[&ana], start)).unwrap();
        let homes = assert_starts(1, || win.homes_for(&[&ana, &new], start + Duration::from_secs(1))).unwrap();
        assert!(homes.iter().any(|(u, h)| *u == new && h == r"C:\Users\New"), "{homes:?}");
        assert_starts(at_most_or_none(0, "the re-read list names both"), || {
            win.homes_for(&[&ana, &new], start + Duration::from_secs(2))
        })
        .unwrap();
    }

    /// The same through a real sweep: this test's own account owns this process; a kept list without it is read again.
    #[cfg(target_os = "macos")]
    #[test]
    fn a_sweep_with_an_owner_missing_from_the_list_reads_the_list_again() {
        let me = std::process::id();
        let uid = nix_uid();
        assert!(
            uid >= 500,
            "precondition: the test runs as a person's account (uid {uid}), which macOS numbers from 501"
        );
        let db = paddock_testkit::private_tempdir().unwrap();
        let mac = SystemSweeper::new(
            HelperHost::Macos,
            Box::new(Changing {
                runs: Default::default(),
                before: "someone 9999".into(),
                after: format!("someone 9999\nme {uid}"),
            }),
        )
        .with_account_db(db.path());
        let sweep = paddock_proc::starts::assert_starts(4, || mac.sweep()).unwrap();
        assert!(
            sweep.processes.iter().any(|p| p.pid == me),
            "precondition: the sweep sees this process"
        );
        assert!(sweep.homes.iter().any(|(u, _)| *u == HelperUser::Uid(uid)), "{:?}", sweep.homes);
    }

    #[cfg(target_os = "macos")]
    fn nix_uid() -> u32 {
        // SAFETY: getuid(2) cannot fail and has no preconditions.
        #[allow(unsafe_code)]
        unsafe {
            libc::getuid()
        }
    }

    fn filetime_plus(t: SystemTime) -> SystemTime {
        t + Duration::from_secs(10)
    }

    /// Sets a folder's modification time. Windows needs write-attributes access and backup semantics to open a folder for
    /// that (as the filetime crate does); a read-only open there is "Access is denied" (ci.yml 8359, dyno windows-latest).
    fn set_mtime(p: &std::path::Path, t: SystemTime) {
        let mut open = std::fs::OpenOptions::new();
        #[cfg(windows)]
        {
            use std::os::windows::fs::OpenOptionsExt;
            const FILE_WRITE_ATTRIBUTES: u32 = 0x0100;
            const FILE_FLAG_BACKUP_SEMANTICS: u32 = 0x0200_0000;
            open.access_mode(FILE_WRITE_ATTRIBUTES).custom_flags(FILE_FLAG_BACKUP_SEMANTICS);
        }
        #[cfg(not(windows))]
        open.read(true);
        open.open(p).unwrap().set_modified(t).unwrap();
    }
}
