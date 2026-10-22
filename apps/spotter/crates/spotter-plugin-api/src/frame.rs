//! The frame codec: a 4-byte little-endian length, then a postcard-encoded [`Message`].
//!
//! Reading is bounded and never panics: the length is checked against the limit before anything is allocated, and
//! a body that does not decode is a [`FrameError`], not a crash.

use crate::protocol::Message;
use std::io::{ErrorKind, Read, Write};

/// The frame format version, sent in [`Message::Hello`].
pub const FRAME_VERSION: u16 = 1;
/// Bytes of the length prefix.
pub const LEN_BYTES: usize = 4;
/// Largest frame either side accepts (4 MiB): a list of [`crate::protocol::MAX_RELEASES`] releases fits easily.
pub const MAX_FRAME: usize = 4 << 20;

/// Why a frame could not be read or written.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FrameError {
    /// The stream ended cleanly between frames.
    Closed,
    /// The stream ended inside a frame, or the pipe failed.
    Io(String),
    /// The announced length exceeds the limit.
    TooLarge(usize),
    /// The body is not a valid message.
    Invalid(String),
}

impl std::fmt::Display for FrameError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            FrameError::Closed => write!(f, "the plugin closed the connection"),
            FrameError::Io(e) => write!(f, "the connection to the plugin failed ({e})"),
            FrameError::TooLarge(n) => write!(f, "a message of {n} bytes exceeds the limit of {MAX_FRAME} bytes"),
            FrameError::Invalid(e) => write!(f, "a message is not valid ({e})"),
        }
    }
}

impl std::error::Error for FrameError {}

/// A message as bytes on the wire, length prefix included.
pub fn encode(message: &Message) -> Result<Vec<u8>, FrameError> {
    let mut out = postcard::to_extend(message, vec![0u8; LEN_BYTES]).map_err(|e| FrameError::Invalid(e.to_string()))?;
    let len = out.len() - LEN_BYTES;
    if len > MAX_FRAME {
        return Err(FrameError::TooLarge(len));
    }
    // `len` fits: MAX_FRAME < u32::MAX.
    out[..LEN_BYTES].copy_from_slice(&(len as u32).to_le_bytes());
    Ok(out)
}

/// Decodes one body (without its prefix); the whole body must be one message.
pub fn decode_body(body: &[u8]) -> Result<Message, FrameError> {
    match postcard::take_from_bytes::<Message>(body) {
        Ok((m, [])) => Ok(m),
        Ok((_, rest)) => Err(FrameError::Invalid(format!("{} trailing bytes", rest.len()))),
        Err(e) => Err(FrameError::Invalid(e.to_string())),
    }
}

/// Reads one message.
pub fn read_message<R: Read>(r: &mut R) -> Result<Message, FrameError> {
    let mut prefix = [0u8; LEN_BYTES];
    let mut got = 0;
    while got < LEN_BYTES {
        match r.read(&mut prefix[got..]) {
            Ok(0) if got == 0 => return Err(FrameError::Closed),
            Ok(0) => return Err(FrameError::Io("the stream ended inside a message".into())),
            Ok(n) => got += n,
            Err(e) if e.kind() == ErrorKind::Interrupted => {}
            Err(e) => return Err(io_error(e)),
        }
    }
    let len = u32::from_le_bytes(prefix) as usize;
    if len > MAX_FRAME {
        return Err(FrameError::TooLarge(len));
    }
    let mut body = vec![0u8; len];
    r.read_exact(&mut body).map_err(|e| match e.kind() {
        ErrorKind::UnexpectedEof => FrameError::Io("the stream ended inside a message".into()),
        _ => io_error(e),
    })?;
    decode_body(&body)
}

/// Writes one message and flushes.
pub fn write_message<W: Write>(w: &mut W, message: &Message) -> Result<(), FrameError> {
    let bytes = encode(message)?;
    w.write_all(&bytes).map_err(io_error)?;
    w.flush().map_err(io_error)
}

