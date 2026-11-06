#![allow(clippy::unwrap_used, clippy::expect_used)]

//! The extras (WP-13) end to end over the fake platform: scripted tools
//! (no real `pmset` or `powercfg` is run), a real IPC connection and a SQLite file in a temp folder.

use dyno_agent::cli::{ask, ask_within, connect, run_job};
use dyno_agent::paths::Dirs;
use dyno_agent::Running;
use dyno_core::extras::changes::{ChangeKind, ChangesQuery};
use dyno_core::extras::files::{FinderMode, FinderQuery, FinderResult};
use dyno_core::jobs::{JobOutput, JobSpec};
use dyno_core::ports::PlatformError;
use dyno_core::protocol::{Message, Reply, Request};
use dyno_core::time::{Timestamp, HOUR};
use dyno_platform::fake::{process, Fake};
use dyno_platform::runner::ToolRunner;
use paddock_ipc::BodyPeer;
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
    fn run_tool(&self, tool: &dyno_platform::runner::Tool, _timeout: Duration) -> Result<dyno_platform::runner::ToolOutput, PlatformError> {
        let (program, args) = (tool.program.as_str(), &tool.args);
        let answer: Result<String, PlatformError> = {
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
        };
        answer.map(|stdout| dyno_platform::runner::ToolOutput {
            ok: true,
            stdout,
            error: String::new(),
        })
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

    fn client(&self) -> BodyPeer<Message> {
        connect(&self.dirs).unwrap().0
    }

    /// Stops the agent and starts another on the same data folder, as a login or an update does.
    fn restart(&mut self) {
        self.running.take().unwrap().stop();
        let fake = Fake::new();
        fake.add_process(process(100, "calm", 1.0));
        self.running = Some(Running::start(self.dirs.clone(), fake.platform(), Box::new(fake.clone()), Box::new(|_| Ok(()))).unwrap());
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
    let JobOutput::Sleep(s) = run_job(&h.client(), JobSpec::SleepBlockers, Duration::from_secs(30)).unwrap() else {
        panic!("sleep")
    };
    assert_eq!(s.blockers[0].name, "Firefox");
    assert!(s.wakes_unavailable.is_some());
    x.use_tools(Tools::with(vec![]));
    let JobOutput::Sleep(s) = run_job(&h.client(), JobSpec::SleepBlockers, Duration::from_secs(30)).unwrap() else {
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

/// Runs the finder as a job and waits for its result.
fn find(peer: &BodyPeer<Message>, query: FinderQuery) -> Result<FinderResult, String> {
    match run_job(peer, JobSpec::FindFiles { query }, Duration::from_secs(60))? {
        JobOutput::Finder(r) => Ok(*r),
        other => panic!("{other:?}"),
    }
}

#[test]
fn the_finder_lists_large_files_and_duplicates_and_moves_nothing() {
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
    let r = find(&peer, query).unwrap();
    assert_eq!(r.large[0].bytes, 4 * MB as u64);
    assert_eq!(r.duplicates.len(), 1);
    assert_eq!(r.duplicates[0].files.len(), 2);
    assert!(a.exists() && b.exists(), "finding moves nothing");
}

#[test]
fn the_finder_refuses_a_folder_that_is_not_there() {
    let h = Harness::start();
    let peer = h.client();
    let missing = std::env::temp_dir().join("dyno-definitely-not-here");
    let q = FinderQuery {
        folder: missing.to_string_lossy().into_owned(),
        mode: FinderMode::Both,
        min_bytes: 0,
    };
    assert!(find(&peer, q).unwrap_err().contains("cannot be opened"));
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
    let JobOutput::Boot(b) = run_job(&h.client(), JobSpec::BootAnalysis, Duration::from_secs(30)).unwrap() else {
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
    let JobOutput::Software(sw) = run_job(&peer, JobSpec::SoftwareInventory, Duration::from_secs(30)).unwrap() else {
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

/// ADR 0029 (D7): the software list read in one run is shown at once, with its time, after the agent restarts, while the system
/// is read again in the background; the next ask has the fresh list.
#[test]
fn the_software_list_is_saved_and_shown_at_once_after_a_restart() {
    let mut h = Harness::start();
    let first = |h: &Harness, tools: Arc<Tools>| {
        let s = h.shared();
        s.extras.use_os("linux");
        s.extras.use_tools(tools);
        let JobOutput::Software(sw) = run_job(&h.client(), JobSpec::SoftwareInventory, Duration::from_secs(30)).unwrap() else {
            panic!("software")
        };
        (sw, s)
    };
    let (live, s) = first(&h, Tools::with(vec![("dpkg-query -W", vec![ok("bash\t5.2\ncurl\t8.4\n")])]));
    assert_eq!((live.items.len(), live.saved_at), (2, None), "read just now");
    s.cache.as_ref().unwrap().flush();
    drop(s);
    h.restart();
    let (saved, s) = first(
        &h,
        Tools::with(vec![("dpkg-query -W", vec![ok("bash\t5.2\ncurl\t8.4\ngit\t2.43\n")])]),
    );
    assert_eq!(saved.items.len(), 2, "the saved list answers at once");
    assert!(saved.saved_at.is_some_and(|t| t.millis() > 0), "with the time it was saved");
    // The background read ends (joined, not polled) and the next ask has the fresh list.
    s.extras.wait_background_software();
    let JobOutput::Software(fresh) = run_job(&h.client(), JobSpec::SoftwareInventory, Duration::from_secs(30)).unwrap() else {
        panic!("software")
    };
    assert_eq!(fresh.saved_at, None);
    assert_eq!(fresh.items.len(), 3);
    s.cache.as_ref().unwrap().flush();
    let (_, _, data) = s.cache.as_ref().unwrap().view("extras.software").unwrap();
    assert!(data.contains("git"), "kept for the next run");
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
    let JobOutput::AppUsage(u) = run_job(&peer, JobSpec::AppUsage, Duration::from_secs(30)).unwrap() else {
        panic!("usage")
    };
    assert!(u.energy_unavailable.is_some());
}
