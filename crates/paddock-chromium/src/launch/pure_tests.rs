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
    let a = command_line(Path::new("/tmp/p"), &opts(Path::new("/b")), Sandbox::On);
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
        command_line(Path::new("/p"), &o, Sandbox::On)
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
                    let a = command_line(Path::new("/p"), &o, sandbox);
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
    for name in [
        "chromium-1-1-1",
        "chromium-2-1-1",
        &format!("chromium-{me}-1-1"),
        "chromium-x-1",
        "other",
    ] {
        std::fs::create_dir_all(base.path().join(name)).unwrap();
    }
    assert_eq!(sweep_profiles_with(base.path(), &|pid| pid == 2), 1);
    let left: std::collections::BTreeSet<String> = std::fs::read_dir(base.path())
        .unwrap()
        .flatten()
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .collect();
    assert!(
        !left.contains("chromium-1-1-1")
            && left.contains("chromium-2-1-1")
            && left.contains("other")
    );
    assert!(
        left.contains(&format!("chromium-{me}-1-1")),
        "this process's own profiles stay"
    );
    assert_eq!(sweep_profiles(&base.path().join("none")), 0);
    let vars = paddock_env::MapVars::new().with("HOME", base.path().as_os_str());
    let app = paddock_env::AppId::new("demo", "Demo").unwrap();
    let dirs =
        paddock_env::AppDirs::resolve(&app, paddock_env::Os::Linux, &vars, None, false).unwrap();
    assert_eq!(profiles_dir(&dirs), dirs.cache().join("browser-profiles"));
    assert!(format!("{:?}", opts(base.path())).contains("Headless"));
}
