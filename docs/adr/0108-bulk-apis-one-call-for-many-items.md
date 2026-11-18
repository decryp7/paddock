# 0108. Bulk APIs: one call for many items, one long-lived process for many reads, a counted budget per command

- Status: Accepted 2026-11-18 (user, relayed by the coordinator): the recommended set (45.25 agent-days); each release is
  gated on its before-release work packages, in the release order; the guard A + C + D; the recommended option on every
  sub-decision. Growth of more than 20% goes back to the user (ER-37).
- Date: 2026-11-18
- Related: engineering-rules PRINCIPLES "Bulk APIs" and ER-20 (commit 7fcacee, 2026-11-18); ER-21 / rule 40 (a recurring
  class gets a redesign); ER-22 / rule 42 (share first); ER-33 (positive controls); ER-37 / rule 76 (common practice and
  cost); ER-41 (new checks start report-only); rule 55 (`docs/known-copies.toml`); ADR 0095 (one cheap repository copy for
  tests; its D2 graph cache is accepted and not built); rule 85 (this ADR's rule).
- Number: 0108. On 2026-11-18 the highest ADR on any `origin` branch was 0107 (`origin/docs/adr-test-env-per-test`;
  `git ls-tree docs/adr/` over every remote branch).
- Inventory: [docs/research/2026-11-18-bulk-api-inventory.md](../research/2026-11-18-bulk-api-inventory.md) (committed with
  this ADR; every finding with its `file:line`, N, cost and bulk shape). It is this ADR's appendix.

Evidence heads read (ADR 0091), re-read before the commit: `origin/main` f5f08f71e (the inventory's line numbers are at this
commit), `origin/train/2026-11-18-15` f9fb1e581 (this branch's base; the claims G1 and G2 rest on were re-read there:
`crates/paddock-proc/src/run.rs:208-225` still sets `Stdio::null()`, `crates/paddock-repo-graph/src/history.rs:139-143` still
runs `git show` per path, `tools/paddock-dev/src/merge_check/mod.rs:35` still says "Not done: `git cat-file --batch`"),
`origin/perf/git-heavy-tests` 51a5a617e and `origin/perf/git-heavy-tests-2` 2ad2e9fa5 (scope only; `crates/paddock-git` is not
pushed there yet). Shipped code: no claim is made about a tag.

## Context: the class

**Work over N items makes N calls, N processes or N round trips where one bulk call or one long-lived process would do.**
It shows in every layer: an Angular store awaits a Tauri command per item, a Tauri command asks a service or worker per
item, a reader starts a tool per item, a writer commits per row, a dev tool starts git per file. Each instance is cheap
alone and slow in aggregate, and the cost differs by OS: a process start is about 6 ms on the Mac and about 45 ms on the
Windows runner (ER-20's measurement, train 14), 0.11 to 0.15 s per git child under test load there (docs/benchmarks.md,
runs 8110 and 8115).

Earlier instances, each fixed one at a time (commits checked on `origin/main` unless noted):

| # | Where | Instance | Fixed by |
|---|---|---|---|
| 1 | Debrief store | the indexer marked each segment with its own autocommit `UPDATE` | bda5389d8 (2026-10-29), one statement per step |
| 2 | paddock-dev golden test | `cargo metadata` once per sample and workspace (ADR 0095 row 1, 828 runs as cited there) | b804ac78a (2026-11-09) |
| 3 | paddock-dev contract tests | the app graph read once per test (ADR 0095 row 2) | 54f8ab5ce (2026-11-09) |
| 4 | paddock-dev queue tests | 6953 git children, then 3576 (docs/benchmarks.md, 2026-11-15, fix/queue-tests-fast) | the queue's one fetch and one read per commit |
| 5 | paddock-dev merge check | 33 git starts for a two-commit merge, 7 of them repeated reads | a06109c92 (perf/git-heavy-tests, not on main): a memo |
| 6 | Dyno macOS readers | `ioreg` per disk read, `lsof` per process | in-process IOKit and libproc (Dyno ADR 0026) |

The inventory found **65 more** (5 sweeps, each loop confirmed by reading its caller): Dyno 8, Pitwall 4, Debrief 21,
Pitcrew 5, Spotter 5, shared crates and ui 6, dev tools, scripts and conformance 16, plus 3 in the merge check that
perf/git-heavy-tests owns. The worst: Debrief runs up to 48 facet scans per typing pause in Search everything (DB-1) and 17
plugin round trips per imported file (DB-15); Spotter rewrites its whole config once per imported watch (SP-1); Pitcrew
opens a new pipe connection to its service per runner, twice per dashboard refresh (PC-1); `./dev plan` starts 2 `git show`
per changed file, about 1,582 for train 13 (TD-1); Dyno's helper takes a full process sweep per page (DY-2).

Why fixing instances does not end it: the single-item shape is the default shape of a new command, port method or helper,
nothing counts calls, and no shared code can run a long-lived batch process (`paddock_proc::run_bounded` closes stdin, so
`git cat-file --batch` is out of reach; `merge_check/mod.rs:35` says so).

## Comparable practice (ER-37)

- **N+1 guards are runtime counts in tests.** Django's `assertNumQueries(num, func)` asserts how many queries a call makes
  (https://docs.djangoproject.com/en/5.1/topics/testing/tools/); Rails' Bullet detects N+1 queries and, with
  `Bullet.raise = true`, fails the test (https://github.com/flyerhzm/bullet).
- **Lint for the obvious shape.** ESLint's core `no-await-in-loop` flags `await` in a loop and points at `Promise.all`
  (https://eslint.org/docs/latest/rules/no-await-in-loop); it does not see N calls started by `Promise.all(xs.map(...))`.
- **Batch APIs return a result per item.** GraphQL DataLoader coalesces single-key loads into one batch call that returns
  one result per key, an `Error` at a key's index for that key (https://github.com/graphql/dataloader); Microsoft Graph
  batches up to 20 requests per call, each with its own status (https://learn.microsoft.com/en-us/graph/json-batching);
  Google AIP-231 batch gets take a repeated `names` field with a documented maximum and keep request order
  (https://google.aip.dev/231; AIP asks for atomic batch gets, Paddock's principle asks for per-item errors, as Graph and
  DataLoader do).
- **git's own bulk interface** is one `cat-file --batch` (or `--batch-command`) process reading object names on stdin
  (https://git-scm.com/docs/git-cat-file). Measured here (Mac, 200 files, 3 runs): one `git show` per file 2.2-2.7 s, one
  batch process 55-57 ms (the inventory's measurement block).

All sources read 2026-11-18. The common-practice set (each sub-decision's cheaper option) costs about 37.5 agent-days
against 45.25 for the decided set; the user chose the decided set with both costs shown.

## Decision

### Rule 85 (docs/rules.md)

A function, Tauri command, port method, IPC message or HTTP endpoint that works on items takes many at once and returns a
result per item; work over many items is one bulk call or one long-lived process or connection; a command's count of
process starts, statements or invokes is a budget a test guards. Full text in docs/rules.md.

### Shared mechanisms

| Id | Mechanism | Home | Owner | d |
|---|---|---|---|---:|
| G1 | **A bounded run that takes standard input**: `run_bounded` with an input (written on its own thread, size bounded) next to today's stdin-closed form (`crates/paddock-proc/src/run.rs:208-225`); the private stdin runner of `tools/paddock-dev/src/input_hash.rs` (`Exec`, `spawn_piped`) moves onto it. Prerequisite of G2. | paddock-proc | `perf/git-heavy-tests-2` | 0.5 |
| G2 | **One shared batched git reader**: `files_at(&[(rev, path)]) -> Vec<Result<Option<Vec<u8>>, _>>` and existence checks over one `git cat-file --batch` / `--batch-check` process, with a `starts()` counter. paddock-repo-graph's `History::file_at`, the private reader in `tools/paddock-dev/src/check_ids.rs` and the merge check's per-file `cat-file blob` move onto it. **Reader choice** (amended 2026-11-19): two options were measured on the Windows runner (docs/benchmarks.md, "Batched git reads", run 8267, 20 trees and 300 blobs): (a) `cat-file` only, one long-lived `git cat-file --batch-command`, no new dependency: 428 ms for 2,000 files and 685 ms for 12,000; (b) gix in process (0.90, object database only) with that `cat-file` as the fallback for ref names, short ids and a partial clone's omitted blobs: 9.4 ms and 39 ms, for about 60 extra crates in `crates/Cargo.lock` (71 in `tools/paddock-dev`) and a hakari exclusion; its cold-build cost is not measured yet. One git start per read was 11.1 s and 11.9 s. The user chose (b) on 2026-11-18; (a) stays in the crate as the fallback and as `Reader::new`. | new crate `crates/paddock-git` (user, 2026-11-18) | `perf/git-heavy-tests-2` | 1.5 |
| G3 | **Counting seams, scoped to the thread or task** (amended 2026-11-18: the built API differs from the first text, so that parallel tests never share one global count). (a) Rust: `paddock_proc::starts` (a re-export of `paddock_env::starts`): `count_starts()` returns a counter of the commands made on its thread, plus any thread or task its scope is carried into; `assert_starts(n \| at_most(k), \|\| ..)` and `assert_starts_async` assert a budget in one line, after Django's `assertNumQueries`. The count is noted in `paddock_env::command::command`, the one `Command` constructor (Clippy forbids `Command::new` elsewhere); it counts commands made, not successful spawns, and the registered copies of the constructor (`docs/known-copies.toml`, `no-console-command`) are not counted until they call it. (b) TypeScript: `countingBackend` / `provideCountingBackend` / `expectBackendCalls` in `@decryp7/paddock-ui/ipc/testing`, which wrap an app's backend so the code under test runs unchanged. (c) The Tauri invoke round trip measured once on the Mac and Windows (rule 48 row): **still open**. Existing per-fake counters stay (`paddock_testkit::FakeRunner::count`, the DuckDB QueryLog count in Debrief's `regress_1_2.rs`, Pitwall `FakeServer.calls()`, Pitcrew `Fake.sent`, Dyno `FakeTools::calls`). Implementation: `feat/bulk-api-seams` (36a563d58). | paddock-env (re-exported by paddock-proc), paddock-ui `ipc/testing` | `feat/bulk-api-seams`; (c) open | 1.0 |
| G4 | **A per-item result type** for bulk commands in `paddock_commands` (amended 2026-11-18: no new crate): `ItemResult<T>` (ok or a typed item error), `BulkReply<T>` (`Result<Vec<ItemResult<T>>, CommandError>`, per-item errors) and `AtomicReply<T>` (`Result<Vec<T>, CommandError>`, for operations atomic by design), with the generated TypeScript twin in `@decryp7/paddock-ui/ipc`. Implementation: `feat/bulk-api-seams` (36a563d58). | paddock-commands | `feat/bulk-api-seams` | 0.5 |
| G5 | **ESLint rule `paddock/no-backend-call-in-loop`** (guard C). | `eslint/src/rules` | WP-0 | 1.0 |
| G6 | **Rule 85 and the review line** (guard A). | `docs/rules.md`, `.claude/agents/reviewer.md` | this ADR | 0.25 |

### Guard: A + C + D (decided)

- **A.** Rule 85, and a line in the reviewer's checklist (`.claude/agents/reviewer.md`): a loop that calls a command,
  port, process, query or HTTP endpoint per item is a finding unless the code says why. Landed with this ADR.
- **C.** `paddock/no-backend-call-in-loop` in the shared ESLint preset: a call on an injected backend or port, or an
  `invoke`, inside a loop body, or inside the callback of `.map`/`.forEach` passed to `Promise.all` or `forkJoin`. An
  intended per-item call carries `// per-item: <reason>`. RuleTester cases plus a positive control (ER-33); report-only
  until its first green run on the real tree, recorded by run id (ER-41). It does not see one call per rendered child
  component (DB-2, SP-4); D covers those.
- **D.** Every bulk command this ADR adds lands with a budget test: N items cost at most K process starts, statements or
  invokes, read from G3's seams (`assert_starts`, `expectBackendCalls`), and the test fails with the per-item code put back (the failing-first test, ER-11).
- Not chosen: a static Rust check (conformance or dylint) for spawns, statements or IPC sends inside loops (2-3 d, many
  false positives; Clippy has no custom lints without dylint); `no-await-in-loop` alone (it misses the `Promise.all(map)`
  shape of SP-2, DB-4, DB-5 and DB-18).

### Sub-decisions (2026-11-18, user: the recommended option each time)

| Id | Decided | Not chosen (cost) |
|---|---|---|
| D-G1 | guard A + C + D | A + B + D with `no-await-in-loop` |
| D-G2 | the shared batch reader (G1 + G2) | memo plus bounded parallel starts per site |
| D-DB1 | the full Debrief store batch: `facets`, `count_many`, `query_charts`, `dashboard_tiles` (WP-5) | `facets` plus the transaction fixes only (3.5 d) |
| D-DB2 | DB-7 (names rebuild) and DB-13 (catalogue build): measure first, then decide | change now |
| D-DB3 | DB-16 (`relog`) and DB-17 (`wevtutil`, `reg export`): keep the per-item tool runs | in-process Windows APIs (1.5 d) |
| D-DY1 | DY-3: bounded parallel `diskutil` | DiskArbitration in process (1.5 d) |
| D-DY2 | DY-5: `codesign` with a cache and the batch shape | Security.framework in process (+1.0 d) |
| D-PC1 | PC-1: a pipe-protocol change, multi-runner `History` and `Metrics` with per-runner results | one connection per refresh (0.5 d) |
| D-SP1 | SP-5: one browser per check pass with an isolated browser context per watch (amends Spotter ADR 0002) | browser reuse within one pass only (1.5 d) |
| D-SP2 | SP-3: local coalescing of pushes (dirty flag, one push per tick) | adopt paddock-live (2.5 d) |
| D-CR5 | keep `sc.exe` for service registration | in-process SCM API (2.5 d) |
| D-TD2 | cache the Cargo graph keyed by the manifests' blob ids | only run the 25 `cargo metadata` calls in parallel |

D-PC1 and D-SP1 are the two places where the decided option costs more than common practice (1.0 d each); D-DB1 is the
largest (4.5 d more than the minimum).

## Work packages

Each package starts with its failing budget test (D above), lands through a train, and records any number it quotes in
docs/benchmarks.md or the app's performance.md (rule 48). Ids refer to the inventory.

| WP | Contents | d | Gate |
|---|---|---:|---|
| WP-0 | G1 and G2 (on `perf/git-heavy-tests-2`, counted under WP-11), G3, G4, G5 (report-only), G6 | 3.25 | first; before Dyno |
| WP-1 Dyno | DY-1 (`powermetrics` once per query), DY-2 (one sweep per paged read, cached `dscl`), DY-5 (signature batch with a cache, no details lock), DY-7 (one transaction per tick and per alarm batch) | 3.0 | **before Dyno's release** |
| WP-2 Dyno | DY-3, DY-4 (merge the two smartctl readers, bounded parallel), DY-6 (`WolStates`), DY-8 | 2.0 | after |
| WP-3 Pitwall | PW-1 (`delete_models`, one scan), PW-2 (embeddings cache: one transaction per batch, running size, `IN (...)` reads) | 1.75 | **before Pitwall 1.1** |
| WP-4 Pitwall | PW-3, PW-4 | 1.0 | after |
| WP-5 Debrief store batch | DB-1 to DB-6 | 6.5 | **before Debrief 1.2** |
| WP-6 Debrief writes | DB-8, DB-9, DB-10, DB-11 (one commit or one transaction per batch) | 1.5 | **before Debrief 1.2** |
| WP-7 Debrief import and images | DB-15 (`SniffMany` and the extension pre-filter), DB-18, DB-19, UI-1 (batched webview log lines, shared by every app) | 3.75 | **before Debrief 1.2** |
| WP-8 Debrief rest | DB-12, DB-14, DB-20, DB-21; DB-7 and DB-13 after measurement (D-DB2) | 2.75 (+3.0 if the measurement says change) | after |
| WP-9 Pitcrew | PC-1 (protocol), PC-2 (one shared `ureq::Agent`, one outcomes pass for all runners) | 2.5 | **before Pitcrew's next release** |
| WP-10 Pitcrew | PC-3, PC-4 (with `has_many` on paddock-secrets' vault port), PC-5 | 1.25 | after |
| WP-11 dev tools | G1 + G2 with TD-1, TD-3, TD-6, TD-8, TD-10 (owner `perf/git-heavy-tests-2`); TD-2 (Cargo graph cache); TD-4 + TD-5 (bulk worktree facts); TD-9 (`Http::get_many`); TD-7, TD-12, SC-2, SC-3, SC-4 | 8.5 | foundation; gates no app |
| WP-12 Spotter | SP-1, SP-2, SP-3 (local coalescing), SP-4, SP-5 (browser contexts in paddock-chromium) | 6.0 | Spotter's next release |
| WP-13 shared rest | CR-6, CR-7, UI-2 | 1.5 | after |

Budget: **45.25 agent-days**; **22.25 before the releases** (WP-0 3.25, Dyno 3.0, Pitwall 1.75, Debrief 11.75 with UI-1,
Pitcrew 2.5), 8.5 of foundation dev-tool work (WP-11) and 14.5 after the releases. A package that grows by more than 20%
goes back to the user. Not in the budget: D-DB2's follow-up if the measurement says change (up to 3.0 d) and the D-DB3,
D-DY1, D-DY2, D-CR5 options not chosen.

**Parallelism and cost (ER-20).** The packages touch disjoint apps and run as parallel lanes within the machine's load
(ER-24) once WP-0 is in, because every package's failing test uses G3 and G4. Where a batch removes the work, no
parallelism is added; the bounded-parallel choices (DY-3, DY-4) are the cases where the tool has no batch form.

## Consequences

- New commands, port methods and IPC messages are bulk-first; a single-item form is a thin wrapper over the bulk one.
- Each app's before-release packages become release gates in its plan; the release order is unchanged (Dyno, Pitwall,
  Debrief, Pitcrew).
- Spotter ADR 0002 ("a throwaway per-watch profile") is amended by D-SP1 when WP-12 lands: isolation moves from a browser
  per watch to a browser context per watch; the memory cap and watchdog move to the shared browser.
- Pitcrew's service protocol changes (D-PC1): the app and the service move together, with a version bump in the hello.
- Four possible bugs the sweep read from code are recorded as fix items in docs/lessons.md (2026-11-18): the Pitcrew
  `job-outcomes.json` race, the Spotter avatar `.tmp` race, Dyno's two smartctl readers missing from
  `docs/known-copies.toml`, and `RepoGit::run` in paddock-dev without a timeout.

## Status of the mechanisms

| Mechanism | Status on 2026-11-18 |
|---|---|
| Rule 85, reviewer line (G6) | landed with this ADR |
| G1, G2 (`crates/paddock-git`) | in progress on `perf/git-heavy-tests-2`, not pushed yet |
| G3 (a, b), G4, G5 | in progress on `feat/bulk-api-seams` (36a563d58); G5 report-only until its first green run (ER-41) |
| G3 (c), the measured invoke round trip | open |
| Budget tests (guard D) | per package; existing per-fake counters are listed under G3 |
