//! Full-text index benchmark (docs/research/2026-10-28-search-performance.md): builds an index of synthetic log
//! events through [`Fts`] (the store's schema, tokenizer, writer budget and commit rhythm) and times typical searches.
//!
//! ```sh
//! DEBRIEF_FTS_BENCH_DOCS=2000000 DEBRIEF_FTS_BENCH_DIR=/some/work/disk \
//!   cargo test --release -p debrief-store-duck --lib fts::bench -- --ignored --nocapture
//! ```
//!
//! - `DEBRIEF_FTS_BENCH_HEAP_MB` / `DEBRIEF_FTS_BENCH_THREADS`: the writer budget (default: [`WriterBudget::default`]).
//! - `DEBRIEF_FTS_BENCH_KEEP=1`: keep the index and only query it on the next run (same docs count).
//!
//! The events are generated (no user data, ER-14): seeded templates of the shapes logs have (camelCase services,
//! GUIDs, hex, paths, durations, IPs, stack traces), so the term dictionary grows like a real one. Memory is the
//! physical footprint on macOS (rule 26), sampled every 20 ms.

use super::*;
use std::time::Instant;

// The store worker's allocator (`debrief-store-worker`), so the numbers are the shipped ones.
#[global_allocator]
static GLOBAL: debrief_alloc::MiMalloc = debrief_alloc::MiMalloc;

/// SplitMix64: small, seeded, deterministic.
pub(crate) struct Rng(u64);

impl Rng {
    pub(crate) fn new(seed: u64) -> Rng {
        Rng(seed)
    }
    pub(crate) fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9e37_79b9_7f4a_7c15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
        z ^ (z >> 31)
    }
    pub(crate) fn below(&mut self, n: usize) -> usize {
        (self.next() % n as u64) as usize
    }
    fn pick<'a>(&mut self, xs: &[&'a str]) -> &'a str {
        xs[self.below(xs.len())]
    }
}

const SERVICES: &[&str] = &[
    "OrderService",
    "PaymentGateway",
    "HikariPool",
    "InventoryClient",
    "AuthTokenValidator",
    "SessionCache",
    "HttpClientFactory",
    "BlobUploader",
    "ReportScheduler",
    "MailSender",
    "SearchIndexer",
    "UserRepository",
    "CircuitBreaker",
    "KafkaConsumer",
    "GrpcChannel",
    "LdapAuthenticator",
    "ThumbnailWorker",
    "BillingService",
    "ShipmentTracker",
    "ConfigWatcher",
    "MetricsExporter",
    "DnsResolver",
    "TlsHandshake",
    "WebSocketHub",
    "FeatureFlagClient",
    "RateLimiter",
    "AuditLogger",
    "PdfRenderer",
    "CacheWarmer",
    "QueueDispatcher",
];
const VERBS: &[&str] = &[
    "started",
    "stopped",
    "completed",
    "failed",
    "retrying",
    "timed out",
    "rejected",
    "accepted",
    "connected",
    "disconnected",
    "opened",
    "closed",
    "flushed",
    "evicted",
    "throttled",
    "scheduled",
    "cancelled",
    "received",
    "sent",
    "skipped",
];
const NOUNS: &[&str] = &[
    "request",
    "connection",
    "transaction",
    "batch",
    "message",
    "session",
    "job",
    "upload",
    "query",
    "lease",
    "token",
    "handshake",
    "checkpoint",
    "snapshot",
    "subscription",
    "partition",
    "worker",
    "pool",
    "channel",
    "file",
];
const REASONS: &[&str] = &[
    "Connection reset by peer",
    "No space left on device",
    "operation timed out",
    "pool exhausted",
    "token expired",
    "access denied",
    "certificate has expired",
    "host unreachable",
    "deadlock detected",
    "too many open files",
    "invalid argument",
    "broken pipe",
    "service unavailable",
    "quota exceeded",
    "checksum mismatch",
];
const USERS: usize = 5_000;
const HOSTS: &[&str] = &[
    "web-01",
    "web-02",
    "api-eu-1",
    "api-us-2",
    "db-primary",
    "db-replica-3",
    "batch-7",
    "edge-cdn-4",
];

