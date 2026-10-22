//! Spotter's pure core (ADR 0001): everything that decides, nothing that does I/O or reads a clock.
//!
//! - [`version`]: version schemes and ordering; [`release`]: candidates, releases and selection;
//!   [`pattern`] / [`user_regex`]: name patterns with limits.
//! - [`source`]: the source kinds, their forms, request plans and response parsers, and the [`source::ReleaseSource`]
//!   port the adapters implement.
//! - [`watch`], [`state`], [`notify`], [`schedule`], [`action`]: what a watch is, what it remembers, when it
//!   notifies, when it checks next and what it does with a new release.
//! - [`config`] / [`store`]: the configuration and state files, versioned and tolerant of damage, with the migration
//!   from the old Release Monitor.
//! - [`journal`] / [`dashboard`]: the dashboard's bounded history and its numbers; [`filter`]: version prefix and soak time; [`pack`]: declarative presets and
//!   version schemes; [`import`]: winget, Brewfile and OPML lists; [`render`]: the port for JavaScript pages.
#![cfg_attr(test, allow(clippy::unwrap_used, clippy::expect_used))]

pub mod action;
pub mod checksum;
pub mod config;
pub mod dashboard;
pub mod filter;
pub mod import;
pub mod journal;
pub mod limits;
pub mod notify;
pub mod pack;
pub mod pattern;

pub mod release;
pub mod render;
pub mod schema;
pub mod schedule;
pub mod source;
pub mod state;
pub mod store;
pub mod time;
pub mod user_regex;
pub mod version;
pub mod watch;
