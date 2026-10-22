//! Runner auto-update: the policy (whether and when) and the state machine (how, with roll-back).

pub mod machine;
pub mod policy;

pub use machine::{UpdateCommand, UpdateEvent, UpdateMachine, UpdateOutcome, UpdatePhase, UpdatePlan, UpdateTimeouts};
pub use policy::{LocalMoment, TimeOfDay, UpdateDecision, UpdateMode, UpdatePolicy, UpdateWindow, Weekday};
