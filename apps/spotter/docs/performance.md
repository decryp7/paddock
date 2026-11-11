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

### 2026-11-11: first run of the test browser on macOS

Chrome for Testing 156.0.8078.4 (Playwright cache copy), freshly copied to a new folder on the Mac mini (macOS, load about 7),
`--remote-debugging-port=0 --headless=new` until `DevToolsActivePort` exists, by hand (`/bin/bash` timing loop), one run per
fresh copy:

| What | Seconds |
|---|---|
| first start of a fresh copy | 7.0, 4.2, 27.6 |
| `--version` alone on a fresh copy, then the first start | 24.3 then 0.6; 21.1 then 0.7 |
| a headless `--dump-dom` run on a fresh copy, then the first start | 31.4 then 0.35 |
| every later start of a copy already run once | 0.35 to 0.7 |
| real_browser test on a fresh copy with `launch::warm_up` | first run 42.06, rest of the test 0.8 |

During the first 34 s of one cold start the browser process had used 0:00.00 CPU and had no child process (XprotectService and
syspolicyd busy); the OS holds the first run of a new bundle. Range: 4.2 to 42.06 s, far more on a loaded CI host (CI runs 7918,
7929 and 7932, the first test of the binary each time).

None recorded beyond CI timings ([docs/research/2026-10-ci-timings.md](../../../docs/research/2026-10-ci-timings.md),
2026-10-24: `build.yml` push, median 16m25s).
