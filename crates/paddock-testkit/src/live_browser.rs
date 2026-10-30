//! The gate of every test that starts a real browser (Paddock rule 7: a real browser or app only in a throwaway Linux CI
//! container or a test VM, never on a desktop someone uses).
//!
//! A real test browser (the pinned Chrome for Testing) starts only where the job says the session is a throwaway one, with
//! [`LIVE_BROWSER_VAR`]: `1` in a Linux CI container (under `xvfb-run` for a browser with a window), or `vm` in a macOS or
//! Windows test VM that [`Host::current`] recognises as one. The Mac and Windows CI runners run in a user's session, where
//! even a headless or off-screen browser is a process of that session (a Dock or taskbar entry, the focus), so these tests
//! never run there. A test that is not run prints where it runs instead; a job set up wrongly fails instead of skipping.
//!
//! Two layers: the drivers' test-browser finders (`paddock-chromium`, Debrief's `chromium-driver`, Spotter's
//! `spotter-render`) go through [`find_test_browser`], and their launch functions, in test builds, ask [`check_launch`]
//! before they start any program, so no test starts a real browser around the gate, however it found the executable.

/// The switch of a session where a test may start a real browser: `1` in a Linux CI container (honoured on Linux only),
/// `vm` in a macOS or Windows test VM (honoured only where [`Host::current`] finds the VM marker). On a Linux desktop,
/// `1` opens real windows on that desktop: set it only under `xvfb-run` (CI does) or in a VM.
pub const LIVE_BROWSER_VAR: &str = "PADDOCK_LIVE_BROWSER";

/// The marker of a Windows test VM: the DWORD value `TestVM` = 1 under this machine-wide key, which only an administrator can
/// write (`reg add HKLM\SOFTWARE\Paddock /v TestVM /t REG_DWORD /d 1 /f`, run once by the VM's set-up,
/// `docs/dev-setup.md`, "Test VMs and real browsers"). No desktop machine has it.
pub const WINDOWS_VM_KEY: &str = r"HKLM\SOFTWARE\Paddock";

/// The value under [`WINDOWS_VM_KEY`] that marks a Windows test VM.
pub const WINDOWS_VM_VALUE: &str = "TestVM";

/// The line every fake browser and stub script of a test carries: the launch gate lets only these start outside a
/// throwaway session ([`is_real_program`]). Real launchers are scripts too (Chrome's and Edge's on Linux), so a `#!` line
/// alone proves nothing.
pub const FAKE_MARKER: &str = "# paddock-fake-browser";

/// How the browser a test wants to start shows itself.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BrowserWindow {
    /// No window (`--headless`): no display needed.
    Headless,
    /// A window, on or off screen: needs a display, which must be a virtual one (or a test VM's).
    Shown,
}

/// The machine a test runs on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Host {
    /// The OS, as `std::env::consts::OS` names it.
    pub os: &'static str,
    /// A macOS or Windows test VM (a hypervisor guest on macOS, `kern.hv_vmm_present`; the administrator-only
    /// [`WINDOWS_VM_KEY`] value on Windows). Never set on Linux, where CI's containers are the throwaway sessions.
    pub test_vm: bool,
}

impl Host {
    /// A machine that is not a test VM.
    pub fn plain(os: &'static str) -> Host {
        Host { os, test_vm: false }
    }

    /// This machine.
    pub fn current() -> Host {
        let os = std::env::consts::OS;
        let test_vm = match os {
            "macos" => macos_guest(),
            "windows" => windows_vm_marked(),
            _ => false,
        };
        Host { os, test_vm }
    }
}

/// `kern.hv_vmm_present`: 1 in a macOS virtual machine (Apple's Virtualization framework, Tart), 0 on a Mac.
#[cfg(target_os = "macos")]
fn macos_guest() -> bool {
    let mut value: libc::c_int = 0;
    let mut size = std::mem::size_of::<libc::c_int>();
    // SAFETY: the name is a NUL-terminated string, and `value`/`size` describe a writable c_int the call fills.
    let rc = unsafe {
        libc::sysctlbyname(
            c"kern.hv_vmm_present".as_ptr(),
            (&raw mut value).cast(),
            &raw mut size,
            std::ptr::null_mut(),
            0,
        )
    };
    rc == 0 && value == 1
}

