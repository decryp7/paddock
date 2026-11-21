//! The Rust test kit of the Paddock apps (plan X40, rule 7 and rule 40). Add it as a `[dev-dependencies]` entry.
//!
//! - [`Scratch`]: a temporary folder per test (work, home and tmp folders) whose `tmp` folder must be empty when the
//!   test ends: code that leaves files in "the temp folder" fails the test that ran it. Replaces `std::env::temp_dir()`
//!   in tests (clippy forbids it, `lint/clippy.toml`).
//! - [`canonical_tempdir`]: one plain temporary folder whose path is canonical (macOS `/var` is `/private/var`), for a test
//!   that compares it with a path the code under test resolved.
//! - [`private_tempdir`]: the drop-in for `tempfile::tempdir()` for a folder given to code that refuses a data folder other accounts can
//!   write: owner-only on Windows too, whatever the temp folder's access list is (CI runners' is open to Authenticated Users).
//! - [`EnvLock`]: one process-wide lock for tests that read or write environment variables, restoring every variable it
//!   changed, and [`EnvLock::hermetic`], which points `HOME`, `APPDATA`, the XDG folders and `TMPDIR` into a [`Scratch`].
//! - [`TestCommand`]: the one way a test starts `./dev`, `cargo` or `node`: an empty environment plus a listed one, a private
//!   home and temp folder, a curated `PATH` (ADR 0100, check C123).
//! - [`golden`]: expected-output files; a difference fails with the first differing line, `PADDOCK_UPDATE_GOLDEN=1`
//!   rewrites them.
//! - [`FakeRunner`]: a scripted stand-in for "run an OS command" seams (launchctl, systemctl, reg, ...), recording every call.
//! - [`StandIn`]: one stand-in program for any program a test plants (`cargo`, `git`, `npm`, ...), the same on Windows, that
//!   prints what it is told, records its calls, sleeps, starts a grandchild and exits with a code; replaces `#!/bin/sh` fixtures.
//! - [`never_panics`] and [`corpus`]: run a parser over deterministic hostile inputs and fail on any panic.
//! - [`net::HeldPort`]: a loopback port held for the test where every connection is closed or reset, instead of a
//!   port read off a listener that was then dropped (ADR 0023).
//! - [`net::broken_server`]: a loopback server that breaks on purpose ([`net::Mode`]: accepts and never answers, drips, cuts in the
//!   middle of an answer, refuses; and a healthy control), for a test of code that talks to a service.
//! - [`limits`]: `limit_cases!`, one test per limit (ADR 0019), stop and cut forms, and the check that cases and rows match.
//! - [`FakeClock`]: the deterministic clock of `paddock_jobs::time`, re-exported so a test needs one dependency.
//! - [`wait`]: `wait_until` / `wait_for` / `poll_for` / `poll_until_within` / `retry_within`, a bounded poll on a condition instead of a sleep.
//! - [`guarded`], [`Progress`], [`StallGuard`]: a slow job (a whole-repository scan) judged on progress, not total time: it fails
//!   only when no unit is finished for the stall limit (ADR 0071).
//! - [`wait_progressing`]: a wait for several steps in a row, judged on a level that moves, never on a total summed over the steps
//!   (ADR 0103).
//! - [`cases`]: [`both`] and [`run_cases`], independent cases of one test side by side, so it takes the slowest case, not the sum
//!   (ADR 0103).
//!
//! - The wait for a program the test started (`wait_ready_or_exit`, ADR 0071) is in `paddock-testkit-start`: it drives
//!   `paddock-proc`'s start judge, and `paddock-proc`'s own tests use this kit, so the kit cannot depend on it.
//! - [`boot_volume`]: [`boot_volume::boot_volume_temp`], the folder for programs a test starts as their own responsible
//!   process (disclaimed or by launchd): never on an external volume, where macOS prompts (TCC, Removable Volumes).
//! - [`exe`]: `write_program`, `spawn`, `output` and `status`, the way a test writes a program and starts one (the Linux `ETXTBSY`
//!   refusal of a file that was written or ended a moment ago is retried for a bounded time; plain `fs::write` plus `Command` is
//!   not).
//! - [`cargo_layout`]: `profile_dir()`, `sibling_bin` and `example_bin`, the programs cargo built for this test, found in both
//!   build-folder layouts (before cargo 1.100 and after) and refusing any other shape.
//!
//! Nothing here touches the user's folders or starts a process on its own, except [`TestCommand`] (a child with a listed environment) and [`boot_volume`], which runs the shared rule
//! (`conformance/ci/boot-volume.sh`) once per process on macOS; the only real time is [`wait`]'s poll interval.

pub mod ambient;
pub mod boot_volume;
mod broken_server;
pub mod cargo_layout;
pub mod cases;
pub mod env;
pub mod exe;
pub mod golden;
pub mod hostile;
pub mod limits;
pub mod net;
pub mod path;
pub mod private_temp;
pub mod process;
pub mod runner;
pub mod scratch;
pub mod stand_in;
mod stand_in_json;
pub mod wait;

pub use ambient::{Ambient, ambient, tempdir};
pub use cases::{both, case, run_cases};
pub use env::{EnvLock, env_lock};
pub use golden::{Golden, GoldenError, GoldenMode, assert_golden, check_golden};
pub use hostile::{assert_never_panics, corpus, never_panics};
pub use paddock_env::proptest_cases;
pub use paddock_env::winpath::{real_or_given, same};
pub use paddock_jobs::time::{Clock, FakeClock};
pub use private_temp::{PrivateTemp, private_tempdir};
pub use process::{TestChild, TestCommand};
pub use runner::{Call, FakeRunner, Reply};
pub use scratch::{
    CanonicalTempDir, Leftovers, Scratch, ScratchError, canonical_tempdir, try_canonical_tempdir,
};
pub use stand_in::{
    Actions, Case, Exec, Files, Installed, Invocation, MAX_SLEEP, Spawn, StandIn, template_literal,
};
pub use wait::{
    HANG_GUARD, Look, Progress, StallGuard, guarded, poll_for, poll_until_within, recv,
    recv_within, retry_within, wait_for, wait_for_within, wait_progressing, wait_until,
    wait_until_within,
};
