#![cfg_attr(test, allow(clippy::unwrap_used, clippy::expect_used))]
//! `dyno-bench <cpu|memory|disk|gpu> [--dir <folder>] [--disk-mib <n>] [--rotational] [--free-margin-mib <n>] [--sustained-secs <n>] [--sustained-write-mib <n>] [--avail-mib <n>] [--millis <n>] [--reps <n>] [--warmup-ms <n>] [--gap-ms <n>] [--wait-go]`
//!
//! Runs one benchmark and writes JSON lines on stdout (ADR 0030 section 2): `progress` events as it goes, a `heartbeat` once a
//! second from a thread of its own, and exactly one `result` last (`dyno_core::bench::wire`). The agent starts it on the person's
//! click, reads the lines, and ends it only when the person cancels or a safety stop fires; it never starts on its own. It reads
//! no files except its scratch file. With `--wait-go` it waits for a `go` line on stdin before it measures, so the agent can
//! write down the Linux unit it runs in first.

use dyno_bench::emit::{Emitter, Heartbeat};
use dyno_bench::repeat::Repeats;
use dyno_bench::{run, Outcome, Settings};
use dyno_core::bench::wire::{Line, HEARTBEAT_EVERY};
use dyno_core::bench::{BenchKind, BenchStatus, DISK_DEFAULT_MIB, SECTION_MILLIS};
use std::path::PathBuf;
use std::process::ExitCode;
use std::sync::Arc;
use std::time::Duration;

/// How long `--wait-go` waits for the agent's `go`.
const GO_TIMEOUT: Duration = Duration::from_secs(20);

/// Disk: the ramp is its warm-up, three repetitions, no gap (ADR 0030 section 3).
const DISK_REPEATS: Repeats = Repeats {
    warmup: Duration::ZERO,
    reps: dyno_core::bench::disk::REPETITIONS,
    gap: Duration::ZERO,
};
/// The longest time budget per timed section a caller may ask for, ms.
const MAX_MILLIS: u64 = 5_000;
/// The most repetitions a caller may ask for.
const MAX_REPS: u32 = 20;
/// The longest warm-up or gap a caller may ask for, ms.
const MAX_PAUSE_MILLIS: u64 = 10_000;
const USAGE: &str =
    "usage: dyno-bench <cpu|memory|disk|gpu> [--dir <folder>] [--disk-mib <n>] [--rotational] [--millis <n>] [--reps <n>] [--warmup-ms <n>] [--gap-ms <n>] [--wait-go]";

/// What the command line asked for.
#[derive(Debug, PartialEq)]
struct Args {
    kind: BenchKind,
    dir: Option<PathBuf>,
    mib: u32,
    millis: u64,
    repeats: Repeats,
    wait_go: bool,
    rotational: bool,
    sustained_secs: Option<u32>,
    sustained_write_mib: u32,
    avail_mib: Option<u64>,
    free_margin_mib: Option<u32>,
}

fn number<T: std::str::FromStr>(flag: &str, value: &str) -> Result<T, String> {
    value.parse().map_err(|_| format!("{flag} must be a number"))
}

