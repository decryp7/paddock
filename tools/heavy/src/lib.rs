//! `heavy`: one heavy step (cargo, npm, playwright, llvm-cov) at a time per machine, replacing the shell `plock` and
//! engineering-rules `scripts/heavy.sh` (ER-24). A pure core over ports (clock, process table, load average) with the real
//! adapters at the edge, the shell plock's on-disk layout so shell and Rust interoperate, and a built-in CLEF troubleshooting
//! log (rule 56).
//!
//! Design lessons made impossible (see the README): liveness compares the OS start time with the record's epoch (never a
//! formatted time); every read tolerates the file vanishing; deletions name exact paths; a lock is published with its
//! owner inside; a non-empty unparsable owner is never broken; a break is rename-aside, re-check, put back; EPERM is alive;
//! commands are masked; waits are logged; the queue is FIFO with `--high` first.

pub mod cli;
pub mod clock;
pub mod config;
pub mod exit;
pub mod gate;
pub mod guard;
pub mod lock;
pub mod log;
pub mod mask;
pub mod probe;
pub mod procs;
pub mod record;
pub mod runner;
pub mod timefmt;
