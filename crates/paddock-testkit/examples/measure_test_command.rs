//! How long `TestCommand::new` takes, and its parts (docs/benchmarks.md, "TestCommand construction"; ADR 0100). Not a test:
//! run it by hand, on the machine whose numbers you want (the Windows test VM through `scripts/vm-win.sh run --ci-like`).
//!
//!     measure_test_command [--fake-toolchain <empty folder>] [--saturate] [--runs <n>] [--git-calls <n>]
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
        #[expect(
            clippy::disallowed_methods,
            reason = "a measuring program with one thread here, not a test: it sets up the machine it measures"
        )]
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
    // `--git-ab <n>`: Git for Windows' launcher `<root>\\bin\\git.exe` against the real git on this process's PATH, called in turn
    // (A, B, A, B, ...) so a busy moment costs both the same: what one launcher in front of every call costs.
    if let Some(n) = flag("--git-ab")
        .and_then(|i| args.get(i + 1))
        .map(|v| v.parse::<usize>().unwrap())
    {
        let process: Vec<PathBuf> =
            std::env::split_paths(&std::env::var_os("PATH").unwrap_or_default()).collect();
        let real = process
            .iter()
            .map(|d| d.join(exe("git")))
            .find(|p| p.is_file());
        let launcher = report
            .child
            .iter()
            .find(|d| d.ends_with("usr/bin") || d.ends_with("usr\\bin"))
            .and_then(|usr_bin| {
                usr_bin
                    .parent()?
                    .parent()
                    .map(|root| root.join("bin").join(exe("git")))
            })
            .filter(|p| p.is_file());
        match (launcher, real) {
            (Some(a), Some(b)) => {
                let (mut ta, mut tb) = (0.0, 0.0);
                for _ in 0..n {
                    for (g, total) in [(&a, &mut ta), (&b, &mut tb)] {
                        let t = Instant::now();
                        #[allow(clippy::disallowed_methods)]
                        // a timing of plain spawns, each awaited here
                        let ok = std::process::Command::new(g)
                            .arg("--version")
                            .output()
                            .is_ok_and(|o| o.status.success());
                        assert!(ok, "{} --version failed", g.display());
                        *total += ms(t);
                    }
                }
                println!(
                    "git-ab: launcher {} {:.1} ms per call; real {} {:.1} ms per call ({n} each, in turn)",
                    a.display(),
                    ta / n as f64,
                    b.display(),
                    tb / n as f64
                );
            }
            (a, b) => println!("git-ab: launcher {a:?}, real {b:?}: not both found"),
        }
    }
    // `--git-calls <n>`: which `git` a child finds on its curated PATH (what the program under test starts when it runs `git`),
    // against the one on this process's PATH, and what n runs of each cost.
    if let Some(n) = flag("--git-calls")
        .and_then(|i| args.get(i + 1))
        .map(|v| v.parse::<usize>().unwrap())
    {
        let git_in = |dirs: &[PathBuf]| {
            dirs.iter()
                .map(|d| d.join(exe("git")))
                .find(|p| p.is_file())
        };
        let process: Vec<PathBuf> =
            std::env::split_paths(&std::env::var_os("PATH").unwrap_or_default()).collect();
        for (label, git) in [
            ("child's PATH", git_in(&report.child)),
            ("process's PATH", git_in(&process)),
        ] {
            let Some(git) = git else {
                println!("git on the {label}: none");
                continue;
            };
            let t = Instant::now();
            for _ in 0..n {
                #[allow(clippy::disallowed_methods)] // a timing of plain spawns, each awaited here
                let ok = std::process::Command::new(&git)
                    .arg("--version")
                    .output()
                    .is_ok_and(|o| o.status.success());
                assert!(ok, "{} --version failed", git.display());
            }
            println!(
                "git on the {label}: {} ; {n} runs of --version: {:.0} ms ({:.1} ms each)",
                git.display(),
                ms(t),
                ms(t) / n as f64
            );
        }
    }
}
