# Performance record

Sighter's benchmarks, budgets and recorded results. How to run a benchmark reproducibly and the results template are in
the family guide, [docs/benchmarks.md](../../../docs/benchmarks.md). Record every run there quoted (newest first) here.
The pure core (`crates/sighter-core`) has two benchmark examples, `bench_core` and `bench_hud` (clock and statistics from `paddock-bench`, memory from `paddock-proc`); no model is benchmarked yet. Both refuse to run above a 1-minute load of 25 and take the environment variables named in their header (`SIGHTER_BENCH_LOAD` is required).

## Budgets (proposed; the constants are in `sighter_core::bench::budgets`, read by benchmarks only)

From the [design review](review/2026-10-design-review.md) §1.7 (2026-10-24):

| Measure | Budget |
|---|---|
| Frame decode | p95 ≤ 50 ms on loopback, ≤ 100 ms on a LAN |
| Locate | p50 ≤ 80 ms and p95 ≤ 250 ms on a GPU; p95 ≤ 600 ms on a CPU |
| Action | p95 ≤ 400 ms |
| HUD | ≤ 2 ms per frame, ≤ 5% |
| 8 targets | ≤ 2 × the locate p95 |
| Model cold load | ≤ 1.5 s |
| Server memory | ≤ 1.5 GB RSS with 3 models (rule 26: measure footprint, PSS or private working set) |
| Studio | 60 fps with 500 boxes; next item ≤ 50 ms |

The [proposal](proposal.md) §7 adds per-stage targets: cache < 5 ms, template < 30 ms, OCR < 100 ms for a region, and a
detector < 50 ms on a GPU at 640 px.

## Gaps

The locate, cold-load and memory budgets above need a model and synthetic captures; only the HUD budgets are measured so far (below). The
[detector research](../../../docs/research/2026-10-sighter-detector.md) quotes only third-party figures. Nothing was
measured on our data.

## Results (newest first)

### 2026-10-29, HUD overhead (trace bus, overlay items, sidecar text) (sighter/core-wave-0)

- **Commit:** 9e2804845
- **Machine:** Mac mini M4 Pro, 14 cores, 64 GB, macOS 27.2
- **Load:** 1-minute load 18.54 at start (above the family guide's quiet threshold of about 12, with other agents' builds running: indication only)
- **Build:** release; no model, no UI rendering
- **Corpus:** synthetic: fixed-seed boxes (50 per frame, 300 per step) and 12 trace events per step
- **Command:** `SIGHTER_BENCH_LOAD=<load> nice -n 10 cargo run --release -p sighter-core --example bench_hud`
- **Runs:** frame: 50 warm-up and 400 timed runs, nearest-rank p95; step: 6 interleaved A/B pairs of 100 timed runs after 20 warm-up, median (paddock-bench `summarize`)
- **Numbers:**

  | Measure | Value | Budget | Verdict |
  |---|---|---|---|
  | HUD work per frame, 50 detections, 2 viewers, p95 | 27.29 us | HUD per frame <= 2.00 ms | within |
  | Step median, trace off | 25.04 us | none | indication |
  | Step median, trace on | 25.62 us (+2.3 %) | change <= 5 % | within |

- **Memory measure:** physical footprint at the end of the run 4.2 MB (paddock-proc::process_memory)
- **Notes:** Pure-core cost only: drawing, the video sidecar write and the UI are not measured. Indicative unless the load is below about 12. Wall time 59.54 ms.

### 2026-10-29, sighter-core geometry, NMS and tile planning (sighter/core-wave-0)

- **Commit:** 9e2804845
- **Machine:** Mac mini M4 Pro, 14 cores, 64 GB, macOS 27.2
- **Load:** 1-minute load 18.54 at start (above the family guide's quiet threshold of about 12, with other agents' builds running: indication only)
- **Build:** release; no model, no allocator override
- **Corpus:** synthetic: 500 boxes from a fixed-seed generator in the example; a 3840 x 2160 grid
- **Command:** `SIGHTER_BENCH_LOAD=<load> nice -n 10 cargo run --release -p sighter-core --example bench_core`
- **Runs:** single series, 20 warm-up and 200 timed runs; median and nearest-rank p95 (paddock-bench `summarize`)
- **Numbers:**

  | Measure | Value | Budget | Verdict |
  |---|---|---|---|
  | NMS, 500 boxes, median | 62.54 us | none | indication |
  | NMS, 500 boxes, p95 | 69.75 us | none | indication |
  | Tile grid, 3840 x 2160, median | 84 ns | none | indication |
  | Chain map, 500 boxes, median | 292 ns | none | indication |

- **Memory measure:** physical footprint at the end of the run 3.9 MB (paddock-proc::process_memory)
- **Notes:** wall time 23.58 ms; no budget applies to these rows; indicative unless the load is below about 12

