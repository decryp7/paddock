//! `wiki`, the LLM-wiki tool: `lint` checks a project wiki (the mechanical checks, the same for every project), `log`
//! appends a correctly formatted dated line to its log, `new` creates a page and its index line. It follows
//! `paddock-cli`'s conventions (`--json` makes standard output exactly one object, errors name the next step); its exit codes
//! are 0 ok, 1 findings, 2 usage or I/O errors.

#![cfg_attr(test, allow(clippy::unwrap_used, clippy::expect_used))]

mod cli;
pub mod date;
pub mod lint;
pub mod ops;
pub mod sources;
pub mod text;
pub mod trace;

pub use cli::{Context, run, run_with};
