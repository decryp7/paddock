//! The OS measurements of ADR 0072 WP-0 (rule 48; results in docs/benchmarks.md): what a client reads when the other side
//! closes with unread received data, whether a pipe closed with `CloseHandle` still delivers what was written before it,
//! what a client waiting in a listener's backlog sees when the listener closes, and the baseline of an ordered close
//! (close-to-release time, refusals per second). It uses only the OS's own APIs, not paddock-ipc, so it measures the platform.
//!
//! Run: `cargo run --release -p paddock-ipc --example os_endings`. Each line is `RESULT <what> <key=value>...`. Nothing
//! sleeps: the orchestration waits on the data itself (`MSG_PEEK` until it arrived), and every read has a hang guard.
#![allow(unsafe_code, clippy::unwrap_used, clippy::expect_used, missing_docs)]

use std::collections::BTreeMap;
use std::io::{ErrorKind, Read, Write};
use std::net::{Shutdown, TcpListener, TcpStream};
use std::time::{Duration, Instant};

const REPS: usize = 200;
const TIMING_REPS: usize = 500;
const GUARD: Duration = Duration::from_secs(10);

/// How a read-to-end finished, as a class.
fn classify(bytes: &[u8], end: Result<(), ErrorKind>, expect: &[u8]) -> String {
    let got = bytes.starts_with(expect);
    match (got, end) {
        (true, Ok(())) => "reason+eof".into(),
        (true, Err(k)) => format!("reason+error({k:?})"),
        (false, Ok(())) => "eof-without-reason".into(),
        (false, Err(k)) => format!("error({k:?})-without-reason"),
    }
}

fn read_end(r: &mut impl Read) -> (Vec<u8>, Result<(), ErrorKind>) {
    let mut bytes = Vec::new();
    let mut buf = [0u8; 4096];
    loop {
        match r.read(&mut buf) {
            Ok(0) => return (bytes, Ok(())),
            Ok(n) => bytes.extend_from_slice(&buf[..n]),
            Err(e) if e.kind() == ErrorKind::Interrupted => {}
            Err(e) => return (bytes, Err(e.kind())),
        }
    }
}

fn report(what: &str, counts: &BTreeMap<String, usize>) {
    let line: Vec<String> = counts.iter().map(|(k, v)| format!("{k}={v}")).collect();
    println!("RESULT {what} reps={REPS} {}", line.join(" "));
}

fn tcp_pair(l: &TcpListener) -> (TcpStream, TcpStream) {
    let c = TcpStream::connect(l.local_addr().unwrap()).unwrap();
    let (s, _) = l.accept().unwrap();
    // Set before anything closes: macOS refuses setsockopt on a socket the peer has reset.
    c.set_read_timeout(Some(GUARD)).unwrap();
    (s, c)
}

/// Waits until `s` holds `n` unread bytes (a condition, not a pause).
fn tcp_wait_unread(s: &TcpStream, n: usize) {
    let mut buf = vec![0u8; n];
    let end = Instant::now() + GUARD;
    while s.peek(&mut buf).unwrap() < n {
        assert!(Instant::now() < end, "the data never arrived");
        std::thread::yield_now();
    }
}

fn tcp_unread() {
    let l = TcpListener::bind("127.0.0.1:0").unwrap();
    let mut counts = BTreeMap::new();
    for _ in 0..REPS {
        let (mut s, mut c) = tcp_pair(&l);
        c.write_all(&[7u8; 4096]).unwrap();
        tcp_wait_unread(&s, 4096);
        s.write_all(b"REASON\n").unwrap();
        drop(s);
        let (bytes, end) = read_end(&mut c);
        *counts.entry(classify(&bytes, end, b"REASON")).or_insert(0) += 1;
    }
    report("tcp-close-with-unread-input", &counts);
}

fn tcp_ordered() {
    // The ordered close: reason, half-close, read what the peer sent, then release.
    let l = TcpListener::bind("127.0.0.1:0").unwrap();
    let mut counts = BTreeMap::new();
    for _ in 0..REPS {
        let (mut s, mut c) = tcp_pair(&l);
        c.write_all(&[7u8; 4096]).unwrap();
        tcp_wait_unread(&s, 4096);
        s.write_all(b"REASON\n").unwrap();
        s.shutdown(Shutdown::Write).unwrap();
        let (bytes, end) = read_end(&mut c);
        drop(c);
        let (drained, _) = read_end(&mut s);
        let class = format!(
            "{}+drained{}",
            classify(&bytes, end, b"REASON"),
            drained.len()
        );
        *counts.entry(class).or_insert(0) += 1;
    }
    report("tcp-ordered-close-with-unread-input", &counts);
}

