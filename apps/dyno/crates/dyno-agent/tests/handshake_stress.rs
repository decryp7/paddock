#![allow(clippy::unwrap_used, clippy::expect_used)]

//! Many short connections one after another, as a polling client makes them (CI run 7763: about 250 in 5 s). Every one is
//! welcomed and answered, and the agent gives back what a closed connection held, so a long-running agent that an app or a
//! script polls never runs out of file descriptors or threads. The agent's log is printed if it fails.

use dyno_agent::cli::{ask_within, connect};
use dyno_agent::paths::Dirs;
use dyno_agent::Running;
use dyno_core::protocol::{Reply, Request};
use dyno_platform::fake::Fake;
use std::time::Duration;

#[path = "common/agent_log.rs"]
mod agent_log;

/// Enough that a connection which kept even one descriptor would pass [`SLACK`] three times over, few enough to stay about
/// a second (300 took 61 s in the emulated CI image under CPU starvation).
const CONNECTIONS: usize = 120;
/// What a few connections in flight may hold above the start (a closed one is released at the agent's next accept).
const SLACK: usize = 40;

/// This process's open file descriptors, where the OS lists them.
fn open_descriptors() -> Option<usize> {
    let dir = if cfg!(target_os = "linux") { "/proc/self/fd" } else { "/dev/fd" };
    if cfg!(unix) {
        std::fs::read_dir(dir).ok().map(Iterator::count)
    } else {
        None
    }
}

#[test]
fn a_hundred_short_connections_in_a_row_are_all_answered_and_none_is_left_holding_descriptors() {
    let _log = agent_log::on_failure();
    let tmp = tempfile::tempdir().unwrap();
    let dirs = Dirs::at(tmp.path().join("data"));
    let fake = Fake::new();
    let running = Running::start(dirs.clone(), fake.platform(), Box::new(fake.clone()), Box::new(|_| Ok(()))).unwrap();
    let ask = || -> Result<(), String> {
        let peer = connect(&dirs)?.0;
        let reply = ask_within(&peer, Request::HelperStatus, Duration::from_secs(60));
        peer.close();
        match reply? {
            Reply::HelperStatus(_) => Ok(()),
            other => Err(format!("unexpected reply {other:?}")),
        }
    };
    // The first answer settles the agent's own start (its store, its first sample) before counting.
    ask().unwrap();
    let start = open_descriptors();
    let mut most = start;
    for i in 0..CONNECTIONS {
        if let Err(e) = ask() {
            panic!(
                "connection {i} of {CONNECTIONS} failed with {:?} descriptors open (at the start {start:?}): {e}",
                open_descriptors()
            );
        }
        most = most.max(open_descriptors());
    }
    if let (Some(start), Some(most)) = (start, most) {
        assert!(
            most <= start + SLACK,
            "closed connections were not given back: {most} descriptors open at most, {start} at the start"
        );
    }
    running.stop();
}
