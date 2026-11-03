//! Power: keep awake (ADR 0032). The pure side: what the user asks for ([`awake::AwakeSpec`]), the state machine that decides
//! when to take and release the OS hold ([`awake::AwakeMachine`], time passed in), the settings and the wire view. The OS
//! calls live in `paddock-power`, run by the agent's power thread.

pub mod awake;
pub mod energy;
pub mod idle;
pub mod schedule;
pub mod settings;

use serde::{Deserialize, Serialize};

/// Whether a power capability works here; the UI never guesses (ADR 0032 section 7).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Capability {
    /// It works.
    #[default]
    Known,
    /// Dyno cannot tell, or it does not work here; `reason` is plain words for the user.
    CantTell {
        /// Why.
        reason: String,
    },
}

/// What this machine can do for each power feature. Appended to as features land.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct PowerCaps {
    /// Holding the computer awake.
    pub keep_awake: Capability,
    /// Putting the computer to sleep the normal way.
    pub sleep: Capability,
    /// Hibernating.
    pub hibernate: Capability,
    /// Shutting down the normal way.
    pub shut_down: Capability,
}

/// Everything the Power page shows. Appended to as features land.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct PowerView {
    /// The keep-awake state.
    pub awake: awake::AwakeView,
    /// The idle and timed actions' state.
    pub idle: idle::IdleView,
    /// What works here.
    pub caps: PowerCaps,
}
