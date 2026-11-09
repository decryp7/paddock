//! `wiki`, the LLM-wiki tool: `lint` checks a project wiki (the mechanical checks, the same for every project), `log`
//! appends a correctly formatted dated line to its log, `new` creates a page and its index line. It follows
//! `paddock-cli`'s conventions (`--json` makes standard output exactly one object, errors name the next step); its exit codes
//! are `paddock_cli::Exit`'s: 0 ok, 1 an I/O failure (an unreadable wiki folder, settings or rules file), 2 a wrong command
//! line, 4 refused (a page that already exists), 5 findings.

#![cfg_attr(test, allow(clippy::unwrap_used, clippy::expect_used))]

mod cli;
pub mod date;
pub mod lint;
pub mod ops;
pub mod sources;
pub mod text;
pub mod trace;

pub use cli::{Context, run, run_with};
