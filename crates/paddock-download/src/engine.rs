//! The download engine: probe, resume, segmented or streamed transfer with one in-order hash, verify, atomic rename.
//!
//! Every transfer runs on its own threads; the calling thread (a job worker) only supervises: it notices a cancel, a failure, a
//! stall or the end within 10 ms, saves the resume state about once a second and on every stop, and returns. A connection that
//! is stuck inside a read is left to end on its own (it writes nothing more once the download stopped), so Cancel never waits
//! for the network.

use crate::config::{Config, MAX_CONNECTIONS};
use crate::error::DownloadError;
use crate::hosts::{HostCaps, Permit};
use crate::request::{Integrity, Request, Sha256Digest};
use crate::resume::ResumeState;
use crate::transport::{
    ByteRange, ContentRange, Get, Response, Transport, is_identity, parse_content_range,
    parse_length, parse_retry_after, validator,
};
use paddock_jobs::{CancelToken, HashingWriter, JobContext, Phase};
use std::collections::BTreeMap;
use std::fs::{File, OpenOptions};
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, AtomicUsize, Ordering};
use std::sync::mpsc::RecvTimeoutError;
use std::sync::{Arc, Condvar, Mutex, MutexGuard};
use std::time::{Duration, Instant, SystemTime};

/// The phase ids of a download job, in order (`resume` only when a partial file is resumed).
pub mod phases {
    /// "Connecting": the first request (items, total 1).
    pub const CONNECT: &str = "connect";
    /// "Checking the partial download": the kept prefix is hashed (bytes, total = its length).
    pub const RESUME: &str = "resume";
    /// "Downloading" (bytes; total = the size, or none while the host does not say).
    pub const DOWNLOAD: &str = "download";
    /// "Verifying": the digest computed while writing is compared and the file renamed into place (items, total 1; no read).
    pub const VERIFY: &str = "verify";
}

const BUF: usize = paddock_jobs::CHUNK;
const RESUME_BUF: usize = 1 << 20;
const TICK: Duration = Duration::from_millis(10);
const ENDED_EARLY: &str = "the connections ended before the download finished";

/// A finished download.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Downloaded {
    /// The destination, now holding the verified file.
    pub path: PathBuf,
    pub bytes: u64,
    /// The SHA-256 of the file, computed while writing.
    pub sha256: Sha256Digest,
    /// Bytes kept from an earlier, stopped run (0 for a fresh download).
    pub resumed_from: u64,
    /// The most connections the download had open at once.
    pub connections: usize,
    /// True when the host served byte ranges (so a stop can be resumed).
    pub ranges: bool,
}

/// Free bytes on the disk that holds a folder (the platform call; tests pass their own).
pub type FreeSpace = dyn Fn(&Path) -> io::Result<u64> + Send + Sync;

/// Runs downloads. Cheap to share (`Arc<Downloader>`); one per app, so the [`HostCaps`] hold across all its downloads.
pub struct Downloader {
    net: Net,
    free_space: Arc<FreeSpace>,
}

/// What the transfer threads need of the downloader.
#[derive(Clone)]
struct Net {
    transport: Arc<dyn Transport>,
    hosts: Arc<HostCaps>,
    config: Config,
}

impl Downloader {
    /// A downloader over `transport` with [`Config::default`] and [`HostCaps::default`] (4 connections per host).
    pub fn new(transport: Arc<dyn Transport>) -> Self {
        Downloader {
            net: Net {
                transport,
                hosts: Arc::new(HostCaps::default()),
                config: Config::default(),
            },
            free_space: Arc::new(|dir: &Path| fs4::available_space(dir)),
        }
    }

    /// A downloader over [`HttpTransport`](crate::HttpTransport).
    #[cfg(feature = "http")]
    pub fn http(config: crate::HttpConfig) -> Result<Self, DownloadError> {
        Ok(Downloader::new(Arc::new(crate::HttpTransport::new(
            config,
        )?)))
    }

    pub fn with_config(mut self, mut config: Config) -> Self {
        config.connections = config.connections.clamp(1, MAX_CONNECTIONS);
        config.attempts = config.attempts.max(1);
        config.min_block = config.min_block.max(1);
        config.max_buffer = config.max_buffer.max(config.min_block);
        self.net.config = config;
        self
    }

    /// Another way to learn the free space of a folder's disk (tests; an app with its own quota).
    pub fn with_free_space(
        mut self,
        free: impl Fn(&Path) -> io::Result<u64> + Send + Sync + 'static,
    ) -> Self {
        self.free_space = Arc::new(free);
        self
    }

    /// The network port this downloader uses (for a small request of the app's own, such as release information).
    pub fn transport(&self) -> &Arc<dyn Transport> {
        &self.net.transport
    }

    pub fn with_hosts(mut self, hosts: Arc<HostCaps>) -> Self {
        self.net.hosts = hosts;
        self
    }

    pub fn config(&self) -> &Config {
        &self.net.config
    }

    pub fn hosts(&self) -> &Arc<HostCaps> {
        &self.net.hosts
    }