fn io_error(e: std::io::Error) -> FrameError {
    match e.kind() {
        ErrorKind::BrokenPipe | ErrorKind::ConnectionReset | ErrorKind::ConnectionAborted => FrameError::Closed,
        _ => FrameError::Io(e.to_string()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::protocol::{ErrorKind as Kind, Manifest, PluginError, Request, Response, WireRelease, MAGIC, PROTOCOL};
    use crate::schema::Config;
    use std::io::Cursor;

    fn all() -> Vec<Message> {
        vec![
            Message::Hello {
                magic: MAGIC,
                frame: FRAME_VERSION,
                protocol: PROTOCOL,
            },
            Message::Welcome { protocol: PROTOCOL },
            Message::Refused {
                reason: "no — café".into(),
            },
            Message::Request {
                id: 1,
                request: Request::List {
                    kind: "k".into(),
                    config: Config::new().with("a", "b"),
                    limit: 5,
                },
            },
            Message::Request {
                id: 2,
                request: Request::Describe,
            },
            Message::Request {
                id: 3,
                request: Request::Shutdown,
            },
            Message::Response {
                id: 1,
                response: Response::Listed(vec![WireRelease::new("1", "n", "l")]),
            },
            Message::Response {
                id: 2,
                response: Response::Described(Manifest::new("p", "P", "1")),
            },
            Message::Response {
                id: 3,
                response: Response::Failed(PluginError::new(Kind::Unauthorized, "no")),
            },
            Message::Response {
                id: 4,
                response: Response::Bye,
            },
        ]
    }

    #[test]
    fn every_message_roundtrips() {
        let mut wire = Vec::new();
        for m in all() {
            write_message(&mut wire, &m).unwrap();
        }
        let mut r = Cursor::new(wire);
        for m in all() {
            assert_eq!(read_message(&mut r).unwrap(), m);
        }
        assert_eq!(read_message(&mut r), Err(FrameError::Closed));
    }

    #[test]
    fn truncated_oversized_and_garbage_frames_are_errors() {
        let bytes = encode(&all()[0]).unwrap();
        for cut in 1..bytes.len() {
            let e = read_message(&mut Cursor::new(&bytes[..cut])).unwrap_err();
            assert!(matches!(e, FrameError::Io(_)), "cut at {cut}: {e:?}");
        }
        let huge = (u32::MAX).to_le_bytes();
        assert_eq!(read_message(&mut Cursor::new(huge)), Err(FrameError::TooLarge(u32::MAX as usize)));
        let garbage = [3u8, 0, 0, 0, 0xff, 0xff, 0xff];
        assert!(matches!(read_message(&mut Cursor::new(garbage)), Err(FrameError::Invalid(_))));
        let mut trailing = bytes.clone();
        trailing[0] += 1;
        trailing.push(0);
        assert!(matches!(read_message(&mut Cursor::new(trailing)), Err(FrameError::Invalid(m)) if m.contains("trailing")));
        let big = Message::Refused {
            reason: "x".repeat(MAX_FRAME + 1),
        };
        assert!(matches!(encode(&big), Err(FrameError::TooLarge(_))));
        for e in [
            FrameError::Closed,
            FrameError::Io("x".into()),
            FrameError::TooLarge(1),
            FrameError::Invalid("y".into()),
        ] {
            assert!(!e.to_string().is_empty());
        }
    }

    struct Failing(ErrorKind);

    impl Read for Failing {
        fn read(&mut self, _: &mut [u8]) -> std::io::Result<usize> {
            Err(self.0.into())
        }
    }

    impl Write for Failing {
        fn write(&mut self, _: &[u8]) -> std::io::Result<usize> {
            Err(self.0.into())
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }

    #[test]
    fn broken_pipes_are_closed_and_other_failures_are_io() {
        assert_eq!(read_message(&mut Failing(ErrorKind::BrokenPipe)), Err(FrameError::Closed));
        assert!(matches!(
            read_message(&mut Failing(ErrorKind::PermissionDenied)),
            Err(FrameError::Io(_))
        ));
        assert_eq!(
            write_message(&mut Failing(ErrorKind::ConnectionReset), &all()[1]),
            Err(FrameError::Closed)
        );
        // A body read that fails with something else than EOF.
        struct PrefixThenFail(u8);
        impl Read for PrefixThenFail {
            fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
                if self.0 < 4 {
                    self.0 += 1;
                    buf[0] = if self.0 == 1 { 9 } else { 0 };
                    Ok(1)
                } else if self.0 == 4 {
                    self.0 += 1;
                    Err(ErrorKind::Interrupted.into())
                } else {
                    Err(ErrorKind::PermissionDenied.into())
                }
            }
        }
        assert!(matches!(read_message(&mut PrefixThenFail(0)), Err(FrameError::Io(_))));
    }
}
