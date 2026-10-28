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

None.