    /// Downloads `req` inside the job `ctx`: phases [`phases::CONNECT`], [`phases::RESUME`] (when resuming),
    /// [`phases::DOWNLOAD`] and [`phases::VERIFY`], each on its own scale and never going backwards. The file appears under
    /// [`Request::dest`] only verified and complete (one rename); before that it is `<name>.part`. A cancel is noticed within
    /// [`paddock_jobs::CANCEL_LATENCY_TARGET_MS`] in every phase. Blocks the calling thread: call it from a job worker, never
    /// from a UI or IPC thread.
    pub fn download(&self, ctx: &JobContext, req: &Request) -> Result<Downloaded, DownloadError> {
        ctx.cancel_token().check().map_err(cancelled)?;
        req.dest()
            .parent()
            .filter(|d| !d.as_os_str().is_empty())
            .map(std::fs::create_dir_all)
            .transpose()?;
        if let Some(size) = req.pinned_size() {
            // Known in advance: refused before connecting.
            self.check_space(req, size)?;
        }
        ctx.phase(Phase::items(phases::CONNECT, "Connecting", 1));
        let ctl = Arc::new(Ctl::new(ctx.cancel_token().clone()));
        let probe = self.probe(req, &ctl)?;
        ctx.add(1);
        ctl.touch();
        match probe {
            Probe::Ranges { size, validator } => self.ranged(ctx, req, &ctl, size, validator),
            Probe::Stream {
                first,
                size,
                validator,
            } => self.streamed(ctx, req, &ctl, first, size, validator),
        }
    }

    fn check_size(&self, req: &Request, size: u64) -> Result<(), DownloadError> {
        if let Some(expected) = req.pinned_size().filter(|&e| e != size) {
            return Err(DownloadError::SizeMismatch {
                expected,
                actual: size,
            });
        }
        let limit = req.limit(self.net.config.max_bytes);
        if size > limit {
            return Err(DownloadError::TooLarge { size, limit });
        }
        self.check_space(req, size)
    }

    /// Refuses a download of `size` bytes that would leave less than [`Config::disk_reserve`] free on the destination's
    /// disk (the bytes of a kept partial file count as written). When the free space cannot be read, the download goes on and
    /// a full disk is reported when it happens.
    fn check_space(&self, req: &Request, size: u64) -> Result<(), DownloadError> {
        let dir = req
            .dest()
            .parent()
            .filter(|d| !d.as_os_str().is_empty())
            .unwrap_or(Path::new("."));
        let Ok(free) = (self.free_space)(dir) else {
            return Ok(());
        };
        let written = std::fs::metadata(req.part_path())
            .map_or(0, |m| m.len())
            .min(size);
        let needed = (size - written).saturating_add(self.net.config.disk_reserve);
        if free < needed {
            return Err(DownloadError::NotEnoughSpace { needed, free });
        }
        Ok(())
    }

    /// Runs [`Net::probe`] on its own thread, so a cancel while connecting does not wait for the host.
    fn probe(&self, req: &Request, ctl: &Arc<Ctl>) -> Result<Probe, DownloadError> {
        let (tx, rx) = std::sync::mpsc::channel();
        let (net, r, c) = (self.net.clone(), req.clone(), ctl.clone());
        spawn(ctl, move || {
            let _ = tx.send(contained(|| net.probe(&r, &c)));
        })?;
        loop {
            match rx.recv_timeout(TICK) {
                Ok(probe) => return probe,
                Err(RecvTimeoutError::Timeout) => {
                    if let Err(c) = ctl.cancel.check() {
                        ctl.stop();
                        return Err(DownloadError::Cancelled(c.0));
                    }
                }
                // The probe thread always answers (a panic included); without one, the download stopped.
                Err(RecvTimeoutError::Disconnected) => return Err(ctl.halt_error()),
            }
        }
    }

    fn ranged(
        &self,
        ctx: &JobContext,
        req: &Request,
        ctl: &Arc<Ctl>,
        size: u64,
        validator: Option<String>,
    ) -> Result<Downloaded, DownloadError> {
        self.check_size(req, size)?;
        let (part, resume_path) = (req.part_path(), req.resume_path());
        let pinned = match req.integrity() {
            Integrity::Sha256(d) => Some(d.to_hex()),
            Integrity::Unpinned => None,
        };
        let mut state = ResumeState::new(req.url(), size, pinned, validator.clone());
        let part_len = std::fs::metadata(&part).map_or(0, |m| m.len());
        let have = ResumeState::load(&resume_path)
            .filter(|s| s.matches(&state) && s.hashed <= part_len)
            .map_or(0, |s| s.hashed);
        if have == 0 {
            remove(&part);
            remove(&resume_path);
        }
        let file = OpenOptions::new()
            .create(true)
            .truncate(false)
            .read(true)
            .write(true)
            .open(&part)?;
        file.set_len(size)?;
        let mut hasher = HashingWriter::new(io::sink());
        if have > 0 {
            ctx.phase(Phase::bytes(
                phases::RESUME,
                "Checking the partial download",
                Some(have),
            ));
            hash_prefix(&file, have, &mut hasher, ctx)?;
        }
        state.hashed = have;
        state.save(&resume_path)?;
        ctx.phase(Phase::bytes(phases::DOWNLOAD, "Downloading", Some(size)));
        ctx.add(have);
        ctl.frontier.store(have, Ordering::Release);
        let connections = if size >= self.net.config.segment_threshold {
            self.net.config.connections
        } else {
            1
        };
        let seg = Arc::new(Segments::new(
            &self.net,
            req,
            ctl.clone(),
            ctx.clone(),
            file,
            size,
            validator,
            hasher,
            connections,
        ));
        let started = if have < size {
            seg.clone().start(connections)
        } else {
            seg.complete();
            Ok(())
        };
        let result = started.and_then(|()| {
            self.supervise(ctl, &mut || save_progress(&resume_path, &mut state, ctl))
        });
        let peak = seg.peak.load(Ordering::Acquire);
        match result {
            Ok(done) => {
                state.hashed = size;
                let out = self.finalize(ctx, req, done, have, peak, true);
                if out.as_ref().is_err_and(DownloadError::keeps_partial) {
                    let _ = state.save(&resume_path);
                }
                out
            }
            Err(e) => {
                ctl.stop();
                if e.keeps_partial() {
                    let _ = save_progress(&resume_path, &mut state, ctl);
                } else {
                    remove(&part);
                    remove(&resume_path);
                }
                Err(e)
            }
        }
    }

