//! `cargo bench -p paddock-jobs`: parallel walk against a plain sequential walk on a synthetic tree in a temp dir (warm cache).
//! Set `PADDOCK_BENCH_FILES` (default 60000) to change the tree size. Prints a table; no external harness.

use paddock_jobs::{CancelToken, WalkOptions, Workers, par_walk};
use std::fs;
use std::path::Path;
use std::time::{Duration, Instant};

fn build(root: &Path, files: usize) {
    // 20 top folders x 10 sub folders x 5 leaf folders = 1000 leaves; files spread evenly.
    let per_leaf = files.div_ceil(1000).max(1);
    let mut made = 0;
    'outer: for a in 0..20 {
        for b in 0..10 {
            for c in 0..5 {
                let dir = root.join(format!("a{a}/b{b}/c{c}"));
                fs::create_dir_all(&dir).expect("create dir");
                for f in 0..per_leaf {
                    fs::write(dir.join(format!("f{f}.log")), b"x").expect("write file");
                    made += 1;
                    if made >= files {
                        break 'outer;
                    }
                }
            }
        }
    }
}

fn sequential_collect(dir: &Path, out: &mut Vec<std::path::PathBuf>) {
    if let Ok(rd) = fs::read_dir(dir) {
        for e in rd.flatten() {
            match e.file_type() {
                Ok(t) if t.is_dir() => sequential_collect(&e.path(), out),
                Ok(_) => out.push(e.path()),
                Err(_) => {}
            }
        }
    }
}

fn sequential(dir: &Path, n: &mut u64) {
    if let Ok(rd) = fs::read_dir(dir) {
        for e in rd.flatten() {
            match e.file_type() {
                Ok(t) if t.is_dir() => sequential(&e.path(), n),
                Ok(_) => *n += 1,
                Err(_) => {}
            }
        }
    }
}

fn time<F: FnMut() -> u64>(mut f: F) -> (Duration, u64) {
    let _ = f(); // warm-up
    let mut best = Duration::MAX;
    let mut count = 0;
    for _ in 0..5 {
        let t = Instant::now();
        count = f();
        best = best.min(t.elapsed());
    }
    (best, count)
}

fn main() {
    let files: usize = std::env::var("PADDOCK_BENCH_FILES")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(60_000);
    let dir = std::env::temp_dir().join(format!("paddock-jobs-bench-{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    build(&dir, files);
    println!(
        "tree: {files} files in 1000 leaf folders ({})",
        dir.display()
    );
    let (base, n) = time(|| {
        let mut n = 0;
        sequential(&dir, &mut n);
        n
    });
    println!(
        "{:<22}{:>10.1} ms  {:>6.2}x  ({n} files)",
        "sequential",
        base.as_secs_f64() * 1e3,
        1.0
    );
    let (collect, _) = time(|| {
        let mut v = Vec::new();
        sequential_collect(&dir, &mut v);
        v.sort();
        v.len() as u64
    });
    println!(
        "{:<22}{:>10.1} ms  {:>6.2}x",
        "sequential+sort",
        collect.as_secs_f64() * 1e3,
        base.as_secs_f64() / collect.as_secs_f64()
    );
    let base = collect; // the like-for-like baseline: same output (all paths, sorted)
    for w in [1, 2, 4, 8] {
        let opts = WalkOptions {
            workers: Workers::new(w),
            ..WalkOptions::default()
        };
        let cancel = CancelToken::new();
        let (t, found) =
            time(|| par_walk(&dir, &opts, &cancel, &()).map_or(0, |r| r.entries.len() as u64));
        assert_eq!(found, n, "same files as the sequential walk");
        println!(
            "{:<22}{:>10.1} ms  {:>6.2}x",
            format!("par_walk, {w} workers"),
            t.as_secs_f64() * 1e3,
            base.as_secs_f64() / t.as_secs_f64()
        );
    }
    let _ = fs::remove_dir_all(&dir);
}
