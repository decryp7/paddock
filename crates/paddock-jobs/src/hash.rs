use crate::cancel::{CancelToken, Cancelled};
use crate::reporter::Counter;
use sha2::{Digest as _, Sha256};
use std::io::{self, Read, Write};

/// Copy chunk size: one cancel check and one progress update per chunk, so cancel latency is one chunk of I/O.
pub const CHUNK: usize = 64 * 1024;

/// Lower-case hex of a digest.
pub fn hex(bytes: &[u8]) -> String {
    use std::fmt::Write as _;
    bytes
        .iter()
        .fold(String::with_capacity(bytes.len() * 2), |mut s, b| {
            let _ = write!(s, "{b:02x}");
            s
        })
}

/// A `Write` adapter that hashes (SHA-256) and counts what passes through, so a download is verified without a second read pass.
/// Optionally reports bytes to a [`Counter`] and checks a [`CancelToken`] before every write; a cancel surfaces as an `io::Error`
/// whose source is [`Cancelled`] (see [`HashingWriter::cancelled`]).
pub struct HashingWriter<'a, W: Write> {
    inner: W,
    hasher: Sha256,
    bytes: u64,
    cancel: Option<&'a CancelToken>,
    counter: Option<&'a dyn Counter>,
}

impl<'a, W: Write> HashingWriter<'a, W> {
    pub fn new(inner: W) -> Self {
        HashingWriter {
            inner,
            hasher: Sha256::new(),
            bytes: 0,
            cancel: None,
            counter: None,
        }
    }

    pub fn with_cancel(mut self, cancel: &'a CancelToken) -> Self {
        self.cancel = Some(cancel);
        self
    }

    pub fn with_counter(mut self, counter: &'a dyn Counter) -> Self {
        self.counter = Some(counter);
        self
    }

    /// Bytes written so far.
    pub fn bytes(&self) -> u64 {
        self.bytes
    }

    /// The cancel reason inside an `io::Error` produced by this writer, if that is what it is.
    pub fn cancelled(e: &io::Error) -> Option<Cancelled> {
        e.get_ref()
            .and_then(|s| s.downcast_ref::<Cancelled>())
            .copied()
    }

    /// Flushes and returns the inner writer with the digest and the byte count.
    pub fn finish(mut self) -> io::Result<(W, [u8; 32], u64)> {
        self.inner.flush()?;
        Ok((self.inner, self.hasher.finalize().into(), self.bytes))
    }
}

impl<W: Write> Write for HashingWriter<'_, W> {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        if let Some(c) = self.cancel {
            c.check().map_err(io::Error::other)?;
        }
        let n = self.inner.write(buf)?;
        self.hasher.update(&buf[..n]);
        self.bytes += n as u64;
        if let Some(c) = self.counter {
            c.add(n as u64);
        }
        Ok(n)
    }

    fn flush(&mut self) -> io::Result<()> {
        self.inner.flush()
    }
}

/// Copies `reader` to `writer` in [`CHUNK`]s, hashing while writing. Returns the SHA-256 and the byte count; the caller compares it
/// to the expected checksum before renaming the temp file into place. A cancel returns an error for which
/// [`HashingWriter::cancelled`] is `Some`.
pub fn copy_hashed<R: Read, W: Write>(
    reader: &mut R,
    writer: W,
    cancel: &CancelToken,
    counter: &dyn Counter,
) -> io::Result<([u8; 32], u64)> {
    let mut out = HashingWriter::new(writer)
        .with_cancel(cancel)
        .with_counter(counter);
    let mut buf = vec![0u8; CHUNK];
    loop {
        let n = match reader.read(&mut buf) {
            Ok(0) => break,
            Ok(n) => n,
            Err(e) if e.kind() == io::ErrorKind::Interrupted => continue,
            Err(e) => return Err(e),
        };
        out.write_all(&buf[..n])?;
    }
    let (_, digest, bytes) = out.finish()?;
    Ok((digest, bytes))
}