    fn streamed(
        &self,
        ctx: &JobContext,
        req: &Request,
        ctl: &Arc<Ctl>,
        first: Option<(Response, Permit)>,
        size: Option<u64>,
        validator: Option<String>,
    ) -> Result<Downloaded, DownloadError> {
        if let Some(n) = size {
            self.check_size(req, n)?;
        }
        let part = req.part_path();
        remove(&req.resume_path());
        let file = File::create(&part)?;
        ctx.phase(Phase::bytes(phases::DOWNLOAD, "Downloading", size));
        let stream = Stream {
            ctl: ctl.clone(),
            transport: self.net.transport.clone(),
            hosts: self.net.hosts.clone(),
            url: req.url().to_string(),
            host: req.host().to_string(),
            config: self.net.config.clone(),
            counter: ctx.clone(),
            size,
            limit: req.limit(self.net.config.max_bytes),
            validator,
        };
        let result = spawn(ctl, move || stream.run(file, first))
            .and_then(|()| self.supervise(ctl, &mut || Ok(())));
        match result {
            Ok(done) => {
                let out = self.finalize(ctx, req, done, 0, 1, false);
                if out.is_err() {
                    remove(&part);
                }
                out
            }
            Err(e) => {
                // Without byte ranges nothing can be resumed.
                remove(&part);
                Err(e)
            }
        }
    }

    /// Waits for the transfer while watching for a cancel, a failure and a stall; `tick` runs about every `save_every`.
    fn supervise(
        &self,
        ctl: &Ctl,
        tick: &mut dyn FnMut() -> io::Result<()>,
    ) -> Result<Finished, DownloadError> {
        let mut saved = Instant::now();
        loop {
            if let Some(done) = lock(&ctl.outcome).take() {
                return Ok(done);
            }
            if let Some(e) = lock(&ctl.fatal).take() {
                ctl.stop();
                return Err(e);
            }
            if let Err(c) = ctl.cancel.check() {
                ctl.stop();
                return Err(DownloadError::Cancelled(c.0));
            }
            if ctl.idle() >= self.net.config.stall_timeout {
                ctl.stop();
                let host = lock(&ctl.host).clone();
                return Err(DownloadError::Stalled {
                    host,
                    secs: self.net.config.stall_timeout.as_secs(),
                });
            }
            if saved.elapsed() >= self.net.config.save_every {
                let _ = tick();
                saved = Instant::now();
            }
            ctl.wait(TICK);
        }
    }

    /// The verify phase: compares what was computed while writing (no read), then syncs and renames the file into place.
    fn finalize(
        &self,
        ctx: &JobContext,
        req: &Request,
        done: Finished,
        resumed_from: u64,
        connections: usize,
        ranges: bool,
    ) -> Result<Downloaded, DownloadError> {
        // The last count of the download phase goes out before the next phase starts on its own scale.
        ctx.reporter().flush();
        ctx.phase(Phase::items(phases::VERIFY, "Verifying", 1));
        ctx.cancel_token().check().map_err(cancelled)?;
        let sha256 = Sha256Digest(done.digest);
        let (part, resume_path) = (req.part_path(), req.resume_path());
        let refuse = |e: DownloadError| {
            remove(&part);
            remove(&resume_path);
            Err(e)
        };
        if let Some(expected) = req.pinned_size().filter(|&n| n != done.bytes) {
            return refuse(DownloadError::SizeMismatch {
                expected,
                actual: done.bytes,
            });
        }
        if let Integrity::Sha256(expected) = req.integrity()
            && expected != sha256
        {
            return refuse(DownloadError::ChecksumMismatch {
                expected: expected.to_hex(),
                actual: sha256.to_hex(),
            });
        }
        {
            let _saving = ctx.uncancellable("Saving the file");
            done.file.sync_all()?;
            drop(done.file);
            std::fs::rename(&part, req.dest())?;
            remove(&resume_path);
            sync_dir(req.dest());
        }
        ctx.add(1);
        Ok(Downloaded {
            path: req.dest().to_path_buf(),
            bytes: done.bytes,
            sha256,
            resumed_from,
            connections,
            ranges,
        })
    }
}

impl Net {
    fn permit(&self, host: &str, ctl: &Ctl) -> Result<Permit, DownloadError> {
        self.hosts
            .acquire(host, &|| ctl.halted(), &|| ctl.touch())
            .ok_or_else(|| ctl.halt_error())
    }

    /// The first request, `Range: bytes=0-0`: tells whether the host serves ranges, the size and the validator. A host that
    /// ignores the range answers 200 with the whole file, which is then streamed as it is.
    fn probe(&self, req: &Request, ctl: &Arc<Ctl>) -> Result<Probe, DownloadError> {
        let host = req.host();
        let mut retry = Retry::default();
        loop {
            let permit = self.permit(host, ctl)?;
            let get = Get {
                url: req.url(),
                range: Some(ByteRange {
                    start: 0,
                    end: Some(0),
                }),
                if_range: None,
                body_timeout: Some(self.config.range_timeout),
            };
            let resp = match self.transport.get(&get) {
                Ok(r) => r,
                Err(e) if e.is_transient() => {
                    drop(permit);
                    retry.failed(&self.config, ctl, host, &e.to_string())?;
                    continue;
                }
                Err(e) => return Err(network(host, e.to_string())),
            };
            if !is_identity(&resp.headers) {
                return Err(encoded(host, &resp));
            }
            let found = validator(&resp.headers);
            match resp.status {
                206 => {
                    return Ok(
                        match resp
                            .headers
                            .content_range
                            .as_deref()
                            .and_then(parse_content_range)
                        {
                            Some(ContentRange::Bytes {
                                start: 0,
                                total: Some(size),
                                ..
                            }) => Probe::Ranges {
                                size,
                                validator: found,
                            },
                            _ => Probe::Stream {
                                first: None,
                                size: None,
                                validator: found,
                            },
                        },
                    );
                }
                200 => {
                    let size = resp
                        .headers
                        .content_length
                        .as_deref()
                        .and_then(parse_length);
                    return Ok(Probe::Stream {
                        first: Some((resp, permit)),
                        size,
                        validator: found,
                    });
                }
                416 => match resp
                    .headers
                    .content_range
                    .as_deref()
                    .and_then(parse_content_range)
                {
                    Some(ContentRange::Unsatisfied { total: 0 }) => {
                        return Ok(Probe::Ranges {
                            size: 0,
                            validator: found,
                        });
                    }
                    _ => return Err(http(host, 416)),
                },
                429 | 503 => {
                    drop(permit);
                    retry.throttled(&self.config, &self.hosts, host, &resp)?;
                }
                s if is_transient_status(s) => {
                    drop(permit);
                    retry.failed(&self.config, ctl, host, &format!("HTTP {s}"))?;
                }
                s => return Err(http(host, s)),
            }
        }
    }
}

