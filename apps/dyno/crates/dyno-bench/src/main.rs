#![cfg_attr(test, allow(clippy::unwrap_used, clippy::expect_used))]
//! `dyno-bench <cpu|memory|disk|gpu> [--dir <folder>] [--disk-mib <n>] [--millis <n>]`
//!
//! Runs one benchmark and prints a JSON object on stdout: `{"state":"done","metrics":[...]}`, or
//! `{"state":"unavailable"|"failed","reason":"..."}`. The agent starts it with a timeout and kills it when it
//! overruns; it never starts on its own. It reads no files except its scratch file.

use dyno_bench::repeat::Repeats;
use dyno_bench::{run, Outcome, Settings};
use dyno_core::bench::{BenchKind, BenchStatus, DISK_DEFAULT_MIB, SECTION_MILLIS};
use std::path::PathBuf;
use std::process::ExitCode;
use std::time::Duration;

/// The longest time budget per timed section a caller may ask for, ms.
const MAX_MILLIS: u64 = 5_000;
/// The most repetitions a caller may ask for.
const MAX_REPS: u32 = 20;
/// The longest warm-up or gap a caller may ask for, ms.
const MAX_PAUSE_MILLIS: u64 = 10_000;
const USAGE: &str = "usage: dyno-bench <cpu|memory|disk|gpu> [--dir <folder>] [--disk-mib <n>] [--millis <n>] [--reps <n>] [--warmup-ms <n>] [--gap-ms <n>]";

/// What the command line asked for.
#[derive(Debug, PartialEq)]
struct Args {
    kind: BenchKind,
    dir: Option<PathBuf>,
    mib: u32,
    millis: u64,
    repeats: Repeats,
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
        repeats: Repeats::STANDARD,
    };
    let mut it = args[1..].iter();
    while let Some(flag) = it.next() {
        let value = it.next().ok_or(format!("{flag} needs a value"))?;
        match flag.as_str() {
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
    let settings = Settings {
        budget: Duration::from_millis(parsed.millis),
        repeats: parsed.repeats,
        dir: parsed.dir.as_deref(),
        disk_mib: parsed.mib,
    };
    let status = match run(parsed.kind, &settings) {
        Ok(metrics) => BenchStatus::Done { metrics },
        Err(Outcome::Unavailable(reason)) => BenchStatus::Unavailable { reason },
        Err(Outcome::Failed(reason)) => BenchStatus::Failed { reason },
    };
    match serde_json::to_string(&status) {
        Ok(text) => {
            println!("{text}");
            ExitCode::SUCCESS
        }
        Err(e) => {
            eprintln!("dyno-bench: {e}");
            ExitCode::FAILURE
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn a(v: &[&str]) -> Vec<String> {
        v.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn arguments_are_parsed_strictly() {
        let p = parse(&a(&["disk", "--dir", "/x", "--disk-mib", "32", "--millis", "99999"])).unwrap();
        assert_eq!(
            (p.kind, p.dir, p.mib, p.millis),
            (BenchKind::Disk, Some(PathBuf::from("/x")), 32, MAX_MILLIS)
        );
        assert_eq!(p.repeats, Repeats::STANDARD, "the method's repetitions unless asked otherwise");
        let r = parse(&a(&["cpu", "--reps", "99", "--warmup-ms", "0", "--gap-ms", "99999999"]))
            .unwrap()
            .repeats;
        assert_eq!(
            (r.reps, r.warmup, r.gap),
            (MAX_REPS, Duration::ZERO, Duration::from_millis(MAX_PAUSE_MILLIS))
        );
        assert_eq!(parse(&a(&["cpu", "--reps", "0"])).unwrap().repeats.reps, 1);
        assert!(parse(&a(&["cpu", "--reps", "x"])).is_err());
        assert!(parse(&a(&[])).is_err());
        assert!(parse(&a(&["nope"])).is_err());
        assert!(parse(&a(&["cpu", "--bogus", "1"])).is_err());
        assert!(parse(&a(&["cpu", "--millis"])).is_err());
        assert!(parse(&a(&["cpu", "--millis", "x"])).is_err());
        assert!(parse(&a(&["disk", "--disk-mib", "x"])).is_err());
    }
}