fn tcp_backlog() {
    let mut counts = BTreeMap::new();
    for _ in 0..REPS {
        let l = TcpListener::bind("127.0.0.1:0").unwrap();
        let mut c = TcpStream::connect(l.local_addr().unwrap()).unwrap();
        c.set_read_timeout(Some(GUARD)).unwrap();
        // conformance: freed-port — the case measured is a closed listener with a client in its backlog; nothing connects to the port again
        drop(l);
        let wrote = c
            .write_all(b"HELLO\n")
            .map_or_else(|e| format!("write-{:?}", e.kind()), |()| "write-ok".into());
        let (bytes, end) = read_end(&mut c);
        *counts
            .entry(format!("{wrote}:{}", classify(&bytes, end, b"REASON")))
            .or_insert(0) += 1;
    }
    report("tcp-listener-closed-with-client-in-backlog", &counts);
}

#[cfg(unix)]
mod unix {
    use super::*;
    use std::os::unix::net::{UnixListener, UnixStream};

    fn path(tag: &str) -> std::path::PathBuf {
        std::env::temp_dir().join(format!("os-end-{}-{tag}", std::process::id()))
    }

    pub fn unread() {
        let p = path("a");
        let _ = std::fs::remove_file(&p);
        let l = UnixListener::bind(&p).unwrap();
        let mut counts = BTreeMap::new();
        for _ in 0..REPS {
            let mut c = UnixStream::connect(&p).unwrap();
            c.set_read_timeout(Some(GUARD)).unwrap();
            let (mut s, _) = l.accept().unwrap();
            c.write_all(&[7u8; 4096]).unwrap();
            s.write_all(b"REASON\n").unwrap();
            drop(s);
            let (bytes, end) = read_end(&mut c);
            *counts.entry(classify(&bytes, end, b"REASON")).or_insert(0) += 1;
        }
        report("unix-close-with-unread-input", &counts);
        let _ = std::fs::remove_file(&p);
    }

    pub fn ordered() {
        let p = path("b");
        let _ = std::fs::remove_file(&p);
        let l = UnixListener::bind(&p).unwrap();
        let mut counts = BTreeMap::new();
        for _ in 0..REPS {
            let mut c = UnixStream::connect(&p).unwrap();
            c.set_read_timeout(Some(GUARD)).unwrap();
            let (mut s, _) = l.accept().unwrap();
            c.write_all(&[7u8; 4096]).unwrap();
            s.write_all(b"REASON\n").unwrap();
            s.shutdown(Shutdown::Write).unwrap();
            let (bytes, end) = read_end(&mut c);
            drop(c);
            let (drained, _) = read_end(&mut s);
            let class = format!(
                "{}+drained{}",
                classify(&bytes, end, b"REASON"),
                drained.len()
            );
            *counts.entry(class).or_insert(0) += 1;
        }
        report("unix-ordered-close-with-unread-input", &counts);
        let _ = std::fs::remove_file(&p);
    }

    pub fn backlog() {
        let p = path("c");
        let mut counts = BTreeMap::new();
        for _ in 0..REPS {
            let _ = std::fs::remove_file(&p);
            let l = UnixListener::bind(&p).unwrap();
            let mut c = UnixStream::connect(&p).unwrap();
            c.set_read_timeout(Some(GUARD)).unwrap();
            drop(l);
            let wrote = c
                .write_all(b"HELLO\n")
                .map_or_else(|e| format!("write-{:?}", e.kind()), |()| "write-ok".into());
            let (bytes, end) = read_end(&mut c);
            *counts
                .entry(format!("{wrote}:{}", classify(&bytes, end, b"REASON")))
                .or_insert(0) += 1;
        }
        report("unix-listener-closed-with-client-in-backlog", &counts);
        let _ = std::fs::remove_file(&p);
    }

    pub fn timing() {
        let p = path("d");
        let _ = std::fs::remove_file(&p);
        let l = UnixListener::bind(&p).unwrap();
        let mut times = Vec::new();
        for _ in 0..TIMING_REPS {
            let mut c = UnixStream::connect(&p).unwrap();
            let (mut s, _) = l.accept().unwrap();
            let t = Instant::now();
            s.write_all(b"REASON\n").unwrap();
            s.shutdown(Shutdown::Write).unwrap();
            let mut buf = [0u8; 16];
            let n = c.read(&mut buf).unwrap();
            assert_eq!(&buf[..n], b"REASON\n");
            drop(c);
            let (_, _) = read_end(&mut s);
            times.push(t.elapsed());
        }
        percentiles("unix-close-to-release", times);
        let t = Instant::now();
        for _ in 0..TIMING_REPS {
            let mut c = UnixStream::connect(&p).unwrap();
            let (mut s, _) = l.accept().unwrap();
            s.write_all(b"REASON\n").unwrap();
            s.shutdown(Shutdown::Write).unwrap();
            let _ = read_end(&mut c);
            drop(c);
            let _ = read_end(&mut s);
        }
        println!(
            "RESULT unix-refusals-per-second reps={TIMING_REPS} per_second={:.0}",
            TIMING_REPS as f64 / t.elapsed().as_secs_f64()
        );
        let _ = std::fs::remove_file(&p);
    }
}

