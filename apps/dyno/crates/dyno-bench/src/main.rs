#![cfg_attr(test, allow(clippy::unwrap_used, clippy::expect_used))]
//! `dyno-bench <cpu|memory|disk|gpu> [--dir <folder>] [--disk-mib <n>] [--millis <n>]`
//!
//! Runs one benchmark and prints a JSON object on stdout: `{"state":"done","metrics":[...]}`, or
//! `{"state":"unavailable"|"failed","reason":"..."}`. The agent starts it with a timeout and kills it when it
//! overruns; it never starts on its own. It reads no files except its scratch file.

use dyno_bench::{run, Outcome, Settings};
use dyno_core::bench::{BenchKind, BenchStatus, DISK_DEFAULT_MIB};
use std::path::PathBuf;
use std::process::ExitCode;
use std::time::Duration;

/// The longest time budget per timed section a caller may ask for, ms.
const MAX_MILLIS: u64 = 5_000;

fn parse(args: &[String]) -> Result<(BenchKind, Option<PathBuf>, u32, u64), String> {
    let kind = args
        .first()
        .and_then(|a| BenchKind::parse(a))
        .ok_or("usage: dyno-bench <cpu|memory|disk|gpu> [--dir <folder>] [--disk-mib <n>] [--millis <n>]")?;
    let (mut dir, mut mib, mut millis) = (None, DISK_DEFAULT_MIB, 1_000_u64);
    let mut it = args[1..].iter();
    while let Some(flag) = it.next() {
        let value = it.next().ok_or(format!("{flag} needs a value"))?;
        match flag.as_str() {
            "--dir" => dir = Some(PathBuf::from(value)),
            "--disk-mib" => mib = value.parse().map_err(|_| "--disk-mib must be a number".to_string())?,
            "--millis" => {
                millis = value
                    .parse::<u64>()
                    .map_err(|_| "--millis must be a number".to_string())?
                    .clamp(10, MAX_MILLIS)
            }
            other => return Err(format!("unknown option {other}")),
        }
    }
    Ok((kind, dir, mib, millis))
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let (kind, dir, mib, millis) = match parse(&args) {
        Ok(p) => p,
        Err(e) => {
            eprintln!("dyno-bench: {e}");
            return ExitCode::from(2);
        }
    };
    let settings = Settings {
        budget: Duration::from_millis(millis),
        dir: dir.as_deref(),
        disk_mib: mib,
    };
    let status = match run(kind, &settings) {
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
        let (k, d, m, ms) = parse(&a(&["disk", "--dir", "/x", "--disk-mib", "32", "--millis", "99999"])).unwrap();
        assert_eq!((k, d, m, ms), (BenchKind::Disk, Some(PathBuf::from("/x")), 32, MAX_MILLIS));
        assert!(parse(&a(&[])).is_err());
        assert!(parse(&a(&["nope"])).is_err());
        assert!(parse(&a(&["cpu", "--bogus", "1"])).is_err());
        assert!(parse(&a(&["cpu", "--millis"])).is_err());
        assert!(parse(&a(&["cpu", "--millis", "x"])).is_err());
        assert!(parse(&a(&["disk", "--disk-mib", "x"])).is_err());
    }
}