#[cfg(not(target_os = "macos"))]
fn macos_guest() -> bool {
    false
}

/// Whether [`WINDOWS_VM_KEY`] has `TestVM` = 1, read with the system's `reg.exe` (bounded by its own exit; nothing else).
#[cfg(windows)]
fn windows_vm_marked() -> bool {
    let reg = std::env::var_os("SystemRoot")
        .map(|r| std::path::PathBuf::from(r).join("System32").join("reg.exe"))
        .unwrap_or_else(|| std::path::PathBuf::from(r"C:\Windows\System32\reg.exe"));
    std::process::Command::new(reg)
        .args(["query", WINDOWS_VM_KEY, "/v", WINDOWS_VM_VALUE])
        .stdin(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .output()
        .is_ok_and(|o| o.status.success() && reg_dword_is_one(&String::from_utf8_lossy(&o.stdout)))
}

#[cfg(not(windows))]
fn windows_vm_marked() -> bool {
    false
}

/// Whether `reg query` output shows `TestVM    REG_DWORD    0x1`.
#[cfg(any(windows, test))]
fn reg_dword_is_one(out: &str) -> bool {
    out.lines().any(|l| {
        let f: Vec<&str> = l.split_whitespace().collect();
        f.len() == 3
            && f[0].eq_ignore_ascii_case(WINDOWS_VM_VALUE)
            && f[1] == "REG_DWORD"
            && f[2] == "0x1"
    })
}

/// Whether a test may start a real browser here.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LiveBrowser {
    /// A throwaway session: Linux with the job's [`LIVE_BROWSER_VAR`] (and a display for a window), or a test VM with
    /// `vm`.
    Allowed,
    /// Not here, and where such tests run instead (the test prints it, so a skip is never silent).
    NotHere(String),
}

/// What a test-browser finder hands a test.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TestBrowser<B> {
    /// The pinned test browser, allowed here.
    Found(B),
    /// No browser for this test, and why (printed by [`found_or_skip`]).
    Skip(String),
}

/// Whether `key` is set in `env`: a value that is not blank and not `0`.
pub fn is_set(env: &dyn Fn(&str) -> Option<String>, key: &str) -> bool {
    env(key).is_some_and(|v| !v.trim().is_empty() && v.trim() != "0")
}

/// [`LiveBrowser`] for the machine `host`, the environment `env` and the browser's `window`. `Err` is a job set up wrongly,
/// which fails the test: the variable for a browser with a window but no display (Linux without `xvfb-run`), `vm` on a
/// macOS or Windows machine that is not a test VM, or `require_var` (the app's "a missing test browser fails" switch) set
/// on Linux without the variable, where every real-browser test would skip.
pub fn live_browser_with(
    host: Host,
    env: &dyn Fn(&str) -> Option<String>,
    window: BrowserWindow,
    require_var: &str,
) -> Result<LiveBrowser, String> {
    let value = env(LIVE_BROWSER_VAR)
        .map(|v| v.trim().to_ascii_lowercase())
        .unwrap_or_default();
    if host.os != "linux" {
        return match (value.as_str(), host.test_vm) {
            ("vm", true) => Ok(LiveBrowser::Allowed),
            ("vm", false) => Err(format!(
                "{LIVE_BROWSER_VAR}=vm but this {} machine is not a test VM (no VM marker): a test never starts a real \
                 browser on a desktop or in a user's session (Paddock rule 7)",
                host.os
            )),
            _ => Ok(LiveBrowser::NotHere(format!(
                "this test starts a real browser, so it runs only in the Linux CI job ({LIVE_BROWSER_VAR}=1, under \
                 xvfb-run) or in a test VM ({LIVE_BROWSER_VAR}=vm), never in a {} user session (Paddock rule 7)",
                host.os
            ))),
        };
    }
    let display = is_set(env, "DISPLAY") || is_set(env, "WAYLAND_DISPLAY");
    match (is_set(env, LIVE_BROWSER_VAR), window) {
        (true, BrowserWindow::Shown) if !display => Err(format!(
            "{LIVE_BROWSER_VAR} is set but there is no display for the browser's window: run the tests under xvfb-run"
        )),
        (true, _) => Ok(LiveBrowser::Allowed),
        (false, _) if is_set(env, require_var) => Err(format!(
            "{require_var} is set on Linux but {LIVE_BROWSER_VAR} is not: run the tests under xvfb-run with \
             {LIVE_BROWSER_VAR}=1, or every real-browser test skips"
        )),
        (false, _) => Ok(LiveBrowser::NotHere(format!(
            "this test starts a real browser: run it in a throwaway Linux session under xvfb-run with {LIVE_BROWSER_VAR}=1, \
             or in a test VM, never on a desktop (Paddock rule 7)"
        ))),
    }
}

