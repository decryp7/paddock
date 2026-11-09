//! The integration tests of this crate, one binary (linking one test binary per file cost more than running them).
//! Each module is a former `tests/<name>.rs`; run one with `cargo test --test it <name>::`.

mod common;
#[path = "support/duck_store.rs"]
mod duck_store;
mod scratch;

mod assistant_usecases;
mod browser;
mod chat;
mod find_names;
mod github_releases;
mod incident_ai;
mod net_tls;
mod parser_generation;
mod properties_knowledge;
mod query_transparency;
mod segmented_download;
mod sql_tool;
mod tools;
mod web;