/// A template: its literal words and the kinds of the values it holds.
#[derive(Clone)]
struct Template {
    text: String,
    structured: bool,
}

/// About 600 templates: service × verb × noun with value slots, so message words repeat like real logs.
fn templates(rng: &mut Rng) -> Vec<Template> {
    let slots = [
        "{Guid}",
        "{Ms} ms",
        "{Path}",
        "{Ip}",
        "{User}",
        "0x{Hex}",
        "{Count}",
        "{Url}",
        "status {Status}",
        "{Host}",
    ];
    (0..600)
        .map(|_| {
            let mut t = format!(
                "{}: {} {} {}",
                rng.pick(SERVICES),
                rng.pick(NOUNS),
                slots[rng.below(slots.len())],
                rng.pick(VERBS)
            );
            for _ in 0..rng.below(3) {
                t.push_str(&format!(" {} {}", rng.pick(NOUNS), slots[rng.below(slots.len())]));
            }
            if rng.below(4) == 0 {
                t.push_str(&format!(": {}", rng.pick(REASONS)));
            }
            Template {
                text: t,
                structured: rng.below(10) < 6,
            }
        })
        .collect()
}

/// One generated event.
pub(crate) struct Event {
    pub id: u64,
    pub ts: i64,
    pub source: u32,
    pub message: String,
    pub exception: Option<String>,
    pub template: Option<String>,
}

/// Deterministic synthetic log events (same seed, same events).
pub(crate) struct Events {
    rng: Rng,
    templates: Vec<Template>,
    /// Zipf-like weights: a few templates are most of the rows, as in real logs.
    cumulative: Vec<u64>,
    next_id: u64,
    ts: i64,
}

impl Events {
    pub(crate) fn new(seed: u64) -> Events {
        let mut rng = Rng::new(seed);
        let templates = templates(&mut rng);
        let mut acc = 0;
        let cumulative = (0..templates.len())
            .map(|i| {
                acc += 1_000_000 / (i as u64 + 1);
                acc
            })
            .collect();
        Events {
            rng,
            templates,
            cumulative,
            next_id: 0,
            ts: 1_790_000_000_000,
        }
    }

    fn value(&mut self, slot: &str, out: &mut String) {
        use std::fmt::Write;
        let r = &mut self.rng;
        let _ = match slot {
            "Guid" => {
                let (a, b) = (r.next(), r.next());
                write!(
                    out,
                    "{:08x}-{:04x}-{:04x}-{:04x}-{:012x}",
                    a >> 32,
                    (a >> 16) & 0xffff,
                    a & 0xffff,
                    b >> 48,
                    b & 0xffff_ffff_ffff
                )
            }
            "Ms" => write!(out, "{}.{}", r.below(5000), r.below(10)),
            "Path" => write!(
                out,
                "/var/lib/{}/data/{}-{}.log",
                ["app", "orders", "billing", "cache"][r.below(4)],
                NOUNS[r.below(NOUNS.len())],
                r.below(100_000)
            ),
            "Ip" => write!(out, "10.{}.{}.{}", r.below(256), r.below(256), r.below(256)),
            "User" => write!(out, "user{}", r.below(USERS)),
            "Hex" => write!(out, "{:08X}", r.next() as u32),
            "Count" => write!(out, "{}", r.below(1_000_000)),
            "Url" => write!(
                out,
                "https://api.example.test/v2/{}/{}",
                NOUNS[r.below(NOUNS.len())],
                r.below(1_000_000)
            ),
            "Status" => write!(
                out,
                "{}",
                [200, 201, 204, 301, 400, 401, 403, 404, 409, 429, 500, 502, 503][r.below(13)]
            ),
            _ => write!(out, "{}", HOSTS[r.below(HOSTS.len())]),
        };
    }

    fn render(&mut self, template: &str) -> String {
        let mut out = String::with_capacity(template.len() + 48);
        let mut rest = template;
        while let Some(open) = rest.find('{') {
            out.push_str(&rest[..open]);
            let close = rest[open..].find('}').map_or(rest.len(), |c| open + c);
            let slot = rest[open + 1..close].to_string();
            self.value(&slot, &mut out);
            rest = &rest[(close + 1).min(rest.len())..];
        }
        out.push_str(rest);
        out
    }

