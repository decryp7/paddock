//! Memory (ADR 0030 section 1): STREAM-style Copy, Scale, Add and Triad on `f64` arrays far larger than the caches, on one thread and
//! on all threads (each a contiguous slice), and a latency sweep from 16 KiB to 512 MiB over a random cyclic chain with one node per
//! 128-byte line, walked with dependent loads. Labelled "STREAM-style", never "STREAM results". Normal 4 KiB pages, no huge pages.

pub mod cache;

use crate::emit::{gap_label, kind_phase, pass_done_label, pass_label, Progress, WARMING_UP};
use crate::repeat::{repeated, Pace, Repeats, Step};
use dyno_core::bench::memory::{array_bytes, stream_bytes, sweep, sweep_id, Stream, HEADLINE_BYTES, LINE};
use dyno_core::bench::BenchMetric;
use paddock_bench::Clock;
use std::hint::black_box;
use std::time::{Duration, Instant};

const SCALAR: f64 = 3.0;

/// One pass of `kernel` over three equal arrays.
pub fn pass(kernel: Stream, a: &mut [f64], b: &mut [f64], c: &mut [f64]) {
    let n = a.len().min(b.len()).min(c.len());
    let (a, b, c) = (&mut a[..n], &mut b[..n], &mut c[..n]);
    match kernel {
        Stream::Copy => c.copy_from_slice(a),
        Stream::Scale => b.iter_mut().zip(c.iter()).for_each(|(b, c)| *b = SCALAR * *c),
        Stream::Add => c.iter_mut().zip(a.iter().zip(b.iter())).for_each(|(c, (a, b))| *c = *a + *b),
        Stream::Triad => a
            .iter_mut()
            .zip(b.iter().zip(c.iter()))
            .for_each(|(a, (b, c))| *a = *b + SCALAR * *c),
    }
}

/// The three arrays, filled with the values STREAM starts from.
pub struct Arrays {
    a: Vec<f64>,
    b: Vec<f64>,
    c: Vec<f64>,
}

impl Arrays {
    /// Arrays of `bytes` each.
    pub fn new(bytes: u64) -> Arrays {
        let n = (bytes / 8) as usize;
        Arrays {
            a: vec![1.0; n],
            b: vec![2.0; n],
            c: vec![0.0; n],
        }
    }

    /// Elements per array.
    pub fn len(&self) -> usize {
        self.a.len()
    }

    /// Whether the arrays are empty.
    pub fn is_empty(&self) -> bool {
        self.a.is_empty()
    }

    /// Runs `kernel` for about `budget` on `threads` threads (each a contiguous slice) and returns GB/s counted as STREAM counts.
    pub fn gbps(&mut self, kernel: Stream, threads: usize, budget: Duration) -> f64 {
        let threads = threads.clamp(1, self.len().max(1));
        let per = self.len().div_ceil(threads).max(1);
        let start = Instant::now();
        let passes: Vec<(u64, usize)> = std::thread::scope(|s| {
            let handles: Vec<_> = self
                .a
                .chunks_mut(per)
                .zip(self.b.chunks_mut(per))
                .zip(self.c.chunks_mut(per))
                .map(|((a, b), c)| {
                    s.spawn(move || {
                        let (mut n, t0) = (0_u64, Instant::now());
                        while t0.elapsed() < budget || n == 0 {
                            pass(kernel, black_box(&mut *a), black_box(&mut *b), black_box(&mut *c));
                            n += 1;
                        }
                        (n, a.len())
                    })
                })
                .collect();
            handles.into_iter().filter_map(|h| h.join().ok()).collect()
        });
        let secs = start.elapsed().as_secs_f64().max(1e-9);
        let bytes: u64 = passes.iter().map(|(n, len)| n * stream_bytes(kernel, *len as u64)).sum();
        black_box((&self.a, &self.b, &self.c));
        bytes as f64 / secs / 1e9
    }
}

/// A single random cycle over `n` nodes (Sattolo's algorithm, fixed seed), as `next[i]`.
pub fn random_cycle(n: usize) -> Vec<u32> {
    let mut next: Vec<u32> = (0..n as u32).collect();
    let mut state = 0x2545_F491_4F6C_DD1D_u64;
    for i in (1..n).rev() {
        state = crate::cpu::mix(state);
        let j = (state % i as u64) as usize;
        next.swap(i, j);
    }
    next
}

/// A chain over `bytes` of memory: one node per [`LINE`] bytes, each holding the slot of the next node, in `buf` (at least
/// `bytes / 4` words). Slots are `LINE / 4` words apart, so every hop touches a new line.
pub fn build_chain(buf: &mut [u32], bytes: u64) -> usize {
    let nodes = (bytes / LINE) as usize;
    let stride = (LINE / 4) as usize;
    let next = random_cycle(nodes.max(2));
    for (i, n) in next.iter().enumerate() {
        buf[i * stride] = *n * stride as u32;
    }
    nodes.max(2)
}

