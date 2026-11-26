//! `throttle`: one heavy step (cargo, npm, playwright, llvm-cov) at a time per machine, replacing the shell `plock` and
//! engineering-playbook `scripts/heavy.sh` (ER-24). A pure core over ports (clock, process table, load average) with the real
//! adapters at the edge (kernel file locks held by handle), and a built-in CLEF troubleshooting log (rule 56).
//!
//! Design lessons made impossible (see the README): a slot is held by an open file with a kernel lock, so a holder that exits or is
//! killed frees it at once and no code judges whether an owner lives or breaks a lock; the lock is not inherited by the command; the
//! lock folder is never on a network file system; every read tolerates the file vanishing; deletions name exact paths; commands are
//! masked; waits are logged; the queue is FIFO with `--high` first. Ticket and marker liveness still compares a pid with the OS start
//! time (never a formatted time; EPERM is alive).

pub mod capture;
pub mod cli;
pub mod clock;
pub mod config;
pub mod core;
pub mod exit;
pub mod fsinfo;
pub mod gate;
pub mod guard;
pub mod hang;
pub mod history;
pub mod lock;
pub mod log;
pub mod mask;
pub mod probe;
pub mod procs;
pub mod record;
pub mod runner;
pub mod slotfile;
pub mod timefmt;
pub mod waitfor;
