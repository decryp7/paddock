//! A supervised child process whose whole tree is ended on drop, on timeout and when it exceeds a memory cap: the shared
//! `paddock-proc` implementation (a process group on Unix, a kill-on-close job object on Windows).

pub use paddock_proc::{end_all, kill_tree, processes_mentioning, Supervised};
