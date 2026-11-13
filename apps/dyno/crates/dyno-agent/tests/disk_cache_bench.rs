#![allow(clippy::unwrap_used, clippy::expect_used)]

//! ADR 0029 WP-5 / D2 / D3: what a saved disk map costs, on a generated tree with `Node`'s real fields (folders, files with
//! their own nodes, refused and partly refused folders, loose totals), realistic name lengths and a few hundred distinct
//! parents. It measures the packed record stream raw and deflated (`flate2`, level 1, the same back end the agent links),
//! the save into `cache.db`, the start-up listing, the load of a first level, and the footprint of a loaded map against a
//! live one of the same size.
//!
//! The budgets of the ADR (start-up listing under 20 ms; the first level of a 1M-node map under 300 ms from the request;
//! saving 1M nodes under 2 s; a loaded map within 10 % of a live one) are asserted by the ignored test (run by hand in a
//! release build on a quiet machine); the ordinary run proves the round trip and the footprint on a small tree. Run the measurement by hand:
//!
//! ```sh
//! cargo test --release -p dyno-agent --test disk_cache_bench -- --nocapture --include-ignored
//! ```

use dyno_agent::cache::db::{
    decode_chunk, encode_chunk, list_saved, load_disk, CacheDb, CacheLimits, FreeSpace, Opened, SaveParams, CHUNK,
};
use dyno_core::disk::index::{DiskIndex, NodeId, RecordWriter, ScanLimits, Sizes};
use dyno_core::disk::reason::UnreadableReason;
use dyno_core::time::Timestamp;
use std::path::Path;
use std::time::Instant;

struct Roomy;

impl FreeSpace for Roomy {
    fn volume_of(&self, _: &Path) -> Option<(u64, u64)> {
        Some((1 << 42, 1 << 41))
    }
}

struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }

    fn below(&mut self, n: u64) -> u64 {
        self.next() % n
    }
}

const WORDS: &[&str] = &[
    "Library",
    "Application Support",
    "Caches",
    "node_modules",
    "target",
    "debug",
    "release",
    "build",
    "dist",
    "src",
    "assets",
    "images",
    "thumbnails",
    "backup",
    "2024",
    "2025",
    "Photos",
    "Movies",
    "Documents",
    "Downloads",
    "project",
    "report",
    "draft",
    "final",
    "archive",
    "logs",
    "session",
    "index",
    "data",
    "chunk",
    "segment",
    "tmp",
    "cache",
    "vendor",
    "lib",
    "bin",
    "share",
];
const EXTENSIONS: &[&str] = &[
    "", ".txt", ".log", ".json", ".png", ".jpg", ".mov", ".db", ".bin", ".rs", ".js", ".o", ".zip", ".pdf",
];

fn name(rng: &mut Rng, folder: bool) -> String {
    let mut n = String::new();
    for _ in 0..1 + rng.below(3) {
        if !n.is_empty() {
            n.push(['-', '_', ' ', '.'][rng.below(4) as usize]);
        }
        n.push_str(WORDS[rng.below(WORDS.len() as u64) as usize]);
    }
    // A number makes most names distinct, as real ones are.
    n.push_str(&format!("{:x}", rng.next() % 0xffff_ffff));
    if !folder {
        n.push_str(EXTENSIONS[rng.below(EXTENSIONS.len() as u64) as usize]);
    }
    n
}

/// A tree of `nodes` nodes: about one in nine a folder, the rest files with a node of their own; a few folders refused, some
/// partly; loose totals in many folders. Parents are picked among the last 4,000 folders, so the tree is deep and wide.
fn generate(nodes: u32) -> DiskIndex {
    let mut rng = Rng(0x9e37_79b9_7f4a_7c15);
    let mut index = DiskIndex::new(
        "Macintosh HD",
        ScanLimits {
            max_nodes: nodes,
            notable_bytes: 1,
            max_hard_links: 1_000,
        },
    );
    let mut folders: Vec<NodeId> = vec![index.root()];
    while index.len() < nodes as usize {
        let parent = folders[folders.len().saturating_sub(1 + rng.below(4_000) as usize)];
        if rng.below(9) == 0 {
            let Some(f) = index.add_folder(parent, name(&mut rng, true)) else {
                break;
            };
            match rng.below(1_000) {
                0 => {
                    index.mark_unreadable(f, UnreadableReason::NeedsFullDiskAccess);
                    index.finish_listing(f);
                }
                1 => {
                    index.mark_partial(f, UnreadableReason::Protected);
                    folders.push(f);
                }
                _ => folders.push(f),
            }
        } else {
            let on_disk = 4096 * (1 + rng.below(2_000));
            let sizes = Sizes {
                on_disk,
                logical: on_disk - rng.below(4096).min(on_disk - 1),
            };
            index.add_file(
                parent,
                name(&mut rng, false),
                sizes,
                Timestamp(1_700_000_000_000 + rng.below(100_000_000) as i64),
            );
            if rng.below(4) == 0 {
                index.add_folded_files(
                    parent,
                    Sizes::same(rng.below(50_000)),
                    1 + rng.below(30),
                    Timestamp(1_700_000_000_000),
                );
            }
        }
    }
    for f in folders.iter().rev() {
        index.finish_listing(*f);
    }
    index
}

