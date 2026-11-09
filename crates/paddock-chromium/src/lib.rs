//! The one Chromium driver of the Paddock apps (plan X14), from Debrief's `chromium-driver` and Spotter's
//! `spotter-render`, which had drifted apart. It has no application types.
//!
//! * [`discover`]: Edge, Chrome or Chromium on Windows, macOS and Linux (pure candidate lists), the app's downloaded
//!   Chrome for Testing, and for tests only `CHROMIUM_TEST_PATH`, never the browser the user works in (rule 7).
//! * [`launch`]: one function builds the whole command line ([`launch::command_line`]: the automation banner stays on,
//!   no stealth; the start page is always last), a throwaway or kept profile with an accent theme, and the browser as a
//!   `paddock_proc::Supervised` tree with a time limit on an injected clock and a memory cap (PSS on Linux).
//! * [`cdp`] / [`page`]: a small synchronous DevTools client and a page handle (navigate, back, evaluate, snapshot,
//!   wait until readable, capture, screenshot, block heavy resources), which knows when the user closed its window.
//! * [`ready`] and [`challenge`]: pure rules for "the page can be read" and "this page needs the user".
//! * [`cue`]: the title text that marks a window as AI-driven. Nothing is ever injected into a page unless the app asks
//!   for DOM-change tracking ([`page::Page::track_dom_changes`]).
//! * [`runtime`]: the pinned Chrome for Testing, installed through `paddock-runtime` as a job.
//! * `fake` (feature `test-support`, Unix): a fake DevTools browser for tests.

mod answer;
pub mod cdp;
pub mod challenge;
pub mod cue;
pub mod discover;
#[cfg(all(unix, any(test, feature = "test-support")))]
pub mod fake;
pub mod launch;
pub mod page;
pub mod ready;
pub mod runtime;

/// Set (to anything) in the environment of a test's fake browser process. Not behind `test-support`: an app checks it at
/// start-up in every build and refuses to run as a fake browser, so a fake that names the wrong binary never opens the
/// app's windows on the desktop.
pub const FAKE_CHILD_ENV: &str = "PADDOCK_FAKE_BROWSER";

pub use challenge::{Challenge, Signals, detect as detect_challenge};
pub use discover::{Browser, Flavor};
pub use launch::{Chromium, EndedBy, Gone, LaunchOptions, Profile, Sandbox, Window, WindowBounds};
pub use page::{Info, Outcome, Page, Snapshot};

/// Everything that can go wrong; messages tell the user what to do.
#[derive(Debug)]
pub enum Error {
    /// No browser is installed or downloaded.
    NotFound,
    /// The process could not start (with the end of its error output when there was any).
    Launch(String),
    /// A step took too long.
    Timeout(&'static str),
    /// The DevTools connection failed.
    Connection(String),
    /// The browser rejected a command, or a page script failed.
    Protocol(String),
    /// The browser or the page is gone (closed by the user, crashed, ended by its time or memory limit).
    Closed,
    /// Installing the pinned browser failed.
    Runtime(paddock_runtime::RuntimeError),
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Error::NotFound => f.write_str(
                "no Chromium-based browser was found; install Edge, Chrome or Chromium, or download one in Settings",
            ),
            Error::Launch(m) => write!(f, "the browser could not be started: {m}"),
            Error::Timeout(what) => write!(f, "the browser did not answer in time ({what})"),
            Error::Connection(m) => write!(f, "the browser connection failed: {m}"),
            Error::Protocol(m) => write!(f, "the browser reported an error: {m}"),
            Error::Closed => f.write_str("the browser was closed"),
            Error::Runtime(e) => e.fmt(f),
        }
    }
}

impl std::error::Error for Error {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Error::Runtime(e) => Some(e),
            _ => None,
        }
    }
}

impl From<paddock_runtime::RuntimeError> for Error {
    fn from(e: paddock_runtime::RuntimeError) -> Self {
        Error::Runtime(e)
    }
}

/// Driver result.
pub type Result<T> = std::result::Result<T, Error>;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_error_says_what_happened() {
        let all = [
            Error::NotFound,
            Error::Launch("x".into()),
            Error::Timeout("the page load"),
            Error::Connection("refused".into()),
            Error::Protocol("bad".into()),
            Error::Closed,
            Error::Runtime(paddock_runtime::RuntimeError::Busy),
        ];
        for e in &all {
            assert!(!e.to_string().is_empty());
        }
        assert!(all[0].to_string().contains("download one"));
        assert!(std::error::Error::source(&all[6]).is_some());
        assert!(std::error::Error::source(&all[1]).is_none());
        assert!(matches!(
            Error::from(paddock_runtime::RuntimeError::Busy),
            Error::Runtime(_)
        ));
    }
}
