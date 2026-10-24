//! The one frame codec of the family's processes (ADR 0001, Debrief ADR 0017 §1): a 4-byte little-endian length, then a
//! postcard-encoded [`Frame`]. A frame's `body` is the protocol's own postcard-encoded message.
//!
//! Decoding is bounded and never panics: the length is checked against the connection's [`FrameLimit`] before anything
//! is allocated, and a body that does not decode is a [`FrameError`], not a crash. A frame over the limit but under
//! [`HARD_MAX_FRAME`] is skipped without allocating it ([`FrameError::Skipped`], with the call it belonged to), so one
//! oversized message fails one call and the connection stays up; a length beyond that is garbage and ends it.

use crate::error::RemoteError;
use crate::token::TOKEN_BYTES;
use serde::{Deserialize, Serialize};
use std::io::{self, Read, Write};

/// Version of the frame layout, exchanged in [`Frame::Hello`] / [`Frame::Welcome`]. Version 2 (Debrief's) carries the
/// connection's [`ProtocolVersion`]; [`Frame::Push`] was added at the end of the enum, which older readers never receive.
pub const VERSION: u16 = 2;
/// Largest frame a connection accepts unless configured otherwise (16 MiB).
pub const DEFAULT_MAX_FRAME: usize = 16 << 20;
/// No connection accepts frames larger than this (64 MiB), whatever it is configured to; longer lengths end the connection.
pub const HARD_MAX_FRAME: usize = 64 << 20;
/// No connection is configured below this (1 KiB), so the handshake always fits.
pub const MIN_FRAME: usize = 1024;
/// Bytes of the length prefix.
pub const LEN_BYTES: usize = 4;
/// Bytes of a frame read to find out which call an oversized frame belonged to (variant and id, both varints).
const HEAD_BYTES: usize = 16;

/// The largest frame body a connection accepts, between [`MIN_FRAME`] and [`HARD_MAX_FRAME`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct FrameLimit(usize);

impl FrameLimit {
    /// A limit of `bytes`, clamped to `MIN_FRAME..=HARD_MAX_FRAME`.
    pub fn new(bytes: usize) -> FrameLimit {
        FrameLimit(bytes.clamp(MIN_FRAME, HARD_MAX_FRAME))
    }

    /// The limit in bytes.
    pub fn get(self) -> usize {
        self.0
    }
}

impl Default for FrameLimit {
    fn default() -> Self {
        FrameLimit(DEFAULT_MAX_FRAME)
    }
}

/// The version of the protocol a connection's bodies speak (a plugin protocol, an agent protocol), as opposed to the
/// frame [`VERSION`]. The host accepts a child that speaks the same major and a minor it knows ([`ProtocolVersion::accepts`]),
/// and otherwise refuses it with a message that says which side to update ([`ProtocolVersion::mismatch`]).
#[derive(
    Debug, Clone, Copy, Default, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize,
)]
pub struct ProtocolVersion {
    /// Incompatible changes (a request or reply changed or was removed).
    pub major: u16,
    /// Compatible additions (a variant added at the end of its enum).
    pub minor: u16,
}

impl ProtocolVersion {
    /// Connections whose protocol carries no version of its own (both sides are built together).
    pub const UNVERSIONED: ProtocolVersion = ProtocolVersion::new(0, 0);

    /// `major.minor`.
    pub const fn new(major: u16, minor: u16) -> ProtocolVersion {
        ProtocolVersion { major, minor }
    }

    /// Whether a host speaking `self` can serve a child built for `child`: same major, and a minor no newer than the
    /// host's (a newer minor may send requests this host does not know).
    pub fn accepts(self, child: ProtocolVersion) -> bool {
        self.major == child.major && child.minor <= self.minor
    }

    /// Which side is behind when a host speaking `self` cannot serve a child built for `child`; `None` when
    /// [`ProtocolVersion::accepts`] holds. A child with an older major is behind; a child with a newer major or a newer
    /// minor means the host is behind. This is the one place the "who must update" decision is made.
    pub fn skew(self, child: ProtocolVersion) -> Option<Skew> {
        if self.accepts(child) {
            None
        } else if child.major < self.major {
            Some(Skew::ChildBehind)
        } else {
            Some(Skew::HostBehind)
        }
    }