// Built once per download and moved straight on; boxing the answer would buy nothing.
#[allow(clippy::large_enum_variant)]
enum Probe {
    Ranges {
        size: u64,
        validator: Option<String>,
    },
    Stream {
        first: Option<(Response, Permit)>,
        size: Option<u64>,
        validator: Option<String>,
    },
}

/// The end of a transfer: the digest and length computed while writing, and the partial file.
struct Finished {
    digest: [u8; 32],
    bytes: u64,
    file: File,
}

/// What every thread of one download shares: stop, the first failure, the outcome, the hash frontier and the time of the last
/// byte (or deliberate wait), for the stall check.
struct Ctl {
    cancel: CancelToken,
    stop: AtomicBool,
    fatal: Mutex<Option<DownloadError>>,
    outcome: Mutex<Option<Finished>>,
    frontier: AtomicU64,
    host: Mutex<String>,
    t0: Instant,
    last: AtomicU64,
    signal: (Mutex<()>, Condvar),
}

impl Ctl {
    fn new(cancel: CancelToken) -> Self {
        Ctl {
            cancel,
            stop: AtomicBool::new(false),
            fatal: Mutex::new(None),
            outcome: Mutex::new(None),
            frontier: AtomicU64::new(0),
            host: Mutex::new(String::new()),
            t0: Instant::now(),
            last: AtomicU64::new(0),
            signal: (Mutex::new(()), Condvar::new()),
        }
    }

    fn halted(&self) -> bool {
        self.stop.load(Ordering::Acquire) || self.cancel.is_cancelled()
    }

    fn halt_error(&self) -> DownloadError {
        DownloadError::Cancelled(
            self.cancel
                .reason()
                .unwrap_or(paddock_jobs::CancelReason::User),
        )
    }

    fn stop(&self) {
        self.stop.store(true, Ordering::Release);
    }

    fn now_ms(&self) -> u64 {
        self.t0.elapsed().as_millis() as u64
    }

    /// A byte arrived, or the download waits on purpose (back-off, host cap): not a stall.
    fn touch(&self) {
        self.last.store(self.now_ms(), Ordering::Release);
    }

    fn idle(&self) -> Duration {
        Duration::from_millis(
            self.now_ms()
                .saturating_sub(self.last.load(Ordering::Acquire)),
        )
    }

    fn fail(&self, e: DownloadError) {
        lock(&self.fatal).get_or_insert(e);
        self.stop();
        self.signal.1.notify_all();
    }

    fn finish(&self, done: Finished) {
        *lock(&self.outcome) = Some(done);
        self.signal.1.notify_all();
    }

    fn wait(&self, d: Duration) {
        let g = lock(&self.signal.0);
        let _ = self.signal.1.wait_timeout(g, d);
    }

    /// Sleeps `d` in short slices, counting as a deliberate wait. False when the download stopped meanwhile.
    fn nap(&self, d: Duration) -> bool {
        let until = Instant::now() + d;
        loop {
            if self.halted() {
                return false;
            }
            self.touch();
            let left = until.saturating_duration_since(Instant::now());
            if left.is_zero() {
                return true;
            }
            std::thread::sleep(left.min(TICK));
        }
    }
}

/// Counts failures and throttles of one request loop.
#[derive(Default)]
struct Retry {
    failures: u32,
    throttles: u32,
}

impl Retry {
    fn failed(
        &mut self,
        config: &Config,
        ctl: &Ctl,
        host: &str,
        detail: &str,
    ) -> Result<(), DownloadError> {
        self.failures += 1;
        if self.failures >= config.attempts {
            return Err(network(host, detail.to_string()));
        }
        if ctl.nap(config.backoff_for(self.failures)) {
            Ok(())
        } else {
            Err(ctl.halt_error())
        }
    }

    fn throttled(
        &mut self,
        config: &Config,
        hosts: &HostCaps,
        host: &str,
        resp: &Response,
    ) -> Result<(), DownloadError> {
        self.throttles += 1;
        if self.throttles > config.max_throttles {
            return Err(DownloadError::Throttled {
                host: host.to_string(),
            });
        }
        hosts.back_off(host, Instant::now() + throttle_wait(config, resp));
        Ok(())
    }
}

fn throttle_wait(config: &Config, resp: &Response) -> Duration {
    config.throttle_wait(
        resp.headers
            .retry_after
            .as_deref()
            .and_then(|v| parse_retry_after(v, SystemTime::now())),
    )
}

fn is_transient_status(s: u16) -> bool {
    s == 408 || (500..=599).contains(&s)
}

fn http(host: &str, status: u16) -> DownloadError {
    DownloadError::Http {
        host: host.to_string(),
        status,
    }
}

fn network(host: &str, detail: String) -> DownloadError {
    DownloadError::Network {
        host: host.to_string(),
        detail,
    }
}

