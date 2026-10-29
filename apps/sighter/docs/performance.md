# Performance record

Sighter's benchmarks, budgets and recorded results. How to run a benchmark reproducibly and the results template are in
the family guide, [docs/benchmarks.md](../../../docs/benchmarks.md). Record every run there quoted (newest first) here.
Sighter has no code in the tree yet, so it has no benchmarks and no results.

## Budgets (proposed, no test)

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

Every budget above needs a benchmark when the code lands, on synthetic captures. The
[detector research](../../../docs/research/2026-10-sighter-detector.md) quotes only third-party figures. Nothing was
measured on our data.

## Results (newest first)

### 2026-10-29, HUD overhead (trace bus, overlay items, sidecar text) (sighter/core-wave-0)

- **Commit:** 2b7071b5f
- **Machine:** Mac mini M4 Pro, 14 cores, 64 GB, macOS 27.2
- **Load:** 1-minute load 22.31 at start
- **Build:** release; no model, no UI rendering
- **Corpus:** synthetic: fixed-seed boxes (50 per frame, 300 per step) and 12 trace events per step
- **Command:** `cargo run --release -p sighter-core --example bench_hud`
- **Runs:** frame: 50 warm-up and 400 timed runs, p95; step: 6 interleaved A/B pairs of 100 timed runs after 20 warm-up, p50
- **Numbers:**

  | Measure | Value | Budget | Verdict |
  |---|---|---|---|
  | HUD work per frame, 50 detections, 2 viewers | 28.04 us | HUD per frame p95 <= 2.00 ms | within |
  | Step p50, trace off | 25.33 us | none (reference) | reference |
  | Step p50, trace on | 25.58 us (+1.0 %) | change <= 5 % | within |

- **Memory measure:** not measured
- **Notes:** Pure-core cost only: drawing, the video sidecar write and the UI are not measured. Indicative unless the load is below about 12.

### 2026-10-29, sighter-core geometry, NMS and tile planning (sighter/core-wave-0)

- **Commit:** 2b7071b5f
- **Machine:** Mac mini M4 Pro, 14 cores, 64 GB, macOS 27.2
- **Load:** 1-minute load 22.31 at start (other agents' builds running; an indication only)
- **Build:** release; no model, no allocator override
- **Corpus:** synthetic: 500 boxes from a fixed-seed generator in the example; a 3840 x 2160 grid
- **Command:** `cargo run --release -p sighter-core --example bench_core`
- **Runs:** single series, 20 warm-up and 200 timed runs; median and p95 (sorted[floor(p x (n - 1))])
- **Numbers:**

  | Measure | Value | Budget | Verdict |
  |---|---|---|---|
  | NMS, 500 boxes | 65.29 us | locate target CPU p95 <= 600.00 ms | within |
  | Tile grid, 3840 x 2160 | 125 ns | HUD frame p95 <= 2.00 ms | within |
  | Chain map, 500 boxes | 334 ns | HUD frame p95 <= 2.00 ms | within |

- **Memory measure:** not measured
- **Notes:** wall time 13.94 ms; indicative only unless the load is below about 12


None.
