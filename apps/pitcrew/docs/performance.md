# Performance record

Pitcrew's benchmarks, budgets and recorded results. How to run a benchmark reproducibly and the results template are in
the family guide, [docs/benchmarks.md](../../../docs/benchmarks.md). Record every run there quoted (newest first) here.

## Benchmarks

| Benchmark | Measures | Command | Budget asserted | CI |
|---|---|---|---|---|
| `app/angular.json` budgets | The initial bundle | `cd app && npx ng build` | initial warn 1 MB / error 2 MB; component style warn 8 kB / error 16 kB | `ci.yml` Pitcrew leg |
| `docs/research/2026-10-autoscale-sim.py` (branch `docs/pitcrew-autoscale`, not on main) | Simulated runner wait and slot-hours for static and auto-scaled runners | `python3 -I 2026-10-autoscale-sim.py [contention]` | none | No |

## Gaps

- No benchmark of the runner manager's own idle CPU or memory, or of job dispatch latency.
- ADR 0021 WP-0 (contention with two concurrent jobs on one host) is pending.
- CI and sccache measurements live in the family research notes ([ci-timings](../../../docs/research/2026-10-ci-timings.md),
  [monorepo spike](../../../docs/research/2026-10-monorepo-spike.md)). `development.md` points to "the garage README for
  the measurements", which is outside this repository.

## Results (newest first)

### 2026-10-28, auto-scale simulation (branch `docs/pitcrew-autoscale`, 9004a4dae, ADR 0021 proposed)

- **Machine, load:** not relevant (a simulation). Idle runner on the Mac: RSS 37 MB, 0.0% CPU (`ps`, indicative only).
  The Mac's load was 150–240 on the evening of 2026-10-22.
- **Corpus:** synthetic. 8 simulated hours, a push about every 10 min, 1–8 jobs of about 6 min each (lognormal), 30
  seeds, 5 s steps.
- **Numbers:**

  | Strategy | Mean wait | p95 wait | Slot-hours | Start/stop actions |
  |---|---|---|---|---|
  | Static, 1 slot (today) | 171 min | 326 min | 9.0 | 0 |
  | Static, 4 slots | 9.3 min | 26 min | 36.0 | 0 |
  | Auto 1–4, hysteresis | 9.7 min | 26.5 min | 29.2 | 40 |
  | Contention 0.25, auto with hysteresis | 59 min | 117 min | 34.1 | 11 |

- **Notes:** the no-contention rows are an upper bound. The gain falls from 18× to 3× at a 25% slowdown per extra job.

### 2026-10-24, CI timings (before the monorepo)

`build.yml` with bundles: median 32m16s (p90 54m34s). Push: median 15m37s. Source:
[docs/research/2026-10-ci-timings.md](../../../docs/research/2026-10-ci-timings.md).