fn encoded(host: &str, resp: &Response) -> DownloadError {
    DownloadError::Protocol {
        host: host.to_string(),
        detail: format!(
            "the host compresses the file (Content-Encoding: {})",
            resp.headers.content_encoding.as_deref().unwrap_or("")
        ),
    }
}

fn cancelled(c: paddock_jobs::Cancelled) -> DownloadError {
    DownloadError::Cancelled(c.0)
}

fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|p| p.into_inner())
}

fn remove(p: &Path) {
    let _ = std::fs::remove_file(p);
}

fn save_progress(path: &Path, state: &mut ResumeState, ctl: &Ctl) -> io::Result<()> {
    state.hashed = ctl.frontier.load(Ordering::Acquire);
    state.save(path)
}

/// Makes the rename durable (Unix; Windows has no directory handles for this).
fn sync_dir(dest: &Path) {
    #[cfg(unix)]
    if let Some(dir) = dest.parent().filter(|d| !d.as_os_str().is_empty())
        && let Ok(d) = File::open(dir)
    {
        let _ = d.sync_all();
    }
    #[cfg(not(unix))]
    let _ = dest;
}

/// Starts a transfer thread. A panic in it (a transport bug) is contained and ends the download with an error.
fn spawn(ctl: &Arc<Ctl>, f: impl FnOnce() + Send + 'static) -> Result<(), DownloadError> {
    let ctl = ctl.clone();
    std::thread::Builder::new()
        .name("paddock-download".into())
        .spawn(move || {
            if let Err(e) = contained(|| {
                f();
                Ok(())
            }) {
                ctl.fail(e);
            }
        })
        .map(|_| ())
        .map_err(DownloadError::Io)
}

/// Runs `f`, turning a panic into an error.
fn contained<T>(f: impl FnOnce() -> Result<T, DownloadError>) -> Result<T, DownloadError> {
    std::panic::catch_unwind(std::panic::AssertUnwindSafe(f)).unwrap_or_else(|p| {
        Err(io::Error::other(format!("internal error: {}", panic_text(&*p))).into())
    })
}

fn panic_text(p: &(dyn std::any::Any + Send)) -> &str {
    p.downcast_ref::<&str>()
        .copied()
        .or_else(|| p.downcast_ref::<String>().map(String::as_str))
        .unwrap_or("unknown panic")
}

/// Hashes the kept prefix of a resumed file (the bytes of an earlier run; a fresh download is never read back).
fn hash_prefix(
    file: &File,
    len: u64,
    hasher: &mut HashingWriter<'static, io::Sink>,
    ctx: &JobContext,
) -> Result<(), DownloadError> {
    let mut buf = vec![0u8; RESUME_BUF];
    let mut at = 0;
    while at < len {
        ctx.cancel_token().check().map_err(cancelled)?;
        let n = (len - at).min(buf.len() as u64) as usize;
        read_exact_at(file, &mut buf[..n], at)?;
        hasher.write_all(&buf[..n])?;
        at += n as u64;
        ctx.add(n as u64);
    }
    ctx.reporter().flush();
    Ok(())
}

#[cfg(unix)]
fn write_all_at(f: &File, buf: &[u8], off: u64) -> io::Result<()> {
    std::os::unix::fs::FileExt::write_all_at(f, buf, off)
}

#[cfg(unix)]
fn read_exact_at(f: &File, buf: &mut [u8], off: u64) -> io::Result<()> {
    std::os::unix::fs::FileExt::read_exact_at(f, buf, off)
}

#[cfg(windows)]
fn write_all_at(f: &File, buf: &[u8], off: u64) -> io::Result<()> {
    use std::os::windows::fs::FileExt;
    let mut done = 0;
    while done < buf.len() {
        let n = f.seek_write(&buf[done..], off + done as u64)?;
        if n == 0 {
            return Err(io::ErrorKind::WriteZero.into());
        }
        done += n;
    }
    Ok(())
}

#[cfg(windows)]
fn read_exact_at(f: &File, buf: &mut [u8], off: u64) -> io::Result<()> {
    use std::os::windows::fs::FileExt;
    let mut done = 0;
    while done < buf.len() {
        let n = f.seek_read(&mut buf[done..], off + done as u64)?;
        if n == 0 {
            return Err(io::ErrorKind::UnexpectedEof.into());
        }
        done += n;
    }
    Ok(())
}

// ---- segmented transfer -------------------------------------------------------------------------------------------------

/// A range being downloaded: `pos..end`. `end` shrinks when an idle connection takes over the far half; both change under the
/// lock, so a range is never written twice.
struct Task {
    span: Mutex<(u64, u64)>,
}

impl Task {
    fn new(start: u64, end: u64) -> Arc<Task> {
        Arc::new(Task {
            span: Mutex::new((start, end)),
        })
    }

    fn span(&self) -> (u64, u64) {
        *lock(&self.span)
    }
}

#[derive(Default)]
struct Board {
    /// Ranges not started, by start.
    pending: BTreeMap<u64, u64>,
    running: Vec<Arc<Task>>,
}

enum Next {
    Task(Arc<Task>),
    Wait,
    Done,
}

/// The in-order hash: bytes at the frontier go straight in; ranges ahead wait in memory (bounded by the window) until it
/// reaches them. The file is never read back.
struct Hash {
    writer: Option<HashingWriter<'static, io::Sink>>,
    ahead: BTreeMap<u64, Vec<u8>>,
}

enum Attempt {
    Complete,
    Halted,
    Throttled(Duration),
    Transient(String),
    Fatal(DownloadError),
}

