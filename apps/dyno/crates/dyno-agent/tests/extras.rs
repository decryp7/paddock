#![allow(clippy::unwrap_used, clippy::expect_used)]

//! The extras (WP-13), backup health (WP-14) and taming proof (WP-36) end to end over the fake platform: scripted tools
//! (no real `tmutil`, `pmset` or `restic` is run), a real IPC connection and a SQLite file in a temp folder.

use dyno_agent::cli::{ask, ask_within, connect};
use dyno_agent::paths::Dirs;
use dyno_agent::Running;
use dyno_core::backup::{BackupState, BackupTool};
use dyno_core::cleanup::DeleteMode;
use dyno_core::extras::changes::{ChangeKind, ChangesQuery};
use dyno_core::extras::files::{FinderMode, FinderQuery};
use dyno_core::ports::PlatformError;
use dyno_core::protocol::{Message, Reply, Request};
use dyno_core::rules::{Command, RuleSet};
use dyno_core::time::{Timestamp, HOUR};
use dyno_ipc::Peer;
use dyno_platform::fake::{process, Fake};
use dyno_platform::issues::ToolRunner;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::Duration;

/// Answers by program name (and optionally first argument), cycling through the scripted answers; records every call.
#[derive(Default)]
struct Tools {
    answers: Mutex<HashMap<String, Vec<Result<String, PlatformError>>>>,
    calls: Mutex<Vec<String>>,
}

impl Tools {
    fn with(list: Vec<(&str, Vec<Result<String, PlatformError>>)>) -> Arc<Tools> {
        let t = Tools::default();
        for (k, v) in list {
            t.answers.lock().unwrap().insert(k.to_string(), v);
        }
        Arc::new(t)
    }
}

impl ToolRunner for Tools {
    fn run(&self, program: &str, args: &[String], _timeout: Duration) -> Result<String, PlatformError> {
        let key = format!(
            "{} {}",
            program.rsplit('/').next().unwrap_or(program),
            args.first().map(String::as_str).unwrap_or("")
        );
        self.calls.lock().unwrap().push(key.trim().to_string());
        let mut answers = self.answers.lock().unwrap();
        let bare = program.rsplit('/').next().unwrap_or(program).to_string();
        let name = if answers.contains_key(key.trim()) {
            key.trim().to_string()
        } else {
            bare
        };
        let hit = answers.get_mut(&name);
        match hit {
            Some(v) if v.len() > 1 => v.remove(0),
            Some(v) => v
                .first()
                .cloned()
                .unwrap_or_else(|| Err(PlatformError::Failed("no scripted answer".into()))),
            None => Err(PlatformError::Unsupported(format!("{program} is not available on this computer."))),
        }
    }
}

struct Harness {
    running: Option<Running>,
    dirs: Dirs,
    _tmp: tempfile::TempDir,
}

impl Harness {
    fn start() -> Harness {
        let tmp = tempfile::tempdir().unwrap();
        let dirs = Dirs::at(tmp.path().join("data"));
        let fake = Fake::new();
        fake.add_process(process(100, "calm", 1.0));
        let running = Running::start(dirs.clone(), fake.platform(), Box::new(fake.clone()), Box::new(|_| Ok(()))).unwrap();
        Harness {
            running: Some(running),
            dirs,
            _tmp: tmp,
        }
    }

    fn client(&self) -> Peer<Message> {
        connect(&self.dirs).unwrap().0
    }

    fn shared(&self) -> Arc<dyno_agent::agent::Shared> {
        self.running.as_ref().unwrap().shared.clone()
    }
}

impl Drop for Harness {
    fn drop(&mut self) {
        if let Some(r) = self.running.take() {
            r.stop();
        }
    }
}

fn ok(s: &str) -> Result<String, PlatformError> {
    Ok(s.to_string())
}

fn real_now() -> Timestamp {
    Timestamp(
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_millis() as i64,
    )
}

