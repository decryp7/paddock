//! A small feed state shared by the tests.
#![allow(dead_code)]

use paddock_live::LiveState;
use serde::{Deserialize, Serialize};

/// What a server overview looks like in miniature: a latest-wins reading, an append-only list and a counter.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Overview {
    pub memory: u64,
    pub events: Vec<String>,
    pub count: u32,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum Change {
    Memory {
        bytes: u64,
    },
    Event {
        text: String,
    },
    Bump,
    /// Makes `apply` panic (a bug in an app's fold).
    Explode,
}

impl LiveState for Overview {
    type Delta = Change;

    fn apply(&mut self, delta: &Change) {
        match delta {
            Change::Memory { bytes } => self.memory = *bytes,
            Change::Event { text } => self.events.push(text.clone()),
            Change::Bump => self.count += 1,
            Change::Explode => panic!("the fold has a bug"),
        }
    }

    fn merge(prev: &mut Change, next: &Change) -> bool {
        match (prev, next) {
            (Change::Memory { bytes }, Change::Memory { bytes: newer }) => {
                *bytes = *newer;
                true
            }
            _ => false,
        }
    }
}

pub fn memory(bytes: u64) -> Change {
    Change::Memory { bytes }
}

pub fn event(text: &str) -> Change {
    Change::Event { text: text.into() }
}
