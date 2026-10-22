//! A plugin that misbehaves on purpose, for the host's tests. Its behaviour is the stem of its file name (the tests
//! copy it to `good`, `crash`, `hang`, …), since plugins get no arguments and a cleared environment.
#![allow(clippy::unwrap_used, clippy::expect_used, missing_docs)]

use spotter_plugin_api::frame::{read_message, write_message, FRAME_VERSION};
use spotter_plugin_api::protocol::{Message, MAGIC};
use spotter_plugin_api::{
    serve, Config, ErrorKind, FieldKind, FieldSpec, Manifest, Plugin, PluginError, ProtocolVersion, SourceKindSpec, WireRelease,
};
use std::io::Write;
use std::process::ExitCode;

struct Fake {
    id: String,
}

impl Plugin for Fake {
    fn manifest(&self) -> Manifest {
        let id = if self.id == "wrong-id" {
            "someone-else".to_string()
        } else {
            self.id.clone()
        };
        Manifest::new(&id, "Fake", "1.0.0").with_kind(SourceKindSpec::new("files", "Files", "Fake files").with_field(FieldSpec::new(
            "dir",
            "Dir",
            FieldKind::Text,
        )))
    }

    fn list(&self, _kind: &str, config: &Config, limit: usize) -> Result<Vec<WireRelease>, PluginError> {
        match self.id.as_str() {
            "list-abort" => std::process::abort(),
            "list-hang" => loop {
                std::thread::sleep(std::time::Duration::from_secs(1));
            },
            "list-memory" => {
                let mut v: Vec<Vec<u8>> = Vec::new();
                loop {
                    v.push(vec![1u8; 8 << 20]);
                    std::thread::sleep(std::time::Duration::from_millis(5));
                }
            }
            "list-panic" => panic!("boom"),
            "list-offline" => Err(PluginError::new(ErrorKind::Unreachable, "the server is offline")),
            _ => Ok((0..limit.min(5))
                .map(|i| WireRelease::new(&format!("1.{i}.0"), &format!("{}-1.{i}.0", config.get("dir").unwrap_or("x")), "l"))
                .collect()),
        }
    }
}

fn main() -> ExitCode {
    let id = std::env::current_exe()
        .ok()
        .and_then(|p| p.file_stem().map(|s| s.to_string_lossy().into_owned()))
        .unwrap_or_default();
    let mut out = std::io::stdout();
    match id.as_str() {
        "exit" => return ExitCode::from(3),
        "hang" => loop {
            std::thread::sleep(std::time::Duration::from_secs(1));
        },
        "garbage" => {
            out.write_all(b"\x05\x00\x00\x00hello world").unwrap();
            out.flush().unwrap();
            std::thread::sleep(std::time::Duration::from_secs(5));
            return ExitCode::SUCCESS;
        }
        "old-protocol" => {
            let hello = Message::Hello {
                magic: MAGIC,
                frame: FRAME_VERSION,
                protocol: ProtocolVersion::new(0, 1),
            };
            write_message(&mut out, &hello).unwrap();
            let _ = read_message(&mut std::io::stdin());
            return ExitCode::FAILURE;
        }
        _ => {}
    }
    serve(Fake { id })
}
