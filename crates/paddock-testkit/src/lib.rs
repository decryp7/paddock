//! The Rust test kit of the Paddock apps (plan X40, rule 7 and rule 40). Add it as a `[dev-dependencies]` entry.
//!
//! - [`Scratch`]: a temporary folder per test (work, home and tmp folders) whose `tmp` folder must be empty when the
//!   test ends: code that leaves files in "the temp folder" fails the test that ran it. Replaces `std::env::temp_dir()`
//!   in tests (clippy forbids it, `lint/clippy.toml`).
//! - [`EnvLock`]: one process-wide lock for tests that read or write environment variables, restoring every variable it
//!   changed, and [`EnvLock::hermetic`], which points `HOME`, `APPDATA`, the XDG folders and `TMPDIR` into a [`Scratch`].
//! - [`golden`]: expected-output files; a difference fails with the first differing line, `PADDOCK_UPDATE_GOLDEN=1`
//!   rewrites them.
//! - [`FakeRunner`]: a scripted stand-in for "run an OS command" seams (launchctl, systemctl, reg, ...), recording every call.
//! - [`never_panics`] and [`corpus`]: run a parser over deterministic hostile inputs and fail on any panic.
//! - [`FakeClock`]: the deterministic clock of `paddock_jobs::time`, re-exported so a test needs one dependency.
//!
//! Nothing here touches the user's folders, starts a process or sleeps.

pub mod env;
pub mod golden;
pub mod hostile;
pub mod runner;
pub mod scratch;

pub use env::{EnvLock, env_lock};
pub use golden::{Golden, GoldenError, GoldenMode, assert_golden, check_golden};
pub use hostile::{corpus, never_panics};
pub use paddock_jobs::time::{Clock, FakeClock};
pub use runner::{Call, FakeRunner, Reply};
pub use scratch::{Leftovers, Scratch, ScratchError};