/// Follows the chain from slot 0 for about `budget`; nanoseconds per dependent load.
pub fn walk_ns(buf: &[u32], budget: Duration) -> f64 {
    let start = Instant::now();
    let (mut at, mut loads) = (0_u32, 0_u64);
    while start.elapsed() < budget || loads == 0 {
        for _ in 0..50_000 {
            at = buf[at as usize];
        }
        loads += 50_000;
    }
    black_box(at);
    start.elapsed().as_secs_f64() * 1e9 / loads as f64
}

/// What the test needs besides the budget and the repetitions.
#[derive(Debug, Clone, Copy, Default)]
pub struct Memory {
    /// The last-level caches together, when known.
    pub caches: Option<u64>,
    /// Memory available at the start, bytes, when known.
    pub available: Option<u64>,
}

/// The memory metrics, each from every repetition. Refused with the reason when the machine has too little free memory.
pub fn run(
    m: Memory,
    budget: Duration,
    repeats: Repeats,
    clock: &dyn Clock,
    pace: &dyn Pace,
    progress: &dyn Progress,
) -> Result<Vec<BenchMetric>, String> {
    let bytes = array_bytes(m.caches, m.available)?;
    let mut arrays = Arrays::new(bytes);
    let threads = std::thread::available_parallelism().map_or(1, |n| n.get()).min(256);
    // The latency chain's top size stays within the same eighth of the free memory.
    let top = m.available.map_or(256 * 1024 * 1024, |a| a / 8);
    let points: Vec<u64> = sweep().into_iter().filter(|p| *p <= top).collect();
    let mut chain = vec![0_u32; (points.last().copied().unwrap_or(LINE * 2) / 4) as usize];
    let sweep_budget = budget / 5;
    let per_pass = Stream::ALL.len() * 2 + points.len().div_ceil(5);
    let _ = per_pass;
    let reps = repeated(
        clock,
        pace,
        repeats,
        |step| {
            let (b, prefix, done, of, warm) = match step {
                Step::Warmup(d) => (d / 8, WARMING_UP.to_string(), 0, repeats.reps, true),
                Step::Rep { n, of } => (budget, pass_label(n, of, "").trim_end_matches(" · ").to_string(), n - 1, of, false),
            };
            let label = |s: &str| format!("{prefix} · {s}");
            let mut stream = Vec::new();
            for k in Stream::ALL {
                progress.report(kind_phase("memory", &label(&format!("{} · one thread", k.title())), done, of));
                let single = arrays.gbps(k, 1, b);
                progress.report(kind_phase(
                    "memory",
                    &label(&format!("{} · all {threads} threads", k.title())),
                    done,
                    of,
                ));
                stream.push((single, arrays.gbps(k, threads, b)));
            }
            let mut lat = Vec::new();
            for p in &points {
                progress.report(kind_phase("memory", &label(&format!("Latency · {} KiB", p / 1024)), done, of));
                let nodes = build_chain(&mut chain, *p);
                let words = nodes * (LINE as usize / 4);
                lat.push(walk_ns(&chain[..words], if warm { b } else { sweep_budget }));
            }
            if let Step::Rep { n, of } = step {
                progress.report(kind_phase("memory", &pass_done_label(n, of), n, of));
            }
            (stream, lat)
        },
        |done, of| progress.report(kind_phase("memory", &gap_label(done, of), done, of)),
    );
    let mut out = Vec::new();
    for (i, k) in Stream::ALL.iter().enumerate() {
        let single: Vec<f64> = reps.iter().map(|(s, _)| s[i].0).collect();
        let multi: Vec<f64> = reps.iter().map(|(s, _)| s[i].1).collect();
        out.push(BenchMetric::measured(
            &format!("mem-stream-{}-single", k.id()),
            &format!("{} (STREAM-style) · one thread", k.title()),
            &single,
            "GB/s",
            true,
        ));
        out.push(BenchMetric::measured(
            &format!("mem-stream-{}-multi", k.id()),
            &format!("{} (STREAM-style) · all {threads} threads", k.title()),
            &multi,
            "GB/s",
            true,
        ));
    }
    for (i, p) in points.iter().enumerate() {
        let v: Vec<f64> = reps.iter().map(|(_, l)| l[i]).collect();
        out.push(BenchMetric::measured(
            &sweep_id(*p),
            &format!("Latency at {}", size_words(*p)),
            &v,
            "ns",
            false,
        ));
    }
    // The headline: main memory, the 256 MiB point (or the largest the free memory allowed).
    let headline = points.iter().rposition(|p| *p <= HEADLINE_BYTES);
    if let Some(i) = headline {
        let v: Vec<f64> = reps.iter().map(|(_, l)| l[i]).collect();
        out.push(BenchMetric::measured("mem-latency", "Main memory latency", &v, "ns", false));
    }
    Ok(out)
}

