//! Opening a folder with very large files: the worker must survive it (apps/debrief/docs/performance.md, 2026-11-05).
//!
//! Written to fail today, and `#[ignore]`d with the failure it shows until the fix removes the marker. The real case is a
//! 1.43 GiB event log and a 230 MB semicolon export under the default 2 GiB cap; these keep the same ratios at a size a
//! test can afford (a 512 MiB cap). The inputs are generated in a temp folder (invented lines, a sample chunk repeated),
//! never committed: `cargo test --release -p debrief-store-worker --test it folder_open -- --ignored`.

use debrief_core::ports::Ingest;
use debrief_core::Profile;
use debrief_store_worker::{StoreWorker, WorkerConfig};
use std::io::Write;
use std::path::{Path, PathBuf};

const MB: u64 = 1 << 20;
/// The cap of these tests: a quarter of the default, as the files are a quarter (or so) of the real ones.
const CAP_MB: u64 = 512;

struct TempDir(PathBuf);

impl TempDir {
    fn new(tag: &str) -> TempDir {
        let mut b = [0u8; 8];
        getrandom::fill(&mut b).unwrap();
        let p = std::env::temp_dir().join(format!(
            "debrief-folder-open-{tag}-{}-{:x}",
            std::process::id(),
            u64::from_le_bytes(b)
        ));
        std::fs::create_dir_all(&p).unwrap();
        TempDir(p)
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn worker() -> StoreWorker {
    // These tests kill the worker: its temp folders go with the test's own (see `super::worker::confined`).
    #[cfg(unix)]
    let mut cfg = super::worker::confined(env!("CARGO_BIN_EXE_debrief-store-worker"));
    #[cfg(not(unix))]
    let mut cfg = WorkerConfig::new(env!("CARGO_BIN_EXE_debrief-store-worker"));
    cfg.memory_cap = Some(CAP_MB * MB);
    StoreWorker::new(cfg).unwrap()
}

/// Opens `folder`; the worker must still be there at the end. What the file became (events, or a contained failure of
/// its plugin) is another question: the bug is that one file takes the whole import down.
fn opens(folder: &Path) {
    let w = worker();
    let store_dir = TempDir::new("store");
    let store = w.open_store(&store_dir.0, Profile::default_profile(), true, None).unwrap();
    let session = store.open(&[folder.to_path_buf()]).unwrap();
    let report = session.wait();
    let report = match report {
        Ok(r) => r,
        Err(e) => panic!("opening the folder stopped the worker: {e}"),
    };
    // The probe must see the thing (ER-33): without the plugins an .evtx is only a "binary file" and never read whole.
    assert!(
        report.unrecognized.iter().all(|u| u.reason != "binary file"),
        "the plugins are not available to this worker; build debrief-plugins next to it: {:?}",
        report.unrecognized
    );
}

/// An `.evtx` of `mb` MiB: the fixture's header, then its chunk repeated (the parser counts chunks from the file size).
fn write_evtx(path: &Path, mb: u64) {
    let fixture = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../debrief-plugin-windows/tests/fixtures/security.evtx");
    let sample = std::fs::read(fixture).unwrap();
    let (header, chunk) = sample.split_at(4096);
    let mut out = std::io::BufWriter::new(std::fs::File::create(path).unwrap());
    out.write_all(header).unwrap();
    for _ in 0..(mb * MB) / chunk.len() as u64 {
        out.write_all(chunk).unwrap();
    }
}

/// The whole file is read into the worker, copied once more for the guarded call, and only then lent to the plugin:
/// 300 MiB costs at least 600 MiB there, over the 512 MiB cap.
#[test]
#[ignore = "fails today: the store worker used more than its memory cap of 512 MB and was stopped (whole-file read plus the guard's copy)"]
fn a_large_binary_file_does_not_stop_the_worker() {
    let data = TempDir::new("evtx");
    write_evtx(&data.0.join("events.evtx"), 300);
    opens(&data.0);
}

/// A semicolon export read as CSV: the adapter for importers that do not stream holds the file, its text and every record.
/// About 13 MB of worker per MB of file was measured, so 64 MiB is well over the cap.
#[test]
#[ignore = "fails today on debrief/1.2-int: the store worker used more than its memory cap of 512 MB and was stopped (whole-file CSV import)"]
fn a_large_delimited_export_does_not_stop_the_worker() {
    let data = TempDir::new("csv");
    let mut out = std::io::BufWriter::new(std::fs::File::create(data.0.join("export.txt")).unwrap());
    let mut written = 0u64;
    let mut n = 0u64;
    while written < 64 * MB {
        let line = format!(
            "Mon Mar {} {:02}:{:02}:{:02} 2026;0x{:08X};INFO;source{};handle request {n} started worker {} session\r\n",
            1 + n % 28,
            n % 24,
            n % 60,
            (n / 7) % 60,
            n % 65_536,
            n % 40,
            n % 97
        );
        out.write_all(line.as_bytes()).unwrap();
        written += line.len() as u64;
        n += 1;
    }
    drop(out);
    opens(&data.0);
}
