//! The pure parts of the launch: the command line, the sandbox decision, display check, theme and profiles (every OS).

use super::*;

fn opts(base: &Path) -> LaunchOptions {
    let mut o = LaunchOptions::new(Profile::Throwaway(base.to_path_buf()));
    o.window = Window::Headless;
    o
}

// ---- the command line ----

#[test]
fn keeps_the_automation_banner_and_hides_nothing() {
    let a = command_line(
        Flavor::Chromium,
        Path::new("/tmp/p"),
        &opts(Path::new("/b")),
        Sandbox::On,
    );
    assert!(a.contains(&"--enable-automation".to_string()));
    assert!(
        a.iter()
            .all(|x| !x.contains("AutomationControlled") && !x.contains("user-agent"))
    );
    assert!(a.contains(&"--user-data-dir=/tmp/p".to_string()));
    assert!(a.contains(&"--disable-extensions".to_string()));
}

#[test]
fn the_window_modes() {
    let with = |w: Window| {
        let mut o = opts(Path::new("/b"));
        o.window = w;
        command_line(Flavor::Chromium, Path::new("/p"), &o, Sandbox::On)
    };
    let placed = with(Window::OnScreen(Some(WindowBounds {
        x: -1800,
        y: 40,
        width: 0,
        height: 800,
    })));
    assert!(
        placed.contains(&"--window-position=-1800,40".to_string()),
        "{placed:?}"
    );
    assert!(placed.contains(&"--window-size=1,800".to_string()));
    let free = with(Window::OnScreen(None));
    assert!(
        !free
            .iter()
            .any(|x| x.starts_with("--window-position") || x.contains("headless"))
    );
    assert!(with(Window::OffScreen).contains(&"--window-position=-32000,-32000".to_string()));
    assert!(with(Window::Headless).contains(&"--headless=new".to_string()));
    for w in [Window::OnScreen(None), Window::OffScreen, Window::Headless] {
        assert_eq!(
            with(w)
                .iter()
                .filter(|a| a.starts_with("--window-size"))
                .count(),
            1
        );
    }
}

#[test]
fn the_start_page_is_last_for_every_combination_of_options() {
    for sandbox in [Sandbox::On, Sandbox::Off] {
        for quiet in [true, false] {
            for cache in [None, Some(99)] {
                for extra in [
                    vec![],
                    vec!["--lang=de".to_string(), START_PAGE.to_string()],
                ] {
                    let mut o = opts(Path::new("/b"));
                    (o.quiet, o.disk_cache_bytes, o.extra_args) = (quiet, cache, extra.clone());
                    let a = command_line(Flavor::Chromium, Path::new("/p"), &o, sandbox);
                    assert_eq!(a.last().map(String::as_str), Some(START_PAGE), "{a:?}");
                    assert_eq!(a.iter().filter(|x| *x == START_PAGE).count(), 1, "{a:?}");
                    assert_eq!(
                        a.contains(&"--no-sandbox".to_string()),
                        sandbox == Sandbox::Off
                    );
                    assert_eq!(
                        a.contains(&"--disable-dev-shm-usage".to_string()),
                        sandbox == Sandbox::Off
                    );
                    assert_eq!(
                        a.contains(&"--disk-cache-size=99".to_string()),
                        cache.is_some()
                    );
                    assert_eq!(a.contains(&"--disable-gpu".to_string()), quiet);
                    assert_eq!(a.contains(&"--lang=de".to_string()), !extra.is_empty());
                    assert!(
                        a.iter()
                            .filter(|x| x.starts_with("--disable-features="))
                            .count()
                            <= 1
                    );
                }
            }
        }
    }
}

