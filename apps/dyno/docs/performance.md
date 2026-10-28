# Performance record

Dyno's benchmarks, budgets and recorded results. How to run a benchmark reproducibly and the results template are in the
family guide, [docs/benchmarks.md](../../../docs/benchmarks.md). Record every run there quoted (newest first) here.

Dyno has two things called "bench":
- **`dyno-bench` and the Benchmarks page** (`crates/dyno-bench`, `app/src/app/features/benchmarks`,
  [ADR 0015](adr/0015-benchmarks-network-and-ip-lookup.md)) are a product feature. They measure the user's machine (CPU,
  memory, disk). `crates/dyno-bench/tests/process.rs` tests the feature's states, not its speed.
- **`dyno-agent bench`** measures Dyno itself and is the benchmark this file covers.

## Benchmarks

| Benchmark | Measures | Command | Corpus | Budget asserted | CI | Results |
|---|---|---|---|---|---|---|
| `dyno-agent bench [seconds] [none\|summary\|processes]` (`crates/dyno-agent/src/cli.rs`) | The agent's own CPU (share of one core) and memory with nobody watching, a summary subscriber or a processes subscriber; JSON output | `cargo run --release -p dyno-agent -- bench 25 processes` | The live machine (read-only) | none | No. `scripts/check-bundle.sh` (`dyno-release.yml`) runs `dyno-agent once`, a smoke test with no budget | below; [architecture.md](architecture.md#dynos-own-overhead) |
| `app/angular.json` budgets | The initial bundle | `cd app && npx ng build` | — | initial warn 900 kB / error 1.5 MB; component style warn 16 kB / error 32 kB | `ci.yml` Dyno leg | — |

## Budgets

| Budget | Value | Owner | Enforced by |
|---|---|---|---|
| Dyno's own cost | about 1% of the machine; the sampler's `sampling::CPU_BUDGET` = 1.0% of one core | [ADR 0002](adr/0002-sampling-architecture.md) | Adaptive intervals stretch at run time when the budget is exceeded; **no test** |
| Idle process | ≤ 0.5% of a core, ≤ 1 wake-up/s | family [ADR 0024](../../../docs/adr/0024-background-work-contract.md) D7 | nothing |

## Gaps

- [Roadmap](roadmap.md) G13: the overhead was measured once, under heavy load, on one machine. There is no CI artifact
  per OS and no regression check. Run `bench` on release builds on a quiet runner per OS, record the load next to the
  numbers, and fail on a regression (through `paddock-bench`).
- The lessons ask to "put the poll cost in the benchmark budgets" (a068435). No benchmark does this yet.
- Memory is RSS, not footprint or PSS (rule 26).

## Results (newest first)

### Date not stated, the agent's own overhead

- **Commit:** not stated
- **Machine:** Mac mini M4 Pro, 14 cores
- **Load:** 1-minute load about 55–85, so the numbers are upper bounds
- **Build:** not stated
- **Command:** `dyno-agent bench 25 none|summary|processes`
- **Numbers:**

  | Scenario | CPU (one core) | Resident memory |
  |---|---|---|
  | Idle, nobody watching | about 2.0% | about 24 MB |
  | Summary subscriber | about 2.7% | about 24 MB |
  | Processes subscriber | about 3.2% | about 24 MB |

- **Notes:** 2% of one core on 14 cores is about 0.14% of the machine. A quiet re-run on a release build on each OS is
  needed. Source: [architecture.md](architecture.md#dynos-own-overhead).
