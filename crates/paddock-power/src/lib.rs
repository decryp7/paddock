//! Power for the Paddock apps (Dyno ADR 0032, D3).
//!
//! - [`PowerOs`] / [`Hold`]: one OS sleep-prevention object (macOS `IOPMAssertion`, Windows power request, Linux logind
//!   inhibitor). It is released on drop and by the OS when the process ends, so a crash never leaves the machine awake.
//! - [`KeepAwake`]: at most one hold, replaced without a gap, with logs that explain each decision (no user content).
//! - [`SleepWakeSource`]: sleep and wake notifications ([`PowerEvent`]).
//! - `FakePower` and `FakeEvents`: the doubles every test uses. They exist only for tests: behind `cfg(test)` and the
//!   `test-support` feature, which a consumer enables in its `[dev-dependencies]` only (Dyno ADR 0033), so a build without
//!   test targets cannot name them. No test in the family may call the real adapters ([`system_power`], [`system_events`])
//!   on a developer's or CI machine (ER-14); they run only in VMs. Building them is inert; calling them is not.
//!
//! The core is pure; the OS calls live in the one small file `native.rs`.

mod error;
#[cfg(any(target_os = "linux", test))]
mod event_thread;
#[cfg(any(test, feature = "test-support"))]
mod fake;
mod keep_awake;
mod model;
mod native;
mod port;

pub use error::PowerError;
#[cfg(any(test, feature = "test-support"))]
pub use fake::{FakeEvents, FakePower};
pub use keep_awake::{KeepAwake, LOG_TARGET};
pub use model::{HoldInfo, HoldRequest, MAX_TEXT_CHARS, PowerEvent, Scope};
pub use port::{EventSink, Hold, PowerOs, SleepWakeSource, Subscription};

pub use native::{events as system_events, power as system_power};
