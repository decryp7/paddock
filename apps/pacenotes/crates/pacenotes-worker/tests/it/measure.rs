//! A measurement helper, not a test (rule 48): `PN_MEASURE_REPO=<a folder> cargo test --release -p pacenotes-worker --test it --
//! --ignored --nocapture measure_pool_and_shortcut`. It indexes the folder with 1, 2 and 4 parser workers (a fresh index each), then
//! runs again over the unchanged tree to show the size and time shortcut. Results go to `docs/performance.md`.

use std::sync::Arc;
use std::time::Instant;

use pacenotes_index::{Denylist, DiskFs, IndexStore, Indexer};
use pacenotes_worker::{OsProbe, ParseWorkers};
use paddock_jobs::{CancelToken, Workers};
use paddock_testkit::Scratch;

use crate::support::settings;

#[test]
#[ignore = "a measurement, run by hand with PN_MEASURE_REPO"]
fn measure_pool_and_shortcut() {
    let repo = std::env::var("PN_MEASURE_REPO").expect("PN_MEASURE_REPO names a folder");
    let fs = DiskFs::new(std::path::Path::new(&repo)).unwrap();
    let deny = Denylist::new();
    for parse in [1usize, 2, 4] {
        let scratch = Scratch::new();
        let workers = ParseWorkers::new(settings(), Arc::new(OsProbe));
        let ix = Indexer {
            parse_workers: Workers::new(parse),
            read_workers: Workers::new(4),
            ..Indexer::new(&fs, &workers, &deny)
        };
        let mut opened = IndexStore::open(&scratch.path("index.db")).unwrap();
        let t = Instant::now();
        let report = ix.run(&mut opened.store, &CancelToken::new()).unwrap();
        let cold = t.elapsed();
        let t = Instant::now();
        let again = ix.run(&mut opened.store, &CancelToken::new()).unwrap();
        println!(
            "parse workers {parse}: first run {cold:?} ({} added, {} tier-1 only, {} too large, {} quarantined); unchanged run {:?} ({} shortcut hits, {} unchanged)",
            report.added,
            report.tier1_only,
            report.too_large,
            report.newly_quarantined.len(),
            t.elapsed(),
            again.shortcut_hits,
            again.unchanged
        );
    }
}
