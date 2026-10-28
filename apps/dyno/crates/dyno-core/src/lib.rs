//! Dyno's pure core: the metrics and process model, grouping, diagnosis, anomaly detection,
//! health, alarms, history downsampling and the agent protocol. No I/O and no clock: the agent
//! passes measurements and time in as values (ADR 0001).
#![cfg_attr(test, allow(clippy::unwrap_used, clippy::expect_used, clippy::field_reassign_with_default))]

pub mod activity;
pub mod alarms;
pub mod alerts;
pub mod anomaly;
pub mod backup;
pub mod baseline;
pub mod bench;
pub mod counters;
pub mod diagnosis;
pub mod diagnostics;
pub mod disk;
pub mod extras;
pub mod facts;
pub mod forecast;
pub mod grouping;
pub mod hardware;
pub mod health;
pub mod helper;
pub mod helper_paths;
pub mod helper_wire;
pub mod history;
pub mod inventory;
pub mod ip_lookup;
pub mod issues;
pub mod lifespan;
pub mod metrics;
pub mod netchange;
pub mod network;
pub mod network_alerts;
pub mod network_names;
pub mod network_traffic;
pub mod os;
pub mod others;
pub mod ports;
pub mod process;
pub mod protocol;
pub mod recorder;
pub mod report;
pub mod sampling;
pub mod settings;
pub mod snapshot;
pub mod startup;
pub mod stats;
pub mod throttle;
pub mod time;
pub mod toolpath;
pub mod tray;
pub mod units;
pub mod unusual;
pub mod weblinks;
pub mod widgets;