    /// The release note for the first entry of `since` (a protocol version and the app version that introduced it) that
    /// has `needed`, for "needs 1.4 or newer"; the oldest such entry wins. `None` when no entry reaches `needed`.
    pub fn introduced_in<'a>(
        since: impl IntoIterator<Item = (ProtocolVersion, &'a str)>,
        needed: ProtocolVersion,
    ) -> Option<&'a str> {
        since
            .into_iter()
            .filter(|(v, _)| *v >= needed)
            .min_by_key(|(v, _)| *v)
            .map(|(_, app_version)| app_version)
    }

    /// Why a host (`host_name`, e.g. "Debrief") speaking `self` refuses `label` built for `child`, telling the user what
    /// to update; `None` when [`ProtocolVersion::accepts`] holds.
    pub fn mismatch(self, child: ProtocolVersion, label: &str, host_name: &str) -> Option<String> {
        self.skew(child).map(|skew| match skew {
            Skew::ChildBehind => format!(
                "{label} is too old: it was built for protocol {child} and this {host_name} speaks {self}; it needs an update \
                 built for protocol {}.x",
                self.major
            ),
            Skew::HostBehind => format!(
                "{label} needs a newer {host_name}: it was built for protocol {child} and this {host_name} speaks {self}; update \
                 {host_name} to a version that speaks protocol {child} or later"
            ),
        })
    }
}

/// Which side must update when a [`ProtocolVersion`] is refused ([`ProtocolVersion::skew`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Skew {
    /// The child (plugin, agent) was built for an older major: it must be updated.
    ChildBehind,
    /// The child needs a newer major or minor than the host speaks: the host (the app) must be updated.
    HostBehind,
}

impl std::fmt::Display for ProtocolVersion {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}.{}", self.major, self.minor)
    }
}

impl std::str::FromStr for ProtocolVersion {
    type Err = String;

    /// `major.minor` (e.g. `1.0`).
    fn from_str(s: &str) -> Result<ProtocolVersion, String> {
        let bad = || format!("\"{s}\" is not a protocol version (major.minor, e.g. 1.0)");
        let (major, minor) = s.trim().split_once('.').ok_or_else(bad)?;
        Ok(ProtocolVersion::new(
            major.parse().map_err(|_| bad())?,
            minor.parse().map_err(|_| bad())?,
        ))
    }
}

/// One message on a connection.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum Frame {
    /// First frame of the connecting side: the frame version, the protocol it speaks, the token and its pid. The two
    /// versions come first, so a host reads them from a peer of any version ([`handshake_versions`]).
    Hello {
        /// The frame [`VERSION`].
        version: u16,
        /// The protocol it speaks.
        protocol: ProtocolVersion,
        /// The token it was given.
        token: [u8; TOKEN_BYTES],
        /// Its process id (checked against the OS where it tells).
        pid: u32,
    },
    /// The listening side accepted [`Frame::Hello`]; the protocol it speaks.
    Welcome {
        /// The frame [`VERSION`].
        version: u16,
        /// The protocol it speaks.
        protocol: ProtocolVersion,
    },
    /// A request; `credit` is how many [`Frame::Item`]s the callee may send before the caller's first [`Frame::Credit`]
    /// (0: the call does not stream).
    Call {
        /// The call id, chosen by the caller, per direction.
        id: u64,
        /// Items allowed ahead.
        credit: u32,
        /// The protocol's request.
        body: Vec<u8>,
    },
    /// The single answer to a call (after its items, for a streamed call).
    Reply {
        /// The call id.
        id: u64,
        /// The protocol's reply.
        body: Vec<u8>,
    },
    /// One item of a streamed answer.
    Item {
        /// The call id.
        id: u64,
        /// The protocol's item.
        body: Vec<u8>,
    },
    /// The call failed with a typed error (id 0 refuses a handshake).
    Fail {
        /// The call id.
        id: u64,
        /// What went wrong.
        error: RemoteError,
    },
    /// The caller gave up on the call; the callee stops at its next check.
    Cancel {
        /// The call id.
        id: u64,
    },
    /// The caller can take `n` more items of the call.
    Credit {
        /// The call id.
        id: u64,
        /// More items allowed.
        n: u32,
    },
    /// An unsolicited event (live updates, alerts); never answered.
    Push {
        /// The protocol's event.
        body: Vec<u8>,
    },
}

impl Frame {
    /// The call this frame belongs to (none for the handshake and pushes).
    pub fn call_id(&self) -> Option<u64> {
        match self {
            Frame::Hello { .. } | Frame::Welcome { .. } | Frame::Push { .. } => None,
            Frame::Call { id, .. }
            | Frame::Reply { id, .. }
            | Frame::Item { id, .. }
            | Frame::Fail { id, .. }
            | Frame::Cancel { id }
            | Frame::Credit { id, .. } => Some(*id),
        }
    }
}

