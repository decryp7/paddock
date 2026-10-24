//! `cargo bench -p paddock-names`: building a catalogue of `PADDOCK_BENCH_ENTRIES` names (default 250,000, the limit) and
//! searching it with typical queries (exact, joined, parts, prefix, words, typo, no match). Prints build time and the median,
//! 95th percentile and worst search time per query kind, and exits with an error when a search over the full catalogue takes
//! longer than `PADDOCK_BENCH_BUDGET_MS` (default 50, the palette's query-latency budget). No external harness, no files.

use paddock_names::{Catalogue, Entry};
use std::time::{Duration, Instant};

const WORDS: &[&str] = &[
    "graphic",
    "builder",
    "report",
    "tray",
    "agent",
    "render",
    "queue",
    "spooler",
    "service",
    "update",
    "sync",
    "backup",
    "monitor",
    "network",
    "driver",
    "display",
    "audio",
    "mail",
    "browser",
    "editor",
    "viewer",
    "player",
    "scanner",
    "printer",
    "cloud",
    "search",
    "index",
    "cache",
    "worker",
    "daemon",
    "helper",
    "installer",
    "launcher",
    "manager",
    "console",
    "terminal",
    "storage",
    "device",
];
const VENDORS: &[&str] = &[
    "CompanyA", "Acme", "Globex", "Initech", "Umbrella", "Hooli", "Vandelay", "Stark",
];

fn env(name: &str, default: u64) -> u64 {
    std::env::var(name)
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(default)
}

/// A small deterministic generator, so every run searches the same catalogue.
struct Lcg(u64);

impl Lcg {
    fn next(&mut self, n: usize) -> usize {
        self.0 = self
            .0
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        ((self.0 >> 33) as usize) % n
    }
}

fn entries(n: usize) -> Vec<Entry> {
    let mut r = Lcg(42);
    (0..n)
        .map(|i| {
            let (a, b, c) = (
                WORDS[r.next(WORDS.len())],
                WORDS[r.next(WORDS.len())],
                WORDS[r.next(WORDS.len())],
            );
            let name = match r.next(4) {
                0 => format!("{}.{a}.{b}.exe", VENDORS[r.next(VENDORS.len())]),
                1 => format!("{a}{b}{c}_x64_{}.{}.dll", r.next(9), r.next(9)),
                2 => format!("{a}-{b}-{i}.log"),
                _ => format!("{}{}{b}.exe", a[..1].to_uppercase(), &a[1..]),
            };
            Entry::new(format!("{i:06}"), name).weight(r.next(1000) as u64)
        })
        .collect()
}

fn percentile(sorted: &[Duration], p: usize) -> Duration {
    sorted[(sorted.len() * p / 100).min(sorted.len() - 1)]
}

fn main() {
    let n = env("PADDOCK_BENCH_ENTRIES", 250_000) as usize;
    let budget = Duration::from_millis(env("PADDOCK_BENCH_BUDGET_MS", 50));
    let data = entries(n);
    let t = Instant::now();
    let catalogue = Catalogue::new(data).expect("within the entry limit");
    println!("built {} entries in {:.2?}", catalogue.len(), t.elapsed());
    let queries = [
        ("exact", "graphicbuilder_x64_1.2.dll"),
        ("joined", "graphic builder"),
        ("parts", "builder"),
        ("prefix", "graph build"),
        ("words", "build graphic exe"),
        ("typo", "grafic bilder"),
        ("none", "qxzvwkj"),
    ];
    let mut worst = Duration::ZERO;
    println!(
        "{:<8} {:>10} {:>10} {:>10}",
        "query", "median", "p95", "worst"
    );
    for (kind, q) in queries {
        let mut times: Vec<Duration> = (0..9)
            .map(|_| {
                let t = Instant::now();
                std::hint::black_box(catalogue.search(q, 10));
                t.elapsed()
            })
            .collect();
        times.sort();
        worst = worst.max(times[times.len() - 1]);
        println!(
            "{kind:<8} {:>10.2?} {:>10.2?} {:>10.2?}",
            percentile(&times, 50),
            percentile(&times, 95),
            times[times.len() - 1]
        );
    }
    if worst > budget {
        eprintln!("a search took {worst:.2?}, over the {budget:?} budget");
        std::process::exit(1);
    }
}