fn parse(args: &[String]) -> Result<Args, String> {
    let kind = args.first().and_then(|a| BenchKind::parse(a)).ok_or(USAGE)?;
    let mut out = Args {
        kind,
        dir: None,
        mib: DISK_DEFAULT_MIB,
        millis: SECTION_MILLIS,
        repeats: if kind == BenchKind::Disk { DISK_REPEATS } else { Repeats::STANDARD },
        wait_go: false,
        rotational: false,
        sustained_secs: None,
        sustained_write_mib: 0,
        avail_mib: None,
        free_margin_mib: None,
    };
    let mut it = args[1..].iter();
    while let Some(flag) = it.next() {
        if flag == "--wait-go" {
            out.wait_go = true;
            continue;
        }
        if flag == "--rotational" {
            out.rotational = true;
            continue;
        }
        let value = it.next().ok_or(format!("{flag} needs a value"))?;
        match flag.as_str() {
            "--sustained-secs" => out.sustained_secs = Some(number::<u32>(flag, value)?.clamp(1, dyno_bench::cpu::sustained::MAX_SECS)),
            "--sustained-write-mib" => out.sustained_write_mib = number::<u32>(flag, value)?.min(64 * 1024),
            "--avail-mib" => out.avail_mib = Some(number(flag, value)?),
            "--free-margin-mib" => out.free_margin_mib = Some(number(flag, value)?),
            "--dir" => out.dir = Some(PathBuf::from(value)),
            "--disk-mib" => out.mib = number(flag, value)?,
            "--millis" => out.millis = number::<u64>(flag, value)?.clamp(10, MAX_MILLIS),
            "--reps" => out.repeats.reps = number::<u32>(flag, value)?.clamp(1, MAX_REPS),
            "--warmup-ms" => out.repeats.warmup = Duration::from_millis(number::<u64>(flag, value)?.min(MAX_PAUSE_MILLIS)),
            "--gap-ms" => out.repeats.gap = Duration::from_millis(number::<u64>(flag, value)?.min(MAX_PAUSE_MILLIS)),
            other => return Err(format!("unknown option {other}")),
        }
    }
    Ok(out)
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.first().map(String::as_str) == Some("--version") {
        println!("dyno-bench {}", env!("CARGO_PKG_VERSION"));
        return ExitCode::SUCCESS;
    }
    let parsed = match parse(&args) {
        Ok(p) => p,
        Err(e) => {
            eprintln!("dyno-bench: {e}");
            return ExitCode::from(2);
        }
    };
    let emitter = Arc::new(Emitter::new(std::io::stdout()));
    // The heartbeat has a thread of its own and shares only the locked output with the measuring threads. A closed pipe means the
    // agent is gone: end, so no benchmark runs on with nobody to hear it.
    let beats = emitter.clone();
    let heartbeat = Heartbeat::start(HEARTBEAT_EVERY, move || {
        beats.emit(&Line::Heartbeat).is_ok() || std::process::exit(4)
    });
    if parsed.wait_go {
        if let Err(why) = wait_for_go(read_stdin_line, GO_TIMEOUT) {
            let status = BenchStatus::Failed { reason: why };
            return finish(&emitter, heartbeat, status);
        }
    }
    let settings = Settings {
        budget: Duration::from_millis(parsed.millis),
        repeats: parsed.repeats,
        dir: parsed.dir.as_deref(),
        disk_mib: parsed.mib,
        rotational: parsed.rotational,
        sustained_secs: parsed.sustained_secs,
        sustained_write_mib: parsed.sustained_write_mib,
        free_margin_mib: parsed.free_margin_mib,
        available_memory: parsed.avail_mib.map(|m| m.saturating_mul(1024 * 1024)),
    };
    let status = match run(parsed.kind, &settings, &*emitter) {
        Ok(metrics) => BenchStatus::Done { metrics },
        Err(Outcome::Unavailable(reason)) => BenchStatus::Unavailable { reason },
        Err(Outcome::Failed(reason)) => BenchStatus::Failed { reason },
    };
    finish(&emitter, heartbeat, status)
}

/// Writes the one `result` line, last: the heartbeat is stopped (and joined) first, so no beat can follow it.
fn finish(emitter: &Emitter<std::io::Stdout>, heartbeat: Heartbeat, status: BenchStatus) -> ExitCode {
    drop(heartbeat);
    match emitter.emit(&Line::Result { status }) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("dyno-bench: could not write the result: {e}");
            ExitCode::FAILURE
        }
    }
}

/// One line from stdin.
fn read_stdin_line() -> std::io::Result<String> {
    use std::io::BufRead;
    let mut line = String::new();
    std::io::stdin().lock().read_line(&mut line).map(|_| line)
}

