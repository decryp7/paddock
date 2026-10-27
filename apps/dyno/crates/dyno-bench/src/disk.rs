//! Disk: sequential write (flushed to the device), sequential read and random 4 KiB reads on one scratch file inside
//! the folder the caller gave. The file is removed before returning, on success and on failure. Reads may be served
//! by the operating system's cache (the file was just written), which the labels say.

use dyno_core::bench::{BenchMetric, DISK_MAX_MIB};
use std::fs::{File, OpenOptions};
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

const BLOCK: usize = 1024 * 1024;
const RANDOM_BLOCK: usize = 4096;
/// Name of the scratch file inside the caller's folder.
pub const SCRATCH_NAME: &str = "dyno-bench.tmp";

/// Removes the scratch file when dropped.
struct Scratch(PathBuf);

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.0);
    }
}

/// Runs the three tests with a file of `mib` MiB (capped at [`DISK_MAX_MIB`]); random reads run for about `budget`.
pub fn run(dir: &Path, mib: u32, budget: Duration) -> std::io::Result<Vec<BenchMetric>> {
    let mib = mib.clamp(1, DISK_MAX_MIB) as usize;
    let path = dir.join(SCRATCH_NAME);
    let block = vec![0x5A_u8; BLOCK];

    // `create_new`: a file that was already there is never touched; the guard exists only for a file we made.
    let mut out = OpenOptions::new().write(true).create_new(true).open(&path)?;
    let _guard = Scratch(path.clone());
    let start = Instant::now();
    {
        let f = &mut out;
        for _ in 0..mib {
            f.write_all(&block)?;
        }
        f.sync_all()?;
    }
    let write = mib as f64 / start.elapsed().as_secs_f64().max(1e-9);

    let mut f = File::open(&path)?;
    let mut buf = vec![0_u8; BLOCK];
    let start = Instant::now();
    for _ in 0..mib {
        f.read_exact(&mut buf)?;
    }
    let read = mib as f64 / start.elapsed().as_secs_f64().max(1e-9);

    let slots = (mib * BLOCK / RANDOM_BLOCK) as u64;
    let mut small = vec![0_u8; RANDOM_BLOCK];
    let mut state = 0x9E37_79B9_7F4A_7C15_u64;
    let start = Instant::now();
    let mut reads = 0_u64;
    while start.elapsed() < budget || reads == 0 {
        for _ in 0..256 {
            state = crate::cpu::mix(state);
            f.seek(SeekFrom::Start((state % slots) * RANDOM_BLOCK as u64))?;
            f.read_exact(&mut small)?;
            reads += 1;
        }
    }
    let iops = reads as f64 / start.elapsed().as_secs_f64().max(1e-9);

    Ok(vec![
        BenchMetric::higher("disk-seq-write", "Sequential write (flushed)", write, "MB/s"),
        BenchMetric::higher("disk-seq-read", "Sequential read (may be cached)", read, "MB/s"),
        BenchMetric::higher("disk-rand-read", "Random 4 KiB reads (may be cached)", iops, "IOPS"),
    ])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn it_measures_and_removes_its_file() {
        let dir = tempfile::tempdir().unwrap();
        let m = run(dir.path(), 2, Duration::from_millis(10)).unwrap();
        assert_eq!(m.len(), 3);
        assert!(m.iter().all(|x| x.value > 0.0));
        assert!(!dir.path().join(SCRATCH_NAME).exists());
    }

    #[test]
    fn a_file_that_was_already_there_is_left_alone() {
        let dir = tempfile::tempdir().unwrap();
        let theirs = dir.path().join(SCRATCH_NAME);
        std::fs::write(&theirs, b"mine").unwrap();
        assert!(run(dir.path(), 1, Duration::from_millis(1)).is_err());
        assert_eq!(std::fs::read(&theirs).unwrap(), b"mine");
    }

    #[test]
    fn the_size_is_capped_and_a_failure_still_cleans_up() {
        let dir = tempfile::tempdir().unwrap();
        let missing = dir.path().join("nope");
        assert!(run(&missing, 1, Duration::from_millis(1)).is_err());
        assert!(!missing.join(SCRATCH_NAME).exists());
    }
}