fn params() -> SaveParams {
    SaveParams {
        target_key: "volume:/".into(),
        target: "{}".into(),
        volume_identity: Some("bench".into()),
        status: "{}".into(),
        limits: "{}".into(),
        scanned_at: 1,
        used_bytes_then: None,
    }
}

fn secs(t: Instant) -> f64 {
    t.elapsed().as_secs_f64()
}

fn mb(n: usize) -> f64 {
    n as f64 / (1024.0 * 1024.0)
}

/// Measures one size; returns (first level seconds with deflate, save seconds with deflate, loaded/live heap ratio, list seconds).
fn measure(nodes: u32) -> (f64, f64, f64, f64) {
    let t = Instant::now();
    let live = generate(nodes);
    println!(
        "\n== {} nodes, {:.1} MB of names and records in memory (generated in {:.2} s)",
        live.len(),
        mb(live.heap_bytes()),
        secs(t)
    );

    // The record stream, cut into 4 MiB chunks, raw and deflated.
    let t = Instant::now();
    let mut w = RecordWriter::new(&live);
    let mut chunks = Vec::new();
    while let Some(c) = w.next_chunk(CHUNK) {
        chunks.push(c);
    }
    let pack_s = secs(t);
    let raw_bytes: usize = chunks.iter().map(Vec::len).sum();
    println!(
        "records: {:.1} MB raw in {} chunks, packed in {:.3} s",
        mb(raw_bytes),
        chunks.len(),
        pack_s
    );
    for (label, compress) in [("raw", false), ("deflate 1", true)] {
        let t = Instant::now();
        let stored: Vec<Vec<u8>> = chunks.iter().map(|c| encode_chunk(c, compress)).collect();
        let enc = secs(t);
        let packed: usize = stored.iter().map(Vec::len).sum();
        let t = Instant::now();
        for s in &stored {
            std::hint::black_box(decode_chunk(s, CHUNK).unwrap());
        }
        println!(
            "{label:>10}: {:.1} MB stored ({:.2}x), encode {:.3} s, decode {:.3} s",
            mb(packed),
            raw_bytes as f64 / packed as f64,
            enc,
            secs(t)
        );
    }

    // The save into cache.db, then the start-up listing and the load of a first level, per setting.
    let mut out = (0.0, 0.0, 0.0, 0.0);
    for (label, compress) in [("raw", false), ("deflate 1", true)] {
        let dir = paddock_testkit::private_tempdir().unwrap();
        let Opened::Ready(mut db, _) = CacheDb::open(&dir.path().join("cache.db")).unwrap() else {
            panic!("busy")
        };
        let limits = CacheLimits {
            max_bytes: 8 << 30,
            compress,
        };
        let t = Instant::now();
        let saved = db.save_disk(&params(), &live, 1, limits, &Roomy).unwrap();
        let save_s = secs(t);
        let file = std::fs::metadata(db.path()).map(|m| m.len()).unwrap_or(0) as usize;
        let t = Instant::now();
        let rows = list_saved(db.path()).unwrap();
        let list_s = secs(t);
        assert_eq!(rows.len(), 1);
        let t = Instant::now();
        let loaded = load_disk(db.path(), saved.id).unwrap();
        let load_s = secs(t);
        let t = Instant::now();
        let level = loaded.children(loaded.root(), 0, 48, 0.0).unwrap();
        let level_s = secs(t);
        assert!(!level.children.is_empty());
        let ratio = loaded.heap_bytes() as f64 / live.heap_bytes() as f64;
        assert_eq!(loaded.len(), live.len());
        assert_eq!(loaded.totals(), live.totals());
        println!(
            "{label:>10}: save {save_s:.3} s ({:.1} MB in the file), listing {:.4} s, load {load_s:.3} s + first level {level_s:.4} s, loaded heap {:.1} MB = {:.3}x live",
            mb(file),
            list_s,
            mb(loaded.heap_bytes()),
            ratio
        );
        if compress {
            out = (load_s + level_s, save_s, ratio, list_s);
        }
    }
    out
}

/// The round trip and the footprint on a small tree: no clock is asserted, so this runs with the ordinary tests.
#[test]
fn a_generated_map_saves_and_loads_back_the_same() {
    let (_, _, ratio, _) = measure(30_000);
    assert!(ratio < 1.10, "a loaded map holds {ratio:.3}x a live one (budget 1.10x)");
}

/// The budgets of the ADR on the million-node tree. They assert wall-clock time, which depends on the machine and its load, so the
/// test is not part of the ordinary run (rule 12): run it by hand in release mode, on a quiet machine.
#[test]
#[ignore = "asserts wall-clock budgets: run it by hand in release mode on a quiet machine"]
fn the_budgets_hold_at_one_million_nodes() {
    let (first_level, save, ratio, listing) = measure(1_000_000);
    assert!(listing < 0.020, "the start-up listing took {listing:.4} s (budget 20 ms)");
    assert!(
        first_level < 0.300,
        "the first level of a 1M-node saved map took {first_level:.3} s (budget 300 ms)"
    );
    assert!(save < 2.0, "saving 1M nodes took {save:.3} s (budget 2 s)");
    assert!(ratio < 1.10, "a loaded map holds {ratio:.3}x a live one (budget 1.10x)");
}

#[test]
#[ignore = "a measurement at the 5M-node hard limit: run it by hand in release mode"]
fn a_map_at_the_hard_limit() {
    measure(5_000_000);
}