#[test]
fn sleep_blockers_come_from_the_scripted_tool_and_a_refusal_is_a_sentence() {
    let h = Harness::start();
    let x = &h.shared().extras;
    x.use_os("linux");
    x.use_tools(Tools::with(vec![(
        "systemd-inhibit",
        vec![ok(
            "WHO UID USER PID COMM WHAT WHY MODE\nFirefox 1000 sam 2210 firefox sleep Playing audio block\n",
        )],
    )]));
    let Reply::Sleep(s) = ask(&h.client(), Request::SleepBlockers).unwrap() else {
        panic!("sleep")
    };
    assert_eq!(s.blockers[0].name, "Firefox");
    assert!(s.wakes_unavailable.is_some());
    x.use_tools(Tools::with(vec![]));
    let Reply::Sleep(s) = ask(&h.client(), Request::SleepBlockers).unwrap() else {
        panic!("sleep")
    };
    assert!(s.blockers.is_empty() && s.blockers_unavailable.unwrap().contains("not available"));
}

#[test]
fn what_changed_compares_software_lists_and_records_the_baseline_silently() {
    let h = Harness::start();
    let s = h.shared();
    s.extras.use_os("macos");
    let list =
        |v: &str| format!(r#"{{"SPApplicationsDataType":[{{"_name":"Editor","version":"{v}"}},{{"_name":"Stable","version":"1"}}]}}"#);
    s.extras.use_tools(Tools::with(vec![(
        "system_profiler SPApplicationsDataType",
        vec![ok(&list("1.0"))],
    )]));
    dyno_agent::extras::record_changes(&s);
    let peer = h.client();
    let q = ChangesQuery { around: None, hours: 24 };
    let Reply::Changes(t) = ask(&peer, Request::Changes { query: q }).unwrap() else {
        panic!("changes")
    };
    assert!(
        t.changes.iter().all(|c| c.kind != ChangeKind::Installed),
        "a baseline is not a change"
    );
    assert!(t.watching_since.is_some());
    // Editor updates and a new program appears.
    let newer = r#"{"SPApplicationsDataType":[{"_name":"Editor","version":"2.0"},{"_name":"Stable","version":"1"},{"_name":"Fresh","version":"3"}]}"#.to_string();
    s.extras
        .use_tools(Tools::with(vec![("system_profiler SPApplicationsDataType", vec![ok(&newer)])]));
    dyno_agent::extras::record_changes(&s);
    let Reply::Changes(t) = ask(&peer, Request::Changes { query: q }).unwrap() else {
        panic!("changes")
    };
    let titles: Vec<&str> = t.changes.iter().map(|c| c.title.as_str()).collect();
    assert!(
        titles.contains(&"Installed Fresh") && titles.contains(&"Updated Editor"),
        "{titles:?}"
    );
    // A different window around an old moment finds nothing.
    let old = ChangesQuery {
        around: Some(real_now().minus(100 * 24 * HOUR)),
        hours: 1,
    };
    let Reply::Changes(t) = ask(&peer, Request::Changes { query: old }).unwrap() else {
        panic!("changes")
    };
    assert!(t.changes.is_empty());
}

fn write(dir: &std::path::Path, name: &str, byte: u8, len: usize) -> std::path::PathBuf {
    let p = dir.join(name);
    std::fs::create_dir_all(p.parent().unwrap()).unwrap();
    std::fs::write(&p, vec![byte; len]).unwrap();
    p
}

#[test]
fn the_finder_lists_removes_to_the_quarantine_and_never_removes_every_copy() {
    let h = Harness::start();
    let peer = h.client();
    let t = tempfile::tempdir().unwrap();
    let root = std::fs::canonicalize(t.path()).unwrap().join("stuff");
    const MB: usize = 1024 * 1024;
    let a = write(&root, "a/movie.mkv", 5, 2 * MB);
    let b = write(&root, "b/movie copy.mkv", 5, 2 * MB);
    write(&root, "big.iso", 1, 4 * MB);
    let query = FinderQuery {
        folder: root.to_string_lossy().into_owned(),
        mode: FinderMode::Both,
        min_bytes: 0,
    };
    let Reply::FileFinder(r) = ask_within(&peer, Request::FindFiles { query }, Duration::from_secs(60)).unwrap() else {
        panic!("finder")
    };
    assert_eq!(r.large[0].bytes, 4 * MB as u64);
    assert_eq!(r.duplicates.len(), 1);
    let ids: Vec<String> = r.duplicates[0].files.iter().map(|f| f.id.clone()).collect();

    // Removing both copies is refused as a whole; nothing moves.
    let both = Request::RemoveFoundFiles {
        scan_id: r.scan_id.clone(),
        ids: ids.clone(),
        mode: DeleteMode::Quarantine,
    };
    let err = ask(&peer, both).unwrap_err();
    assert!(err.contains("every copy"), "{err}");
    assert!(a.exists() && b.exists());

    // An unknown result is refused.
    assert!(ask(
        &peer,
        Request::RemoveFoundFiles {
            scan_id: "nope".into(),
            ids: ids.clone(),
            mode: DeleteMode::Quarantine
        }
    )
    .is_err());

    // One copy goes to the quarantine, is logged, and can be put back from Activity's quarantine.
    let one = Request::RemoveFoundFiles {
        scan_id: r.scan_id.clone(),
        ids: vec![ids[1].clone()],
        mode: DeleteMode::Quarantine,
    };
    let Reply::Outcome(o) = ask(&peer, one).unwrap() else {
        panic!("outcome")
    };
    assert_eq!(o.deleted, 1, "{:?}", o.failed);
    let q = o.quarantine_id.unwrap();
    let remaining = [&a, &b].iter().filter(|p| p.exists()).count();
    assert_eq!(remaining, 1);
    let Reply::Activities(log) = ask(&peer, Request::Activity { limit: 20 }).unwrap() else {
        panic!("activity")
    };
    assert!(log.iter().any(|e| e.title.starts_with("Removed 1 file")), "{log:?}");
    assert!(ask(&peer, Request::RestoreQuarantine { id: q }).is_ok());
    assert!(a.exists() && b.exists());
}

#[test]
fn the_finder_refuses_a_folder_that_is_not_there_and_stale_selections() {
    let h = Harness::start();
    let peer = h.client();
    let q = FinderQuery {
        folder: "/definitely/not/here".into(),
        mode: FinderMode::Both,
        min_bytes: 0,
    };
    assert!(ask(&peer, Request::FindFiles { query: q })
        .unwrap_err()
        .contains("cannot be opened"));
    assert!(ask(
        &peer,
        Request::RemoveFoundFiles {
            scan_id: "x".into(),
            ids: vec![],
            mode: DeleteMode::RecycleBin
        }
    )
    .is_err());
}

fn backup_report(peer: &Peer<Message>, refresh: bool) -> dyno_core::backup::BackupReport {
    let req = if refresh { Request::RefreshBackup } else { Request::Backup };
    let Reply::Backup(mut r) = ask(peer, req).unwrap() else {
        panic!("backup")
    };
    for _ in 0..200 {
        if !r.checking {
            return *r;
        }
        std::thread::sleep(Duration::from_millis(50));
        let Reply::Backup(next) = ask(peer, Request::Backup).unwrap() else {
            panic!("backup")
        };
        r = next;
    }
    panic!("the backup check never finished")
}

#[test]
fn a_stale_backup_raises_an_alarm_and_a_fresh_one_clears_it() {
    let h = Harness::start();
    let s = h.shared();
    s.extras.use_os("macos");
    let old = "/Volumes/.timemachine/X/2026-01-01-000000.backup\n";
    s.extras.use_backup_tools(Tools::with(vec![
        ("tmutil destinationinfo", vec![ok("Name : Disk\n")]),
        ("tmutil latestbackup", vec![ok(old)]),
    ]));
    let peer = h.client();
    // The first request starts the check; it is read-only and finishes in the background.
    let r = backup_report(&peer, false);
    let tm = r.statuses.iter().find(|x| x.tool == BackupTool::TimeMachine).unwrap();
    assert_eq!(tm.state, BackupState::Stale);
    assert!(r.checked_at.is_some());
    // Duplicati and the others are "cannot check"/errors from the scripted runner, never a failure of the request.
    let Reply::Alarms(alarms) = ask(
        &peer,
        Request::Alarms {
            filter: Default::default(),
        },
    )
    .unwrap() else {
        panic!("alarms")
    };
    assert!(
        alarms.iter().any(|a| a.title.contains("Time Machine backup is out of date")),
        "{alarms:?}"
    );

    // A fresh backup clears it.
    let fresh = format!(
        "/Volumes/.timemachine/X/{}.backup\n",
        // Local time does not matter for "recent": the stale limit is 48 hours.
        "2099-01-01-000000"
    );
    s.extras.use_backup_tools(Tools::with(vec![
        ("tmutil destinationinfo", vec![ok("Name : Disk\n")]),
        ("tmutil latestbackup", vec![ok(&fresh)]),
    ]));
    let r = backup_report(&peer, true);
    assert_eq!(
        r.statuses.iter().find(|x| x.tool == BackupTool::TimeMachine).unwrap().state,
        BackupState::Healthy
    );
}

#[test]
fn background_alerts_can_be_switched_off() {
    let h = Harness::start();
    let s = h.shared();
    s.extras.use_os("macos");
    s.extras.use_backup_tools(Tools::with(vec![
        ("tmutil destinationinfo", vec![ok("Name : Disk\n")]),
        ("tmutil latestbackup", vec![ok("/x/2026-01-01-000000.backup\n")]),
    ]));
    let peer = h.client();
    let Reply::Settings(mut settings) = ask(&peer, Request::Settings).unwrap() else {
        panic!("settings")
    };
    settings.backup.alert_when_stale = false;
    ask(&peer, Request::SaveSettings { settings }).unwrap();
    backup_report(&peer, true);
    let Reply::Alarms(alarms) = ask(
        &peer,
        Request::Alarms {
            filter: Default::default(),
        },
    )
    .unwrap() else {
        panic!("alarms")
    };
    assert!(!alarms.iter().any(|a| a.title.contains("out of date")));
}

#[test]
fn maintenance_windows_are_validated_and_reported() {
    let h = Harness::start();
    let peer = h.client();
    let Reply::Settings(mut settings) = ask(&peer, Request::Settings).unwrap() else {
        panic!("settings")
    };
    settings.maintenance.enabled = true;
    let err = ask(
        &peer,
        Request::SaveSettings {
            settings: settings.clone(),
        },
    )
    .unwrap_err();
    assert!(err.contains("Add a window"), "{err}");
    settings.maintenance.windows = vec![dyno_core::extras::maintenance::Window {
        days: vec![
            dyno_core::extras::maintenance::Weekday::Mon,
            dyno_core::extras::maintenance::Weekday::Tue,
            dyno_core::extras::maintenance::Weekday::Wed,
            dyno_core::extras::maintenance::Weekday::Thu,
            dyno_core::extras::maintenance::Weekday::Fri,
            dyno_core::extras::maintenance::Weekday::Sat,
            dyno_core::extras::maintenance::Weekday::Sun,
        ],
        start_minute: 0,
        end_minute: 1439,
    }];
    ask(&peer, Request::SaveSettings { settings }).unwrap();
    let Reply::Maintenance(m) = ask(&peer, Request::Maintenance).unwrap() else {
        panic!("maintenance")
    };
    assert!(m.enabled);
    // An invalid backup override is refused with the engine's words.
    let Reply::Settings(mut settings) = ask(&peer, Request::Settings).unwrap() else {
        panic!("settings")
    };
    settings.backup.overrides = vec![dyno_core::backup::ToolOverride {
        tool: BackupTool::Restic,
        dirs: vec!["relative".into()],
    }];
    assert!(ask(&peer, Request::SaveSettings { settings }).unwrap_err().contains("restic"));
}

#[test]
fn a_temporary_rule_switches_itself_off_and_says_so() {
    let h = Harness::start();
    let peer = h.client();
    let Reply::Rules(mut set) = ask(&peer, Request::Rules).unwrap() else {
        panic!("rules")
    };
    let r = &mut set.rules[0];
    r.enabled = true;
    r.expires_at = Some(real_now().minus(1000));
    let id = r.id.clone();
    let set: RuleSet = *set;
    ask(&peer, Request::SaveRules { rules: set }).unwrap();
    let mut gone = false;
    for _ in 0..100 {
        let Reply::Rules(now_set) = ask(&peer, Request::Rules).unwrap() else {
            panic!("rules")
        };
        let rule = now_set.rules.iter().find(|r| r.id == id).unwrap();
        if !rule.enabled && rule.expires_at.is_none() {
            gone = true;
            break;
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    assert!(gone, "the expired rule is switched off");
    let Reply::Activities(log) = ask(&peer, Request::Activity { limit: 20 }).unwrap() else {
        panic!("activity")
    };
    assert!(
        log.iter().any(|e| e.title.contains("temporary rule") && e.title.contains("ended")),
        "{log:?}"
    );
}

#[test]
fn a_measured_effect_is_written_to_the_activity_log() {
    let h = Harness::start();
    let s = h.shared();
    dyno_agent::sampler::apply_commands(
        &s,
        vec![Command::Measured {
            rule_id: "r".into(),
            rule_name: "Calm builds".into(),
            app: "Chrome".into(),
            before: 78.0,
            after: 31.0,
            after_secs: 21,
            description: "lowered its priority".into(),
        }],
        real_now(),
    );
    let Reply::Activities(log) = ask(&h.client(), Request::Activity { limit: 20 }).unwrap() else {
        panic!("activity")
    };
    let e = log.iter().find(|e| e.title.starts_with("Measured")).unwrap();
    assert_eq!(e.title, "Measured: Chrome used 78% CPU before and 31% after 21 s");
    assert!(e.detail.contains("It helped"));
}

#[test]
fn boot_analysis_uses_the_stored_boots_and_says_what_it_cannot_know() {
    let h = Harness::start();
    let s = h.shared();
    s.extras.use_os("macos");
    s.extras.use_tools(Tools::with(vec![]));
    let t = real_now();
    for (i, secs) in [40.0f32, 42.0, 41.0, 90.0].iter().enumerate() {
        lock_put_boot(&s, t.minus((10 - i as i64) * 24 * HOUR), *secs);
    }
    let Reply::Boot(b) = ask(&h.client(), Request::BootAnalysis).unwrap() else {
        panic!("boot")
    };
    assert_eq!(b.boots.len(), 4);
    assert_eq!(b.boots[0].ready_secs, Some(90.0), "newest first");
    assert!(b.verdict.unwrap().contains("longer than the usual"));
    assert!(b.steps_unavailable.unwrap().contains("macOS"));
}

fn lock_put_boot(s: &dyno_agent::agent::Shared, at: Timestamp, ready: f32) {
    let core = dyno_agent::agent::lock(&s.core);
    core.store
        .put_boot(&dyno_core::protocol::BootRecord {
            at,
            boot_secs: Some(10.0),
            ready_secs: Some(ready),
        })
        .unwrap();
}

#[test]
fn the_inventory_report_is_written_only_to_a_valid_path() {
    let h = Harness::start();
    let s = h.shared();
    s.extras.use_os("linux");
    s.extras
        .use_tools(Tools::with(vec![("dpkg-query -W", vec![ok("bash\t5.2\ncurl\t8.4\n")])]));
    let peer = h.client();
    let Reply::Software(sw) = ask(&peer, Request::SoftwareInventory).unwrap() else {
        panic!("software")
    };
    assert_eq!(sw.items.len(), 2);
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("inv.md");
    let req = |p: &std::path::Path| Request::SaveInventoryReport {
        path: p.to_string_lossy().into_owned(),
        format: dyno_core::extras::software::InventoryFormat::Markdown,
    };
    let Reply::InventorySaved(saved) = ask_within(&peer, req(&path), Duration::from_secs(60)).unwrap() else {
        panic!("saved")
    };
    assert!(saved.bytes > 0);
    let text = std::fs::read_to_string(&path).unwrap();
    assert!(text.contains("| bash | 5.2 |"), "{text}");
    assert!(ask(&peer, req(&dir.path().join("inv.json"))).is_err(), "wrong extension");
    assert!(ask(&peer, req(&dir.path().join("missing/inv.md"))).is_err(), "folder must exist");
}

#[test]
fn gpu_and_usage_degrade_to_a_reason() {
    let h = Harness::start();
    let s = h.shared();
    s.extras.use_os("linux");
    s.extras.use_tools(Tools::with(vec![]));
    let peer = h.client();
    let Reply::Gpu(g) = ask(&peer, Request::GpuProcesses).unwrap() else {
        panic!("gpu")
    };
    assert!(g.processes.is_empty() && g.unavailable.is_some());
    let Reply::AppUsage(u) = ask(&peer, Request::AppUsage).unwrap() else {
        panic!("usage")
    };
    assert!(u.energy_unavailable.is_some());
}