/// A Chromium build (Chrome for Testing) starts without its field-trial testing config; branded Chrome and Edge never
/// apply it and get no switch. On Windows CI that config's network-service sandbox could not start for a browser in the
/// runner's user folder and Chrome started the network service a second time (all 12 Chrome logs of runs 7149 and 7154);
/// loads were lost in those runs (see [`NO_TESTING_FIELD_TRIALS`]). Every feature to turn off is in one
/// `--disable-features` switch, the caller's merged in without repeats.
#[test]
fn chromium_builds_start_without_testing_field_trials_and_one_feature_switch() {
    for quiet in [true, false] {
        for extra in [
            vec![],
            vec![
                "--disable-features=Foo,Translate".to_string(),
                "--lang=de".to_string(),
            ],
        ] {
            let mut o = opts(Path::new("/b"));
            (o.quiet, o.extra_args) = (quiet, extra.clone());
            for flavor in [Flavor::Chromium, Flavor::Chrome, Flavor::Edge] {
                let a = command_line(flavor, Path::new("/p"), &o, Sandbox::On);
                assert_eq!(
                    a.contains(&NO_TESTING_FIELD_TRIALS.to_string()),
                    flavor == Flavor::Chromium,
                    "{flavor:?}: {a:?}"
                );
                assert_eq!(a.last().map(String::as_str), Some(START_PAGE), "{a:?}");
                let switches: Vec<&String> = a
                    .iter()
                    .filter(|x| x.starts_with("--disable-features="))
                    .collect();
                assert!(switches.len() <= 1, "{a:?}");
                let features: Vec<&str> = switches
                    .first()
                    .map(|s| s["--disable-features=".len()..].split(',').collect())
                    .unwrap_or_default();
                assert_eq!(
                    features.contains(&"Translate"),
                    quiet || !extra.is_empty(),
                    "{a:?}"
                );
                assert_eq!(
                    features.contains(&"Foo"),
                    !extra.is_empty(),
                    "the caller's features are kept: {a:?}"
                );
                let mut unique = features.clone();
                unique.sort_unstable();
                unique.dedup();
                assert_eq!(unique.len(), features.len(), "no feature twice: {a:?}");
                assert_eq!(a.contains(&"--lang=de".to_string()), !extra.is_empty());
            }
        }
    }
}

/// The helper counts come from the browser's histograms; missing ones count nothing.
#[test]
fn helper_starts_are_read_from_the_histograms() {
    let launch = vec![
        json!({"name": "ChildProcess.Launched.UtilityProcessHash", "count": 3}),
        json!({"name": "ChildProcess.LaunchFailed.UtilityProcessErrorCode", "count": 1}),
        json!({"name": "ChildProcess.LaunchFailed.WinLastError", "count": 1}),
    ];
    let sandbox = vec![
        json!({"name": "Chrome.SystemNetworkContextManager.NetworkSandboxLaunchFailed.ErrorCode", "count": 1}),
        json!({"name": "Chrome.SystemNetworkContextManager.NetworkSandboxState", "count": 6,
               "buckets": [{"low": 1, "high": 2, "count": 3}, {"low": 4, "high": 5, "count": 3}]}),
    ];
    assert_eq!(
        HelperStarts::from_histograms(&launch, &sandbox),
        HelperStarts {
            started: 3,
            failed: 1,
            network_sandbox_failed: 1,
            network_sandbox_given_up: 3
        }
    );
    assert_eq!(
        HelperStarts::from_histograms(&[], &[]),
        HelperStarts::default()
    );
    let odd = vec![
        json!({"name": "Chrome.SystemNetworkContextManager.NetworkSandboxState"}),
        json!({"count": 9}),
        json!(null),
    ];
    assert_eq!(
        HelperStarts::from_histograms(&odd, &odd),
        HelperStarts::default()
    );
}

#[test]
fn the_sandbox_is_dropped_only_for_root_that_asked_for_it() {
    assert_eq!(Sandbox::decide(Some("1"), true), Sandbox::Off);
    assert_eq!(
        Sandbox::decide(Some("1"), false),
        Sandbox::On,
        "a user's browser keeps its sandbox"
    );
    assert_eq!(
        Sandbox::decide(None, true),
        Sandbox::On,
        "root without the opt-in is not changed"
    );
    assert_eq!(Sandbox::decide(Some(""), true), Sandbox::On);
    assert_eq!(Sandbox::decide(Some("0"), true), Sandbox::On);
    let here = Sandbox::from_environment();
    assert!(here == Sandbox::On || running_as_root());
}

