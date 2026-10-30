//! The cost of the start times in a process-table snapshot (ADR 0033, "Parallelism and cost"). Run with
//! `cargo run --release -p paddock-proc --example snapshot_cost`; results are recorded in `docs/benchmarks.md`.
use std::time::Instant;

fn main() {
    const ROUNDS: u32 = 50;
    let pids: Vec<u32> = paddock_proc::snapshot().keys().copied().collect();
    let t = Instant::now();
    for _ in 0..ROUNDS {
        std::hint::black_box(paddock_proc::snapshot());
    }
    let whole = t.elapsed() / ROUNDS;
    let t = Instant::now();
    for _ in 0..ROUNDS {
        for &p in &pids {
            std::hint::black_box(paddock_proc::ProcessId::of(p));
        }
    }
    let starts = t.elapsed() / ROUNDS;
    println!(
        "processes {}, snapshot {whole:?}, of which start times {starts:?}",
        pids.len()
    );
}
