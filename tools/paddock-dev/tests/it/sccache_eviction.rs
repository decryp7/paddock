//! Why no folder but sccache's own may sit under `SCCACHE_DIR` (ADR 0111; Paddock run 8373, job 42677, E0463 "can't find crate for
//! `foldhash`"): sccache's local cache is one LRU over every file below its folder, so a cargo target file placed there is evicted
//! as soon as an insert takes the folder over `SCCACHE_CACHE_SIZE`. Linux CI set `SCCACHE_DIR` to the whole `/sccache` mount, where
//! `./dev` kept paddock-dev's build folder, and rlibs vanished in the middle of a build.
//!
//! This is the real sccache with a 1 MiB cap on a private folder and a private port, never the machine's server (ER-14, ER-45).
//! It runs on every OS (the LRU is sccache's own, the same everywhere). It needs `sccache` and `rustc` on PATH: the CI image has both. Without them the test fails with a message that says what is
//! missing (never a silent pass, ER-33); the control below is its positive control: the same file beside the cache folder, not in
//! it, survives, so the test can tell eviction from a file that was never there.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use paddock_testkit::TestCommand;
use std::path::{Path, PathBuf};

/// A cargo build folder's rlib of 3 MiB, three times the cap.
const RLIB: &str = "paddock-dev/rustc-x/release/libfoldhash.rlib";

/// A port for sccache's server, derived from this process and a counter (the server binds it itself; nothing here binds and drops a
/// listener to read its port). `Cache::start` checks that the server answering on it is this test's own, and tries another port if not.
fn free_port() -> u16 {
    static NEXT: std::sync::atomic::AtomicU16 = std::sync::atomic::AtomicU16::new(0);
    let n = NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let base = 30_000 + (u16::try_from(std::process::id() % 20_000).unwrap_or(0));
    base.wrapping_add(n) % 20_000 + 30_000
}

/// sccache with its own server on a private port, a 1 MiB cap and `cache_dir` as its folder; the server stops with the value.
struct Cache {
    cache_dir: PathBuf,
    port: u16,
    work: PathBuf,
}

impl Cache {
    fn command(&self, program: &str) -> TestCommand {
        let mut c = TestCommand::new_in(program, &self.work);
        c.env("SCCACHE_DIR", &self.cache_dir)
            .env("SCCACHE_CACHE_SIZE", "1M")
            .env("SCCACHE_SERVER_PORT", self.port.to_string())
            .env("SCCACHE_IGNORE_SERVER_IO_ERROR", "1")
            .env("RUSTUP_AUTO_INSTALL", "0");
        c
    }

    /// Starts the server on a port of its own and checks that the server answering there is this one: its "Cache location" must be
    /// `cache_dir`. A port some other sccache already serves answers with another folder, and is skipped without stopping that
    /// server (it is not ours); after five tries the test fails rather than run on a server it does not own.
    fn start(cache_dir: &Path, work: &Path) -> Cache {
        for _ in 0..5 {
            let c = Cache {
                cache_dir: cache_dir.to_path_buf(),
                port: free_port(),
                work: work.to_path_buf(),
            };
            let started = c
                .command("sccache")
                .arg("--start-server")
                .output()
                .expect("sccache is on PATH (the CI image has it): not measured otherwise");
            let stats = c.command("sccache").arg("--show-stats").output().unwrap();
            let ours = String::from_utf8_lossy(&stats.stdout)
                .lines()
                .any(|l| l.starts_with("Cache location") && l.contains(&cache_dir.display().to_string()));
            if started.status.success() && ours {
                return c;
            }
            // Not ours (a taken port, or a server of another folder): forget it without stopping anything.
            std::mem::forget(c);
        }
        panic!(
            "no port in five tries served this test's own sccache server (a Cache location other than {})",
            cache_dir.display()
        );
    }

    /// One cacheable compile: a miss that inserts an entry and so makes the cache fill its cap.
    fn compile_one_crate(&self) {
        std::fs::create_dir_all(self.work.join("out")).unwrap();
        std::fs::write(self.work.join("x.rs"), "pub fn f() -> u8 { 1 }\n").unwrap();
        let out_dir = format!("--out-dir={}", self.work.join("out").display());
        let source = self.work.join("x.rs");
        let out = self
            .command("sccache")
            .args([
                "rustc",
                "--crate-type=lib",
                "--crate-name=x",
                "--edition=2021",
                "--emit=dep-info,link",
            ])
            .arg(out_dir)
            .arg(source)
            .output()
            .expect("rustc is on PATH: not measured otherwise");
        assert!(out.status.success(), "the compile failed: {}", String::from_utf8_lossy(&out.stderr));
        let stats = self.command("sccache").arg("--show-stats").output().unwrap();
        let stats = String::from_utf8_lossy(&stats.stdout);
        assert!(
            stats
                .lines()
                .any(|l| l.starts_with("Cache misses ") && !l.trim_end().ends_with(" 0")),
            "the compile was not cached, so the test saw no insert: {stats}"
        );
    }
}

impl Drop for Cache {
    fn drop(&mut self) {
        let _ = self.command("sccache").arg("--stop-server").output();
    }
}

fn big_file(path: &Path) {
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, vec![0u8; 3 * 1024 * 1024]).unwrap();
}

#[test]
fn a_cargo_file_under_the_sccache_folder_is_evicted_once_the_cache_is_over_its_cap() {
    let scratch = tempfile::tempdir().unwrap();
    let mount = scratch.path().join("mount");
    big_file(&mount.join(RLIB));
    let cache = Cache::start(&mount, &scratch.path().join("work"));
    assert!(mount.join(RLIB).exists(), "nothing evicts before an insert");
    cache.compile_one_crate();
    assert!(
        !mount.join(RLIB).exists(),
        "sccache kept the 3 MiB cargo file under its folder with a 1 MiB cap: the premise of ADR 0111 is gone"
    );
}

#[test]
fn control_the_same_file_beside_the_sccache_folder_survives() {
    let scratch = tempfile::tempdir().unwrap();
    let mount = scratch.path().join("mount");
    big_file(&mount.join(RLIB));
    // The lane's own folder, one level down: what setup-rust gives a Linux job (ADR 0111).
    let cache = Cache::start(&mount.join("sccache-s1"), &scratch.path().join("work"));
    cache.compile_one_crate();
    assert!(mount.join(RLIB).exists(), "a file outside sccache's folder is never its to evict");
    assert!(
        std::fs::read_dir(mount.join("sccache-s1")).unwrap().next().is_some(),
        "the cache itself holds the entry"
    );
}
