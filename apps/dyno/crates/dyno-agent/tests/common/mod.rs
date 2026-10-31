//! Helpers shared by the agent's integration tests.

use dyno_agent::cli::ask;
use dyno_core::protocol::{Message, Reply, Request};
use dyno_core::time::Timestamp;
use dyno_ipc::Peer;
use std::time::Duration;

/// A sample whose only disk is almost full (the low-disk alert raises a critical alarm).
pub fn full_disk_sample() -> dyno_core::metrics::SystemSample {
    let mut s = dyno_platform::fake::quiet_sample(Timestamp(0));
    s.disks = vec![dyno_core::metrics::DiskSample {
        id: "c".into(),
        name: "C:".into(),
        mount: "C:".into(),
        total: 100 << 30,
        free: 2 << 30,
        removable: false,
    }];
    s
}

/// The first alarm the agent lists, waiting for it to be raised (bounded: 10 s).
pub fn wait_for_alarm(peer: &Peer<Message>) -> dyno_core::alarms::Alarm {
    for _ in 0..100 {
        let Reply::Alarms(list) = ask(
            peer,
            Request::Alarms {
                filter: Default::default(),
            },
        )
        .unwrap() else {
            panic!("alarms")
        };
        if let Some(a) = list.into_iter().next() {
            return a;
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    panic!("no alarm was raised");
}