struct Segments {
    ctl: Arc<Ctl>,
    transport: Arc<dyn Transport>,
    hosts: Arc<HostCaps>,
    url: String,
    host: String,
    validator: Option<String>,
    config: Config,
    counter: JobContext,
    file: Mutex<Option<File>>,
    size: u64,
    window: u64,
    board: Mutex<Board>,
    moved: Condvar,
    hash: Mutex<Hash>,
    /// Connections with an id at or above this stand down (lowered by 429/503).
    limit: AtomicUsize,
    active: AtomicUsize,
    peak: AtomicUsize,
    alive: AtomicUsize,
    throttles: AtomicU32,
}

impl Segments {
    #[allow(clippy::too_many_arguments)]
    fn new(
        d: &Net,
        req: &Request,
        ctl: Arc<Ctl>,
        counter: JobContext,
        file: File,
        size: u64,
        validator: Option<String>,
        hasher: HashingWriter<'static, io::Sink>,
        connections: usize,
    ) -> Self {
        let from = ctl.frontier.load(Ordering::Acquire);
        let block = d.config.block_size(size - from, connections);
        let mut board = Board::default();
        let mut at = from;
        while at < size {
            let next = (at + block).min(size);
            board.pending.insert(at, next);
            at = next;
        }
        *lock(&ctl.host) = req.host().to_string();
        Segments {
            ctl,
            transport: d.transport.clone(),
            hosts: d.hosts.clone(),
            url: req.url().to_string(),
            host: req.host().to_string(),
            validator,
            config: d.config.clone(),
            counter,
            file: Mutex::new(Some(file)),
            size,
            window: d.config.max_buffer.max(block),
            board: Mutex::new(board),
            moved: Condvar::new(),
            hash: Mutex::new(Hash {
                writer: Some(hasher),
                ahead: BTreeMap::new(),
            }),
            limit: AtomicUsize::new(connections),
            active: AtomicUsize::new(0),
            peak: AtomicUsize::new(0),
            alive: AtomicUsize::new(0),
            throttles: AtomicU32::new(0),
        }
    }

    fn start(self: Arc<Self>, connections: usize) -> Result<(), DownloadError> {
        self.ctl.touch();
        self.alive.store(connections, Ordering::Release);
        for id in 0..connections {
            let me = self.clone();
            spawn(&self.ctl, move || me.worker(id))?;
        }
        Ok(())
    }

    fn worker(&self, id: usize) {
        while !self.ctl.halted() {
            if id >= self.limit.load(Ordering::Acquire) {
                let b = lock(&self.board);
                if b.pending.is_empty() && b.running.is_empty() {
                    break;
                }
                let _ = self.moved.wait_timeout(b, TICK * 2);
                continue;
            }
            match self.next_task() {
                Next::Task(t) => self.run_task(id, &t),
                Next::Wait => {
                    let b = lock(&self.board);
                    let _ = self.moved.wait_timeout(b, TICK);
                }
                Next::Done => break,
            }
        }
        if self.alive.fetch_sub(1, Ordering::AcqRel) == 1 && !self.ctl.halted() {
            self.complete();
        }
    }

    /// The last connection ended: the download is complete when the hash reached the end.
    fn complete(&self) {
        let complete = self.ctl.frontier.load(Ordering::Acquire) >= self.size;
        let digest = lock(&self.hash)
            .writer
            .take()
            .filter(|_| complete)
            .map(HashingWriter::finish);
        let file = lock(&self.file).take();
        match (digest, file) {
            (Some(Ok((_, digest, _))), Some(file)) => self.ctl.finish(Finished {
                digest,
                bytes: self.size,
                file,
            }),
            _ => self.ctl.fail(network(&self.host, ENDED_EARLY.into())),
        }
    }

    fn next_task(&self) -> Next {
        let frontier = self.ctl.frontier.load(Ordering::Acquire);
        let mut b = lock(&self.board);
        if let Some((&s, &e)) = b.pending.first_key_value()
            && e <= frontier.saturating_add(self.window)
        {
            b.pending.remove(&s);
            let t = Task::new(s, e);
            b.running.push(t.clone());
            return Next::Task(t);
        }
        // Nothing to start (or the next range is beyond the window): take over the far half of the lowest range that is worth
        // splitting, so neither the tail nor the hash frontier waits on one slow connection.
        let split = 2 * self.config.min_block;
        let victim = b
            .running
            .iter()
            .filter_map(|t| {
                let (p, e) = t.span();
                (e.saturating_sub(p) >= split).then_some((p, t.clone()))
            })
            .min_by_key(|(p, _)| *p);
        if let Some((_, v)) = victim {
            // Under the task's lock: its connection may have moved on since, but only by claiming bytes it then writes, so
            // the split point is past them and no byte is written twice.
            let mut span = lock(&v.span);
            let (p, e) = *span;
            let mid = p + e.saturating_sub(p) / 2;
            span.1 = mid;
            drop(span);
            let t = Task::new(mid, e.max(mid));
            b.running.push(t.clone());
            return Next::Task(t);
        }
        if b.pending.is_empty() && b.running.is_empty() {
            Next::Done
        } else {
            Next::Wait
        }
    }

    fn finish_task(&self, t: &Arc<Task>) {
        lock(&self.board).running.retain(|r| !Arc::ptr_eq(r, t));
        self.moved.notify_all();
    }

    /// Gives the rest of `t` back (a connection that must stand down).
    fn requeue(&self, t: &Arc<Task>) {
        let mut b = lock(&self.board);
        b.running.retain(|r| !Arc::ptr_eq(r, t));
        let (p, e) = t.span();
        if p < e {
            b.pending.insert(p, e);
        }
        drop(b);
        self.moved.notify_all();
    }

    fn lower_limit(&self) {
        // One step down per throttled answer; when another connection stepped down at the same moment, that step counts.
        let l = self.limit.load(Ordering::Acquire);
        if l > 1 {
            let _ = self
                .limit
                .compare_exchange(l, l - 1, Ordering::AcqRel, Ordering::Acquire);
        }
    }