#[test]
fn headful_windows_need_a_display_on_linux_and_say_what_to_do() {
    let none = |_: &str| None;
    let only =
        |name: &'static str, v: &'static str| move |k: &str| (k == name).then(|| v.to_string());
    for w in [Window::OffScreen, Window::OnScreen(None)] {
        let e = check_display(w, true, none).unwrap_err().to_string();
        assert!(e.contains("no display") && e.contains("xvfb-run"), "{e}");
        assert!(check_display(w, true, only("DISPLAY", ":0")).is_ok());
        assert!(check_display(w, true, only("WAYLAND_DISPLAY", "wayland-0")).is_ok());
        assert!(
            check_display(w, true, only("DISPLAY", "  ")).is_err(),
            "a blank DISPLAY is none"
        );
        assert!(check_display(w, false, none).is_ok());
    }
    assert!(check_display(Window::Headless, true, none).is_ok());
}

#[test]
fn stderr_tail_is_bounded_single_line_and_printable() {
    assert_eq!(stderr_tail(b""), "");
    assert_eq!(stderr_tail(b"one\n\n  two \x07\n"), "one | two");
    assert!(stderr_tail("x".repeat(5000).as_bytes()).len() <= STDERR_TAIL);
    assert_eq!(stderr_tail(&[0xff, b'a']), "\u{fffd}a");
}

#[test]
fn the_theme_is_written_and_merged() {
    let d = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(d.path().join("Default")).unwrap();
    std::fs::write(
        d.path().join("Default/Preferences"),
        r#"{"browser":{"other":1},"x":2}"#,
    )
    .unwrap();
    write_theme(d.path(), 0x336699).unwrap();
    let v: Value = serde_json::from_str(
        &std::fs::read_to_string(d.path().join("Default/Preferences")).unwrap(),
    )
    .unwrap();
    assert_eq!(
        (v["browser"]["other"].clone(), v["x"].clone()),
        (json!(1), json!(2))
    );
    assert_eq!(v["browser"]["theme"]["user_color2"], 0xFF336699u32 as i32);
    std::fs::write(d.path().join("Default/Preferences"), "[1]").unwrap();
    write_theme(d.path(), 0x112233).unwrap();
    let v: Value = serde_json::from_str(
        &std::fs::read_to_string(d.path().join("Default/Preferences")).unwrap(),
    )
    .unwrap();
    assert!(v.is_object(), "a damaged file is replaced");
}

