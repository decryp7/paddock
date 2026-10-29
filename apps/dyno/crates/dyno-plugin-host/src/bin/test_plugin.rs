//! A plugin for dyno-plugin-host's own tests; never shipped. Its behaviour comes from its id (`DYNO_PLUGIN_ID`, set
//! by the host), up to the first `_`:
//!
//! - `ok`: one sensor
//! - `panic`, `hang`, `memory`: the sensors call panics, never returns, or allocates 1 GiB
//! - `exit`: exits before connecting
//! - `garbage`: describes itself, then answers the sensors call with bytes that are no frame
//! - `liar`: says it is another plugin
#![allow(missing_docs, clippy::unwrap_used, clippy::expect_used)]

use dyno_ipc::frame::{read_frame, write_frame};
use dyno_ipc::{ChildLink, Frame};
use dyno_plugin_api::*;
use std::io::Write;
use std::time::Duration;

struct Test {
    id: String,
    mode: String,
}

impl Plugin for Test {
    fn manifest(&self) -> Manifest {
        let id = if self.mode == "liar" {
            "someone-else".to_string()
        } else {
            self.id.clone()
        };
        Manifest::new(id, "Test", "0.0.1").contributing(Contribution::Sensors)
    }
    fn as_sensors(&self) -> Option<&dyn SensorProvider> {
        Some(self)
    }
}

impl SensorProvider for Test {
    fn sensors(&self) -> Vec<SensorReading> {
        match self.mode.as_str() {
            "panic" => panic!("the test plugin panics"),
            "hang" => loop {
                std::thread::sleep(Duration::from_secs(60));
            },
            "memory" => {
                let mut v = vec![0u8; 1 << 30];
                for i in (0..v.len()).step_by(4096) {
                    v[i] = 1;
                }
                vec![SensorReading::new("big", "Big", v.len() as f64, "B")]
            }
            _ => vec![SensorReading::new("pid", "Process id", f64::from(std::process::id()), "")],
        }
    }
}

/// Speaks the protocol by hand so it can break it.
fn garbage(test: Test) {
    let mut conn = ChildLink::from_stdin().unwrap();
    let limit = conn.limit();
    let s = conn.stream_mut();
    while let Ok(Some(frame)) = read_frame::<Frame<PluginMessage>>(s, limit) {
        if let Frame::Call {
            id,
            body: PluginMessage::Request(req),
        } = frame
        {
            if req == PluginRequest::Describe {
                let reply = Frame::Reply {
                    id,
                    body: PluginMessage::Reply(PluginReply::Manifest(test.manifest())),
                };
                write_frame(s, &reply, limit).unwrap();
            } else {
                s.write_all(&[3, 0, 0, 0, 0xee, 0xee, 0xee]).unwrap();
            }
        }
    }
}

fn main() -> std::process::ExitCode {
    let id = std::env::var("DYNO_PLUGIN_ID").unwrap_or_default();
    let mode = id.split('_').next().unwrap_or_default().to_string();
    let test = Test { id, mode };
    match test.mode.as_str() {
        "exit" => std::process::ExitCode::from(7),
        "garbage" => {
            garbage(test);
            std::process::ExitCode::SUCCESS
        }
        _ => serve(test),
    }
}
