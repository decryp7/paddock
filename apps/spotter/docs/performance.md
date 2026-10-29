# Performance record

Spotter's benchmarks, budgets and recorded results. How to run a benchmark reproducibly and the results template are in
the family guide, [docs/benchmarks.md](../../../docs/benchmarks.md). Record every run there quoted (newest first) here.

## Benchmarks

| Benchmark | Measures | Command | Budget asserted | CI |
|---|---|---|---|---|
| `app/angular.json` budgets | The initial bundle | `cd app && npx ng build` | initial warn 1 MB / error 2 MB; component style warn 8 kB / error 16 kB | `ci.yml` Spotter leg |

The elapsed-time asserts in `crates/spotter-engine/tests` (`engine_robustness.rs` < 2 s, `long_operations.rs` < 10 s,
`mail_delivery.rs` < 5/10 s, `fetch_http.rs` ≥ 280 ms between requests to one host) and in
`crates/spotter-render/tests` are timeout, cancel and politeness checks, not performance budgets.

## Gaps

- No benchmark exists for check throughput (checks per minute per host), idle CPU, or the memory of the managed browsers.
  The lessons record that idle Chrome on Linux was measured as about 17 GB against about 0.4 GB in use (bea504150), with
  a 2 GiB watchdog. A Windows cold start of 8 Edge browsers took about 20 s (97a0f34d4). Neither is a repeatable
  benchmark.
- C29 window weight is not configured.

## Results (newest first)

None recorded beyond CI timings ([docs/research/2026-10-ci-timings.md](../../../docs/research/2026-10-ci-timings.md),
2026-10-24: `build.yml` push, median 16m25s).