/// 16 KiB, 512 KiB, 4 MiB.
pub fn size_words(bytes: u64) -> String {
    if bytes >= 1024 * 1024 {
        format!("{} MiB", bytes / (1024 * 1024))
    } else {
        format!("{} KiB", bytes / 1024)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use dyno_core::bench::memory::MIB;

    #[test]
    fn the_four_kernels_compute_what_stream_computes() {
        let (mut a, mut b, mut c) = (vec![1.0; 8], vec![2.0; 8], vec![0.0; 8]);
        pass(Stream::Copy, &mut a, &mut b, &mut c);
        assert_eq!(c, vec![1.0; 8]);
        pass(Stream::Scale, &mut a, &mut b, &mut c);
        assert_eq!(b, vec![3.0; 8], "b = 3 * c");
        pass(Stream::Add, &mut a, &mut b, &mut c);
        assert_eq!(c, vec![4.0; 8], "c = a + b");
        pass(Stream::Triad, &mut a, &mut b, &mut c);
        assert_eq!(a, vec![15.0; 8], "a = b + 3 * c");
    }

    #[test]
    fn unequal_lengths_use_the_shortest_and_never_panic() {
        let (mut a, mut b, mut c) = (vec![1.0; 8], vec![2.0; 3], vec![0.0; 5]);
        pass(Stream::Add, &mut a, &mut b, &mut c);
        assert_eq!(&c[..3], &[3.0; 3]);
        assert_eq!(&c[3..], &[0.0; 2]);
    }

    #[test]
    fn threads_cover_the_arrays_in_contiguous_slices_and_count_stream_bytes() {
        let mut arrays = Arrays::new(MIB);
        assert_eq!(arrays.len(), 131_072);
        assert!(arrays.gbps(Stream::Triad, 1, Duration::from_millis(5)) > 0.0);
        assert!(arrays.gbps(Stream::Copy, 4, Duration::from_millis(5)) > 0.0);
        assert!(
            arrays.gbps(Stream::Add, 1000, Duration::from_millis(5)) > 0.0,
            "more threads than elements is clamped"
        );
        // Whatever the thread count, every element took part: Copy sets all of c to a.
        let mut a = Arrays::new(MIB);
        a.gbps(Stream::Copy, 3, Duration::from_millis(1));
        assert!(a.c.iter().all(|x| *x == 1.0));
    }

    #[test]
    fn the_chain_is_one_cycle_through_every_line() {
        let bytes = 64 * 1024;
        let mut buf = vec![0_u32; (bytes / 4) as usize];
        let nodes = build_chain(&mut buf, bytes);
        assert_eq!(nodes, 512);
        let (mut at, mut seen) = (0_usize, std::collections::HashSet::new());
        for _ in 0..nodes {
            assert!(seen.insert(at), "a node was visited twice");
            assert_eq!(at % 32, 0, "every hop lands on a 128-byte line");
            at = buf[at] as usize;
        }
        assert_eq!(at, 0, "it came back to the start after one lap");
        assert!(walk_ns(&buf, Duration::from_millis(5)) > 0.0);
    }

    #[test]
    fn a_run_reports_stream_and_the_sweep_with_a_headline_and_refuses_without_room() {
        struct Quiet;
        impl Progress for Quiet {
            fn report(&self, _: dyno_core::bench::wire::PhaseProgress) {}
        }
        let r = Repeats {
            warmup: Duration::ZERO,
            reps: 2,
            gap: Duration::ZERO,
        };
        let m = Memory {
            caches: Some(MIB),
            available: Some(16 * 1024 * MIB),
        };
        let out = run(
            m,
            Duration::from_millis(5),
            r,
            &paddock_bench::SystemClock::new(),
            &crate::repeat::SleepPace,
            &Quiet,
        )
        .unwrap();
        assert_eq!(out.iter().filter(|x| x.id.starts_with("mem-stream-")).count(), 8);
        assert!(out.iter().any(|x| x.id == "mem-latency-16k") && out.iter().any(|x| x.id == "mem-latency-512m"));
        assert!(out
            .iter()
            .any(|x| x.id == "mem-latency" && x.samples.len() == 2 && !x.higher_is_better));
        assert!(out.iter().all(|x| x.value > 0.0));
        let tight = Memory {
            caches: Some(512 * MIB),
            available: Some(1024 * MIB),
        };
        let e = run(
            tight,
            Duration::from_millis(5),
            r,
            &paddock_bench::SystemClock::new(),
            &crate::repeat::SleepPace,
            &Quiet,
        )
        .unwrap_err();
        assert!(e.contains("Not enough free memory"));
    }
}
