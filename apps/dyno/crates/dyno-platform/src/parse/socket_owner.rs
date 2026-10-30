//! The owners of a socket that several processes hold, pure so it is tested on every OS.

/// The owners among the processes that hold one socket: every holder whose parent does not hold it too. A child holds
/// its parent's sockets between fork and exec and keeps one it inherited, so it is not an owner; unrelated holders (two
/// programs sharing a port with `SO_REUSEPORT`, a socket passed over a Unix socket) all are. Sorted, so the answer never
/// depends on the order the OS listed them in. `parent` gives a process's parent when it is known.
pub fn owners(holders: &[u32], parent: impl Fn(u32) -> Option<u32>) -> Vec<u32> {
    let mut out: Vec<u32> = holders
        .iter()
        .copied()
        .filter(|&pid| parent(pid).is_none_or(|p| p == pid || !holders.contains(&p)))
        .collect();
    out.sort_unstable();
    out.dedup();
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Review P2 (2026-10-30): a child holds its parent's sockets between fork and exec (and keeps one it was given), so
    /// a listener can come back with two pids; it is the parent's, not whichever the OS listed first (CI saw the sibling
    /// test's child, 11788, instead of 11787).
    #[test]
    fn a_child_holding_its_parents_socket_is_not_an_owner() {
        let parent = |pid: u32| match pid {
            11788 => Some(11787),
            11787 => Some(1),
            30 => Some(20),
            20 => Some(10),
            _ => None,
        };
        assert_eq!(owners(&[11788, 11787], parent), [11787]);
        assert_eq!(owners(&[11787, 11788], parent), [11787]);
        assert_eq!(owners(&[30, 20, 10], parent), [10], "a chain of inheritance ends at its root");
        assert_eq!(owners(&[5, 5], parent), [5]);
        assert!(owners(&[], parent).is_empty());
        // Unrelated holders all own it, in pid order whatever order the OS used.
        assert_eq!(owners(&[42, 7], |_| None), [7, 42]);
        assert_eq!(owners(&[7, 42], |_| None), [7, 42]);
    }
}
