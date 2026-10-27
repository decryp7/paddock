//! The frame codec: a 4-byte little-endian length, then the postcard encoding of a value.
//!
//! The length is checked against the connection's [`FrameLimit`] before anything is allocated, and decoding never
//! panics, whatever the bytes.

use crate::error::IpcError;
use serde::de::DeserializeOwned;
use serde::Serialize;
use std::io::{self, Read, Write};

/// The default largest frame body, in bytes.
pub const MAX_FRAME: usize = 4 * 1024 * 1024;

/// No limit can be configured above this.
pub const HARD_MAX_FRAME: usize = 16 * 1024 * 1024;

/// The largest frame body a connection accepts, between 1 byte and [`HARD_MAX_FRAME`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FrameLimit(usize);

impl FrameLimit {
    /// A limit of `bytes`, clamped to `1..=HARD_MAX_FRAME`.
    pub fn new(bytes: usize) -> Self {
        FrameLimit(bytes.clamp(1, HARD_MAX_FRAME))
    }

    /// The limit in bytes.
    pub fn get(self) -> usize {
        self.0
    }
}

impl Default for FrameLimit {
    fn default() -> Self {
        FrameLimit(MAX_FRAME)
    }
}

fn too_large(len: usize, limit: FrameLimit) -> IpcError {
    IpcError::Protocol(format!("a frame of {len} bytes exceeds the limit of {} bytes", limit.get()))
}

/// `value` as one frame (length and body).
pub fn encode<T: Serialize>(value: &T, limit: FrameLimit) -> Result<Vec<u8>, IpcError> {
    let body = postcard::to_stdvec(value).map_err(|e| IpcError::Protocol(format!("cannot encode a frame: {e}")))?;
    if body.len() > limit.get() {
        return Err(too_large(body.len(), limit));
    }
    let len = u32::try_from(body.len()).map_err(|_| too_large(body.len(), limit))?;
    let mut out = Vec::with_capacity(4 + body.len());
    out.extend_from_slice(&len.to_le_bytes());
    out.extend_from_slice(&body);
    Ok(out)
}

/// Decodes one frame body; every byte must be used.
pub fn decode<T: DeserializeOwned>(body: &[u8]) -> Result<T, IpcError> {
    match postcard::take_from_bytes::<T>(body) {
        Ok((value, [])) => Ok(value),
        Ok((_, rest)) => Err(IpcError::Protocol(format!("{} unexpected bytes after a frame", rest.len()))),
        Err(e) => Err(IpcError::Protocol(format!("a malformed frame: {e}"))),
    }
}

/// Writes `value` as one frame and flushes.
pub fn write_frame<T: Serialize>(w: &mut impl Write, value: &T, limit: FrameLimit) -> Result<(), IpcError> {
    let bytes = encode(value, limit)?;
    w.write_all(&bytes)?;
    w.flush()?;
    Ok(())
}