/// Which kind of frame a skipped (oversized) frame was.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FrameKind {
    /// [`Frame::Call`].
    Call,
    /// [`Frame::Reply`].
    Reply,
    /// [`Frame::Item`].
    Item,
    /// [`Frame::Fail`].
    Fail,
    /// [`Frame::Push`].
    Push,
    /// Any other frame (never large unless garbage).
    Other,
}

/// What the first bytes of a frame body say: its kind and, for call frames, the call id.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FrameHead {
    /// The kind.
    pub kind: FrameKind,
    /// The call id, for call frames.
    pub id: Option<u64>,
}

/// Reads the kind and call id from the start of a frame body without decoding the rest.
pub fn peek_head(body: &[u8]) -> Option<FrameHead> {
    let (variant, rest) = postcard::take_from_bytes::<u32>(body).ok()?;
    let kind = match variant {
        2 => FrameKind::Call,
        3 => FrameKind::Reply,
        4 => FrameKind::Item,
        5 => FrameKind::Fail,
        8 => FrameKind::Push,
        0..=8 => FrameKind::Other,
        _ => return None,
    };
    let id = match kind {
        FrameKind::Call | FrameKind::Reply | FrameKind::Item | FrameKind::Fail => {
            Some(postcard::take_from_bytes::<u64>(rest).ok()?.0)
        }
        FrameKind::Push | FrameKind::Other => None,
    };
    Some(FrameHead { kind, id })
}

/// Why a frame could not be read or written.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FrameError {
    /// The stream ended (cleanly between frames).
    Closed,
    /// The stream ended in the middle of a frame, or the transport failed.
    Io(String),
    /// The length is beyond what this connection can skip (or, writing, beyond its limit): the connection ends.
    TooLarge {
        /// The announced or encoded length.
        len: usize,
        /// The limit.
        max: usize,
    },
    /// A frame over the limit was read past without keeping it; the stream is still in step. `head` names the call it
    /// belonged to when its first bytes said so.
    Skipped {
        /// Its length.
        len: usize,
        /// The limit.
        max: usize,
        /// Its kind and call id, when readable.
        head: Option<FrameHead>,
    },
    /// The body is not a valid frame.
    Invalid(String),
    /// A read timeout set on the stream passed before a frame arrived (handshakes only; the stream is out of step).
    TimedOut,
}

impl std::fmt::Display for FrameError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            FrameError::Closed => write!(f, "the connection was closed"),
            FrameError::Io(e) => write!(f, "the connection failed ({e})"),
            FrameError::TooLarge { len, max } | FrameError::Skipped { len, max, .. } => {
                write!(f, "a frame of {len} bytes exceeds the limit of {max} bytes")
            }
            FrameError::Invalid(e) => write!(f, "a frame is not valid ({e})"),
            FrameError::TimedOut => write!(f, "no frame arrived in time"),
        }
    }
}

impl std::error::Error for FrameError {}

/// `frame` as bytes on the wire (length prefix included). Fails when the encoded frame exceeds `limit`.
pub fn encode(frame: &Frame, limit: FrameLimit) -> Result<Vec<u8>, FrameError> {
    let mut out = postcard::to_extend(frame, vec![0u8; LEN_BYTES])
        .map_err(|e| FrameError::Invalid(e.to_string()))?;
    let len = out.len() - LEN_BYTES;
    if len > limit.get() {
        return Err(FrameError::TooLarge {
            len,
            max: limit.get(),
        });
    }
    // `len` is at most the limit, which is at most `HARD_MAX_FRAME`: it fits in the prefix.
    let prefix = u32::try_from(len).unwrap_or(u32::MAX);
    out[..LEN_BYTES].copy_from_slice(&prefix.to_le_bytes());
    Ok(out)
}

/// The body length a prefix announces; `Err(TooLarge)` beyond `limit`.
pub fn body_len(prefix: [u8; LEN_BYTES], limit: FrameLimit) -> Result<usize, FrameError> {
    let len = usize::try_from(u32::from_le_bytes(prefix)).unwrap_or(usize::MAX);
    if len > limit.get() {
        return Err(FrameError::TooLarge {
            len,
            max: limit.get(),
        });
    }
    Ok(len)
}

/// Decodes one frame body (without its length prefix). The whole body must be one frame.
pub fn decode_body(body: &[u8]) -> Result<Frame, FrameError> {
    match postcard::take_from_bytes::<Frame>(body) {
        Ok((frame, [])) => Ok(frame),
        Ok((_, rest)) => Err(FrameError::Invalid(format!(
            "{} trailing bytes",
            rest.len()
        ))),
        Err(e) => Err(FrameError::Invalid(e.to_string())),
    }
}

