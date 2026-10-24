//! Service health for the Paddock apps: service state, cause and repair action, the registry and its snapshot, and the
//! cause-key notification centre with versioned persistence.
//!
//! ADR 0010, ADR 0012 §3; plan S3. Taken from Debrief's `health/model.rs`, `health/registry.rs`, the generic half of
//! `health/hub.rs` (repairs) and `notification_centre/` (Debrief ADR 0055), without Debrief's services, sources or
//! actions.
//!
//! - [`model`]: the closed [`ServiceState`], the plain [`Cause`] with its next step, the [`RepairAction`], the
//!   [`Snapshot`] every window reads.
//! - [`Health`]: the app registers its services ([`ServiceSpec`]: id, label, repair closure); supervisors report into
//!   it; each real change is announced once to a [`Sink`]; [`Health::repair`] runs the repair on a background thread in
//!   the shell, never through the broken service (Paddock rule 19).
//! - [`notify`]: the cause-key notification centre: one message per cause, updated in place, resolved when the cause
//!   clears; app messages saved in a versioned file, window messages in the window's project. [`notify::health`] turns
//!   a health change into its one message.
//! - [`notify::os`]: OS notifications over the centre: one policy, rate limits, quiet hours, buttons and clicks (plan X8).
//! - `tauri` (feature `tauri`): the `paddock://service-health` and `paddock://notifications` events to
//!   every window, the commands (`health_snapshot`, `health_repair`, `notifications_*`) and their table.
//!
//! ```
//! use paddock_health::{Cause, Health, RepairAction, ServiceSpec, ServiceState};
//!
//! let health = Health::new(
//!     vec![ServiceSpec::new("llm", "the model server").repair(|| Ok(()))],
//!     Box::new(|snapshot, change| println!("{} is {:?} (revision {})", change.service, change.to, snapshot.revision)),
//! )
//! .unwrap();
//! // The supervisor saw the server stop: one announcement, however often it says so.
//! health.down("llm", Cause::new("The model server stopped.").hint("Choose Restart."), Some(RepairAction::Restart));
//! assert!(!health.down("llm", Cause::new("The model server stopped.").hint("Choose Restart."), Some(RepairAction::Restart)));
//! let id = health.begin_repair("llm").unwrap();
//! health.finish_repair(&id); // `Health::repair` does both on a background thread.
//! assert_eq!(health.state("llm"), Some(ServiceState::Ok));
//! ```
//!
//! The core is pure Rust with no Tauri dependency; the adapter lives in `tauri` behind the `tauri` feature.

pub mod error;
pub mod health;
pub mod model;
pub mod notify;
pub mod registry;

pub use error::{HealthError, HealthErrorCode};
pub use health::{Health, RepairRefused};
pub use model::{
    Cause, Change, RaiseCap, RepairAction, SERVICE_HEALTH_EVENT, ServiceHealth, ServiceState,
    Snapshot, cause_key,
};
/// The family's clock port (`paddock_jobs::time`): the notification centre reads [`Clock::wall_ms`].
pub use paddock_jobs::time::{Clock, FakeClock, SystemClock};
pub use registry::{Registry, Repair, ServiceSpec, Sink, SpecError};

#[cfg(feature = "tauri")]
pub mod tauri;

#[cfg(test)]
mod tests;
