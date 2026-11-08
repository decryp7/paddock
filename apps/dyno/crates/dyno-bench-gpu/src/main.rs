#![cfg_attr(test, allow(clippy::unwrap_used, clippy::expect_used))]
//! `dyno-bench-gpu [--list | --adapter <n>] [--smoke]`
//!
//! The GPU runner (ADR 0030 D2): its own process, so the large graphics stack and a driver that crashes or hangs stay out of
//! `dyno-bench`. It writes the same JSON lines (progress, a heartbeat from a thread of its own, one result last). `--list` prints
//! the adapters as one JSON line and exits; `--smoke` runs one repetition of tiny work for the mechanics tests.

use dyno_bench::emit::{Emitter, Heartbeat, Progress};
use dyno_bench_gpu::{list_adapters, run, Settings};
use dyno_core::bench::wire::{Line, HEARTBEAT_EVERY};
use dyno_core::bench::{BenchKind, BenchStatus};
use std::process::ExitCode;
use std::sync::Arc;

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.first().map(String::as_str) == Some("--version") {
        println!("dyno-bench-gpu {}", env!("CARGO_PKG_VERSION"));
        return ExitCode::SUCCESS;
    }
    if args.first().map(String::as_str) == Some("--list") {
        println!("{}", serde_json::to_string(&list_adapters()).unwrap_or_else(|_| "[]".into()));
        return ExitCode::SUCCESS;
    }
    let mut settings = Settings::default();
    let mut wait_go = false;
    let mut it = args.iter();
    while let Some(a) = it.next() {
        match a.as_str() {
            "--smoke" => settings.smoke = true,
            "--wait-go" => wait_go = true,
            "--adapter" => match it.next().and_then(|v| v.parse().ok()) {
                Some(n) => settings.adapter = Some(n),
                None => {
                    eprintln!("dyno-bench-gpu: --adapter needs a number");
                    return ExitCode::from(2);
                }
            },
            other => {
                eprintln!("dyno-bench-gpu: unknown option {other}; usage: dyno-bench-gpu [--list | --adapter <n>] [--smoke]");
                return ExitCode::from(2);
            }
        }
    }
    let emitter = Arc::new(Emitter::new(std::io::stdout()));
    let beats = emitter.clone();
    let heartbeat = Heartbeat::start(HEARTBEAT_EVERY, move || {
        beats.emit(&Line::Heartbeat).is_ok() || std::process::exit(4)
    });
    if wait_go {
        // The agent writes down the unit this runs in, then says go (Linux); a closed input without it is a refusal.
        let mut line = String::new();
        let said = std::io::stdin().read_line(&mut line).is_ok() && line.trim() == "go";
        if !said {
            drop(heartbeat);
            let status = BenchStatus::Failed {
                reason: "The agent did not tell the graphics runner to go.".into(),
            };
            return match emitter.emit(&Line::Result { status }) {
                Ok(()) => ExitCode::SUCCESS,
                Err(_) => ExitCode::FAILURE,
            };
        }
    }
    let progress: &dyn Progress = &*emitter;
    let status = match run(&settings, progress) {
        Ok(metrics) => BenchStatus::Done { metrics },
        Err(dyno_bench_gpu::Outcome::Unavailable(reason)) => BenchStatus::Unavailable { reason },
        Err(dyno_bench_gpu::Outcome::Failed(reason)) => BenchStatus::Failed { reason },
    };
    drop(heartbeat);
    let _ = BenchKind::Gpu;
    match emitter.emit(&Line::Result { status }) {
        Ok(()) => ExitCode::SUCCESS,
        Err(_) => ExitCode::FAILURE,
    }
}
