#![allow(clippy::unwrap_used, clippy::expect_used)]

//! The live push says who read the process list and the "Other users" total (WP-PROC), end to end over the fake platform.

use dyno_agent::cli::ask;
use dyno_agent::paths::Dirs;
use dyno_agent::Running;
use dyno_core::others::{OtherUsers, ProcessProvenance};
use dyno_core::ports::{PlatformError, ProcessSource};
use dyno_core::process::{Measured, ProcessDetails, ProcessRef, ProcessSample};
use dyno_core::protocol::{LiveDetail, Message, Request};
use dyno_core::read_via::ReadVia;
use dyno_platform::fake::Fake;
use paddock_ipc::{BodyOptions, BodyPeer};
use std::sync::mpsc::channel;
use std::time::Duration;

/// The fake's process table, saying the helper read it and counted three other accounts' processes.
struct Helped(Fake);
impl ProcessSource for Helped {
    fn processes(&mut self) -> Vec<ProcessSample> {
        self.0.clone().processes()
    }
    fn details(&mut self, t: ProcessRef) -> Result<ProcessDetails, PlatformError> {
        self.0.clone().details(t)
    }
    fn provenance(&self) -> ProcessProvenance {
        ProcessProvenance {
            processes: ReadVia::Helper,
            others: ReadVia::Helper,
            lists_others: false,
            others_total: Some(OtherUsers {
                count: 3,
                cpu: Measured::Value(1.5),
                memory: Measured::Value(3 << 30),
                partial: false,
            }),
            hint: None,
        }
    }
}

#[test]
fn the_live_push_carries_who_read_the_processes_and_the_helpers_others_total() {
    let tmp = tempfile::tempdir().unwrap();
    let dirs = Dirs::at(tmp.path().join("data"));
    let fake = Fake::new();
    fake.add_process(dyno_platform::fake::process(100, "busy", 60.0));
    let mut platform = fake.platform();
    platform.processes = Box::new(Helped(fake.clone()));
    let running = Running::start(dirs.clone(), platform, Box::new(fake.clone()), Box::new(|_| Ok(()))).unwrap();
    let d = dyno_agent::discovery::Discovery::read(&dirs.discovery()).unwrap();
    let conn = dyno_agent::wire::connect(&d.endpoint, &d.token().unwrap()).unwrap();
    let (tx, rx) = channel();
    let peer = BodyPeer::start(
        conn,
        BodyOptions::new("the agent").on_push(move |m| {
            if let Message::Live(l) = m {
                let _ = tx.send(*l);
            }
        }),
    )
    .unwrap();
    ask(
        &peer,
        Request::Subscribe {
            detail: LiveDetail::Processes,
        },
    )
    .unwrap();
    let live = rx.recv_timeout(Duration::from_secs(30)).expect("a live update");
    running.stop();
    assert_eq!((live.via.processes, live.via.others), (ReadVia::Helper, ReadVia::Helper));
    assert_eq!(live.other_users.expect("the helper's total").count, 3);
}
