#![allow(clippy::unwrap_used, clippy::expect_used)]

//! How long the file finder takes on a generated tree, by the number of threads reading it (rule 48: measured, not assumed).
//! Ignored by default; run it in release mode:
//!
//! ```sh
//! cargo test --release -p dyno-agent --test finder_bench -- --ignored --nocapture
//! ```
//!
//! The tree is made in a temp folder: many small files in many folders (the walk) and a few hundred files over the size floor,
//! a third of them copies (the hashing). Every run reads the same tree, so the numbers compare like for like.

use dyno_agent::disk::engine::LiveWorkers;
use dyno_agent::extras::finder::{run, Limits, Linger};
use dyno_core::extras::files::{FinderMode, FinderQuery};
use paddock_jobs::{EventSink, JobRegistry, JobSnapshot, Workers};
use std::sync::Arc;
use std::time::Instant;

struct Quiet;
impl EventSink for Quiet {
    fn emit(&self, _: &JobSnapshot) {}
}

const FOLDERS: usize = 200;
const SMALL_PER_FOLDER: usize = 250;
const MIB: usize = 1024 * 1024;

fn make_tree(root: &std::path::Path) {
    for d in 0..FOLDERS {
        let dir = root.join(format!("d{d}")).join("sub");
        std::fs::create_dir_all(&dir).unwrap();
        for i in 0..SMALL_PER_FOLDER {
            std::fs::write(dir.join(format!("f{i}.txt")), b"small").unwrap();
        }
    }
    // 100 files over the floor, each with two copies in other folders: 300 files of about 1 MiB.
    for k in 0..100usize {
        let body: Vec<u8> = (0..MIB + 17).map(|i| ((i * 31 + k * 7) % 251) as u8).collect();
        for c in 0..3usize {
            let dir = root.join(format!("d{}", (k * 3 + c) % FOLDERS));
            std::fs::write(dir.join(format!("big-{k}-{c}.bin")), &body).unwrap();
        }
    }
}

#[test]
#[ignore = "a measurement, not a check: run it by hand in release mode"]
fn the_finder_on_a_generated_tree() {
    let tmp = tempfile::tempdir().unwrap();
    let root = std::fs::canonicalize(tmp.path()).unwrap().join("tree");
    let made = Instant::now();
    make_tree(&root);
    eprintln!(
        "tree: {} folders, {} small files, 300 files of about 1 MiB (made in {:.1} s)",
        FOLDERS * 2,
        FOLDERS * SMALL_PER_FOLDER,
        made.elapsed().as_secs_f32()
    );
    let query = FinderQuery {
        folder: root.to_string_lossy().into_owned(),
        mode: FinderMode::Both,
        min_bytes: 0,
    };
    for threads in [1usize, 2, 4, 8] {
        // The first run of each count warms the OS file cache like the one before it did; three runs, the best and the median.
        let mut times: Vec<f32> = (0..3)
            .map(|_| {
                let registry = JobRegistry::new(Arc::new(Quiet));
                let ctx = registry.register("find-files", "Finding files", None);
                let started = Instant::now();
                let result = ctx
                    .run(|ctx| {
                        run(
                            ctx,
                            &query,
                            "bench",
                            Limits::default(),
                            Workers::new(threads),
                            Linger::counting(std::sync::Arc::new(LiveWorkers::default()), 0, 4, threads),
                        )
                    })
                    .unwrap();
                assert_eq!(result.duplicates.len(), 100);
                started.elapsed().as_secs_f32()
            })
            .collect();
        times.sort_by(|a, b| a.partial_cmp(b).unwrap());
        eprintln!("threads {threads}: best {:.2} s, median {:.2} s", times[0], times[1]);
    }
}
