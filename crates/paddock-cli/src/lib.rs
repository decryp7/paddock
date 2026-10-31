//! Command-line conventions shared by the Paddock apps (plan X25).
//!
//! Every app's CLI (Pitwall, Dyno, Debrief, the agent and service programs) looks and behaves alike:
//!
//! - [`Exit`] and [`CliError`]: stable exit codes, and an error is a message that says what to do next.
//! - [`Format`] and [`Report`]: `--json` makes standard output exactly one JSON object, failures included
//!   (`{"ok":false,"error":..,"hint":..,"exit":..,"kind":..}`; a report that did its work and ended unsuccessfully
//!   carries its data too, with `error` and `hint` when it names them); text is for people. Data goes to stdout, notes and progress to stderr.
//! - [`Table`]: results as aligned text, CSV or JSON, bounded cells, a truncation note on stderr.
//! - [`Progress`]: throttled, de-duplicated phase lines on stderr, driven by an injected clock; [`StopFlag`] is the
//!   Ctrl+C request an app's signal handler sets.
//! - [`Help`] and [`version_line`]: `--help` and `--version` in one style.
//! - [`CommonFlags`]: `--json`, `--quiet`, `--help`, `--version` taken out of the arguments before the app parses its own.
//! - [`parse_time`], [`format_size`], [`parse_size`]: values on the command line, read once into typed values.
//!
//! Pure: nothing here reads the environment, the clock or the terminal; writers and times are passed in.

mod error;
mod exit;
mod flags;
mod help;
mod progress;
mod report;
mod table;
#[cfg(test)]
mod testutil;
mod values;

pub use error::CliError;
pub use exit::Exit;
pub use flags::{CommonFlags, Format};
pub use help::{Help, version_line};
pub use progress::{LINE_EVERY, Progress, StopFlag};
pub use report::{Report, finish};
pub use table::{MAX_CELL_CHARS, Table, TableFormat, cell_text, write_table};
pub use values::{format_size, parse_size, parse_time};
