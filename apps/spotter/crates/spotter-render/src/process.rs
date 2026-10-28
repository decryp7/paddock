//! A supervised child process whose whole tree is ended on drop, on timeout and when it exceeds a memory cap: the shared
//! `paddock-proc` implementation (a process group on Unix, a kill-on-close job object on Windows).

pub use paddock_proc::{Supervised, kill_tree, processes_mentioning};

/// A live read of the memory of the process tree at `root`: the shared real-memory figure (PSS on Linux, physical
/// footprint on macOS, private working set on Windows), reading the pids of the tree only and never the machine's whole
/// process table (ADR 0024 rule 4: the watchdog calls this every few seconds).
pub fn current_tree_memory(root: u32) -> u64 {
    paddock_proc::tree_memory(root)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_tree_memory_of_this_process_is_read_and_a_missing_one_is_nothing() {
        assert!(current_tree_memory(std::process::id()) > 0);
        assert_eq!(current_tree_memory(u32::MAX - 1), 0);
    }
}
