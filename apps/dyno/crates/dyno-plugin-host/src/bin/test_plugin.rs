//! A plugin for dyno-plugin-host's own tests; never shipped. Its behaviour comes from its id (`DYNO_PLUGIN_ID`, set
//! by the host), up to the first `_`:
//!
//! - `ok`: one sensor, one cleanup category (`tmp`, proposing a relative path too, which the host drops), one action
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
        Manifest::new(id, "Test", "0.0.1")
            .contributing(Contribution::Sensors)
            .contributing(Contribution::Cleanup)
            .contributing(Contribution::Actions)
    }
    fn as_sensors(&self) -> Option<&dyn SensorProvider> {
        Some(self)
    }
    fn as_cleanup(&self) -> Option<&dyn CleanupProvider> {
        Some(self)
    }
    fn as_actions(&self) -> Option<&dyn ActionProvider> {
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

impl CleanupProvider for Test {
    fn categories(&self) -> Vec<CategoryInfo> {
        vec![CategoryInfo {
            id: "tmp".into(),
            name: "Temp".into(),
            description: "Test files".into(),
            risk: Risk::Safe,
        }]
    }
    fn scan(&self, _: &str, limits: ScanLimits) -> Result<Vec<Candidate>, PluginError> {
        if limits.max_depth == 0 {
            return Err(PluginError::new("the folder is missing"));
        }
        let dir = std::env::current_dir().unwrap();
        let cand = |path: String| Candidate {
            path,
            bytes: 10,
            modified_ms: 0,
            reason: "test".into(),
        };
        Ok(vec![cand(dir.join("a.tmp").display().to_string()), cand("relative.tmp".into())])
    }
}

impl ActionProvider for Test {
    fn actions(&self) -> Vec<ActionInfo> {
        vec![ActionInfo {
            id: "echo".into(),
            name: "Echo".into(),
            description: "Echoes the app".into(),
        }]
    }
    fn run(&self, _: &str, event: RuleEvent) -> Result<String, PluginError> {
        Ok(format!("{} at {}%", event.app, event.cpu))
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
