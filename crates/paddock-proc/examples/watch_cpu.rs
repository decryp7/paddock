//! CPU a memory watcher costs: samples a synthetic child (`sleep`) every 200 ms, once the old way (the whole process table
//! per tick) and once per pid, and prints the CPU time of this process over the run.
//! `cargo run --release -p paddock-proc --example watch_cpu -- 60`

#[cfg(unix)]
use std::time::Duration;

#[cfg(unix)]
fn cpu() -> Duration {
    let mut ru = std::mem::MaybeUninit::<libc::rusage>::zeroed();
    // SAFETY: getrusage fills the struct on success, which is checked before the read.
    let ru = unsafe {
        assert_eq!(libc::getrusage(libc::RUSAGE_SELF, ru.as_mut_ptr()), 0);
        ru.assume_init()
    };
    let t = |v: libc::timeval| Duration::new(v.tv_sec as u64, v.tv_usec as u32 * 1000);
    t(ru.ru_utime) + t(ru.ru_stime)
}

#[cfg(unix)]
fn run(label: &str, secs: u64, tick: impl Fn()) {
    let (start, c0) = (std::time::Instant::now(), cpu());
    while start.elapsed() < Duration::from_secs(secs) {
        tick();
        std::thread::sleep(Duration::from_millis(200));
    }
    let used = (cpu() - c0).as_secs_f64();
    println!(
        "{label}: {used:.2} s CPU over {secs} s = {:.1}% of one core",
        used * 100.0 / secs as f64
    );
}

#[cfg(unix)]
fn main() {
    let secs = std::env::args()
        .nth(1)
        .and_then(|s| s.parse().ok())
        .unwrap_or(60);
    let mut child = std::process::Command::new("sleep")
        .arg("1000")
        .spawn()
        .expect("sleep");
    let pid = child.id();
    let processes = paddock_proc::snapshot().len();
    println!("{processes} processes on this machine");
    run("whole table per tick (before)", secs, || {
        let table = paddock_proc::snapshot();
        std::hint::black_box(paddock_proc::tree_memory_with(&table, pid, |_| {
            paddock_proc::process_memory(pid)
        }));
    });
    run("per pid (after)", secs, || {
        std::hint::black_box(paddock_proc::tree_memory(pid));
    });
    let _ = child.kill();
    let _ = child.wait();
}

#[cfg(not(unix))]
fn main() {
    println!("Unix only (it reads getrusage)");
}