fn percentiles(what: &str, mut times: Vec<Duration>) {
    times.sort();
    let at = |q: f64| times[((times.len() as f64 - 1.0) * q) as usize].as_micros();
    println!(
        "RESULT {what} reps={} p50_us={} p99_us={} max_us={}",
        times.len(),
        at(0.5),
        at(0.99),
        times[times.len() - 1].as_micros()
    );
}

fn tcp_timing() {
    let l = TcpListener::bind("127.0.0.1:0").unwrap();
    let mut times = Vec::new();
    for _ in 0..TIMING_REPS {
        let (mut s, mut c) = tcp_pair(&l);
        let t = Instant::now();
        s.write_all(b"REASON\n").unwrap();
        s.shutdown(Shutdown::Write).unwrap();
        let mut buf = [0u8; 16];
        let n = c.read(&mut buf).unwrap();
        assert_eq!(&buf[..n], b"REASON\n");
        drop(c);
        let _ = read_end(&mut s);
        times.push(t.elapsed());
    }
    percentiles("tcp-close-to-release", times);
    let t = Instant::now();
    for _ in 0..TIMING_REPS {
        let (mut s, mut c) = tcp_pair(&l);
        s.write_all(b"REASON\n").unwrap();
        s.shutdown(Shutdown::Write).unwrap();
        let _ = read_end(&mut c);
        drop(c);
        let _ = read_end(&mut s);
    }
    println!(
        "RESULT tcp-refusals-per-second reps={TIMING_REPS} per_second={:.0}",
        TIMING_REPS as f64 / t.elapsed().as_secs_f64()
    );
}

#[cfg(windows)]
mod pipes {
    use super::*;
    use windows_sys::Win32::Foundation::{
        CloseHandle, ERROR_BROKEN_PIPE, ERROR_PIPE_CONNECTED, GENERIC_READ, GENERIC_WRITE,
        GetLastError, HANDLE, INVALID_HANDLE_VALUE,
    };
    use windows_sys::Win32::Storage::FileSystem::{
        CreateFileW, FILE_ATTRIBUTE_NORMAL, OPEN_EXISTING, ReadFile, WriteFile,
    };
    use windows_sys::Win32::System::Pipes::{
        ConnectNamedPipe, CreateNamedPipeW, DisconnectNamedPipe, PIPE_READMODE_BYTE,
        PIPE_REJECT_REMOTE_CLIENTS, PIPE_TYPE_BYTE, PIPE_WAIT,
    };

