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
//! - [`net::HeldPort`]: a loopback port held for the test where every connection is closed or reset, instead of a
//!   port read off a listener that was then dropped (ADR 0023).
//! - [`limits`]: `limit_cases!`, one test per limit (ADR 0019), stop and cut forms, and the check that cases and rows match.
//! - [`FakeClock`]: the deterministic clock of `paddock_jobs::time`, re-exported so a test needs one dependency.
//! - [`wait`]: `wait_until` / `wait_for` / `poll_for` / `poll_until_within` / `retry_within`, a bounded poll on a condition instead of a sleep.
//! - [`guarded`], [`Progress`], [`StallGuard`]: a slow job (a whole-repository scan) judged on progress, not total time: it fails
//!   only when no unit is finished for the stall limit (ADR 0071).
//!
//! - [`boot_volume`]: [`boot_volume::boot_volume_temp`], the folder for programs a test starts as their own responsible
//!   process (disclaimed or by launchd): never on an external volume, where macOS prompts (TCC, Removable Volumes).
//!
//! Nothing here touches the user's folders or starts a process, except [`boot_volume`], which runs the shared rule
//! (`conformance/ci/boot-volume.sh`) once per process on macOS; the only real time is [`wait`]'s poll interval.

pub mod boot_volume;
pub mod env;
pub mod golden;
pub mod hostile;
pub mod limits;
pub mod net;
pub mod runner;
pub mod scratch;
pub mod wait;

pub use env::{EnvLock, env_lock};
pub use golden::{Golden, GoldenError, GoldenMode, assert_golden, check_golden};
pub use hostile::{assert_never_panics, corpus, never_panics};
pub use paddock_env::winpath::{real_or_given, same};
pub use paddock_jobs::time::{Clock, FakeClock};
pub use runner::{Call, FakeRunner, Reply};
pub use scratch::{Leftovers, Scratch, ScratchError};
pub use wait::{
    HANG_GUARD, Progress, StallGuard, guarded, poll_for, poll_until_within, retry_within, wait_for,
    wait_for_within, wait_until, wait_until_within,
};