    fn exception(&mut self) -> String {
        let svc = self.rng.pick(SERVICES);
        let mut x = format!(
            "System.{}Exception: {}\n",
            [
                "InvalidOperation",
                "Timeout",
                "IO",
                "Http.HttpRequest",
                "Net.Sockets.Socket",
                "ObjectDisposed"
            ][self.rng.below(6)],
            self.rng.pick(REASONS)
        );
        for _ in 0..6 + self.rng.below(14) {
            x.push_str(&format!(
                "   at Contoso.{svc}.{}{}(String id) in /src/{svc}/{}.cs:line {}\n",
                ["Handle", "Process", "Send", "Read", "Validate"][self.rng.below(5)],
                self.rng.pick(NOUNS).to_uppercase(),
                svc,
                self.rng.below(900)
            ));
        }
        x
    }
}

impl Iterator for Events {
    type Item = Event;
    fn next(&mut self) -> Option<Event> {
        let total = *self.cumulative.last()?;
        let at = self.rng.next() % total;
        let i = self.cumulative.partition_point(|&c| c <= at);
        let t = self.templates[i].clone();
        let message = self.render(&t.text);
        let exception = (self.rng.below(100) < 3).then(|| self.exception());
        self.ts += self.rng.below(40) as i64;
        let id = self.next_id;
        self.next_id += 1;
        Some(Event {
            id,
            ts: self.ts,
            source: (id / 250_000) as u32 % 8 + 1,
            message,
            exception,
            template: t.structured.then(|| t.text.clone()),
        })
    }
}

/// The physical footprint of this process (rule 26), in bytes.
fn footprint() -> u64 {
    #[cfg(target_os = "macos")]
    {
        let mut info = std::mem::MaybeUninit::<libc::rusage_info_v2>::zeroed();
        // SAFETY: proc_pid_rusage fills a buffer of the size of the flavour asked for; read only on success.
        let rc = unsafe { libc::proc_pid_rusage(std::process::id() as i32, libc::RUSAGE_INFO_V2, info.as_mut_ptr().cast()) };
        if rc == 0 {
            // SAFETY: filled by the call.
            return unsafe { info.assume_init() }.ri_phys_footprint;
        }
    }
    memory_stats::memory_stats().map_or(0, |m| m.physical_mem as u64)
}

/// Peak footprint of a phase, sampled every 20 ms on a thread.
struct Peak {
    peak: Arc<AtomicU64>,
    stop: Arc<AtomicBool>,
}

impl Peak {
    fn start() -> Peak {
        let peak = Arc::new(AtomicU64::new(footprint()));
        let stop = Arc::new(AtomicBool::new(false));
        let (p, s) = (peak.clone(), stop.clone());
        std::thread::spawn(move || {
            while !s.load(Ordering::Relaxed) {
                p.fetch_max(footprint(), Ordering::Relaxed);
                std::thread::sleep(Duration::from_millis(20));
            }
        });
        Peak { peak, stop }
    }
    fn take_mb(&self) -> u64 {
        let now = footprint();
        self.peak.swap(now, Ordering::Relaxed).max(now) >> 20
    }
}

impl Drop for Peak {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
    }
}

fn env<T: std::str::FromStr>(name: &str) -> Option<T> {
    std::env::var(name).ok().and_then(|v| v.parse().ok())
}

fn dir_mb(p: &Path) -> f64 {
    fn size(p: &Path) -> u64 {
        std::fs::read_dir(p)
            .map(|rd| {
                rd.flatten()
                    .map(|e| match e.metadata() {
                        Ok(m) if m.is_dir() => size(&e.path()),
                        Ok(m) => m.len(),
                        Err(_) => 0,
                    })
                    .sum()
            })
            .unwrap_or(0)
    }
    size(p) as f64 / 1e6
}

fn percentile(sorted: &[f64], p: f64) -> f64 {
    sorted[((sorted.len() - 1) as f64 * p).round() as usize]
}