#[test]
fn profiles_are_unique_and_leftovers_of_dead_processes_are_swept() {
    let base = tempfile::tempdir().unwrap();
    let dirs: std::collections::HashSet<PathBuf> =
        (0..1000).map(|_| unique_dir(base.path())).collect();
    assert_eq!(dirs.len(), 1000);
    let me = std::process::id();
    let mine = ProcessId::new(me, StartTime::from_micros(500));
    for name in [
        "chromium-1-1-1",
        "chromium-2-1-1",
        &format!("chromium-{me}-1-1"),
        // By identity: pid 3 runs, but the one that started at 7 µs is gone (its pid was handed on).
        "chromium-3-s7-1-1",
        "chromium-3-s9-1-1",
        // This process's own (the same start time, and one read a little apart), and an earlier process that had this pid.
        &format!("chromium-{me}-s500-1-1"),
        &format!("chromium-{me}-s501-1-1"),
        &format!("chromium-{me}-s10000500-1-1"),
        "chromium-x-1",
        "other",
    ] {
        std::fs::create_dir_all(base.path().join(name)).unwrap();
    }
    let alive = |pid: u32, started: Option<StartTime>| match started {
        Some(s) => pid == 3 && s == StartTime::from_micros(9),
        None => pid == 2,
    };
    assert_eq!(sweep_profiles_with(base.path(), Some(mine), &alive), 3);
    let left: std::collections::BTreeSet<String> = std::fs::read_dir(base.path())
        .unwrap()
        .flatten()
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .collect();
    for gone in [
        "chromium-1-1-1",
        "chromium-3-s7-1-1",
        &format!("chromium-{me}-s10000500-1-1"),
    ] {
        assert!(!left.contains(gone), "{gone} was left");
    }
    for kept in [
        "chromium-2-1-1",
        "chromium-3-s9-1-1",
        "other",
        "chromium-x-1",
        &format!("chromium-{me}-1-1"),
        &format!("chromium-{me}-s500-1-1"),
        &format!("chromium-{me}-s501-1-1"),
    ] {
        assert!(left.contains(kept), "{kept} was removed");
    }
    let name = unique_dir(base.path());
    let (pid, started) = owner_of(&name.file_name().unwrap().to_string_lossy()).unwrap();
    assert_eq!((pid, started), (me, super::me().map(|m| m.started())));
    assert_eq!(sweep_profiles(&base.path().join("none")), 0);
    let vars = paddock_env::MapVars::new()
        .with("HOME", base.path().as_os_str())
        .with("USERPROFILE", base.path().as_os_str())
        .with("LOCALAPPDATA", base.path().as_os_str());
    let app = paddock_env::AppId::new("demo", "Demo").unwrap();
    let dirs = paddock_env::AppDirs::resolve(&app, paddock_env::Os::current(), &vars, None, false)
        .unwrap();
    assert_eq!(profiles_dir(&dirs), dirs.cache().join("browser-profiles"));
    assert!(format!("{:?}", opts(base.path())).contains("Headless"));
}

#[test]
fn the_cap_is_checked_once_more_when_the_browser_connects() {
    let mut o = opts(Path::new("/b"));
    o.memory_cap = Some(10);
    o.memory_sampler = Some(Arc::new(u64::from));
    assert!(
        over_cap_at_start(&o, Some(11))
            .unwrap()
            .to_string()
            .contains("more memory than allowed")
    );
    assert!(over_cap_at_start(&o, Some(10)).is_none());
    assert!(over_cap_at_start(&o, None).is_none(), "no process");
    o.memory_sampler = None;
    assert!(
        over_cap_at_start(&o, Some(std::process::id())).is_some(),
        "this process uses more than 10 bytes"
    );
    o.memory_cap = None;
    assert!(over_cap_at_start(&o, Some(11)).is_none(), "no cap");
}

/// Review P1: on Linux two processes can read the same start time about a second apart (each reads the boot time once,
/// in whole seconds), so a live owner whose start time this process reads a little differently must keep its profile.
#[cfg(unix)]
#[test]
fn a_live_owner_whose_start_time_reads_a_little_apart_keeps_its_profile() {
    let base = tempfile::tempdir().unwrap();
    let mut other = std::process::Command::new("/bin/sleep")
        .arg("30")
        .spawn()
        .unwrap();
    let id = ProcessId::of(other.id()).unwrap();
    let at = |d: i64| id.started().as_micros().saturating_add_signed(d);
    let near = format!("chromium-{}-s{}-1-1", id.pid(), at(500));
    let far = format!("chromium-{}-s{}-1-1", id.pid(), at(10_000_000));
    for name in [&near, &far] {
        std::fs::create_dir_all(base.path().join(name)).unwrap();
    }
    let removed = sweep_profiles_with(base.path(), super::me(), &owner_alive);
    // Ended through the handle of this test's own child, before any assert can fail (ER-14).
    other.kill().unwrap();
    other.wait().unwrap();
    assert_eq!(removed, 1);
    assert!(
        base.path().join(&near).exists(),
        "the live owner's profile was removed"
    );
    assert!(
        !base.path().join(&far).exists(),
        "a profile of a process that is gone (its pid handed on) stays"
    );
}

/// A profile named without its owner's start time is judged by the pid alone.
#[test]
fn an_owner_without_a_start_time_is_judged_by_its_pid() {
    assert!(owner_alive(std::process::id(), None));
    assert!(!owner_alive(i32::MAX as u32 - 1, None), "no such process");
}