/// A driver's test-browser finder: the gate first ([`live_browser_with`]); where it allows the browser, `lookup` (the
/// driver's own search for the pinned build, run only then). A browser that is missing where `require_var` is set is an
/// `Err`, like a job set up wrongly; one that is missing otherwise is a [`TestBrowser::Skip`] that names `hint`.
pub fn find_test_browser<B>(
    host: Host,
    env: &dyn Fn(&str) -> Option<String>,
    window: BrowserWindow,
    require_var: &str,
    hint: &str,
    lookup: impl FnOnce() -> Option<B>,
) -> Result<TestBrowser<B>, String> {
    match live_browser_with(host, env, window, require_var)? {
        LiveBrowser::NotHere(why) => Ok(TestBrowser::Skip(why)),
        LiveBrowser::Allowed => match lookup() {
            Some(b) => Ok(TestBrowser::Found(b)),
            None if is_set(env, require_var) => Err(format!(
                "{require_var} is set but no pinned test browser was found: {hint}"
            )),
            None => Ok(TestBrowser::Skip(format!("no pinned test browser: {hint}"))),
        },
    }
}

/// The browser of a [`find_test_browser`] result for a test: `Some` when found; `None` after printing why (the test then
/// returns); panics on an `Err`, so a job set up wrongly fails.
pub fn found_or_skip<B>(found: Result<TestBrowser<B>, String>) -> Option<B> {
    match found {
        Ok(TestBrowser::Found(b)) => Some(b),
        Ok(TestBrowser::Skip(why)) => {
            eprintln!("not run here: {why}");
            None
        }
        Err(why) => panic!("{why}"),
    }
}

/// Whether `exe` is a real program: an existing file that is not a test's fake, which says so with the [`FAKE_MARKER`]
/// line in its first 4 KiB. A path that does not exist starts nothing.
pub fn is_real_program(exe: &std::path::Path) -> bool {
    use std::io::Read;
    if !exe.is_file() {
        return false;
    }
    let mut head = Vec::with_capacity(4096);
    let read = std::fs::File::open(exe).and_then(|f| f.take(4096).read_to_end(&mut head));
    !(read.is_ok()
        && String::from_utf8_lossy(&head)
            .lines()
            .any(|l| l.trim() == FAKE_MARKER))
}

/// `#!/bin/sh`, the [`FAKE_MARKER`] and `body`: the text of a fake browser or stub script.
pub fn fake_script(body: &str) -> String {
    format!("#!/bin/sh\n{FAKE_MARKER}\n{body}\n")
}

/// Whether `exe` is one of `installed` (a driver's installed-browser candidates), comparing canonical paths where they
/// exist (a link or `..` must not hide an installed browser).
pub fn is_installed(
    exe: &std::path::Path,
    installed: impl IntoIterator<Item = std::path::PathBuf>,
) -> bool {
    let canonical =
        |p: &std::path::Path| std::fs::canonicalize(p).unwrap_or_else(|_| p.to_path_buf());
    let exe = canonical(exe);
    installed.into_iter().any(|c| canonical(&c) == exe)
}

