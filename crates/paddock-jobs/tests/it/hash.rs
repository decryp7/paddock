use paddock_jobs::*;
use std::io::{self, Read, Write};
use std::sync::atomic::{AtomicU64, Ordering};

const ABC: &str = "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad";

#[test]
fn hex_formats_bytes() {
    assert_eq!(hex(&[0, 1, 0xab, 0xff]), "0001abff");
    assert_eq!(hex(&[]), "");
}

#[test]
fn hashes_while_writing_without_a_second_pass() {
    let mut w = HashingWriter::new(Vec::new());
    w.write_all(b"a").unwrap();
    w.write_all(b"bc").unwrap();
    w.flush().unwrap();
    assert_eq!(w.bytes(), 3);
    let (out, digest, n) = w.finish().unwrap();
    assert_eq!(
        (out.as_slice(), n, hex(&digest).as_str()),
        (b"abc".as_slice(), 3, ABC)
    );
}

#[test]
fn reports_bytes_and_stops_on_cancel() {
    let counter = AtomicU64::new(0);
    let cancel = CancelToken::new();
    let mut w = HashingWriter::new(Vec::new())
        .with_cancel(&cancel)
        .with_counter(&counter);
    w.write_all(b"abcd").unwrap();
    assert_eq!(counter.load(Ordering::Relaxed), 4);
    cancel.cancel();
    let e = w.write_all(b"more").unwrap_err();
    assert_eq!(
        HashingWriter::<Vec<u8>>::cancelled(&e),
        Some(Cancelled(CancelReason::User))
    );
    assert_eq!(
        HashingWriter::<Vec<u8>>::cancelled(&io::Error::other("disk")),
        None
    );
    assert_eq!(
        HashingWriter::<Vec<u8>>::cancelled(&io::Error::from(io::ErrorKind::NotFound)),
        None
    );
}

#[test]
fn copy_hashed_matches_and_counts() {
    let data: Vec<u8> = (0..300_000u32).map(|i| (i % 251) as u8).collect();
    let counter = AtomicU64::new(0);
    let mut out = Vec::new();
    let (digest, n) = copy_hashed(
        &mut data.as_slice(),
        &mut out,
        &CancelToken::new(),
        &counter,
    )
    .unwrap();
    assert_eq!(out, data);
    assert_eq!(n, 300_000);
    assert_eq!(counter.load(Ordering::Relaxed), 300_000);
    let mut again = HashingWriter::new(Vec::new());
    again.write_all(&data).unwrap();
    assert_eq!(again.finish().unwrap().1, digest);
    assert_eq!(
        copy_hashed(&mut io::empty(), Vec::new(), &CancelToken::new(), &())
            .unwrap()
            .1,
        0
    );
}

struct Flaky(u8);
impl Read for Flaky {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        self.0 += 1;
        match self.0 {
            1 => Err(io::ErrorKind::Interrupted.into()),
            2 => {
                buf[0] = b'x';
                Ok(1)
            }
            3 => Err(io::Error::other("link down")),
            _ => Ok(0),
        }
    }
}

#[test]
fn copy_hashed_retries_interrupted_and_surfaces_read_errors() {
    let e = copy_hashed(&mut Flaky(0), Vec::new(), &CancelToken::new(), &()).unwrap_err();
    assert_eq!(e.to_string(), "link down");
}

#[test]
fn copy_hashed_cancels_within_one_chunk() {
    struct Endless(CancelToken, u64);
    impl Read for Endless {
        fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
            self.1 += buf.len() as u64;
            if self.1 >= 4 * CHUNK as u64 {
                self.0.cancel();
            }
            Ok(buf.len())
        }
    }
    let cancel = CancelToken::new();
    let mut r = Endless(cancel.clone(), 0);
    let e = copy_hashed(&mut r, io::sink(), &cancel, &()).unwrap_err();
    assert!(HashingWriter::<io::Sink>::cancelled(&e).is_some());
    assert!(
        r.1 <= 6 * CHUNK as u64,
        "stopped within a couple of chunks, read {}",
        r.1
    );
}

struct FailFlush;
impl Write for FailFlush {
    fn write(&mut self, b: &[u8]) -> io::Result<usize> {
        Ok(b.len())
    }
    fn flush(&mut self) -> io::Result<()> {
        Err(io::Error::other("flush failed"))
    }
}

#[test]
fn finish_surfaces_flush_errors() {
    assert!(HashingWriter::new(FailFlush).finish().is_err());
}
