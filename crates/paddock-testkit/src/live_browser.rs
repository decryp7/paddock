//! The gate of every test that starts a real browser (Paddock rule 7: tests never put anything on the user's desktop).
//!
//! A real test browser (the pinned Chrome for Testing) starts only on Linux, in a job that says it is a throwaway test
//! session: it sets [`LIVE_BROWSER_VAR`] to `1` and, for a browser with a window, runs under a virtual display
//! (`xvfb-run`). The Mac and Windows CI runners run in a user's session, where even a headless or off-screen browser is a
//! process of that session (a Dock or taskbar entry, the focus), so these tests never run there. A test that is not run
//! prints where it runs instead; a job set up wrongly fails instead of skipping.
//!
//! The test-browser finders of the drivers (`paddock-chromium`, Debrief's `chromium-driver`, Spotter's `spotter-render`)
//! go through [`find_test_browser`], so a test cannot get a browser around the gate.

/// Set to `1` by a test job that may start a real browser: Linux CI containers, under `xvfb-run` for a browser with a
/// window. Honoured on Linux only.
pub const LIVE_BROWSER_VAR: &str = "PADDOCK_LIVE_BROWSER";

/// How the browser a test wants to start shows itself.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BrowserWindow {
    /// No window (`--headless`): no display needed.
    Headless,
    /// A window, on or off screen: needs a display, which must be a virtual one.
    Shown,
}

/// Whether a test may start a real browser here.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LiveBrowser {
    /// Linux, the job's [`LIVE_BROWSER_VAR`], and a display when the browser has a window.
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

/// [`LiveBrowser`] for the OS `os` (as `std::env::consts::OS` names it), the environment `env` and the browser's
/// `window`. `Err` is a job set up wrongly, which fails the test: the variable for a browser with a window but no display
/// (no `xvfb-run`), or `require_var` (the app's "a missing test browser fails" switch) set on Linux without the variable,
/// where every real-browser test would skip.
pub fn live_browser_with(
    os: &str,
    env: &dyn Fn(&str) -> Option<String>,
    window: BrowserWindow,
    require_var: &str,
) -> Result<LiveBrowser, String> {
    if os != "linux" {
        return Ok(LiveBrowser::NotHere(format!(
            "this test starts a real browser, so it runs only in the Linux CI job ({LIVE_BROWSER_VAR}=1, under xvfb-run), \
             never in a {os} user session (Paddock rule 7)"
        )));
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
             never on a desktop (Paddock rule 7)"
        ))),
    }
}

/// A driver's test-browser finder: the gate first ([`live_browser_with`]); where it allows the browser, `lookup` (the
/// driver's own search for the pinned build, run only then). A browser that is missing where `require_var` is set is an
/// `Err`, like a job set up wrongly; one that is missing otherwise is a [`TestBrowser::Skip`] that names `hint`.
pub fn find_test_browser<B>(
    os: &str,
    env: &dyn Fn(&str) -> Option<String>,
    window: BrowserWindow,
    require_var: &str,
    hint: &str,
    lookup: impl FnOnce() -> Option<B>,
) -> Result<TestBrowser<B>, String> {
    match live_browser_with(os, env, window, require_var)? {
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
                    live_browser_with(os, &env(XVFB), window, REQUIRE)
                else {
                    panic!("{os} {window:?} was allowed")
                };
                assert!(
                    why.contains("Linux") && why.contains(LIVE_BROWSER_VAR) && why.contains(os),
                    "{why}"
                );
                let looked = std::cell::Cell::new(false);
                let found = find_test_browser(os, &env(XVFB), window, REQUIRE, "hint", || {
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
                live_browser_with("linux", &env(XVFB), window, REQUIRE),
                Ok(LiveBrowser::Allowed)
            );
        }
        let wayland = env(&[(LIVE_BROWSER_VAR, "1"), ("WAYLAND_DISPLAY", "w")]);
        assert_eq!(
            live_browser_with("linux", &wayland, BrowserWindow::Shown, REQUIRE),
            Ok(LiveBrowser::Allowed)
        );
        // Headless needs no display.
        let no_display = env(&[(LIVE_BROWSER_VAR, "1")]);
        assert_eq!(
            live_browser_with("linux", &no_display, BrowserWindow::Headless, REQUIRE),
            Ok(LiveBrowser::Allowed)
        );
        // A developer's Linux desktop (a display the job did not vouch for), or "0": not there.
        for pairs in [
            &[("DISPLAY", ":0")][..],
            &[(LIVE_BROWSER_VAR, "0"), ("DISPLAY", ":99")][..],
        ] {
            assert!(matches!(
                live_browser_with("linux", &env(pairs), BrowserWindow::Shown, REQUIRE),
                Ok(LiveBrowser::NotHere(_))
            ));
        }
    }

    #[test]
    fn a_job_set_up_wrongly_fails_instead_of_skipping() {
        let e = live_browser_with(
            "linux",
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
            let e = live_browser_with("linux", &env(pairs), BrowserWindow::Headless, REQUIRE)
                .unwrap_err();
            assert!(e.contains(LIVE_BROWSER_VAR) && e.contains(REQUIRE), "{e}");
        }
        // A blank or "0" switch is not set: the tests say where they run instead.
        for pairs in [&[(REQUIRE, " ")][..], &[(REQUIRE, "0")][..]] {
            assert!(matches!(
                live_browser_with("linux", &env(pairs), BrowserWindow::Headless, REQUIRE),
                Ok(LiveBrowser::NotHere(_))
            ));
        }
    }

    #[test]
    fn the_finder_looks_only_where_allowed_and_a_required_browser_must_be_there() {
        assert_eq!(
            find_test_browser(
                "linux",
                &env(XVFB),
                BrowserWindow::Shown,
                REQUIRE,
                "hint",
                || Some(7)
            ),
            Ok(TestBrowser::Found(7))
        );
        let e = find_test_browser(
            "linux",
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
            "linux",
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
                "linux",
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
}