/// Splits `buf` into the first complete frame and the bytes it used: `Ok(None)` when more bytes are needed. For callers
/// that buffer themselves, and the fuzz target.
pub fn decode(buf: &[u8], limit: FrameLimit) -> Result<Option<(Frame, usize)>, FrameError> {
    let Some(prefix) = buf.first_chunk::<LEN_BYTES>() else {
        return Ok(None);
    };
    let len = body_len(*prefix, limit)?;
    let Some(body) = buf.get(LEN_BYTES..LEN_BYTES + len) else {
        return Ok(None);
    };
    decode_body(body).map(|f| Some((f, LEN_BYTES + len)))
}

/// The frame version and the protocol version at the start of a [`Frame::Hello`] body, read without decoding the rest
/// (a peer of another version may lay the rest out differently). `None` when the body does not start like a `Hello`.
pub fn handshake_versions(body: &[u8]) -> Option<(u16, Option<ProtocolVersion>)> {
    let (variant, rest) = postcard::take_from_bytes::<u32>(body).ok()?;
    if variant != 0 {
        return None;
    }
    let (version, rest) = postcard::take_from_bytes::<u16>(rest).ok()?;
    let protocol = if version >= 2 {
        postcard::take_from_bytes::<ProtocolVersion>(rest)
            .ok()
            .map(|(p, _)| p)
    } else {
        None
    };
    Some((version, protocol))
}

fn io_error(e: &io::Error) -> FrameError {
    match e.kind() {
        io::ErrorKind::BrokenPipe
        | io::ErrorKind::ConnectionReset
        | io::ErrorKind::ConnectionAborted => FrameError::Closed,
        io::ErrorKind::UnexpectedEof => FrameError::Io("the stream ended inside a frame".into()),
        io::ErrorKind::TimedOut | io::ErrorKind::WouldBlock => FrameError::TimedOut,
        _ => FrameError::Io(e.to_string()),
    }
}

fn read_full(r: &mut impl Read, buf: &mut [u8]) -> Result<(), FrameError> {
    r.read_exact(buf).map_err(|e| io_error(&e))
}

/// Reads one frame's body without decoding it. A body over `limit` (and at most [`HARD_MAX_FRAME`]) is read past in
/// small pieces and reported as [`FrameError::Skipped`]; beyond that it is [`FrameError::TooLarge`].
pub fn read_body(r: &mut impl Read, limit: FrameLimit) -> Result<Vec<u8>, FrameError> {
    let mut prefix = [0u8; LEN_BYTES];
    let mut got = 0;
    while got < LEN_BYTES {
        match r.read(&mut prefix[got..]) {
            Ok(0) if got == 0 => return Err(FrameError::Closed),
            Ok(0) => return Err(FrameError::Io("the stream ended inside a frame".into())),
            Ok(n) => got += n,
            Err(e) if e.kind() == io::ErrorKind::Interrupted => {}
            Err(e) => return Err(io_error(&e)),
        }
    }
    match body_len(prefix, limit) {
        Ok(len) => {
            let mut body = vec![0u8; len];
            read_full(r, &mut body)?;
            Ok(body)
        }
        Err(FrameError::TooLarge { len, max }) if len <= HARD_MAX_FRAME => {
            let mut head = [0u8; HEAD_BYTES];
            let first = len.min(HEAD_BYTES);
            read_full(r, &mut head[..first])?;
            let rest = u64::try_from(len - first).unwrap_or(u64::MAX);
            let skipped =
                io::copy(&mut r.by_ref().take(rest), &mut io::sink()).map_err(|e| io_error(&e))?;
            if skipped < rest {
                return Err(FrameError::Io("the stream ended inside a frame".into()));
            }
            Err(FrameError::Skipped {
                len,
                max,
                head: peek_head(&head[..first]),
            })
        }
        Err(e) => Err(e),
    }
}

/// Reads one frame. [`FrameError::Closed`] when the stream ends before its first byte.
pub fn read_frame(r: &mut impl Read, limit: FrameLimit) -> Result<Frame, FrameError> {
    decode_body(&read_body(r, limit)?)
}

/// Writes one frame (and flushes).
pub fn write_frame(w: &mut impl Write, frame: &Frame, limit: FrameLimit) -> Result<(), FrameError> {
    let bytes = encode(frame, limit)?;
    w.write_all(&bytes).map_err(|e| io_error(&e))?;
    w.flush().map_err(|e| io_error(&e))
}
