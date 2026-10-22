//! Dyno's pure core: the metrics and process model, grouping, diagnosis, anomaly detection, the rules engine,
//! cleanup policy, health and tuning, history downsampling and the agent protocol. No I/O and no clock: the agent
//! passes measurements and time in as values (ADR 0001).
#![cfg_attr(test, allow(clippy::unwrap_used, clippy::expect_used, clippy::field_reassign_with_default))]

pub mod activity;
pub mod alerts;
pub mod anomaly;
pub mod cleanup;
pub mod diagnosis;
pub mod facts;
pub mod forecast;
pub mod grouping;
pub mod health;
pub mod helper;
pub mod history;
pub mod inventory;
pub mod metrics;
pub mod ports;
pub mod process;
pub mod protect;
pub mod protocol;
pub mod report;
pub mod rules;
pub mod sampling;
pub mod settings;
pub mod snapshot;
pub mod startup;
pub mod stats;
pub mod time;
pub mod tray;
pub mod tuning;
pub mod units;
pub mod widgets;
