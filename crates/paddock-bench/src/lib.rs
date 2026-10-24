//! Benchmark harness with stored budgets (plan X26, rule 16).
//!
//! The flow, the same in every app:
//!
//! 1. The app's benchmark (a `cargo test --release` target, a script) records [`Measurement`]s with a [`Recorder`]: a
//!    [`Kind`] says what is measured (startup, throughput, time to first event, query latency, memory) and so which way
//!    is better and the unit. Time comes from a [`Clock`], so the harness logic is tested with [`ManualClock`] and no
//!    machine speed.
//! 2. [`Results::save`] writes the run to the app's results file (`bench/<app>.results.json`, versioned, atomic).
//! 3. The budgets live in the app's `bench/budgets.toml` ([`Budgets`]): a limit per measurement, a tolerance, and how much
//!    worse than the previous run counts as a regression.
//! 4. [`check`] compares results with budgets (and the previous results) and gives a [`Verdict`] per measurement; the
//!    `paddock-bench check` command prints it and exits 5 when anything missed, for the nightly workflow
//!    (`.gitea/workflow-templates/bench-nightly.yml`).
//!
//! Nothing here reads the system clock unless the caller passes [`SystemClock`], and nothing measures memory: the app
//! passes the numbers (`paddock-proc` has the process-memory reader).

mod budget;
mod check;
mod cli;
mod clock;
mod error;
mod kind;
mod recorder;
mod results;
mod stats;

pub use budget::{Budget, Budgets, DEFAULT_REGRESSION_PCT, DEFAULT_TOLERANCE_PCT, MAX_BUDGETS};
pub use check::{Outcome, Row, Summary, Verdict, check};
pub use cli::run;
pub use clock::{Clock, ManualClock, SystemClock};
pub use error::{BenchError, Result};
pub use kind::{Direction, Kind};
pub use recorder::Recorder;
pub use results::{Host, MAX_FILE_BYTES, MAX_MEASUREMENTS, Measurement, RESULTS_SCHEMA, RESULTS_VERSION, Results};
pub use stats::{Samples, summarize};