    fn run_task(&self, id: usize, t: &Arc<Task>) {
        let mut failures = 0u32;
        loop {
            let (pos, end) = t.span();
            let halt = || self.ctl.halted();
            let permit =
                (pos < end).then(|| self.hosts.acquire(&self.host, &halt, &|| self.ctl.touch()));
            let Some(permit) = permit.flatten() else {
                return self.finish_task(t);
            };
            let n = self.active.fetch_add(1, Ordering::AcqRel) + 1;
            self.peak.fetch_max(n, Ordering::AcqRel);
            let get = Get {
                url: &self.url,
                range: Some(ByteRange {
                    start: pos,
                    end: Some(end - 1),
                }),
                if_range: self.validator.as_deref(),
                body_timeout: Some(self.config.range_timeout),
            };
            let attempt = match self.transport.get(&get) {
                Ok(resp) => self.receive(t, pos, resp).unwrap_or_else(Attempt::Fatal),
                Err(e) if e.is_transient() => Attempt::Transient(e.to_string()),
                Err(e) => Attempt::Fatal(network(&self.host, e.to_string())),
            };
            self.active.fetch_sub(1, Ordering::AcqRel);
            drop(permit);
            match attempt {
                Attempt::Complete => return self.finish_task(t),
                Attempt::Halted => return,
                Attempt::Fatal(e) => return self.ctl.fail(e),
                Attempt::Throttled(wait) => {
                    self.hosts.back_off(&self.host, Instant::now() + wait);
                    if self.limit.load(Ordering::Acquire) <= 1
                        && self.throttles.fetch_add(1, Ordering::AcqRel) + 1
                            > self.config.max_throttles
                    {
                        return self.ctl.fail(DownloadError::Throttled {
                            host: self.host.clone(),
                        });
                    }
                    self.lower_limit();
                    if id >= self.limit.load(Ordering::Acquire) {
                        return self.requeue(t);
                    }
                }
                Attempt::Transient(detail) => {
                    if t.span().0 > pos {
                        failures = 0;
                    }
                    failures += 1;
                    if failures >= self.config.attempts {
                        return self.ctl.fail(network(&self.host, detail));
                    }
                    if !self.ctl.nap(self.config.backoff_for(failures)) {
                        return;
                    }
                }
            }
        }
    }

    /// Reads one answer into the task's range. `Err` is a failure no retry will fix.
    fn receive(
        &self,
        t: &Arc<Task>,
        pos: u64,
        mut resp: Response,
    ) -> Result<Attempt, DownloadError> {
        match resp.status {
            206 => {}
            429 | 503 => return Ok(Attempt::Throttled(throttle_wait(&self.config, &resp))),
            200 | 416 => {
                return Err(DownloadError::Changed {
                    host: self.host.clone(),
                    detail: format!("a range request was answered HTTP {}", resp.status),
                });
            }
            s if is_transient_status(s) => return Ok(Attempt::Transient(format!("HTTP {s}"))),
            s => return Err(http(&self.host, s)),
        }
        if !is_identity(&resp.headers) {
            return Err(encoded(&self.host, &resp));
        }
        let range = resp
            .headers
            .content_range
            .as_deref()
            .and_then(parse_content_range);
        if !matches!(range, Some(ContentRange::Bytes { start, total, .. }) if start == pos && total.is_none_or(|n| n == self.size))
        {
            return Err(DownloadError::Protocol {
                host: self.host.clone(),
                detail: format!(
                    "asked for bytes from {pos}, the host answered {:?}",
                    resp.headers.content_range
                ),
            });
        }
        let mut buf = vec![0u8; BUF];
        loop {
            if self.ctl.halted() {
                return Ok(Attempt::Halted);
            }
            let n = match resp.body.read(&mut buf) {
                Ok(0) => break,
                Ok(n) => n,
                Err(e) if e.kind() == io::ErrorKind::Interrupted => continue,
                Err(e) => return Ok(Attempt::Transient(e.to_string())),
            };
            let mut off = 0;
            while off < n {
                // Claim the next bytes of the range (it may have shrunk: another connection took over its far half).
                let (start, take) = {
                    let mut span = lock(&t.span);
                    let take = span.1.saturating_sub(span.0).min((n - off) as u64);
                    let start = span.0;
                    span.0 += take;
                    (start, take as usize)
                };
                if take == 0 {
                    return Ok(Attempt::Complete);
                }
                self.store(start, &buf[off..off + take])?;
                self.counter.add(take as u64);
                self.ctl.touch();
                off += take;
            }
        }
        let (p, e) = t.span();
        Ok(if p >= e {
            Attempt::Complete
        } else {
            Attempt::Transient("the connection closed early".into())
        })
    }

    /// Writes `data` at `off`, then hands it to the in-order hash.
    fn store(&self, off: u64, data: &[u8]) -> io::Result<()> {
        {
            let f = lock(&self.file);
            let file = f
                .as_ref()
                .ok_or_else(|| io::Error::other("the download already ended"))?;
            write_all_at(file, data, off)?;
        }
        let mut h = lock(&self.hash);
        let frontier = self.ctl.frontier.load(Ordering::Acquire);
        if off != frontier {
            h.ahead.insert(off, data.to_vec());
            return Ok(());
        }
        let Hash { writer, ahead } = &mut *h;
        let w = writer
            .as_mut()
            .ok_or_else(|| io::Error::other("the download already ended"))?;
        w.write_all(data)?;
        let mut at = off + data.len() as u64;
        while let Some(chunk) = ahead.remove(&at) {
            w.write_all(&chunk)?;
            at += chunk.len() as u64;
        }
        self.ctl.frontier.store(at, Ordering::Release);
        drop(h);
        self.moved.notify_all();
        Ok(())
    }
}

