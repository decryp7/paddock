//! The protocol between the Pitcrew app and the Pitcrew service (ADR 0001).
//!
//! One request, one response, over a private local stream (a named pipe on Windows, a Unix socket on macOS).
//! Each frame is a 4-byte little-endian length followed by that many bytes of JSON. The first request on a
//! connection is [`Request::Hello`]; the service refuses other protocol versions with
//! [`ErrorCode::ProtocolMismatch`]. Frames are bounded by [`MAX_FRAME`], so neither side can be made to allocate
//! without limit.

use crate::distribution::RunnerKind;
use crate::health::RunnerStatus;
use crate::logs::LogLine;
use crate::names::RunnerName;
use crate::time::Timestamp;
use crate::update::{UpdateOutcome, UpdatePhase};
use crate::version::RunnerVersion;
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use std::io::{Read, Write};

/// The protocol version this build speaks.
pub const PROTOCOL_VERSION: u32 = 1;
/// Largest frame either side sends or accepts.
pub const MAX_FRAME: usize = 1024 * 1024;

/// A request from the app.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum Request {
    /// Opens the conversation.
    Hello {
        /// The app's protocol version.
        protocol: u32,
    },
    /// Every runner's status.
    Status,
    /// Start a runner (clears a crash loop).
    Start {
        /// The runner.
        runner: RunnerName,
    },
    /// Stop a runner (gracefully: a running job finishes first).
    Stop {
        /// The runner.
        runner: RunnerName,
    },
    /// Restart a runner.
    Restart {
        /// The runner.
        runner: RunnerName,
    },
    /// Log lines after `after`.
    Logs {
        /// The runner.
        runner: RunnerName,
        /// The last sequence number the app has.
        after: Option<u64>,
        /// Most lines to return.
        limit: u32,
    },
    /// Log lines containing `query` (plain text).
    SearchLogs {
        /// The runner.
        runner: RunnerName,
        /// The text.
        query: String,
        /// Most lines to return.
        limit: u32,
    },
    /// Check for a runner update now.
    CheckUpdate {
        /// The runner.
        runner: RunnerName,
    },
    /// Install the available update now (between jobs).
    ApplyUpdate {
        /// The runner.
        runner: RunnerName,
    },
    /// Cancel an update that has not swapped the binary yet.
    CancelUpdate {
        /// The runner.
        runner: RunnerName,
    },
}

/// What the app shows about one runner.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RunnerSnapshot {
    /// The runner.
    pub name: RunnerName,
    /// Which runner it is.
    pub kind: RunnerKind,
    /// Its status.
    pub status: RunnerStatus,
    /// The installed version, when known.
    pub current_version: Option<RunnerVersion>,
    /// The latest version found by the last check.
    pub latest_version: Option<RunnerVersion>,
    /// How the last update ended.
    pub last_update: Option<UpdateRecord>,
    /// The update in progress.
    pub update_phase: UpdatePhase,
}

/// A finished update, as kept by the service.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateRecord {
    /// How it ended.
    pub outcome: UpdateOutcome,
    /// When it ended.
    pub finished_at: Timestamp,
}

/// A response from the service.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum Response {
    /// The service's answer to [`Request::Hello`].
    Hello {
        /// The service's protocol version.
        protocol: u32,
        /// The service's version.
        service_version: String,
    },
    /// Every runner's status.
    Status {
        /// The runners.
        runners: Vec<RunnerSnapshot>,
    },
    /// Log lines.
    Logs {
        /// The lines, oldest first.
        lines: Vec<LogLine>,
    },
    /// Done.
    Ok,
    /// Refused or failed.
    Error(ServiceError),
}

/// Why the service refused or failed a request.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ErrorCode {
    /// The feature is not built yet (the skeleton's honest answer).
    NotImplemented,
    /// The app and the service speak different protocol versions.
    ProtocolMismatch,
    /// No runner has this name.
    UnknownRunner,
    /// The request is malformed.
    BadRequest,
    /// The caller may not do this (e.g. a non-administrator changing a machine-wide runner).
    PermissionDenied,
    /// Something else is in progress (e.g. an update).
    Busy,
    /// An internal error; the service log has the details.
    Internal,
}

/// An error with a message for the user.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, thiserror::Error)]
#[serde(rename_all = "camelCase")]
#[error("{message}")]
pub struct ServiceError {
    /// What kind of error.
    pub code: ErrorCode,
    /// What happened and what to do, for the user.
    pub message: String,
}

impl ServiceError {
    /// A "not implemented yet" error for `what`.
    pub fn not_implemented(what: &str) -> Self {
        ServiceError {
            code: ErrorCode::NotImplemented,
            message: format!("{what} is not implemented yet"),
        }
    }
}