/// The launch-time gate a driver asks in test builds, before it starts `exe` with `window`. Refused: a browser the user
/// installed (`installed`: one of the driver's installed-browser candidates), always; and any other real program
/// ([`is_real_program`]) unless [`live_browser_with`] allows a real browser here. Fakes (scripts) and missing paths pass.
pub fn launch_check_with(
    host: Host,
    env: &dyn Fn(&str) -> Option<String>,
    exe: &std::path::Path,
    real_program: bool,
    installed: bool,
    window: BrowserWindow,
) -> Result<(), String> {
    if installed {
        return Err(format!(
            "a test never starts a browser the user installed ({}): use the pinned test browser from the driver's \
             discover::find_for_tests (Paddock rule 7)",
            exe.display()
        ));
    }
    if !real_program {
        return Ok(());
    }
    match live_browser_with(host, env, window, "")? {
        LiveBrowser::Allowed => Ok(()),
        LiveBrowser::NotHere(why) => Err(format!("{} was not started: {why}", exe.display())),
    }
}

/// What `exe` starts: a bare program name (no path separator) is looked up on `path_var` the way `Command` does (with
/// `.exe` on Windows); a name not found there, and any path, stay as they are.
pub fn resolve_program(
    exe: &std::path::Path,
    path_var: Option<&str>,
    windows: bool,
) -> std::path::PathBuf {
    if exe.components().count() != 1 || exe.is_absolute() || exe.as_os_str().is_empty() {
        return exe.to_path_buf();
    }
    let names: Vec<std::path::PathBuf> = if windows && exe.extension().is_none() {
        vec![exe.with_extension("exe"), exe.to_path_buf()]
    } else {
        vec![exe.to_path_buf()]
    };
    path_var
        .into_iter()
        .flat_map(|p| std::env::split_paths(p).collect::<Vec<_>>())
        .flat_map(|dir| names.iter().map(move |n| dir.join(n)))
        .find(|c| c.is_file())
        .unwrap_or_else(|| exe.to_path_buf())
}

/// The launch-time gate for the machine `host` and the environment `env`: `exe` is resolved first ([`resolve_program`],
/// so a bare `google-chrome` is judged as the program `Command` would start), then refused when it is one of `installed`
/// (the driver's installed-browser candidates, compared canonically) or a real program outside a throwaway session.
pub fn check_launch_with(
    host: Host,
    env: &dyn Fn(&str) -> Option<String>,
    exe: &std::path::Path,
    installed: impl IntoIterator<Item = std::path::PathBuf>,
    window: BrowserWindow,
) -> Result<(), String> {
    let resolved = resolve_program(exe, env("PATH").as_deref(), host.os == "windows");
    launch_check_with(
        host,
        env,
        &resolved,
        is_real_program(&resolved),
        is_installed(&resolved, installed),
        window,
    )
}

/// [`check_launch_with`] for this machine and process.
pub fn check_launch(
    exe: &std::path::Path,
    installed: impl IntoIterator<Item = std::path::PathBuf>,
    window: BrowserWindow,
) -> Result<(), String> {
    check_launch_with(Host::current(), &process_env, exe, installed, window)
}