    const ACCESS_DUPLEX: u32 = 3;
    static NEXT: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);

    fn wide(s: &str) -> Vec<u16> {
        s.encode_utf16().chain(std::iter::once(0)).collect()
    }

    fn name() -> Vec<u16> {
        wide(&format!(
            r"\\.\pipe\os-end-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, std::sync::atomic::Ordering::SeqCst)
        ))
    }

    fn server(name: &[u16]) -> HANDLE {
        let h = unsafe {
            CreateNamedPipeW(
                name.as_ptr(),
                ACCESS_DUPLEX,
                PIPE_TYPE_BYTE | PIPE_READMODE_BYTE | PIPE_WAIT | PIPE_REJECT_REMOTE_CLIENTS,
                1,
                65536,
                65536,
                0,
                std::ptr::null(),
            )
        };
        assert_ne!(h, INVALID_HANDLE_VALUE, "CreateNamedPipeW: {}", unsafe {
            GetLastError()
        });
        h
    }

    fn client(name: &[u16]) -> HANDLE {
        let h = unsafe {
            CreateFileW(
                name.as_ptr(),
                GENERIC_READ | GENERIC_WRITE,
                0,
                std::ptr::null(),
                OPEN_EXISTING,
                FILE_ATTRIBUTE_NORMAL,
                std::ptr::null_mut(),
            )
        };
        assert_ne!(h, INVALID_HANDLE_VALUE, "CreateFileW: {}", unsafe {
            GetLastError()
        });
        h
    }

    fn connected() -> (HANDLE, HANDLE) {
        let n = name();
        let s = server(&n);
        let c = client(&n);
        let ok = unsafe { ConnectNamedPipe(s, std::ptr::null_mut()) };
        assert!(ok != 0 || unsafe { GetLastError() } == ERROR_PIPE_CONNECTED);
        (s, c)
    }

    fn write(h: HANDLE, bytes: &[u8]) {
        let mut n = 0u32;
        assert_ne!(
            unsafe {
                WriteFile(
                    h,
                    bytes.as_ptr(),
                    bytes.len() as u32,
                    &mut n,
                    std::ptr::null_mut(),
                )
            },
            0,
            "WriteFile: {}",
            unsafe { GetLastError() }
        );
    }

    /// Reads to the end: `Ok` at ERROR_BROKEN_PIPE (the end of a pipe), else the Win32 error.
    fn read_end(h: HANDLE) -> (Vec<u8>, Result<(), u32>) {
        let mut bytes = Vec::new();
        let mut buf = [0u8; 4096];
        loop {
            let mut n = 0u32;
            let ok = unsafe {
                ReadFile(
                    h,
                    buf.as_mut_ptr(),
                    buf.len() as u32,
                    &mut n,
                    std::ptr::null_mut(),
                )
            };
            if ok == 0 {
                let e = unsafe { GetLastError() };
                return (
                    bytes,
                    if e == ERROR_BROKEN_PIPE {
                        Ok(())
                    } else {
                        Err(e)
                    },
                );
            }
            bytes.extend_from_slice(&buf[..n as usize]);
        }
    }

    fn class(bytes: &[u8], end: Result<(), u32>) -> String {
        let got = bytes.starts_with(b"REASON");
        match (got, end) {
            (true, Ok(())) => "reason+broken-pipe(109)".into(),
            (true, Err(e)) => format!("reason+win32({e})"),
            (false, Ok(())) => "broken-pipe(109)-without-reason".into(),
            (false, Err(e)) => format!("win32({e})-without-reason"),
        }
    }

    pub fn unread(disconnect: bool) {
        let mut counts = BTreeMap::new();
        for _ in 0..REPS {
            let (s, c) = connected();
            write(c, &[7u8; 4096]);
            write(s, b"REASON\n");
            unsafe {
                if disconnect {
                    DisconnectNamedPipe(s);
                }
                CloseHandle(s);
            }
            let (bytes, end) = read_end(c);
            unsafe { CloseHandle(c) };
            *counts.entry(class(&bytes, end)).or_insert(0) += 1;
        }
        report(
            if disconnect {
                "pipe-disconnectnamedpipe-with-unread-input"
            } else {
                "pipe-closehandle-with-unread-input"
            },
            &counts,
        );
    }

    /// The pending instance of a listener is closed with a client connected to it that was never accepted.
    pub fn backlog() {
        let mut counts = BTreeMap::new();
        for _ in 0..REPS {
            let n = name();
            let s = server(&n);
            let c = client(&n);
            unsafe { CloseHandle(s) };
            let mut wrote = 0u32;
            let w =
                unsafe { WriteFile(c, b"HELLO\n".as_ptr(), 6, &mut wrote, std::ptr::null_mut()) };
            let wrote = if w != 0 {
                "write-ok".to_string()
            } else {
                format!("write-win32({})", unsafe { GetLastError() })
            };
            let (bytes, end) = read_end(c);
            unsafe { CloseHandle(c) };
            *counts
                .entry(format!("{wrote}:{}", class(&bytes, end)))
                .or_insert(0) += 1;
        }
        report("pipe-listener-closed-with-client-not-accepted", &counts);
    }

    pub fn timing() {
        let mut times = Vec::new();
        for _ in 0..TIMING_REPS {
            let (s, c) = connected();
            let t = Instant::now();
            write(s, b"REASON\n");
            let mut buf = [0u8; 16];
            let mut n = 0u32;
            assert_ne!(
                unsafe { ReadFile(c, buf.as_mut_ptr(), 16, &mut n, std::ptr::null_mut()) },
                0
            );
            unsafe { CloseHandle(c) };
            let (_, _) = read_end(s);
            unsafe { CloseHandle(s) };
            times.push(t.elapsed());
        }
        percentiles("pipe-close-to-release", times);
        let t = Instant::now();
        for _ in 0..TIMING_REPS {
            let (s, c) = connected();
            write(s, b"REASON\n");
            unsafe { CloseHandle(s) };
            let _ = read_end(c);
            unsafe { CloseHandle(c) };
        }
        println!(
            "RESULT pipe-refusals-per-second reps={TIMING_REPS} per_second={:.0}",
            TIMING_REPS as f64 / t.elapsed().as_secs_f64()
        );
    }
}

fn main() {
    println!(
        "RESULT platform os={} arch={}",
        std::env::consts::OS,
        std::env::consts::ARCH
    );
    tcp_unread();
    tcp_ordered();
    tcp_backlog();
    tcp_timing();
    #[cfg(unix)]
    {
        unix::unread();
        unix::ordered();
        unix::backlog();
        unix::timing();
    }
    #[cfg(windows)]
    {
        pipes::unread(false);
        pipes::unread(true);
        pipes::backlog();
        pipes::timing();
    }
}
