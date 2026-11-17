//! How long `TestCommand::new` takes, and its parts (docs/benchmarks.md, "TestCommand construction"; ADR 0100). Not a test:
//! run it by hand, on the machine whose numbers you want (the Windows test VM through `scripts/vm-win.sh run --ci-like`).
//!
//!     measure_test_command [--fake-toolchain <empty folder>] [--saturate] [--runs <n>]
//!
//! `--fake-toolchain` makes a toolchain of the shape rustup installs in the folder (one 15 MB `rustup` with its proxies as hard
//! links of it, and a separate `cargo`) and puts it first on this process's `PATH` and in `CARGO`, so the command links real
//! files; `--saturate` first gives the `rustup` file 1020 more hard links, near NTFS's limit of 1024, so a further link fails
//! as it does once many commands hold links to it. It prints one line per measure.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::print_stdout)]

use paddock_testkit::TestCommand;
use std::path::{Path, PathBuf};
use std::time::Instant;

const PROXIES: [&str; 6] = [
    "rustc",
    "cargo-clippy",
    "cargo-fmt",
    "rustfmt",
    "clippy-driver",
    "rustdoc",
];
const SIZE: usize = 15 * 1024 * 1024;

fn exe(name: &str) -> String {
    if cfg!(windows) {
        format!("{name}.exe")
    } else {
        name.to_string()
    }
}

fn fake_toolchain(dir: &Path, saturate: bool) -> PathBuf {
    std::fs::create_dir_all(dir).unwrap();
    let rustup = dir.join(exe("rustup"));
    std::fs::write(&rustup, vec![0u8; SIZE]).unwrap();
    for p in PROXIES {
        std::fs::hard_link(&rustup, dir.join(exe(p))).unwrap();
    }
    std::fs::write(dir.join(exe("cargo")), vec![0u8; SIZE]).unwrap();
    if saturate {
        let sat = dir.join("saturate");
        std::fs::create_dir_all(&sat).unwrap();
        for i in 0..1020 {
            if let Err(e) = std::fs::hard_link(&rustup, sat.join(format!("l{i}"))) {
                println!("saturate: link {i} failed: {e}");
                break;
            }
        }
    }
    rustup
}

fn ms(t: Instant) -> f64 {
    t.elapsed().as_secs_f64() * 1000.0
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let flag = |f: &str| args.iter().position(|a| a == f);
    let runs: usize = flag("--runs")
        .and_then(|i| args.get(i + 1))
        .map_or(10, |v| v.parse().unwrap());
    if let Some(i) = flag("--fake-toolchain") {
        let dir = PathBuf::from(&args[i + 1]);
        let rustup = fake_toolchain(&dir, flag("--saturate").is_some());
        let path = std::env::var_os("PATH").unwrap_or_default();
        let mut dirs = vec![dir.clone()];
        dirs.extend(std::env::split_paths(&path));
        // SAFETY: set before any other thread starts; this program has one thread until here.
        unsafe {
            std::env::set_var("PATH", std::env::join_paths(dirs).unwrap());
            std::env::set_var("CARGO", dir.join(exe("cargo")));
        }
        // The two ways a link can be made: a hard link and a copy, seven each, as one command makes them.
        let probe = dir.join("probe");
        std::fs::create_dir_all(&probe).unwrap();
        let t = Instant::now();
        let mut linked = 0;
        for p in PROXIES.iter().chain(["rustup"].iter()) {
            if std::fs::hard_link(&rustup, probe.join(format!("h-{}", exe(p)))).is_ok() {
                linked += 1;
            }
        }
        println!("hard links x7: {:.1} ms ({linked} made)", ms(t));
        let t = Instant::now();
        for p in PROXIES.iter().chain(["rustup"].iter()) {
            std::fs::copy(&rustup, probe.join(format!("c-{}", exe(p)))).unwrap();
        }
        println!("copies x7 of {} MB: {:.1} ms", SIZE / 1024 / 1024, ms(t));
        std::fs::remove_dir_all(&probe).unwrap();
    }
    let git = if cfg!(windows) { "git.exe" } else { "git" };
    let t = Instant::now();
    #[allow(clippy::disallowed_methods)] // a timing of one plain spawn, awaited here
    let ok = std::process::Command::new(git)
        .arg("--exec-path")
        .output()
        .is_ok_and(|o| o.status.success());
    println!("git --exec-path: {:.1} ms (ok {ok})", ms(t));
    let mut times = Vec::new();
    let mut held = Vec::new();
    for _ in 0..runs {
        let t = Instant::now();
        let c = TestCommand::new(if cfg!(windows) { "cmd" } else { "sh" });
        times.push(ms(t));
        held.push(c);
    }
    let first = times[0];
    let mut rest = times[1..].to_vec();
    rest.sort_by(f64::total_cmp);
    let median = rest.get(rest.len() / 2).copied().unwrap_or(first);
    println!(
        "TestCommand::new: first {first:.1} ms, median of the next {} {median:.1} ms, max {:.1} ms",
        rest.len(),
        rest.last().copied().unwrap_or(first)
    );
    let t = Instant::now();
    let report = held[0].path_report();
    println!(
        "path_report: {:.1} ms; links {}",
        ms(t),
        report.toolchain.len()
    );
}