// ---- one stream (no byte ranges) ----------------------------------------------------------------------------------------

/// A host without byte ranges: one GET of the whole file. A dropped connection is retried from the start and the bytes already
/// written are skipped (not counted again), as long as the host still describes the same file.
struct Stream {
    ctl: Arc<Ctl>,
    transport: Arc<dyn Transport>,
    hosts: Arc<HostCaps>,
    url: String,
    host: String,
    config: Config,
    counter: JobContext,
    size: Option<u64>,
    limit: u64,
    validator: Option<String>,
}

impl Stream {
    fn run(self, file: File, first: Option<(Response, Permit)>) {
        *lock(&self.ctl.host) = self.host.clone();
        self.ctl.touch();
        match self.transfer(file, first) {
            Ok(Some(done)) => self.ctl.finish(done),
            Ok(None) => {}
            Err(e) => self.ctl.fail(e),
        }
    }

    /// The whole file, `None` when the download stopped.
    fn transfer(
        &self,
        file: File,
        mut first: Option<(Response, Permit)>,
    ) -> Result<Option<Finished>, DownloadError> {
        let counter = self.counter.clone();
        let mut out = HashingWriter::new(file).with_counter(&counter);
        let mut retry = Retry::default();
        while !self.ctl.halted() {
            let Some((resp, permit)) = first
                .take()
                .map_or_else(|| self.request(&mut retry), |x| Ok(Some(x)))?
            else {
                continue;
            };
            let before = out.bytes();
            let result = self.receive(resp, &mut out);
            drop(permit);
            match result {
                Ok(true) => {
                    let bytes = out.bytes();
                    let (file, digest, _) = out.finish()?;
                    return Ok(Some(Finished {
                        digest,
                        bytes,
                        file,
                    }));
                }
                Ok(false) => {}
                Err(StreamError::Transient(detail)) => {
                    if out.bytes() > before {
                        retry.failures = 0;
                    }
                    retry.failed(&self.config, &self.ctl, &self.host, &detail)?;
                }
                Err(StreamError::Fatal(e)) => return Err(e),
            }
        }
        Ok(None)
    }

    /// A new GET of the whole file; `Ok(None)` after a throttle or a transient failure (already waited for).
    fn request(&self, retry: &mut Retry) -> Result<Option<(Response, Permit)>, DownloadError> {
        let permit = self
            .hosts
            .acquire(&self.host, &|| self.ctl.halted(), &|| self.ctl.touch())
            .ok_or_else(|| self.ctl.halt_error())?;
        let get = Get {
            url: &self.url,
            range: None,
            if_range: None,
            body_timeout: None,
        };
        match self.transport.get(&get) {
            Ok(r) if r.status == 200 => Ok(Some((r, permit))),
            Ok(r) if r.status == 429 || r.status == 503 => {
                drop(permit);
                retry
                    .throttled(&self.config, &self.hosts, &self.host, &r)
                    .map(|()| None)
            }
            Ok(r) if is_transient_status(r.status) => {
                drop(permit);
                retry
                    .failed(
                        &self.config,
                        &self.ctl,
                        &self.host,
                        &format!("HTTP {}", r.status),
                    )
                    .map(|()| None)
            }
            Ok(r) => Err(http(&self.host, r.status)),
            Err(e) if e.is_transient() => {
                drop(permit);
                retry
                    .failed(&self.config, &self.ctl, &self.host, &e.to_string())
                    .map(|()| None)
            }
            Err(e) => Err(network(&self.host, e.to_string())),
        }
    }

    /// Reads one answer to the end. `Ok(true)` when the file is complete, `Ok(false)` when the download stopped.
    fn receive(
        &self,
        mut resp: Response,
        out: &mut HashingWriter<'_, File>,
    ) -> Result<bool, StreamError> {
        if !is_identity(&resp.headers) {
            return Err(StreamError::Fatal(encoded(&self.host, &resp)));
        }
        let length = resp
            .headers
            .content_length
            .as_deref()
            .and_then(parse_length);
        if validator(&resp.headers) != self.validator
            || (self.size.is_some() && length.is_some() && length != self.size)
        {
            return Err(StreamError::Fatal(DownloadError::Changed {
                host: self.host.clone(),
                detail: "the host describes another file after reconnecting".into(),
            }));
        }
        let mut skip = out.bytes();
        let mut buf = vec![0u8; BUF];
        loop {
            if self.ctl.halted() {
                return Ok(false);
            }
            let n = match resp.body.read(&mut buf) {
                Ok(0) => break,
                Ok(n) => n,
                Err(e) if e.kind() == io::ErrorKind::Interrupted => continue,
                Err(e) => return Err(StreamError::Transient(e.to_string())),
            };
            let drop_n = skip.min(n as u64) as usize;
            skip -= drop_n as u64;
            let fresh = &buf[drop_n..n];
            self.ctl.touch();
            if fresh.is_empty() {
                continue;
            }
            let total = out.bytes() + fresh.len() as u64;
            if total > self.limit {
                return Err(StreamError::Fatal(DownloadError::TooLarge {
                    size: total,
                    limit: self.limit,
                }));
            }
            if let Some(size) = self.size.filter(|&s| total > s) {
                return Err(StreamError::Fatal(DownloadError::SizeMismatch {
                    expected: size,
                    actual: total,
                }));
            }
            out.write_all(fresh)
                .map_err(|e| StreamError::Fatal(e.into()))?;
        }
        match self.size {
            Some(size) if out.bytes() < size || skip > 0 => {
                Err(StreamError::Transient("the connection closed early".into()))
            }
            None if skip > 0 => Err(StreamError::Transient("the connection closed early".into())),
            _ => Ok(true),
        }
    }
}

enum StreamError {
    Transient(String),
    Fatal(DownloadError),
}
