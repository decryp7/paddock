//! The spotter-engine integration tests, one binary: each module is one area.
#![allow(clippy::unwrap_used, clippy::expect_used, missing_docs)]

mod coverage_gaps;
mod engine_api;
mod engine_flow;
mod engine_robustness;
mod failure_paths;
mod fetch_http;
mod folder_products;
mod long_operations;
mod mail_delivery;
mod release_gaps;
mod schedules;