/// The searches timed: typed text as the search box sends it (the last word is a prefix unless a space ends it).
pub(crate) const QUERIES: &[&str] = &[
    "timeout ",
    "timeout",
    "connection refused ",
    "orderservice",
    "hikaripool exhausted",
    "user4217 ",
    "t",
    "pa",
    "conn",
    "a3f",
    "status 503 ",
    "nullreference ",
];

#[test]
#[ignore = "benchmark: run with --ignored and DEBRIEF_FTS_BENCH_DOCS"]
fn index_build_and_queries() {
    let docs: u64 = env("DEBRIEF_FTS_BENCH_DOCS").unwrap_or(200_000);
    let base: PathBuf = std::env::var("DEBRIEF_FTS_BENCH_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|_| std::env::temp_dir());
    let dir = base.join(format!("debrief-fts-bench-{docs}"));
    let keep = std::env::var_os("DEBRIEF_FTS_BENCH_KEEP").is_some();
    let default = WriterBudget::default();
    let budget = WriterBudget {
        heap: env::<usize>("DEBRIEF_FTS_BENCH_HEAP_MB").map_or(default.heap, |m| m << 20),
        threads: env("DEBRIEF_FTS_BENCH_THREADS").unwrap_or(default.threads),
    }
    .floored();
    let peak = Peak::start();
    let (load1, cores) = (load_average(), std::thread::available_parallelism().map_or(1, |n| n.get()));
    eprintln!(
        "fts bench: {docs} docs, writer {} MB x {} threads, {cores} cores, load {load1:.1}, footprint {} MB",
        budget.heap >> 20,
        budget.threads,
        footprint() >> 20
    );

    // The tokenizer alone (single thread), over the first 200k events.
    {
        let events: Vec<Event> = Events::new(7).take(200_000).collect();
        let bytes: usize = events
            .iter()
            .map(|e| e.message.len() + e.exception.as_ref().map_or(0, String::len) + e.template.as_ref().map_or(0, String::len))
            .sum();
        let mut tok = WordsTokenizer::default();
        let t = Instant::now();
        let mut n = 0usize;
        for e in &events {
            for text in [Some(&e.message), e.exception.as_ref(), e.template.as_ref()].into_iter().flatten() {
                let mut s = tok.token_stream(text);
                while s.advance() {
                    n += 1;
                }
            }
        }
        let s = t.elapsed().as_secs_f64();
        eprintln!(
            "tokenizer: {:.0} events/s, {:.1} MB/s, {:.1} words/event ({:.0} bytes/event)",
            events.len() as f64 / s,
            bytes as f64 / s / 1e6,
            n as f64 / events.len() as f64,
            bytes as f64 / events.len() as f64
        );
    }

    let marker = dir.join("bench-docs.txt");
    let reuse = keep && std::fs::read_to_string(&marker).ok().and_then(|s| s.trim().parse::<u64>().ok()) == Some(docs);
    if !reuse {
        let _ = std::fs::remove_dir_all(&dir);
        peak.take_mb();
        let (fts, _) = Fts::open_or_create(&dir, 1, budget).unwrap();
        let ts_field = fts.index.schema().get_field("ts").unwrap();
        let mut w = fts.writer().unwrap();
        let started = Instant::now();
        let (mut add_s, mut commit_s, mut bytes) = (0.0, 0.0, 0u64);
        let per_commit = (SEGMENTS_PER_STEP as u64) * crate::segments::SEGMENT_ROWS;
        let mut events = Events::new(42).take(docs as usize).peekable();
        let mut commits = 0;
        while events.peek().is_some() {
            let t = Instant::now();
            for e in events.by_ref().take(per_commit as usize) {
                bytes +=
                    (e.message.len() + e.exception.as_ref().map_or(0, String::len) + e.template.as_ref().map_or(0, String::len)) as u64;
                let mut d = doc!(
                    fts.f.id => e.id,
                    fts.f.source => e.source as u64,
                    fts.f.seg => e.id / crate::segments::SEGMENT_ROWS,
                    ts_field => e.ts,
                    fts.f.text => e.message,
                );
                if let Some(x) = e.exception {
                    d.add_text(fts.f.text, x);
                }
                if let Some(t) = e.template {
                    d.add_text(fts.f.text, t);
                }
                w.add_document(d).unwrap();
            }
            add_s += t.elapsed().as_secs_f64();
            let t = Instant::now();
            fts.commit(&mut w, 1).unwrap();
            commit_s += t.elapsed().as_secs_f64();
            commits += 1;
        }
        let build_peak = peak.take_mb();
        let segments_before = fts.segments();
        let t = Instant::now();
        w.wait_merging_threads().unwrap();
        let merge_s = t.elapsed().as_secs_f64();
        let merge_peak = peak.take_mb();
        let total = started.elapsed().as_secs_f64();
        fts.reader.reload().unwrap();
        eprintln!(
            "build: {docs} docs in {total:.1} s = {:.0} docs/s, {:.1} MB/s of text; add {add_s:.1} s, {commits} commits {commit_s:.1} s, \
             final merges {merge_s:.1} s; peak footprint {build_peak} MB (merges {merge_peak} MB); segments {segments_before} -> {}; \
             index {:.0} MB ({:.1} bytes/doc), load {:.1}",
            docs as f64 / total,
            bytes as f64 / total / 1e6,
            fts.segments(),
            dir_mb(&dir),
            dir_mb(&dir) * 1e6 / docs as f64,
            load_average()
        );
        drop(fts);
        std::fs::write(&marker, docs.to_string()).unwrap();
    }

    // Queries on the reopened index (OS file cache warm, process cold).
    peak.take_mb();
    let before = footprint() >> 20;
    let t = Instant::now();
    let (fts, recreated) = Fts::open_or_create(&dir, 1, budget).unwrap();
    assert!(!recreated);
    eprintln!(
        "open: {:.1} ms, {} docs in {} segments, footprint {before} -> {} MB",
        t.elapsed().as_secs_f64() * 1e3,
        fts.docs(),
        fts.segments(),
        footprint() >> 20
    );
    let runs = env("DEBRIEF_FTS_BENCH_RUNS").unwrap_or(30usize);
    eprintln!(
        "{:<24} {:>10} {:>9} {:>9} {:>9} | {:>10} {:>9} {:>9}",
        "query", "hits", "first ms", "p50 ms", "p95 ms", "all_ids", "p50 ms", "p95 ms"
    );
    for text in QUERIES {
        let q = WordQuery::parse(text);
        let t = Instant::now();
        let (hits, _) = fts.search(&q, 100).unwrap();
        let first = t.elapsed().as_secs_f64() * 1e3;
        let mut top = Vec::with_capacity(runs);
        for _ in 0..runs {
            let t = Instant::now();
            std::hint::black_box(fts.search(&q, 100).unwrap());
            top.push(t.elapsed().as_secs_f64() * 1e3);
        }
        top.sort_by(f64::total_cmp);
        let mut all = Vec::with_capacity(runs);
        let mut n = 0;
        for _ in 0..runs.min(10) {
            let t = Instant::now();
            n = fts.all_ids(&q, MAX_FILTER_HITS).unwrap().map_or(usize::MAX, |v| v.len());
            all.push(t.elapsed().as_secs_f64() * 1e3);
        }
        all.sort_by(f64::total_cmp);
        eprintln!(
            "{:<24} {hits:>10} {first:>9.2} {:>9.2} {:>9.2} | {:>10} {:>9.1} {:>9.1}",
            format!("{text:?}"),
            percentile(&top, 0.5),
            percentile(&top, 0.95),
            if n == usize::MAX { "over cap".to_string() } else { n.to_string() },
            percentile(&all, 0.5),
            percentile(&all, 0.95)
        );
    }
    eprintln!("query peak footprint {} MB, load {:.1}", peak.take_mb(), load_average());
    drop(fts);
    if !keep {
        let _ = std::fs::remove_dir_all(&dir);
    }
}

/// The 1-minute load average (0 when unknown).
#[cfg(unix)]
fn load_average() -> f64 {
    let mut l = [0f64; 3];
    // SAFETY: getloadavg writes at most 3 doubles into the array.
    let n = unsafe { libc::getloadavg(l.as_mut_ptr(), 3) };
    if n > 0 {
        l[0]
    } else {
        0.0
    }
}

#[cfg(not(unix))]
fn load_average() -> f64 {
    0.0
}

use std::path::PathBuf;
