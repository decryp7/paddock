# 95. Tests get a writable repository copy through one cheap helper, and pay whole-repository setup once per run

- Status: Proposed 2026-11-10. Drafted at the user's request of 2026-11-10 (ER-21: one class, five budget overruns, four on Windows,
  fixed one cause at a time). Includes the coordinator's input of 2026-11-10 from the review of
  `origin/fix/golden-live-windows-timeout`. Nothing here is built yet. ER-23 does not apply (no drawings).
- Date: 2026-11-10
- Related: ER-12 (no raised timeouts), ER-20 / rule 51 (more parallelism or a cheaper operation), ER-21 / rule 40, ER-22
  / rules 42 and 55, ER-27 (phases with durations); ADR 0025 (kept clones), ADR 0078 (accepted: `paddock_testkit::scratch`,
  one run root), ADR 0080 (CI speed), ADR 0088 (ratchets), ADR 0094 (drafted with this one).
- Number: 0095 was free on every `origin` branch on 2026-11-10 (see ADR 0094's Number line; the same listing).
- Evidence: read on 2026-11-10 with `git show`, `git grep` and `git log` after `git fetch`; paths without a branch are
  from this ADR's base `origin/train/2026-11-09-3`. The fix commit's numbers are quoted from its message, not re-measured
  here (no build or CI run for this ADR). No user data was read. The heads, re-read right before the commit:

```evidence-heads
read: 2026-11-10
origin/train/2026-11-09-3 67a633ef3
origin/main b51c72509
origin/fix/golden-live-windows-timeout 61b82cd32
origin/docs/adr-windows-gate-ratchets db7782e01
```

## Context: the incidents

The golden live tests in `tools/paddock-dev/tests/it/golden.rs` compare `./dev`'s planner with `scripts/affected.mjs`
on the real repository. Each pays a large fixed setup, and on the Windows host runner each new cause took them past the
240 s budget (the waiver `slow-timeout = { period = "60s", terminate-after = 4 }` in
`tools/paddock-dev/.config/nextest.toml`):

| # | Run | Cause | Fixed by |
|---|---|---|---|
| 1 | 7810 job 33008 | the oracle ran `affected.mjs` once per sample: 36 samples × 23 workspaces of `cargo metadata`, 195 s of 240 s | the batched `oracle()` (`golden.rs:51-55` doc comment) |
| 2 | 7787 (macOS) | every graph-reading test runs `cargo metadata` over every workspace; started together they queue on cargo's package-cache lock past 60 s | test group `repo-graph`, `max-threads = 2` (`60f2d7249`, 2026-11-09) |
| 3 | 7879, 7881, 7893 | `copy_checked_out_files` copied about 11,000 files (550 MB), hashed and compressed them with `git add -f -A` (780 MB of objects) and rewrote them with `reset --hard`; both edit tests do it at once. Green only with the host to itself (7881: 183 s and 236 s) | `61212a3fb` on `origin/fix/golden-live-windows-timeout` (not on any train yet): the copy borrows the source's object store (`objects/info/alternates`) and takes its index entries (`ls-files --stage` into `update-index --index-info`), then `checkout-index --all`; only files edited in the source are hashed. Mac, per its message: copy 38 s → 3.9 s with both running; the two tests 35 s and 39 s → 7.4 s and 21 s. Windows after the fix: not measured yet |
| 4 | 7808 | `conformance/ci/kept-clone.sh` ran `reset --hard` before its sparse patterns, writing the whole tree of the clone's branch: 50 to 110 s on the Windows CI PC | `5544158b2` (2026-11-09): sparse patterns first (`kept-clone.sh:166-174`), with a test (`conformance/test/kept-clone.test.mjs:177`) |

**The review round on the fix** (`61b82cd32`, 19:55, on the same branch). `61212a3fb`'s mechanism test counted loose
objects only (`own_loose_objects`), which races git's detached auto-gc: the old code made 14,382 objects, above the
default `gc.auto` threshold of 6,700 loose objects (git-config, `gc.auto`, https://git-scm.com/docs/git-config, read
from the documentation, not re-fetched today), so `git commit` may start a background `gc --auto` that packs them while
the test counts, and a failing-first run could pass. `61b82cd32` sets `gc.auto 0` in the copy (`golden.rs:656` there),
counts loose plus packed objects with `git count-objects -v`, and still fails with the old helper swapped in (per its
message); it also makes the two narrowing tests print `copy`, `read_graph` and per-edit durations to stderr, and writes
`git`'s stdin from its own thread. This ADR takes those as the helper's behaviour.

**Still open in the fixed copy.** It writes every checked-out file once (`checkout-index --all`): cheaper than hash,
compress and rewrite, but still the whole tree, and file creation is what is slow on the Windows host. It lives in one
test file, so the next test that needs a copy will write its own. The other live tests (`live_plan_...`,
`live_range_plans_...`, `live_ci_jobs_table_...`) print no phases.

**A per-binary `OnceLock` does not share across tests in CI.** nextest runs every test in its own process
(https://nexte.st/docs/design/why-process-per-test/, read, not re-fetched), and CI runs nextest (`--profile ci`). A
`OnceLock` helps only under `cargo test`. One setup per run needs a cross-process once: a file in the run root under an
OS file lock.

**Other sites (git grep).** `git add -A|--all|.` in tests: 14 sites in `tools/paddock-dev/tests`, `tools/wiki/tests`,
`crates/paddock-repo-graph/tests` and `tools/paddock-dev/src/gates/edge.rs` tests; all but `golden.rs:628` (the copy,
gone in `61212a3fb`) work on small fixture repositories in temp folders and cost nothing. `reset --hard`: only
`kept-clone.sh:173` (after the sparse patterns) and `golden.rs:643` (gone in the fix). Test files that name the real
repository root and run a git write or a recursive copy: 3 (`golden.rs` 17 lines, `ipc_endpoints.rs` 4, `wrapper.rs` 1);
only `golden.rs` writes a copy of the real tree, the other two write fixtures. No TypeScript or `.mjs` test copies the
real tree (their `ls-files` calls read the index only). One CI candidate: `.gitea/actions/stable-workspace/action.yml:98`,
whose Windows fallback `clone_checkout` runs a non-sparse `checkout -q -f` of the whole tree into the stable path (used
by Debrief's `tauri-smoke` and `bundle` matrix jobs, `debrief.yml:639`, `:900`); its duration is not measured.

## Pattern

**A test or CI step pays a cost proportional to the whole repository (write every file, hash every file, read every
workspace's graph) once per test or per sample, and the budget hides it until the slowest host is shared.** Each
instance was found by a timeout and fixed by hand in its own file; the next whole-repository cost came in through the
same door.

**Invariant.** A test that needs a writable copy of the real repository gets it from one helper whose cost does not
grow with files it does not need; whole-repository reads that do not change during a run are made once per run; every
such test prints its phases with durations; and a check refuses a whole-tree write that does not go through the helper.

## Decision

1. **`paddock_testkit::repo`** (D1), behind a `git` feature of the test kit (its doc line "starts no process" gets the
   exception), built on ADR 0078's `ScratchDir` under the run root:
   - `RepoCopy::of(src).paths(cones).build() -> RepoCopy { root, phases }`: `61212a3fb`'s mechanism (alternates, index
     entries from `ls-files --stage` filtered to regular files on disk, `checkout-index`, edited files hashed, LF
     checkout, `commit --no-verify`), with `gc.auto 0` and `core.autocrlf input` set before any write. With `paths`,
     only those cones are written (D3); without, every checked-out file (today's behaviour).
   - Git runs through `paddock_proc` with a timeout; each git step is a phase (`copy.index`, `copy.checkout`,
     `copy.edited`, `copy.commit`) with its duration, file count and exit status (ER-27).
   - Its mechanism test counts with `git count-objects -v` (`count` + `in-pack`, in the copy's own store) against
     folders + 1 + edited files, with a planted full `add -A` copy as the positive control that must fail (ER-33).
2. **Once per run, across processes** (D2): `paddock_testkit::repo::once(key, build)` makes a read-only artifact under
   the run root (a base copy, the `cargo metadata` graph of every workspace as JSON), keyed by the source's HEAD tree,
   index checksum and the content hash of edited files, under an OS file lock (the `paddock-slots` lock that already
   backs `./dev ci slot`). The first test builds it; the others wait on the lock (bounded, with a named error) and read
   it. Graph-reading tests then read the cached graph instead of running `cargo metadata`, and the `repo-graph` group
   goes away once its tests do. A writable copy is never shared: each test that edits gets its own `RepoCopy`, cheap
   because of §1.
3. **Phases in every live test** (D4): the two narrowing tests already print copy, read_graph and per-edit (`61b82cd32`); every other live golden test prints `copy`, `read_graph`, `oracle` and `per-edit`
   durations (the `success-output = "final"` override already shows them in the job log), so the next budget overrun
   names its phase from the log alone.
4. **The check** (D5), a `./dev check whole-tree` in the RT-5 host:
   - test files (Rust, `.mjs`, `.ts`) that name the real repository root (`repo_root()`, `CARGO_MANIFEST_DIR` with `..`,
     `PADDOCK`, `rev-parse --show-toplevel`) and run a git write (`add -A|--all|.`, `reset --hard`, `checkout -f`,
     `checkout-index --all`, `read-tree -u`, `clone`, `worktree add`, `update-index --index-info`) or a recursive copy:
     an error unless the file uses `paddock_testkit::repo`, or the line carries `// whole-tree: fixture only — <why>`
     (today `ipc_endpoints.rs` and `wrapper.rs`);
   - CI scripts (`.gitea/`, `conformance/ci/`, `apps/*/scripts/ci/`): `reset --hard` or `checkout -f` with no sparse
     setup earlier in the same function, unless marked `# whole-tree: <why>` (`kept-clone.sh:173` is after its
     sparse patterns and passes; `stable-workspace` is marked or moved by WP-0's finding);
   - a planted site of each kind as positive controls. Small fixture repositories are not flagged: their `add -A` is
     the cheap, normal way to build a fixture.
5. **kept-clone keeps its own code** (D6): it fetches a commit from the remote into a slot's clone, which is not a copy
   of a local checkout; it is covered by the CI half of §4 and its existing run-7808 test. `stable-workspace`'s Windows
   fallback is measured (WP-0) and, if it writes the whole tree, replaced by kept-clone in those jobs.
6. **Budgets shrink after** (D7): once WP-3 has run on the Windows host, the golden waiver drops to the measured
   worst case plus 50 %, or to the default 60 s if it fits; a waiver that is not needed is removed (ER-12).

## Parallelism and cost (rule 51)

The point of this ADR. Cheaper first: no hashing (done in `61212a3fb`), no rewrite, write only the cones a test needs
(§1, D3), read the graph once per run instead of once per test (§2). Parallel second: tests stay parallel; the once-lock
serialises only the first build of a shared artifact, and the graph read keeps cargo's own lock as its one contention
point (one run of it per workspace per run instead of one per test).

## Alternatives

- **Raise the budget.** ER-12; five overruns so far.
- **`OnceLock` per test binary** (the coordinator's first suggestion): correct under `cargo test`, no effect under
  nextest's process per test. Kept for in-process state only.
- **Merge the live tests into one test.** Shares setup without a lock, but one red result hides the others and the one
  test's budget grows with every case.
- **`git worktree add` from the source repository.** Writes a worktree entry into the developer's or CI's real
  repository (ER-14: a test must not change the checkout it runs from) and still writes every file.
- **`git clone --shared`.** Fails in CI's depth-1, blob-less, sparse clone ("lazy fetching disabled", `golden.rs:601`).
- **Hard links or reflinks of the checkout.** A test that edits a file through a hard link edits the source; reflinks
  are APFS and Btrfs only, not NTFS, the host that is slow.

## Consequences

- One helper to fix when the next copy problem appears; a log line per phase in every live test; a lint that sends new
  whole-tree writes to the helper.
- The test kit gains a process-starting feature (behind `git`, a dev-dependency only).
- Cones (D3) make a test state what it reads; a cone that is too small fails as a missing file, never as a wrong plan,
  because the oracle reads the same copy.

## Rollback

Tests call the old private function again; the lint is removed from the RT-5 registry; the run-root cache is a folder
the leak check already removes with the run root.

## Decisions for the user

| # | Decision | Options | Recommended |
|---|---|---|---|
| D1 | Where the helper lives | (a) `paddock_testkit::repo` behind a `git` feature, on ADR 0078's scratch; (b) `paddock-repo-files` with a `testing` feature; (c) private to `paddock-dev`'s tests | **(a)**: test support has one home; `paddock-dev` already depends on the test kit |
| D2 | Shared setup | (a) a cross-process once per run (lock + run-root cache) for read-only artifacts, a writable copy per test; (b) `OnceLock` per binary; (c) one merged live test | **(a)**: the only one that shares under nextest |
| D3 | Sparse copies | (a) `paths(cones)` in the helper, adopted where WP-0 shows the planner's inputs fit a cone; (b) always full | **(a)**, after WP-0's measurement |
| D4 | Phase output | (a) every live test and the helper print phases with durations; (b) only on failure | **(a)** (ER-27; the waiver already shows output) |
| D5 | The check | (a) `./dev check whole-tree` over tests and CI scripts with markers and positive controls; (b) review only | **(a)** |
| D6 | kept-clone | (a) keeps its own code, covered by the CI half of the check; `stable-workspace` measured and moved to kept-clone if needed; (b) a shared Rust library for both | **(a)**: different job (remote fetch, not a local copy) |
| D7 | Budgets | (a) shrink the golden waiver to measured + 50 % after WP-3 and drop `repo-graph` once its tests read the cached graph; (b) keep both | **(a)** |
| D8 | Tag gating | (a) none: test and CI code only; (b) block tags until done | **(a)** |

## Work packages (raw agent-days, one agent)

| WP | Content | Days |
|---|---|---|
| WP-0 | Measure: phase durations of the live tests on the Windows host (one dispatched run after `61212a3fb` lands); `stable-workspace` `clone_checkout` duration from Debrief job logs; which files `affected.mjs` and `cargo metadata` read (an strace/fs_usage-free method: a cone copy that passes the golden cases) | 0.5 |
| WP-1 | `paddock_testkit::repo::RepoCopy` (alternates, index reuse, cones, `gc.auto 0`, phases through `paddock_proc`), the `count-objects` mechanism test with its planted positive control, tests on Linux, macOS and Windows | 1.5 |
| WP-2 | `repo::once` (lock, key, run-root cache, bounded wait), the cached graph; move the `repo-graph` tests onto it | 1 |
| WP-3 | `golden.rs` onto the helper (delete `copy_checked_out_files` and its object counter), phase output in every live test, shrink the waivers (D7) | 0.5 |
| WP-4 | `./dev check whole-tree` with positive controls, the two fixture markers, the CI-script half | 1 |
| WP-5 | Lesson, rule text (proposed: "a test that needs a copy of the real repository takes it from `paddock_testkit::repo`"), ADR index | 0.25 |

**4.75 days**; first cut (WP-0, WP-1, WP-3) 2.5 days. Gates no tag.
