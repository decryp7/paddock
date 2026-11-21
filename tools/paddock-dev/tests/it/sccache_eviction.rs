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

use paddock_env::{Os, winpath};
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
    /// server (it is not ours); after five tries the test fails rather than run on a server it does not own. The failure names, for
    /// every try, the port, both exit codes, what sccache printed, the server's own log and who listens on the port (ER-27).
    fn start(cache_dir: &Path, work: &Path) -> Cache {
        let mut tries = Vec::new();
        for _ in 0..5 {
            let c = Cache {
                cache_dir: cache_dir.to_path_buf(),
                port: free_port(),
                work: work.to_path_buf(),
            };
            let log = c.work.join(format!("sccache-server-{}.log", c.port));
            let started = c
                .command("sccache")
                .env("SCCACHE_ERROR_LOG", &log)
                .env("SCCACHE_LOG", "debug")
                .arg("--start-server")
                .output()
                .expect("sccache is on PATH (the CI image has it): not measured otherwise");
            let stats = c.command("sccache").arg("--show-stats").output().unwrap();
            let ours = String::from_utf8_lossy(&stats.stdout)
                .lines()
                .any(|l| l.starts_with("Cache location") && location_names(l, cache_dir, paddock_env::Os::current()));
            if started.status.success() && ours {
                return c;
            }
            tries.push(format!(
                "port {port}: --start-server {s1}, stdout {o1:?}, stderr {e1:?}; --show-stats {s2}, location {loc:?}, stderr {e2:?}; \
                 listener: {who}; server log ({log}): {tail}",
                port = c.port,
                s1 = started.status,
                o1 = tail(&started.stdout),
                e1 = tail(&started.stderr),
                s2 = stats.status,
                loc = String::from_utf8_lossy(&stats.stdout)
                    .lines()
                    .find(|l| l.starts_with("Cache location"))
                    .unwrap_or("<no Cache location line>")
                    .to_string(),
                e2 = tail(&stats.stderr),
                who = listener(c.port),
                log = log.display(),
                tail = std::fs::read(&log).map_or_else(|e| format!("<none: {e}>"), |b| tail(&b)),
            ));
            // Not ours (a taken port, or a server of another folder): forget it without stopping anything.
            std::mem::forget(c);
        }
        panic!(
            "no port in five tries served this test's own sccache server (a Cache location other than {}); env passed: \
             SCCACHE_DIR={}, SCCACHE_CACHE_SIZE=1M, SCCACHE_SERVER_PORT=<port>, SCCACHE_IGNORE_SERVER_IO_ERROR=1; tries:\n{}",
            cache_dir.display(),
            cache_dir.display(),
            tries.join("\n")
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

/// The last 2000 bytes of a program's output, as text.
fn tail(bytes: &[u8]) -> String {
    let text = String::from_utf8_lossy(bytes);
    let start = text.len().saturating_sub(2000);
    let start = (start..=text.len()).find(|i| text.is_char_boundary(*i)).unwrap_or(text.len());
    text[start..].trim().to_string()
}

/// Who listens on `port`, with its pid, from the OS's own tool (`netstat -ano` on Windows, `lsof` elsewhere); diagnostics only.
fn listener(port: u16) -> String {
    let out = if cfg!(windows) {
        TestCommand::new("netstat").args(["-ano", "-p", "TCP"]).output()
    } else {
        TestCommand::new("lsof")
            .args(["-nP", &format!("-iTCP:{port}"), "-sTCP:LISTEN"])
            .output()
    };
    match out {
        Ok(o) => {
            let needle = format!(":{port} ");
            let lines: Vec<String> = String::from_utf8_lossy(&o.stdout)
                .lines()
                .filter(|l| cfg!(not(windows)) || l.contains(&needle))
                .map(|l| l.split_whitespace().collect::<Vec<_>>().join(" "))
                .collect();
            if lines.is_empty() { "nobody".into() } else { lines.join(" | ") }
        }
        Err(e) => format!("<not measured: {e}>"),
    }
}

impl Drop for Cache {
    fn drop(&mut self) {
        let _ = self.command("sccache").arg("--stop-server").output();
    }
}

/// Whether sccache's `Cache location  Local disk: "<dir>"` line names `dir`. sccache prints the folder with Rust's `{:?}`, so on
/// Windows every backslash is doubled; that escaping is undone, and the two folders are compared with `winpath::same` (the shared
/// rule: verbatim prefix stripped, `/` and `\` one separator, case ignored on Windows), never as strings.
fn location_names(line: &str, dir: &Path, os: Os) -> bool {
    let Some((_, quoted)) = line.split_once("Local disk: \"") else {
        return false;
    };
    let printed = quoted.trim_end().trim_end_matches('"');
    let printed = if os == Os::Windows {
        printed.replace("\\\\", "\\")
    } else {
        printed.to_string()
    };
    winpath::same(Path::new(&printed), dir, os)
}

#[test]
fn the_location_check_reads_windows_forms_of_one_folder_as_the_same() {
    // What the Windows runner printed (run 8495, job 43574): the folder as `{:?}` escapes it, against the folder as the test holds it.
    let dir = Path::new(r"C:\gitea-runner\work\8a221e7c66bab078\tmp\.tmpoYlqSG\mount");
    let line = r#"Cache location                  Local disk: "C:\\gitea-runner\\work\\8a221e7c66bab078\\tmp\\.tmpoYlqSG\\mount""#;
    let win = Os::Windows;
    assert!(location_names(line, dir, win), "doubled backslashes are the same folder");
    assert!(
        location_names(
            line,
            Path::new(r"\\?\C:\gitea-runner\work\8a221e7c66bab078\tmp\.tmpoYlqSG\mount"),
            win
        ),
        "verbatim prefix"
    );
    assert!(location_names(&line.replace("gitea-runner", "GITEA-Runner"), dir, win), "case");
    // Positive controls: another folder, and one that only starts like ours, are not ours.
    assert!(!location_names(line, Path::new(r"C:\gitea-runner\work\other\mount"), win));
    assert!(!location_names(
        r#"Cache location  Local disk: "C:\\x\\y""#,
        Path::new(r"C:\x\z"),
        win
    ));
    assert!(!location_names("Cache location  Remote: redis", dir, win));
    // Unix: exact, nothing unescaped.
    assert!(location_names(
        r#"Cache location  Local disk: "/tmp/a/mount""#,
        Path::new("/tmp/a/mount"),
        Os::Linux
    ));
    assert!(!location_names(
        r#"Cache location  Local disk: "/tmp/a/Mount""#,
        Path::new("/tmp/a/mount"),
        Os::Linux
    ));
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
