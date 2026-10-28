# Performance record

Pitwall's benchmarks, budgets and recorded results. How to run a benchmark reproducibly and the results template are in
the family guide, [docs/benchmarks.md](../../../docs/benchmarks.md). Record every run there quoted (newest first) here.

The Benchmarks page (`app/src/app/features/benchmarks`, `crates/pitwall-core/src/benchmarks.rs`, `e2e/benchmarks.e2e.ts`)
is a product feature: it benchmarks the user's models. It is not a test of Pitwall's own speed.

## Benchmarks

| Benchmark | Measures | Command | Budget asserted | CI |
|---|---|---|---|---|
| `app/angular.json` budgets | The initial bundle | `cd app && npx ng build` | initial warn 1.2 MB / error 2 MB; component style warn 12 kB / error 24 kB | `ci.yml` Pitwall leg |
| `crates/pitwall-server/tests/llama_e2e.rs` | A real llama-server round trip (a hang guard, not a budget) | `PITWALL_LLAMA_E2E=1 cargo test -p pitwall-server --test llama_e2e` | none | No |

## Budgets

[ADR 0006](adr/0006-memory-policy.md) (memory policy) and [ADR 0005](adr/0005-queue-and-scheduling.md) (queue) set
behaviour, not measured budgets. Rule 16's budgets (start-up, request latency, queueing, memory) are not set.

## Gaps

- No start-up, request latency, queueing, idle CPU or server memory benchmark exists.
- The model-load memory Debrief depends on (about 7–8 GB for the 9B model, estimated in Debrief's
  [cpu-memory research](../../debrief/docs/research/2026-10-28-cpu-memory.md)) has never been measured.
- C29 window weight is not configured.

## Results (newest first)

None recorded beyond CI timings ([docs/research/2026-10-ci-timings.md](../../../docs/research/2026-10-ci-timings.md),
2026-10-24: `build.yml` with bundles, median 20m38s).