/// Reads one frame. `Ok(None)` when the stream ends cleanly between frames; a stream that ends inside a frame, an
/// oversized length or a malformed body is an error.
pub fn read_frame<T: DeserializeOwned>(r: &mut impl Read, limit: FrameLimit) -> Result<Option<T>, IpcError> {
    let mut header = [0u8; 4];
    let mut filled = 0;
    while filled < header.len() {
        match r.read(&mut header[filled..]) {
            Ok(0) if filled == 0 => return Ok(None),
            Ok(0) => return Err(IpcError::Protocol("the stream ended inside a frame header".into())),
            Ok(n) => filled += n,
            Err(e) if e.kind() == io::ErrorKind::Interrupted => {}
            Err(e) => return Err(e.into()),
        }
    }
    let len = usize::try_from(u32::from_le_bytes(header)).unwrap_or(usize::MAX);
    if len > limit.get() {
        return Err(too_large(len, limit));
    }
    let mut body = vec![0u8; len];
    r.read_exact(&mut body).map_err(|e| match e.kind() {
        io::ErrorKind::UnexpectedEof => IpcError::Protocol("the stream ended inside a frame".into()),
        _ => e.into(),
    })?;
    decode(&body).map(Some)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::wire::Frame;
    use proptest::prelude::*;

    #[test]
    fn limits_are_clamped() {
        assert_eq!(FrameLimit::new(0).get(), 1);
        assert_eq!(FrameLimit::new(usize::MAX).get(), HARD_MAX_FRAME);
        assert_eq!(FrameLimit::default().get(), MAX_FRAME);
    }

    #[test]
    fn frames_roundtrip_and_end_cleanly() {
        let mut buf = Vec::new();
        let f: Frame<Vec<u8>> = Frame::Reply {
            id: 3,
            body: vec![1, 2, 3],
        };
        write_frame(&mut buf, &f, FrameLimit::default()).unwrap();
        write_frame(&mut buf, &f, FrameLimit::default()).unwrap();
        let mut r = &buf[..];
        assert_eq!(
            read_frame::<Frame<Vec<u8>>>(&mut r, FrameLimit::default()).unwrap(),
            Some(f.clone())
        );
        assert_eq!(read_frame::<Frame<Vec<u8>>>(&mut r, FrameLimit::default()).unwrap(), Some(f));
        assert_eq!(read_frame::<Frame<Vec<u8>>>(&mut r, FrameLimit::default()).unwrap(), None);
    }

    #[test]
    fn an_oversized_frame_is_refused_on_both_sides_before_allocating() {
        let limit = FrameLimit::new(16);
        let err = encode(&vec![0u8; 100], limit).unwrap_err();
        assert!(matches!(err, IpcError::Protocol(m) if m.contains("exceeds")));
        // A header announcing 4 GiB: refused from the header alone.
        let mut r = &[0xff, 0xff, 0xff, 0xff, 1, 2][..];
        assert!(matches!(read_frame::<Vec<u8>>(&mut r, limit), Err(IpcError::Protocol(m)) if m.contains("exceeds")));
    }

    #[test]
    fn truncated_and_malformed_frames_are_protocol_errors() {
        let mut r = &[1, 0][..];
        assert!(matches!(
            read_frame::<u8>(&mut r, FrameLimit::default()),
            Err(IpcError::Protocol(_))
        ));
        let mut r = &[5, 0, 0, 0, 1][..];
        assert!(matches!(
            read_frame::<u8>(&mut r, FrameLimit::default()),
            Err(IpcError::Protocol(_))
        ));
        // A body with a byte too many.
        let mut r = &[2, 0, 0, 0, 1, 1][..];
        assert!(matches!(read_frame::<u8>(&mut r, FrameLimit::default()), Err(IpcError::Protocol(m)) if m.contains("unexpected")));
        // An unknown enum variant.
        let mut r = &[1, 0, 0, 0, 99][..];
        assert!(matches!(
            read_frame::<Frame<u8>>(&mut r, FrameLimit::default()),
            Err(IpcError::Protocol(_))
        ));
    }

    #[test]
    fn read_errors_keep_their_meaning() {
        struct Broken;
        impl Read for Broken {
            fn read(&mut self, _: &mut [u8]) -> io::Result<usize> {
                Err(io::Error::from(io::ErrorKind::ConnectionReset))
            }
        }
        assert_eq!(read_frame::<u8>(&mut Broken, FrameLimit::default()), Err(IpcError::Closed));
    }

    proptest! {
        #[test]
        fn decoding_random_bytes_never_panics(bytes in proptest::collection::vec(any::<u8>(), 0..512)) {
            let mut r = &bytes[..];
            let _ = read_frame::<Frame<Vec<String>>>(&mut r, FrameLimit::new(256));
            let _ = decode::<Frame<(u64, String, Option<f64>)>>(&bytes);
        }

        #[test]
        fn any_frame_roundtrips(id in any::<u64>(), body in proptest::collection::vec(any::<u8>(), 0..256)) {
            let f: Frame<Vec<u8>> = Frame::Call { id, body };
            let bytes = encode(&f, FrameLimit::default()).unwrap();
            let mut r = &bytes[..];
            prop_assert_eq!(read_frame::<Frame<Vec<u8>>>(&mut r, FrameLimit::default()).unwrap(), Some(f));
        }
    }
}