/// Problems reading or writing frames.
#[derive(Debug, thiserror::Error)]
pub enum FrameError {
    /// The stream failed or closed.
    #[error("the connection failed: {0}")]
    Io(#[from] std::io::Error),
    /// The frame is larger than [`MAX_FRAME`].
    #[error("a message of {0} bytes is larger than the limit of {MAX_FRAME} bytes")]
    TooLarge(usize),
    /// The frame is not a valid message.
    #[error("the message is not valid: {0}")]
    Malformed(String),
}

/// Writes one message as a frame.
pub fn write_frame<T: Serialize>(w: &mut impl Write, message: &T) -> Result<(), FrameError> {
    let body = serde_json::to_vec(message).map_err(|e| FrameError::Malformed(e.to_string()))?;
    if body.len() > MAX_FRAME {
        return Err(FrameError::TooLarge(body.len()));
    }
    let len = u32::try_from(body.len()).map_err(|_| FrameError::TooLarge(body.len()))?;
    w.write_all(&len.to_le_bytes())?;
    w.write_all(&body)?;
    w.flush()?;
    Ok(())
}

/// Reads one frame and parses it as a message.
pub fn read_frame<T: DeserializeOwned>(r: &mut impl Read) -> Result<T, FrameError> {
    let mut len = [0u8; 4];
    r.read_exact(&mut len)?;
    let len = usize::try_from(u32::from_le_bytes(len)).unwrap_or(usize::MAX);
    if len > MAX_FRAME {
        return Err(FrameError::TooLarge(len));
    }
    let mut body = vec![0u8; len];
    r.read_exact(&mut body)?;
    serde_json::from_slice(&body).map_err(|e| FrameError::Malformed(e.to_string()))
}

/// The service's answer to a hello.
pub fn answer_hello(app_protocol: u32, service_version: &str) -> Response {
    if app_protocol == PROTOCOL_VERSION {
        Response::Hello {
            protocol: PROTOCOL_VERSION,
            service_version: service_version.to_string(),
        }
    } else {
        Response::Error(ServiceError {
            code: ErrorCode::ProtocolMismatch,
            message: format!(
                "the Pitcrew app speaks protocol {app_protocol} but the service speaks {PROTOCOL_VERSION}; install the same Pitcrew version for both"
            ),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    fn name(s: &str) -> RunnerName {
        RunnerName::parse(s).unwrap()
    }

    #[test]
    fn frames_round_trip() {
        let requests = vec![
            Request::Hello {
                protocol: PROTOCOL_VERSION,
            },
            Request::Status,
            Request::Logs {
                runner: name("a"),
                after: Some(3),
                limit: 100,
            },
            Request::SearchLogs {
                runner: name("a"),
                query: "timed out".into(),
                limit: 10,
            },
        ];
        let mut buf = vec![];
        for r in &requests {
            write_frame(&mut buf, r).unwrap();
        }
        let mut cur = Cursor::new(buf);
        for r in &requests {
            assert_eq!(&read_frame::<Request>(&mut cur).unwrap(), r);
        }
        assert!(matches!(read_frame::<Request>(&mut cur), Err(FrameError::Io(_))), "end of stream");
    }

    #[test]
    fn the_wire_format_is_camel_case_json_with_a_type_tag() {
        let json = serde_json::to_string(&Request::CheckUpdate { runner: name("build-pc") }).unwrap();
        assert_eq!(json, r#"{"type":"checkUpdate","runner":"build-pc"}"#);
        let err = Response::Error(ServiceError::not_implemented("Starting runners"));
        assert_eq!(
            serde_json::to_string(&err).unwrap(),
            r#"{"type":"error","code":"notImplemented","message":"Starting runners is not implemented yet"}"#
        );
    }

    #[test]
    fn oversized_and_malformed_frames_are_refused() {
        let mut huge = (u32::try_from(MAX_FRAME).unwrap() + 1).to_le_bytes().to_vec();
        huge.extend([0u8; 16]);
        assert!(matches!(
            read_frame::<Request>(&mut Cursor::new(huge)),
            Err(FrameError::TooLarge(_))
        ));
        let mut junk = 3u32.to_le_bytes().to_vec();
        junk.extend(b"{x}");
        assert!(matches!(
            read_frame::<Request>(&mut Cursor::new(junk)),
            Err(FrameError::Malformed(_))
        ));
        let bad_name = br#"{"type":"start","runner":"../etc"}"#;
        let mut frame = u32::try_from(bad_name.len()).unwrap().to_le_bytes().to_vec();
        frame.extend(bad_name);
        assert!(
            matches!(read_frame::<Request>(&mut Cursor::new(frame)), Err(FrameError::Malformed(_))),
            "names are validated on the way in"
        );
        let too_big = Request::SearchLogs {
            runner: name("a"),
            query: "x".repeat(MAX_FRAME),
            limit: 1,
        };
        assert!(matches!(write_frame(&mut vec![], &too_big), Err(FrameError::TooLarge(_))));
    }

    #[test]
    fn hello_checks_the_protocol_version() {
        assert_eq!(
            answer_hello(PROTOCOL_VERSION, "0.1.0"),
            Response::Hello {
                protocol: PROTOCOL_VERSION,
                service_version: "0.1.0".into()
            }
        );
        assert!(matches!(
            answer_hello(99, "0.1.0"),
            Response::Error(ServiceError {
                code: ErrorCode::ProtocolMismatch,
                ..
            })
        ));
    }
}