/// Waits for a `go` line from `read`, at most `timeout` (a thread reads, so a silent stdin cannot hold the process). The end of the
/// input without `go` is a refusal: the agent closed it.
fn wait_for_go(read: impl FnOnce() -> std::io::Result<String> + Send + 'static, timeout: Duration) -> Result<(), String> {
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let _ = tx.send(read());
    });
    match rx.recv_timeout(timeout) {
        Ok(Ok(line)) if line.trim() == "go" => Ok(()),
        Ok(Ok(_)) => Err("The agent closed the runner's input without telling it to go.".into()),
        Ok(Err(e)) => Err(format!("The runner could not read the agent's go ({e}).")),
        Err(_) => Err(format!(
            "The agent did not tell the runner to go within {} seconds.",
            timeout.as_secs()
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn a(v: &[&str]) -> Vec<String> {
        v.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn go_is_waited_for_and_anything_else_is_refused() {
        let t = Duration::from_secs(10);
        assert!(wait_for_go(|| Ok("go\n".to_string()), t).is_ok());
        assert!(wait_for_go(|| Ok("nope\n".to_string()), t)
            .unwrap_err()
            .contains("without telling it to go"));
        assert!(wait_for_go(|| Ok(String::new()), t).is_err(), "a closed input is not a go");
        assert!(wait_for_go(|| Err(std::io::ErrorKind::BrokenPipe.into()), t)
            .unwrap_err()
            .contains("could not read"));
        // A silent input times out instead of holding the process.
        let (keep, rx) = std::sync::mpsc::channel::<()>();
        let err = wait_for_go(
            move || rx.recv().map(|()| String::new()).map_err(std::io::Error::other),
            Duration::from_millis(50),
        )
        .unwrap_err();
        assert!(err.contains("did not tell"), "{err}");
        drop(keep);
    }

    #[test]
    fn arguments_are_parsed_strictly() {
        let p = parse(&a(&["disk", "--dir", "/x", "--disk-mib", "32", "--millis", "99999"])).unwrap();
        assert_eq!(
            (p.kind, p.dir, p.mib, p.millis),
            (BenchKind::Disk, Some(PathBuf::from("/x")), 32, MAX_MILLIS)
        );
        assert_eq!(p.free_margin_mib, None, "the default free-space margin unless one is stated");
        assert_eq!(parse(&a(&["disk", "--free-margin-mib", "64"])).unwrap().free_margin_mib, Some(64));
        assert!(parse(&a(&["disk", "--free-margin-mib", "x"])).is_err());
        assert_eq!(p.repeats, DISK_REPEATS, "disk: the ramp is its warm-up, three repetitions, no gap");
        assert_eq!(
            parse(&a(&["cpu"])).unwrap().repeats,
            Repeats::STANDARD,
            "the method's repetitions unless asked otherwise"
        );
        assert!(parse(&a(&["disk", "--rotational"])).unwrap().rotational);
        let r = parse(&a(&["cpu", "--reps", "99", "--warmup-ms", "0", "--gap-ms", "99999999"]))
            .unwrap()
            .repeats;
        assert_eq!(
            (r.reps, r.warmup, r.gap),
            (MAX_REPS, Duration::ZERO, Duration::from_millis(MAX_PAUSE_MILLIS))
        );
        assert_eq!(parse(&a(&["cpu", "--reps", "0"])).unwrap().repeats.reps, 1);
        assert!(parse(&a(&["cpu", "--wait-go"])).unwrap().wait_go);
        assert!(!parse(&a(&["cpu"])).unwrap().wait_go);
        assert!(parse(&a(&["cpu", "--reps", "x"])).is_err());
        assert!(parse(&a(&[])).is_err());
        assert!(parse(&a(&["nope"])).is_err());
        assert!(parse(&a(&["cpu", "--bogus", "1"])).is_err());
        assert!(parse(&a(&["cpu", "--millis"])).is_err());
        assert!(parse(&a(&["cpu", "--millis", "x"])).is_err());
        assert!(parse(&a(&["disk", "--disk-mib", "x"])).is_err());
    }
}