/// The process's environment, as the `env` of the functions above.
pub fn process_env(key: &str) -> Option<String> {
    std::env::var(key).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    const REQUIRE: &str = "APP_REQUIRE_TEST_BROWSER";
    const XVFB: &[(&str, &str)] = &[(LIVE_BROWSER_VAR, "1"), ("DISPLAY", ":99"), (REQUIRE, "1")];

    fn env(pairs: &'static [(&'static str, &'static str)]) -> impl Fn(&str) -> Option<String> {
        move |k: &str| {
            pairs
                .iter()
                .find(|(n, _)| *n == k)
                .map(|(_, v)| v.to_string())
        }
    }

    /// Regression (Debrief CI 7141, 7146): the Mac runner runs in the user's session and had the test browser, so the
    /// live tests opened Chrome on the user's desktop. Never on a desktop OS, whatever the job sets, headless or not.
    #[test]
    fn a_real_browser_never_starts_in_a_desktop_session() {
        for os in ["macos", "windows"] {
            for window in [BrowserWindow::Headless, BrowserWindow::Shown] {
                let Ok(LiveBrowser::NotHere(why)) =
                    live_browser_with(Host::plain(os), &env(XVFB), window, REQUIRE)
                else {
                    panic!("{os} {window:?} was allowed")
                };
                assert!(
                    why.contains("Linux") && why.contains(LIVE_BROWSER_VAR) && why.contains(os),
                    "{why}"
                );
                let looked = std::cell::Cell::new(false);
                let found =
                    find_test_browser(Host::plain(os), &env(XVFB), window, REQUIRE, "hint", || {
                        looked.set(true);
                        Some(1)
                    });
                assert!(matches!(found, Ok(TestBrowser::Skip(_))), "{os}: {found:?}");
                assert!(!looked.get(), "the browser is not even looked for");
            }
        }
    }

    #[test]
    fn linux_needs_the_job_s_variable_and_a_display_for_a_window() {
        for window in [BrowserWindow::Headless, BrowserWindow::Shown] {
            assert_eq!(
                live_browser_with(Host::plain("linux"), &env(XVFB), window, REQUIRE),
                Ok(LiveBrowser::Allowed)
            );
        }
        let wayland = env(&[(LIVE_BROWSER_VAR, "1"), ("WAYLAND_DISPLAY", "w")]);
        assert_eq!(
            live_browser_with(
                Host::plain("linux"),
                &wayland,
                BrowserWindow::Shown,
                REQUIRE
            ),
            Ok(LiveBrowser::Allowed)
        );
        // Headless needs no display.
        let no_display = env(&[(LIVE_BROWSER_VAR, "1")]);
        assert_eq!(
            live_browser_with(
                Host::plain("linux"),
                &no_display,
                BrowserWindow::Headless,
                REQUIRE
            ),
            Ok(LiveBrowser::Allowed)
        );
        // A developer's Linux desktop (a display the job did not vouch for), or "0": not there.
        for pairs in [
            &[("DISPLAY", ":0")][..],
            &[(LIVE_BROWSER_VAR, "0"), ("DISPLAY", ":99")][..],
        ] {
            assert!(matches!(
                live_browser_with(
                    Host::plain("linux"),
                    &env(pairs),
                    BrowserWindow::Shown,
                    REQUIRE
                ),
                Ok(LiveBrowser::NotHere(_))
            ));
        }
    }

    #[test]
    fn a_job_set_up_wrongly_fails_instead_of_skipping() {
        let e = live_browser_with(
            Host::plain("linux"),
            &env(&[(LIVE_BROWSER_VAR, "1"), ("DISPLAY", " ")]),
            BrowserWindow::Shown,
            REQUIRE,
        )
        .unwrap_err();
        assert!(e.contains("xvfb-run"), "{e}");
        for pairs in [
            &[(REQUIRE, "1")][..],
            &[(REQUIRE, " 1 ")][..],
            &[(REQUIRE, "yes")][..],
        ] {
            let e = live_browser_with(
                Host::plain("linux"),
                &env(pairs),
                BrowserWindow::Headless,
                REQUIRE,
            )
            .unwrap_err();
            assert!(e.contains(LIVE_BROWSER_VAR) && e.contains(REQUIRE), "{e}");
        }
        // A blank or "0" switch is not set: the tests say where they run instead.
        for pairs in [&[(REQUIRE, " ")][..], &[(REQUIRE, "0")][..]] {
            assert!(matches!(
                live_browser_with(
                    Host::plain("linux"),
                    &env(pairs),
                    BrowserWindow::Headless,
                    REQUIRE
                ),
                Ok(LiveBrowser::NotHere(_))
            ));
        }
    }

    #[test]
    fn the_finder_looks_only_where_allowed_and_a_required_browser_must_be_there() {
        assert_eq!(
            find_test_browser(
                Host::plain("linux"),
                &env(XVFB),
                BrowserWindow::Shown,
                REQUIRE,
                "hint",
                || Some(7)
            ),
            Ok(TestBrowser::Found(7))
        );
        let e = find_test_browser(
            Host::plain("linux"),
            &env(XVFB),
            BrowserWindow::Shown,
            REQUIRE,
            "install it",
            || None::<u8>,
        )
        .unwrap_err();
        assert!(e.contains(REQUIRE) && e.contains("install it"), "{e}");
        let optional = env(&[(LIVE_BROWSER_VAR, "1"), (REQUIRE, "0")]);
        let Ok(TestBrowser::Skip(why)) = find_test_browser(
            Host::plain("linux"),
            &optional,
            BrowserWindow::Headless,
            REQUIRE,
            "install it",
            || None::<u8>,
        ) else {
            panic!("a missing optional browser skips")
        };
        assert!(why.contains("install it"), "{why}");
        // A job set up wrongly is an error before any search.
        let wrong = env(&[(REQUIRE, "1")]);
        assert!(
            find_test_browser(
                Host::plain("linux"),
                &wrong,
                BrowserWindow::Headless,
                REQUIRE,
                "h",
                || -> Option<u8> { panic!("searched") }
            )
            .is_err()
        );
    }

    #[test]
    fn found_or_skip_hands_out_skips_or_fails() {
        assert_eq!(found_or_skip(Ok(TestBrowser::Found(3))), Some(3));
        assert_eq!(
            found_or_skip(Ok(TestBrowser::<u8>::Skip("why".into()))),
            None
        );
        let failed = std::panic::catch_unwind(|| found_or_skip::<u8>(Err("set up wrongly".into())));
        assert!(failed.is_err());
    }

    #[test]
    fn is_set_trims_and_treats_zero_as_unset() {
        let e = env(&[("A", "1"), ("B", " "), ("C", "0"), ("D", " 0 "), ("E", "x")]);
        assert_eq!(
            ["A", "B", "C", "D", "E", "F"].map(|k| is_set(&e, k)),
            [true, false, false, false, true, false]
        );
        let _ = process_env("PATH");
    }

    /// The VM opt-in (P1-4): `vm` starts a real browser in a macOS or Windows test VM, and is refused anywhere else.
    #[test]
    fn a_test_vm_may_start_a_real_browser_with_vm_and_nothing_else_may() {
        let vm = env(&[(LIVE_BROWSER_VAR, " VM ")]);
        for os in ["macos", "windows"] {
            let guest = Host { os, test_vm: true };
            for window in [BrowserWindow::Headless, BrowserWindow::Shown] {
                assert_eq!(
                    live_browser_with(guest, &vm, window, REQUIRE),
                    Ok(LiveBrowser::Allowed),
                    "{os}"
                );
            }
            // The same switch on a machine without the VM marker is a mistake, not a skip.
            let e = live_browser_with(Host::plain(os), &vm, BrowserWindow::Headless, REQUIRE)
                .unwrap_err();
            assert!(e.contains("not a test VM"), "{e}");
            // `1` is the Linux container's switch: a VM still needs `vm`, and no switch skips.
            for pairs in [&[(LIVE_BROWSER_VAR, "1")][..], &[][..]] {
                assert!(matches!(
                    live_browser_with(guest, &env(pairs), BrowserWindow::Shown, REQUIRE),
                    Ok(LiveBrowser::NotHere(_))
                ));
            }
        }
        // On Linux `vm` is a set switch like `1` (a Linux VM is a throwaway session too).
        let linux_vm = env(&[(LIVE_BROWSER_VAR, "vm"), ("DISPLAY", ":1")]);
        assert_eq!(
            live_browser_with(
                Host::plain("linux"),
                &linux_vm,
                BrowserWindow::Shown,
                REQUIRE
            ),
            Ok(LiveBrowser::Allowed)
        );
        let here = Host::current();
        assert_eq!(here.os, std::env::consts::OS);
        if here.os == "linux" {
            assert!(!here.test_vm);
        }
    }

    /// The launch-time gate (P1-1): whatever found the executable, a test starts no installed browser and no real
    /// program outside a throwaway session; fakes (scripts) and missing paths pass.
    #[test]
    fn the_launch_gate_refuses_installed_browsers_and_real_programs_outside_a_throwaway_session() {
        let exe = std::path::Path::new("/opt/browsers/chrome");
        let ci = env(XVFB);
        let none = env(&[]);
        // An installed browser: never, not even in CI.
        for host in [
            Host::plain("linux"),
            Host::plain("macos"),
            Host {
                os: "macos",
                test_vm: true,
            },
        ] {
            let e =
                launch_check_with(host, &ci, exe, true, true, BrowserWindow::Headless).unwrap_err();
            assert!(e.contains("installed"), "{e}");
        }
        // A real program (the pinned build, a downloaded runtime, a hard-coded path): only where the gate allows.
        assert_eq!(
            launch_check_with(
                Host::plain("linux"),
                &ci,
                exe,
                true,
                false,
                BrowserWindow::Shown
            ),
            Ok(())
        );
        for host in [Host::plain("macos"), Host::plain("windows")] {
            let e = launch_check_with(host, &ci, exe, true, false, BrowserWindow::Headless)
                .unwrap_err();
            assert!(e.contains("was not started") && e.contains("rule 7"), "{e}");
        }
        assert!(
            launch_check_with(
                Host::plain("linux"),
                &none,
                exe,
                true,
                false,
                BrowserWindow::Headless
            )
            .is_err()
        );
        let vm = env(&[(LIVE_BROWSER_VAR, "vm")]);
        assert_eq!(
            launch_check_with(
                Host {
                    os: "macos",
                    test_vm: true
                },
                &vm,
                exe,
                true,
                false,
                BrowserWindow::Shown
            ),
            Ok(())
        );
        // A fake (a script) or a missing path: anywhere.
        for host in [
            Host::plain("macos"),
            Host::plain("windows"),
            Host::plain("linux"),
        ] {
            assert_eq!(
                launch_check_with(host, &none, exe, false, false, BrowserWindow::Shown),
                Ok(())
            );
        }
        assert!(
            check_launch(
                std::path::Path::new("/nonexistent/chrome"),
                Vec::new(),
                BrowserWindow::Shown
            )
            .is_ok()
        );
    }

    /// Re-review P1-a: `Command::new("google-chrome")` finds the program on PATH, but the gate looked at the bare name,
    /// which is no file, and let it pass. A bare name is resolved on PATH first, as `Command` does.
    #[test]
    fn a_bare_program_name_is_judged_as_what_path_starts() {
        let d = tempfile::tempdir().unwrap();
        let bin = d.path().join("bin");
        std::fs::create_dir(&bin).unwrap();
        let chrome = bin.join("google-chrome");
        std::fs::write(&chrome, [0x7f, b'E', b'L', b'F']).unwrap();
        let fake = bin.join("fake-browser");
        std::fs::write(&fake, fake_script("exit 0")).unwrap();
        let path = std::env::join_paths([d.path().join("empty"), bin.clone()])
            .unwrap()
            .into_string()
            .unwrap();
        let env = |k: &str| (k == "PATH").then(|| path.clone());
        let bare = std::path::Path::new("google-chrome");
        assert_eq!(resolve_program(bare, Some(&path), false), chrome);
        // A real program found on PATH: refused outside a throwaway session, and as an installed browser everywhere.
        let e = check_launch_with(
            Host::plain("macos"),
            &env,
            bare,
            Vec::new(),
            BrowserWindow::Headless,
        )
        .unwrap_err();
        assert!(
            e.contains("was not started") && e.contains("google-chrome"),
            "{e}"
        );
        let ci = |k: &str| match k {
            "PATH" => Some(path.clone()),
            LIVE_BROWSER_VAR => Some("1".into()),
            _ => None,
        };
        let e = check_launch_with(
            Host::plain("linux"),
            &ci,
            bare,
            [chrome.clone()],
            BrowserWindow::Headless,
        )
        .unwrap_err();
        assert!(e.contains("installed"), "{e}");
        assert_eq!(
            check_launch_with(
                Host::plain("linux"),
                &ci,
                bare,
                Vec::new(),
                BrowserWindow::Headless
            ),
            Ok(())
        );
        // A marked fake on PATH, a name on no PATH entry, and a path (never looked up) pass anywhere.
        let none = |k: &str| (k == "PATH").then(|| path.clone());
        assert_eq!(
            check_launch_with(
                Host::plain("macos"),
                &none,
                std::path::Path::new("fake-browser"),
                Vec::new(),
                BrowserWindow::Shown
            ),
            Ok(())
        );
        assert_eq!(
            check_launch_with(
                Host::plain("macos"),
                &none,
                std::path::Path::new("chromium"),
                Vec::new(),
                BrowserWindow::Shown
            ),
            Ok(())
        );
        assert_eq!(
            resolve_program(std::path::Path::new("chromium"), Some(&path), false),
            std::path::PathBuf::from("chromium")
        );
        assert_eq!(
            resolve_program(std::path::Path::new("./google-chrome"), Some(&path), false),
            std::path::PathBuf::from("./google-chrome")
        );
        assert_eq!(resolve_program(bare, None, false), bare.to_path_buf());
        // Windows: `chrome` is found as `chrome.exe`.
        let exe = bin.join("chrome.exe");
        std::fs::write(&exe, [0x4d, 0x5a]).unwrap();
        assert_eq!(
            resolve_program(std::path::Path::new("chrome"), Some(&path), true),
            exe
        );
    }

    #[test]
    fn only_marked_fakes_and_missing_paths_are_not_real_programs() {
        let d = tempfile::tempdir().unwrap();
        let fake = d.path().join("fake-browser");
        std::fs::write(&fake, fake_script("exit 0")).unwrap();
        // Review P2-1: Chrome's and Edge's Linux launchers are bash scripts, and a wrapper script can start a real
        // browser: a `#!` line is no proof of a fake, and neither is a Windows batch file.
        let launcher = d.path().join("google-chrome");
        std::fs::write(
            &launcher,
            "#!/bin/bash\nexec -a \"$0\" /opt/google/chrome/chrome \"$@\"\n",
        )
        .unwrap();
        let cmd = d.path().join("fake.cmd");
        std::fs::write(&cmd, "@echo off").unwrap();
        let binary = d.path().join("chrome");
        std::fs::write(&binary, [0x7f, b'E', b'L', b'F']).unwrap();
        let empty = d.path().join("empty");
        std::fs::write(&empty, "").unwrap();
        assert!(!is_real_program(&fake));
        assert!(!is_real_program(&d.path().join("missing")));
        assert!(!is_real_program(d.path()), "a folder");
        for real in [&launcher, &cmd, &binary, &empty] {
            assert!(is_real_program(real), "{}", real.display());
        }
        // The marker counts only as a line of its own, not inside another one.
        let quoted = d.path().join("quoted");
        std::fs::write(&quoted, format!("#!/bin/sh\necho '{FAKE_MARKER}'\n")).unwrap();
        assert!(is_real_program(&quoted));
    }

    #[test]
    fn installed_paths_compare_canonically() {
        let d = tempfile::tempdir().unwrap();
        let real = d.path().join("chrome");
        std::fs::write(&real, "x").unwrap();
        let via_dots = d.path().join("sub").join("..").join("chrome");
        std::fs::create_dir(d.path().join("sub")).unwrap();
        assert!(is_installed(&via_dots, [real.clone()]));
        #[cfg(unix)]
        {
            let link = d.path().join("link");
            std::os::unix::fs::symlink(&real, &link).unwrap();
            assert!(
                is_installed(&link, [real.clone()]),
                "a link to an installed browser"
            );
        }
        assert!(is_installed(
            std::path::Path::new("/no/such/chrome"),
            [std::path::PathBuf::from("/no/such/chrome")]
        ));
        assert!(!is_installed(&real, [d.path().join("other")]));
    }

    #[test]
    fn the_windows_vm_marker_is_the_admin_only_registry_value() {
        let out =
            "\r\nHKEY_LOCAL_MACHINE\\SOFTWARE\\Paddock\r\n    TestVM    REG_DWORD    0x1\r\n\r\n";
        assert!(reg_dword_is_one(out));
        assert!(!reg_dword_is_one(&out.replace("0x1", "0x0")));
        assert!(!reg_dword_is_one(&out.replace("REG_DWORD", "REG_SZ")));
        assert!(!reg_dword_is_one(""));
        assert!(
            WINDOWS_VM_KEY.starts_with("HKLM\\"),
            "machine-wide: only an administrator writes it"
        );
        if !cfg!(windows) {
            assert!(!windows_vm_marked());
        }
    }
}
