# 76. Owned background work: every thread, task and child process has an owner that ends it within a bound and says so

- Status: Proposed (2026-11-09). Drafted at the coordinator's request (ER-21 / rule 40). Needs the user's decisions below
  and an independent review before any build.
- Date: 2026-11-09
- Related: ER-21 / rule 40 (recurring bugs mean a missing design rule), ER-22 / rules 42, 43 and 55 (share first, shared
  behaviour tested once, one implementation per mechanism), ER-14 / rule 7 (no processes left behind), ER-12 / rule 5 (no
  sleeps, no retries that hide a failure; failing test first), ER-27 (logs explain every failure; rule 56 on its branch),
  ER-28 (a cause is confirmed only by measurement), ER-32 (bounded waits), ER-33 / rule 71 (a check counts only with a
  positive control), rule 52 (live work never at a starvable level), rule 53 and ADR 0033 (processes by identity),
  rule 48 (benchmarks recorded).
- Builds on, does not replace: **ADR 0020** (one shared executor: `submit`, `every`/`after`, `lease`,
  `lease_abandonable`, cooperative abort, declared exemptions; accepted 2026-10-27, **not built yet**: `paddock-jobs` has
  no `exec` module on `origin/main`) and **ADR 0024** (background work contract; its check **C42**, "`thread::spawn` /
  `Builder::spawn` outside `paddock-jobs` fails unless a `lease` waiver", accepted 2026-10-28, **not built yet**: no
  `C40`-`C42` in `conformance/src/checks` on `origin/main`). Those ADRs decide *where* background work runs and at which
  priority; neither says who **ends** it, when, within which bound, or what a dropped handle does. This ADR adds that, and
  specifies C42 so it can see ownership, not only the call.
- Composes with: **ADR 0072** (accepted 2026-11-09, `origin/docs/adr-0072-ordered-close`: every connection end says why;
  `Peer::close` returns at once, `Peer::wait_released(timeout)` waits for the release, the `Accepted` drop fallback never
  blocks and hands off to a drain pool). This ADR supplies the ownership and the join under those calls; it does not change
  0072's ordering. It amends 0072's `OwnerReason` by one variant (D6).
- Not in scope, by design: start waits (**ADR 0071**, ready or exit), shared test machines and their lease holders
  (**ADR 0074**, `./dev machine lease` with `--owner-pid`; this ADR only requires that the lease holder is a registered
  detached process, §6), test doubles (ADR 0073).
- Number: 0076 was free on every `origin` branch on 2026-11-09 (family ADRs on branches end at 0074; 0075 is being drafted
  for copy waivers). Check number: this ADR **builds C42** as ADR 0024 numbered it, and needs no new number.
- Evidence read at `origin/main` `b51c72509` unless a row names a branch. The survey is a syn-based prototype of the check
  (scratch: `/Volumes/d7sst7/dev-targets/scratch/adr-0076/scan`, its output `sites2.tsv` next to it); its cost is recorded
  in [benchmarks.md](../benchmarks.md#2026-11-09-a-syn-scan-of-every-spawn-site-in-the-monorepo-docsadr-0076-owned-background-work).

## Pattern

**Background work outlives what started it.** Whenever a component starts a thread, an async task or a child process, it
decides on its own whether to keep the handle, whether to signal and wait for the work when it stops or is dropped, and
for how long. Most do not keep the handle at all; many keep it but have no stop path that waits; a few wait without a
bound. The work then runs on after its owner is gone, holding files, descriptors, folders, locks, CPU or a connection,
and nothing in the log says so.

**The invariant that should hold.** Every thread, task and child process started by Paddock code is held by exactly one
owner. The owner's explicit stop signals the work and waits for it to end within a declared bound, then reports what
ended, how long it took and what overran. Dropping the owner without a stop does the same without blocking: it signals,
and hands the handles to one process-wide reaper that waits within the same bound and logs the outcome. Work that must
outlive its owner (a relaunched app, a document opened in another program, a call into the OS that cannot be cancelled)
is detached only through a declared kind with a registered reason and a cap. A test that builds and ends an owner finds
no thread, task, child process or descriptor of it left.

## Context: the incidents (2026-11-08 and 2026-11-09)

| # | Where | What happened | Why | State |
| --- | --- | --- | --- | --- |
| 1 | `paddock-ipc` `Peer`/`BodyPeer`, seen in Dyno's `helper_seam` tests (CI run 7763, Linux) | "the host closed the connection during the handshake"; measured in the CI image: each dropped `BodyPeer` kept 6 descriptors and 5 threads, about 250 connections in 5 s, the in-process agent hit EMFILE (lessons on `origin/train/2026-11-08-1`, `docs/lessons.md:1551-1573`) | `Peer` is `Clone` around `Arc<Inner>` (`crates/paddock-ipc/src/peer.rs:242-244`) and has no `Drop`; its reader, writer and push threads are started by `spawn(...).map(drop)` (`peer.rs:254-260`), each holding a clone of `Inner`, so dropping the last user handle frees nothing and nothing ever joins them. `BodyPeer` (`body.rs:96`) has no `Drop` either. | Fixed in the test rig only (`adc28dd0a`: the rig closes each connection). The lesson ends "Class: a handle whose drop does not release its resource (`BodyPeer`, `Peer`); reported for an ER-21 decision." This ADR is that decision. |
| 2 | Debrief `debrief-store-duck` tests | "Directory not empty" from `remove_dir_all` at `tests/it/segments.rs:240` (the moved store) and `tests/it/search.rs:220` (`index` right after `drop(store)`), once each under a full nextest run; 30 of 30 green alone (lessons on `origin/fix/debrief-lint-1-temp-client`, `docs/lessons.md:1503`) | **Not measured.** Lesson's probable cause: the store's index threads still write after `drop`. Candidates found for this ADR by reading (ER-28 list, "The failing tests the fixes start with"): (a) the FTS service joins its own thread on drop (`src/fts.rs:685-693`) but its loop leaves on `stop` without `drop_writer` when a turn just worked or segments are pending (`fts.rs:820-834`), and the error path drops the writer with `*s.writer = None` (`fts.rs:828`); the Tantivy `IndexWriter` is then dropped with the shared state, and Tantivy 0.27's `Drop for IndexWriter` only kills the segment updater and joins the indexing workers (`index_writer.rs:821-828`), while merges already scheduled on its `merge_thread_{i}` pool (`segment_updater.rs:312-313`, `:541`) keep writing into `index/` until done; only `wait_merging_threads` waits for them (`index_writer.rs:355-376`). (b) the SQL console's timer thread is signalled but never joined (`src/console/mod.rs:215`, `Drop for Stopper` at `:249`). | Open; WP-7 starts with the failing test in "The failing tests the fixes start with". |
| 3 | Test children | A test that fails before it kills its children leaves them running: `paddock-proc` `sys.rs:607-613` starts three `sleep 60` as plain `std::process::Child` in a `Vec` (no guard), Dyno `app/src-tauri/src/quit.rs:831-836` one `sleep 60` (or `ping -n 60`); `std::process::Child` has no `Drop` and is not waited for ([std docs](https://doc.rust-lang.org/std/process/struct.Child.html), read 2026-11-09). The interim VM lock's holder (`scripts/vm-lock.sh` on `origin/fix/vm-smoke-take-vm-lock`) is a detached process that lives as long as an owner folder, which a killed script leaves until the next acquirer removes it. | No owner kills them on a failed assert; the holder's life is tied to a folder, not to a process identity. | The VM holder is replaced by ADR 0074's lease (`--owner-pid`, by identity); the test children are covered by §5. |
| 4 | Agents | An agent started one busy process per vCPU in the Windows test VM over SSH; sshd stopped answering and the clean-up in the same session never ran (lessons on `origin/train/2026-11-08-1`, `docs/lessons.md:1543-1549`). Burners left on the host by agents were stopped by hand the same nights (coordinator's report in the brief; no log line cited). | The load had no owner, no bound and no recorded identity. | §7. |
| 5 | Debrief start-up sweeps and other detached threads | `apps/debrief/app/src-tauri/src/lib.rs:649` (old logs pruned) and `:687` (scratch stores swept and removed) run in `std::thread::spawn` whose handles are dropped; Quit cuts them wherever they are. 121 more fire-and-forget starts in shipped code (survey). | No owner; at best idempotent by luck. | At risk, not seen failing. |

Earlier instances of the same class: lessons on `origin/main` line 485 (a CI container whose PID 1 is `sleep` never reaps
orphans), line 646 (`cmd /c` and login-shell children outliving `taskkill /T`), line 1027 (an orphaned agent after Quit),
line 1081 ("endpoint folders leak a third way": a test process ends while a plugin still starts), and ADR 0020's own
survey ("DB writers, abandonable plugin and OS calls ... unnamed, uncapped and invisible").

## Instances found: every spawn site (survey, `origin/main` `b51c72509`, 2026-11-09)

**Method.** A syn 2 pass over all 3,222 `.rs` files under `crates/`, `apps/` and `tools/`; it skips items under
`#[cfg(test)]`, `#[test]`, `#[cfg(feature = "testing")]`, files declared as `#[cfg(test)] mod x;`, and classifies the
rest by path (`tests/`, `benches/`, `examples/`, `build.rs` are test; `fake*.rs`, `testing.rs`, `testchild.rs` and crates
named `*testkit*`, `*-test*`, `*bench`, `*testplan*`, `*e2e*` are test support; `tools/`, `xtask`, `cargo-*` are dev
tools; `vendor/` is vendored). It finds `thread::spawn` (also imported), `thread::Builder::...spawn`, `spawn_scoped` and
the spawns inside `thread::scope`, `tokio::spawn`, `task::spawn`, `async_runtime::spawn`, every `spawn_blocking`,
`rayon::spawn`, `Command`-like `.spawn()`, `.output()`/`.status()` on a command, `paddock_proc::Supervised::spawn*` and
`run_bounded`, and records what happens to the handle: discarded (`;`, `let _`, a `_name` never used), joined or awaited
at once, bound and joined, waited or terminated in the same function, or stored, passed on or returned. The prototype's
own positive control is a fixture with one site per class and one inside a `#[cfg(test)]` module (reported correctly,
the test one skipped). **Limits** (fixed in WP-2): "join present in file" means a `.join()`/`.wait()` exists somewhere
in the file, not that it joins this handle (66 sites need a reading); a call of a project's own wrapper named `spawn`
(`self.spawn(...)`, `captures.spawn(...)`) counts as a site of unknown kind (10 sites); one wrapper may start several
threads per call (`peer.rs:254`, three per connection); inline `#[cfg(test)]` modules are not counted.

**Hand check.** 16 of the 158 sites classified fire-and-forget or owned-but-detached were drawn at random (seed 76) and
read: 14 are raw starts classified correctly; 2 are calls of a project wrapper (`jobs/service.rs:509`,
`debrief-plugin-native/src/tool.rs:145`), counted once more at the wrapper. No raw start was misclassified.

**Counts.** 647 sites: 442 in shipped code (249 files, 64 crates), 129 in tests, 29 in test support, 13 in dev tools,
2 vendored (`apps/dyno/vendor/tray-icon`).

| Shipped, by kind | Owned and joined | Owned, a join in the file (to read) | Owned but detached | Fire-and-forget | Total |
| --- | --- | --- | --- | --- | --- |
| Threads (`thread::spawn`, `Builder::spawn`) | 64 | 58 | 24 | 76 | 222 |
| Scoped threads (`thread::scope`) | 30 | – | – | – | 30 |
| Blocking tasks (`spawn_blocking`) | 81 (awaited) | – | – | 6 | 87 |
| Async tasks (tokio, Tauri `async_runtime`) | – | – | 3 | 33 | 36 |
| Child processes: `.spawn()` | 20 | 6 | 7 | 1 | 34 |
| Child processes: `Supervised`, `run_bounded`, `.output()`/`.status()` | 23 | – | – | – | 23 |
| Calls of a project wrapper named `spawn` | – | 2 | 3 | 5 | 10 |
| **Total** | **218** | **66** | **37** | **121** | **442** |

| Shipped, by owner | Owned and joined | A join in the file | Owned but detached | Fire-and-forget | Total |
| --- | --- | --- | --- | --- | --- |
| Shared crates (`crates/`) | 56 | 18 | 16 | 19 | 109 |
| Dyno | 47 | 20 | 4 | 22 | 93 |
| Pitwall | 16 | 2 | 3 | 22 | 43 |
| Debrief | 86 | 23 | 10 | 34 | 153 |
| Pitcrew | 4 | 3 | 4 | 14 | 25 |
| Spotter | 8 | – | – | 10 | 18 |
| workbench-demo | 1 | – | – | – | 1 |

Notes on the classes:
- **"Owned and joined" is not yet "ended within a bound".** 81 of the 218 are `spawn_blocking(...).await`: if the awaiting
  future is dropped (a cancelled command), the blocking work runs on ([tokio `JoinHandle`](https://docs.rs/tokio/latest/tokio/task/struct.JoinHandle.html):
  "tasks spawned using `spawn_blocking` cannot be aborted"; read 2026-11-09). Most joins have no bound.
- **Detached by design, legitimately** (to be registered, §6), from reading: relaunching the app (`debrief/app/.../lifecycle.rs:727`,
  `updater.rs:471`, `paddock-updater/src/tauri/native.rs:25`), opening a document or URL in another program
  (`paddock-files/src/open.rs:309-328`, `debrief/app/.../opener.rs:231-233`, `paddock-health/src/tauri/native.rs:137`,
  `paddock-siblings/src/native.rs:21`), calls the OS cannot cancel (Spotter `spotter-sources/src/folder.rs:106`, a hung
  network share by design; Dyno `dyno-agent/src/network.rs:271`, a name lookup; `dyno-agent/src/extras/finder.rs:122`,
  `abandonable`), and watchdogs (`paddock-chromium/src/launch.rs:817`, its copies in `chromium-driver` and
  `spotter-render`).
- **Already right, and the model for the mechanism:** `paddock_proc::Supervised` ends its whole tree on drop and registers
  it for `end_all` (`supervised.rs:1-9`, `:486`); `debrief-store-duck`'s `CatalogueService` and `FtsService` signal and
  join on drop (`catalogue.rs:206-214`, `fts.rs:685-693`); `paddock_proc::cleanup::join` logs a panicked join
  (`cleanup.rs:27`). None of them bounds the join or reports its duration.
- **ADR 0024's count** (raw grep of thread spawns, 2026-10-28): shared 71, Debrief 127, Dyno 49, Pitcrew 24, Spotter 10,
  Pitwall 7. The syn count of threads only (222) is lower, because it skips inline test modules; it is higher for
  Pitwall because it also counts tokio tasks.

The full list of the 158 fire-and-forget and owned-but-detached shipped sites is the appendix; the 66 "join in the file"
sites are read in WP-0.

## Decision

### 1. The ownership rule

1. **Every start has an owner.** Production code starts a thread, task or child process only through an owner: a
   `TaskGroup` (§2), `paddock_proc::Supervised` (children), `std::thread::scope` (joined by construction), or a
   registered detached kind (§6). Raw `thread::spawn`, `Builder::spawn`, `tokio::spawn`, `task::spawn_blocking`,
   `tauri::async_runtime::spawn*` and `Command::spawn` are allowed only inside the mechanism's own crates.
2. **Stop is a command, and it waits within a bound.** `stop(bound)` signals every member (its `StopToken`; its
   `on_stop` hooks unblock I/O: close a socket, interrupt a DuckDB query, end a child), then joins them until the bound,
   and returns a `StopReport`: each member's role, outcome (`ended`, `panicked`, `overran`), duration. Members that overran
   are handed to the reaper (§3) and logged as such; `stop` never waits past its bound (ER-32).
3. **Drop does the same without blocking.** Dropping an owner that was not stopped signals every member and hands the
   group to the reaper with the group's bound; it never joins inside `Drop` (safe on a UI thread and during an unwind, as
   ADR 0072's `Accepted` fallback). The hand-off logs one line at `warn` (a stop was missing), which the leak harness (§5)
   treats as a finding unless the test declares it.
4. **No detached work without a registered reason** (§6), and none without a cap.
5. **The handle the user holds is not the handle the threads hold.** A type whose threads need shared state keeps the
   user's handle separate from the threads' state (`Peer { handle: Arc<PeerHandle> }` with `Drop for PeerHandle`, the
   threads holding only `Arc<Inner>`), so dropping the last user handle is observable and ends the work (incident 1).

### 2. One shared mechanism: `TaskGroup`

A small pure-std crate (D1 decides its home; the recommendation is a new crate `paddock-tasks`, which `paddock-jobs`
re-exports and on which ADR 0020's executor is later built):

```text
pub struct TaskGroup     // one owner's background work; not Clone
  fn new(owner: &'static str, bound: Duration, clock: Arc<dyn Clock>) -> TaskGroup
  fn thread(&mut self, role: &'static str, f: impl FnOnce(StopToken) + Send + 'static) -> io::Result<()>
  fn child(&mut self, role: &'static str, child: paddock_proc::Supervised)        // ended with the group
  fn task(&mut self, role, fut)            // feature "tokio": on the current runtime, aborted at stop
  fn blocking(&mut self, role, f)          // feature "tokio": spawn_blocking with a StopToken, joined at stop
  fn on_stop(&mut self, hook: impl FnOnce() + Send + 'static)   // unblocks I/O (close a socket, interrupt a query)
  fn stop_token(&self) -> StopToken
  fn stop(self, bound: Option<Duration>) -> StopReport           // signal, join within the bound, hand overruns to the reaper
impl Drop for TaskGroup                    // signal; hand everything left to the reaper; never blocks

pub struct StopToken     // cheap clone; is_stopped(), wait_timeout(d) on a condvar (no polling), child()
pub struct StopReport    // owner, members [(role, Outcome{Ended, Panicked, Overran}, duration)], handed_to_reaper
pub mod reaper           // one per process: pending(), settle(bound) -> Settled (tests, Quit), a periodic summary
pub enum Detached        // §6: Relaunch, OpenElsewhere, UncancellableCall{what}, ProcessLifetime{what}
pub fn detached(kind: Detached, role: &'static str, f) -> io::Result<()>   // capped per kind, counted, logged
```

- **`StopToken` is ADR 0020's `CancelToken`, not a second one** (rule 55): the token moves into the new crate and
  `paddock-jobs` re-exports it, so a job's cancel and an owner's stop are one mechanism, with child tokens.
- **Time comes from the injected clock** (ADR 0041), so the bound and the overrun are tested on a `FakeClock`.
- **Panics** are caught per member and reported as `Panicked` with the role (the containment ADR 0020 asks of
  `paddock_guard::catch`).
- **Process children** stay `paddock_proc::Supervised` (it already ends the whole tree, by identity, ADR 0033); the group
  holds them so one `stop` ends threads and children in one report. `paddock_proc` keeps its `Registry`/`end_all` for a
  crash or panic hook.
- **ADR 0020's executor, when it is built**, returns its `lease` and `submit` handles as members of the caller's
  `TaskGroup` (a lease is a member with role and cap; `lease_abandonable` is `Detached::UncancellableCall`), so the
  executor inherits ownership instead of each lease inventing its stop. Until then `TaskGroup` starts plain named threads
  at the creator's priority (rule 52 unchanged: shutdown is live work, nothing here lowers a priority).

**Composition with ADR 0072.** `Peer` holds its reader, writer and push threads in a `TaskGroup`; 0072's ordered close
(seal, fail and cancel, flush, half-close, drain, release) runs on those threads exactly as 0072 specifies;
`Peer::close` stays non-blocking; `Peer::wait_released(timeout)` becomes "the group's `stop` returned `Ended` for every
member"; and dropping the last user handle (`PeerHandle::drop`) calls `close(Closing::ByOwner(Dropped))` and drops the
group, which hands it to the reaper. The reaper's bound for a peer is 0072's own worst case (flush cap + reason deadline
+ drain cap, 4.25 s), so a dropped peer releases its descriptors and threads within it, never later. `LinkServer`, the
MCP loopback and the Dyno helper hold their accept loop and per-connection groups the same way; 0072's drain pool is a
`TaskGroup` with a member cap.

### 3. The reaper

One thread per process, started on first use and itself a registered `Detached::ProcessLifetime` member. It receives
groups dropped without a stop, waits for each until its bound on the clock, and logs one line per group: owner, roles,
outcome, how long it ran after the drop. A group still running at its bound is logged at `error` with its roles and left
running (it cannot be killed safely); `reaper::pending()` lists it, the diagnostics bundle includes the list, and the
leak harness fails on it. `reaper::settle(bound)` waits until nothing is pending or the bound passes; the app's Quit path
(ADR 0037's composition root) stops its top-level groups, then calls `settle` with the app's quit bound, so a Quit ends
every owned thread and child or names the one that did not.

### 4. The check: C42, built as a syn pass, with a register and a ratchet

C42 as ADR 0024 numbered it, specified here so it sees ownership, built like ADR 0072's C102 (syn in `paddock-dev`,
part of the push check over changed files and of CI over the whole tree):

- **What it finds.** Every spawn primitive of the survey method above, in non-test code, with the `use` paths of each
  file resolved (a renamed import is still found). Inside `paddock-tasks`, `paddock-proc` (children) and, when built,
  `paddock-jobs::exec`, raw spawns are allowed; elsewhere every raw start is a finding unless it is a `std::thread::scope`
  spawn or is in the register.
- **Compile-time half.** The lint preset (`lint/clippy.toml`) adds `disallowed-methods` for `std::thread::spawn`,
  `std::thread::Builder::spawn`, `std::process::Command::spawn`, `tokio::spawn`, `tokio::task::spawn`,
  `tokio::task::spawn_blocking`, `tauri::async_runtime::spawn` and `tauri::async_runtime::spawn_blocking` (with
  `allow-invalid` where a crate lacks the dependency), so the compiler resolves paths the syn pass might miss. Each
  allowed site carries `#[expect(clippy::disallowed_methods, reason = "detached: <register id>")]`, and C42 checks that the
  id exists. Whether clippy resolves a method path such as `Builder::spawn` reliably is verified in WP-2 against a fixture;
  if it does not, the syn pass alone is the check (D3).
- **The register** `docs/background-work.toml`: `[[detached]]` (site, `Detached` kind, reason, owner, cap) and
  `[[exempt]]` (ADR 0020's declared exemptions: real-time loops, the exit watchdog, a process that is itself the loop),
  each with a reason and an owner. A site in neither section, or an entry whose site no longer exists, fails the check.
- **The ratchet** `conformance/baselines/C42.json` lists today's findings by file plus a hash of the whitespace-collapsed
  line (the C71 method, lessons 2026-11-08), each with a `blocks_tag` (D7). It only shrinks; the tag check that already
  refuses a tag for an expired copy (ADR 0034, C58) refuses a matching tag while an entry remains (as ADR 0072 D8).
- **Positive control** (ER-33). A fixture tree with one production-style site per primitive (each import style, each
  handle disposition) and one inside a `#[cfg(test)]` module; a test asserts the pass reports each production site and
  skips the test one, so a primitive the parser stops seeing turns the test red. The scratch prototype is its starting
  point.

### 5. The test-harness leak check

Three layers, from cheapest:

1. **Children holding the test's output.** Every `.config/nextest.toml` (seven today) sets
   `leak-timeout = { period = "500ms", result = "fail" }` in the default and CI profiles, so a test whose children still
   hold its stdout or stderr after it ended fails as LEAK instead of passing ([nextest leaky tests](https://nexte.st/docs/features/leaky-tests/),
   read 2026-11-09: the `result = "fail"` form exists since 0.9.95; the machine has 0.9.151; nextest sees only children
   that inherited stdout or stderr). C14's hermetic wrapper keeps killing and reporting survivors of a whole run.
2. **Per-owner ending tests.** `paddock-testkit::leaks::Guard` snapshots, in the test's own process: the thread list with
   names, the open descriptors (handles on Windows), the child processes by identity (`paddock_proc::ProcessId`, pid plus
   start time, started after the guard) and `reaper::pending()`. `guard.check(bound)` first calls `reaper::settle(bound)`
   (a condition wait, never a sleep; ER-12), then fails with the names of every new thread, descriptor path and child
   command line. Every owner type in the register's `[[owner]]` section (each type that holds a `TaskGroup`) names an
   ending test that builds it, uses it, stops it and drops it, and asserts the guard; a second test drops it without a
   stop and asserts the reaper's line and a clean guard. The probes per OS (Linux `/proc/self/task` and `/proc/self/fd`;
   macOS the task's thread list and `proc_pidinfo` descriptor list; Windows a Toolhelp thread snapshot and
   `GetProcessHandleCount`) are chosen and measured in WP-3; the descriptor count on Windows is a count, not a list.
   Third-party threads that live for the process (a tokio runtime's workers, whose idle blocking threads stay for 10 s by
   default, [tokio Builder](https://docs.rs/tokio/latest/tokio/runtime/struct.Builder.html), read 2026-11-09) are declared
   per test by name prefix, never by a blanket allow.
3. **The app's Quit.** ADR 0037's composition test (its step 5, "Quit, and check that the process tree is gone") also
   asserts the reaper settled with nothing pending.

**Positive controls** (ER-33): `leaks::Broken` owners, one per fault (a detached thread, a leaked descriptor, a child left
running, a group whose member ignores its stop and overruns the reaper's bound, a dropped `Peer` without the §1.5 split);
a test asserts the guard fails on each with the fault named. A test with a child left on purpose (holding stdout) proves
the nextest setting fails it, once per OS leg.

### 6. Detached kinds (closed set, each capped and logged)

| Kind | For | Cap | Ends |
| --- | --- | --- | --- |
| `Relaunch` | the app starting its next instance or its updater | 1 per process | never (it is meant to outlive us); logged with the child's `ProcessId` |
| `OpenElsewhere` | a document, folder or URL opened in another program | none needed (the OS owns it; the child is not waited for); counted | never; logged |
| `UncancellableCall { what }` | a blocking OS or plugin call with no cancel (`getaddrinfo`, a hung network share, a stuck counter read); ADR 0020's `lease_abandonable` | 8 per process per `what` (ADR 0020's written-off cap); beyond it the call is refused with a message naming the cap | when the call returns; the caller got its error at its deadline |
| `ProcessLifetime { what }` | one thread for the whole process (the reaper, the log writer, the tray loop), and ADR 0074's lease holder process | 1 per `what` | at process exit; listed in the leak guard's per-process allowance |

Anything else is a `TaskGroup` member. A kind is added only by amending this table.

### 7. Agents: load and helpers have owners too

- `./dev burn --cores <n> --max <duration> [--target vm:<name>]` (`paddock-dev`): starts the load as children it holds,
  records their identities, ends them at `--max` or on `./dev burn --stop`, and refuses more than cores minus one (the
  2026-11-09 lesson). In a test VM it runs through ADR 0074's `lease run`, so the guest-side load has the lease as its
  owner and the next holder's restore ends what is left.
- A PreToolUse hook (as `.claude/hooks/no-pgrep-wait.sh`, with its `.test.sh`) refuses the known unbounded shapes
  (`yes > /dev/null &`, `while :; do :; done &`, `stress-ng` without `--timeout`, `nohup ... &` or `disown`) and names
  `./dev burn` or a bounded `./dev wait` as the next step.
- `./dev leftovers` lists processes whose command line or working folder names the agent's worktree or scratch folder
  (the method of C14's hermetic wrapper), by identity; the agent-cleanup checklist ends with it, and `--stop` ends them
  through `paddock_proc` (rule 53).

### 8. Logs (ER-27)

- Each `stop` and each reaper hand-off logs one line: owner, roles, outcome per member, duration, bound, overruns,
  whether the stop was explicit or a drop. No content.
- `detached(...)` logs the kind, role, reason id and the running count per kind; reaching a cap is an `error` line
  naming the kind and the next step.
- The reaper logs a summary every 10 minutes while anything is pending, and at Quit.
- The diagnostics bundle (the family's `paddock-diag`) gains `reaper::pending()`, the detached counts and the live
  thread names of the process.

### 9. Migration, ordered by release (rule 1; ADR 0049 small merges; rule 43 a shared change lands with its adoption)

| Merge | Content | Gate |
| --- | --- | --- |
| M0 | C42 in report mode with the register and the ratchet (every finding of today listed, `blocks_tag` per entry), the prototype's fixture as positive control, nextest `leak-timeout` in all seven configs (red tests fixed or ratcheted first) | none; changes no product |
| M1 | `paddock-tasks` (`TaskGroup`, `StopToken` moved from `paddock-jobs`, reaper, `Detached`), `paddock-testkit::leaks` with its `Broken` controls, on three OS | none |
| M2 | `paddock-ipc`: `Peer`/`BodyPeer` user-handle split, threads in a group, `wait_released` on the group, `LinkServer` and launch; `paddock-mcp` loopback; land together with ADR 0072 M2 to M4 (one change to the same files) | `dyno-v*` |
| M3 | **Dyno** (next release after 1.1.0): app, agent, helper, platform, plugin host; the shared crates Dyno ships (`paddock-proc` runners, `paddock-service`, `paddock-window`, `paddock-startup`, `paddock-health`, `paddock-shell`, `paddock-updater`, `paddock-watch`, `paddock-files`, `paddock-dialogs`, `paddock-chromium`) | `dyno-v*` |
| M4 | **Pitwall 1.1**: `pitwall-server` tokio tasks into groups on the runtime (`TaskGroup::task`), the app's tray and menu tasks, `pitwall-loopback`; shared crates it ships that M3 did not cover | `pitwall-v1.1*` |
| M5 | **Debrief 1.2**: store-duck (incident 2, failing test first), app start-up sweeps into an app-lifetime group stopped at Quit, plugin crates, `debrief-ai`, `chromium-driver`; `debrief-ipc` is a registered copy (rule 55) waived until it is replaced by `paddock-ipc` in 1.2 PH1-PH4 | `debrief-v1.2*` |
| M6 | Pitcrew 1.1 and Spotter (after its 1.0 tag): the remaining sites | `pitcrew-v1.1*`, `spotter-v1.1*` |
| M7 | C42 to error mode for every app whose ratchet is empty; rule text, lessons, ADR index | – |

Each merge removes its sites from the ratchet. **Rollback:** each merge reverts alone; no wire or file format changes.

## Parallelism and cost (rule 51)

- **Threads: none added** in steady state. One reaper thread per process, idle on a condvar (no polling). `TaskGroup`
  adds a few words and one `Arc` per member. A `StopToken` wait is a condvar wait: a stop wakes the member at once
  instead of at its next poll.
- **Stop and Quit latency:** bounded by design. Groups stop in parallel (each signals at once, the waits overlap), so a
  Quit costs the slowest member up to the app's quit bound (default 2 s, D4), not the sum. Today some stops never end
  (incident 1) and some are cut by process exit (incident 5).
- **Cheaper, not only bounded:** stop hooks unblock I/O at once (close the socket, interrupt the query) instead of
  waiting for a poll slice; 0072's sliced reads stay where a hook is impossible.
- **The check:** the syn pass took 5.90-6.04 s for the whole tree on the Mac mini at load 5
  ([benchmarks.md](../benchmarks.md#2026-11-09-a-syn-scan-of-every-spawn-site-in-the-monorepo-docsadr-0076-owned-background-work));
  the push check runs it on changed files only (milliseconds), CI on the whole tree.
- **The leak guard:** a process-table pass for children costs 13.6-14.7 ms on macOS (ADR 0033's recorded snapshot); thread
  and descriptor lists are one directory read or one system call each. It runs in the ending tests only (about one per
  owner type), not around every test.
- **To measure and record (rule 48):** Quit time p50/p99 per app before and after M3 to M6; the guard's cost per OS in
  WP-3; reaper hand-offs per hour in a day of normal use (expected 0 after migration).

## Work packages (raw estimates, one agent; re-estimated from the survey)

| WP | Content | Days |
| --- | --- | --- |
| WP-0 (M0) | Read the 66 "join in the file" sites and the 10 wrapper calls; write the register (`[[detached]]`, `[[exempt]]`, `[[owner]]`) and the ratchet with `blocks_tag`; nextest `leak-timeout` in seven configs and fix or ratchet what it turns red | 2 |
| WP-1 (M1) | `paddock-tasks`: `TaskGroup`, `StopToken` (moved, re-exported), reaper, `Detached` with caps, tokio feature, log lines; model tests on a `FakeClock` (stop within the bound, overrun handed off, drop never blocks, panics reported); crate coverage gate | 2.5 |
| WP-2 (M0/M1) | C42 in `paddock-dev`: the syn pass from the prototype (import resolution, wrappers, per-handle join tracking instead of the file heuristic), the register and ratchet comparison, the fixture positive control; the clippy preset lines and their fixture (D3) | 1.5 |
| WP-3 (M1) | `paddock-testkit::leaks::Guard` on Linux, macOS and Windows (probe choice measured), `settle`, the `Broken` controls, the per-test allowance for runtime threads | 2 |
| WP-4 (M2) | `paddock-ipc` and `paddock-mcp` owners (user-handle split, groups, `wait_released`), their ending tests; with ADR 0072's M2-M4 | 1 |
| WP-5 (M3) | Dyno and the shared crates it ships: 26 fire-and-forget or detached Dyno sites, 20 to read, about 35 shared sites; their ending tests; Dyno installers built (rule 54) | 2.5 |
| WP-6 (M4) | Pitwall 1.1: 25 sites, mostly tokio tasks; ending tests | 1 |
| WP-7 (M5) | Debrief 1.2: 44 sites plus 23 to read; incident 2 with its failing test first; start-up sweeps; plugin crates | 2.5 |
| WP-8 (M6) | Pitcrew and Spotter: 28 sites; ending tests | 1 |
| WP-9 | Agents: `./dev burn`, `./dev leftovers`, the hook with its test, the checklist line | 1 |
| WP-10 (M7) | Proposed rule text and ER candidate, lessons entry, ADR index, CHANGELOGs, ADR 0020/0024/0072 amendment notes | 0.5 |
| | **Total** | **17.5** |

**Range 15 to 20 days.** Above the 8-15 target the brief set, because the survey found 442 shipped sites, not the few
the incidents showed: 158 to migrate or register and 76 to read. The low end holds if D3 takes clippy alone (WP-2 to
1 day) and D8 keeps agents to the checklist (WP-9 to 0.25); the high end if the Windows leak probes need more than one
iteration or Pitwall's runtime shutdown needs its own design. **A smaller first cut (about 10 days):** WP-0 to WP-5 and
WP-10 (the mechanism, the check in report mode, the leak harness, `paddock-ipc` and Dyno), with WP-6 to WP-9 landing in
each app's own release phase as D7 orders; the ratchet keeps the rest visible and blocks each app's tag until done.

WP-0 and WP-1 run in parallel; WP-2 and WP-3 follow WP-1's API; WP-4 lands with ADR 0072's merges; WP-5 to WP-8 follow
the release order, one app at a time.

## Decisions for the user

| # | Question | Options | Recommendation |
| --- | --- | --- | --- |
| D1 | Where `TaskGroup` lives | (a) A new small crate `paddock-tasks` (std and `log`; a `tokio` feature), with `CancelToken`/`StopToken` moved there and re-exported by `paddock-jobs`; ADR 0020's executor builds on it. (b) A module of `paddock-jobs` (where ADR 0020 puts the executor): `paddock-ipc`, `paddock-mcp` and the 46 crates with spawn sites that do not depend on `paddock-jobs` would pull in `croner`, `jiff`, `sha2` and `serde`. (c) `paddock-proc` (it owns children): 46 of the 64 crates with sites do not depend on it, and it pulls in `sysinfo`. (d) `paddock-runtime`: it is the pinned-download crate (Chromium, FFmpeg, llama.cpp); the name would mislead. | (a): interface segregation, one token for cancel and stop (rule 55). |
| D2 | What `Drop` of an owner does | (a) Signal, hand to the reaper with the bound, never block, log at `warn`. (b) Signal and join with the bound inside `Drop` (blocks a UI thread or an unwind for up to the bound). (c) Signal only. | (a). |
| D3 | The static check | (a) C42 as a syn pass in `paddock-dev` plus the clippy `disallowed-methods` lines (compiler-resolved paths), the register and the ratchet. (b) The syn pass only. (c) Clippy only (no register, no handle disposition). | (a), falling back to (b) if WP-2 shows clippy cannot match `Builder::spawn` or tokio's re-exports. |
| D4 | Bounds | Group stop bound 2 s by default (per group, set where the group is made); a `Peer` group 0072's 4.25 s; the reaper's error line at the bound; the app's quit bound 2 s after the last group stop; `UncancellableCall` cap 8 per `what`. Protocol defaults in code, not user Settings (no user tunes them). | Accept. |
| D5 | The detached kinds | §6's closed set of four kinds, a register entry per site, amended only by amending this ADR. | Accept. |
| D6 | ADR 0072 amendment | Add `OwnerReason::Dropped` to 0072's closing vocabulary, so a peer whose last handle was dropped ends with a reason (`stopping`-class code on the wire, `Dropped` in the log) instead of a bare end. No frame change. | Yes. |
| D7 | Release gating | (a) The ratchet's `blocks_tag` blocks Dyno's next tag, Pitwall 1.1, Debrief 1.2 and Pitcrew 1.1 on their own entries; Spotter 1.0 is not blocked (it is in flight; its entries block `spotter-v1.1*`). (b) Report only, no tag blocks. | (a), as ADR 0072 D8. |
| D8 | Agents | (a) `./dev burn`, `./dev leftovers` and the hook (WP-9). (b) The checklist line only. | (a): the incident left nothing to clean up with; a hook is what held for `pgrep` waits (ER-32). |
| D9 | A binding rule | Propose a Paddock rule (number from the coordinator) and an ER candidate: "every thread, task and child process has one owner that holds its handle; stop signals and joins within a bound and reports; drop hands off to the reaper without blocking; detached work only through a registered kind with a cap; every owner type has an ending test that finds nothing left". | Yes, with C42 and the leak guard as its checks. |
| D10 | Scope now | (a) The whole migration in each app's release (17.5 days, D7 gates). (b) The first cut now (about 10 days: mechanism, check, harness, `paddock-ipc`, Dyno), the rest in each app's phase. | (b): it closes incident 1 for good and the class for the next release; the ratchet keeps the rest enforced. |

## Consequences

- A dropped owner releases its threads, children and descriptors within a known bound, or the log names the member that
  did not; "Directory not empty" after a drop and EMFILE from dropped peers become test failures with a name.
- Quit ends every owned thread and child or says which one it could not.
- Every new spawn is either a member of an owner or a registered detached kind; a raw spawn does not pass the push check.
- More ceremony per start (an owner, a role, a bound), checked once. Before v1, no shims: raw helpers are replaced, not
  wrapped.
- ADR 0020's executor, when built, inherits ownership; ADR 0024's C42 is built here, with handle disposition.

## Alternatives rejected

- **Fix each instance where it is found** (the rig fix of incident 1): right for the rig, and the next owner type leaked
  the same day.
- **Join in `Drop` without a bound:** a member that ignores its stop hangs the dropping thread, a UI thread included.
- **Detach and rely on process exit:** cuts deletes and writes in the middle (incident 5) and leaves the work running in
  tests, which share one process for minutes (incidents 1 and 2).
- **Only the executor of ADR 0020:** it is large and not started; the ownership rule is small, needed now, and the
  executor can be built on it.
- **A leak check around every test:** runtime threads and lazily started process-wide threads would need broad
  allowances that hide real leaks; per-owner ending tests plus nextest's leak setting cover the class with less noise.
- **A grep-based check:** it cannot tell inline test modules from production code or a kept handle from a dropped one
  (the survey's 442 against ADR 0024's grep counts).

## The failing tests the fixes start with (rule 5 / ER-11; not written in this change)

- **Incident 1, `paddock-ipc`** (`tests/it/peer.rs` or the endings harness of ADR 0072):
  `a_dropped_body_peer_releases_its_threads_and_descriptors`: open an in-process pair, take a `leaks::Guard`, start a
  `BodyPeer`, make one call, drop it without `close()`; assert the host side sees the connection end within 0072's bound
  and `guard.check(bound)` finds no new thread or descriptor. Fails on `origin/main`: the three peer threads and the
  descriptors stay (the measured 5 threads and 6 descriptors per connection include both ends).
- **Incident 2, `debrief-store-duck`** (unit test in `src/fts.rs`, with a test seam after a `Worked` turn, ADR 0041):
  `a_stopped_index_service_waits_for_its_merges`: index enough segments that a merge is scheduled, set stop at the seam
  right after a `Worked` turn, drop the `FtsService`; assert the writer slot is empty and `wait_merging_threads` ran (a
  test counter), and with the guard that no `merge_thread_` or `thrd-tantivy-index` thread is left. Today the loop leaves
  with the writer still in the slot (`fts.rs:820-834`), so the assertion fails deterministically. ER-28: the cause of the
  two CI failures is confirmed only when the two integration tests stay green under a full nextest run as often as their
  failure rate requires with the fix, and fail again with it reverted; the other candidates to rule out by measurement are
  (b) the console timer thread (not on these tests' path), (c) another `Arc` holding the store's shared state after
  `drop(store)` (count the strong references at drop), and (d) DuckDB's own checkpoint or WAL write at close (list the
  files that make the folder non-empty when `remove_dir_all` fails).
- **Incident 3, test children:** `sys.rs:607-613` and `quit.rs:831-836` move to `paddock_proc::testchild` children held
  by `Supervised` (ended on drop); nextest's `leak-timeout` (M0) is the check, proven by its own positive control.

## Appendix: shipped sites classified fire-and-forget (F) or owned but detached (D)

Line numbers at `origin/main` `b51c72509`; kind `Thread`, `Task` (tokio or Tauri async), `Blocking` (`spawn_blocking`),
`Process`, `wrapper` (a call of a project function named `spawn`). WP-0 confirms each row and adds the 66 "join in the
file" sites it reads.

| File | Sites (line, class/kind) |
| --- | --- |
| `apps/debrief/app/src-tauri/src/after_first_paint.rs` | 66 D/Thread, 81 F/Thread |
| `apps/debrief/app/src-tauri/src/drag_ghost/mod.rs` | 401 D/Thread |
| `apps/debrief/app/src-tauri/src/health/hub.rs` | 363 D/Thread |
| `apps/debrief/app/src-tauri/src/jobs/service.rs` | 509 F/wrapper |
| `apps/debrief/app/src-tauri/src/lib.rs` | 649 F/Thread, 687 F/Thread |
| `apps/debrief/app/src-tauri/src/lifecycle.rs` | 727 F/Process |
| `apps/debrief/app/src-tauri/src/live.rs` | 425 F/Thread |
| `apps/debrief/app/src-tauri/src/opener.rs` | 233 F/Thread |
| `apps/debrief/app/src-tauri/src/plugin_packages/commands.rs` | 167 F/Blocking |
| `apps/debrief/app/src-tauri/src/settings.rs` | 571 D/Thread |
| `apps/debrief/app/src-tauri/src/tools.rs` | 98 F/Thread |
| `apps/debrief/app/src-tauri/src/tray.rs` | 303 F/Blocking |
| `apps/debrief/app/src-tauri/src/updater.rs` | 258 D/Task, 471 D/Process |
| `apps/debrief/app/src-tauri/src/video/media.rs` | 78 F/Blocking |
| `apps/debrief/crates/chromium-driver/src/cdp.rs` | 80 F/Thread |
| `apps/debrief/crates/chromium-driver/src/launch.rs` | 393 F/Thread |
| `apps/debrief/crates/debrief-ai-worker/src/child.rs` | 396 F/Thread |
| `apps/debrief/crates/debrief-ai/src/engine/pitwall/mod.rs` | 382 F/Thread |
| `apps/debrief/crates/debrief-ai/src/llm/lazy.rs` | 99 F/Thread, 177 D/Thread |
| `apps/debrief/crates/debrief-ai/src/worker/frames.rs` | 93 F/Thread, 258 F/Thread |
| `apps/debrief/crates/debrief-collect/src/runner.rs` | 62 F/Thread |
| `apps/debrief/crates/debrief-core/src/events.rs` | 209 F/Thread |
| `apps/debrief/crates/debrief-core/src/plugin/watchdog.rs` | 51 D/Thread |
| `apps/debrief/crates/debrief-ipc/src/child.rs` | 75 F/Thread |
| `apps/debrief/crates/debrief-ipc/src/peer.rs` | 185 F/Task, 186 F/Task, 413 F/Blocking |
| `apps/debrief/crates/debrief-ipc/src/runtime.rs` | 31 F/Task |
| `apps/debrief/crates/debrief-plugin-api/src/forward.rs` | 79 F/Thread, 97 F/Thread |
| `apps/debrief/crates/debrief-plugin-dotnet/src/eventpipe/mod.rs` | 61 F/Thread, 133 F/Thread, 173 F/Thread |
| `apps/debrief/crates/debrief-plugin-dotnet/src/plugin/tool.rs` | 78 F/Thread |
| `apps/debrief/crates/debrief-plugin-host/src/builtins.rs` | 205 F/Thread |
| `apps/debrief/crates/debrief-plugin-native/src/tool.rs` | 68 F/Thread, 145 D/wrapper, 368 D/wrapper |
| `apps/debrief/crates/debrief-plugin-windows/src/command.rs` | 50 F/Thread |
| `apps/dyno/app/src-tauri/src/lib.rs` | 492 F/wrapper, 501 F/Thread, 507 F/Thread, 518 F/Thread, 527 F/Thread |
| `apps/dyno/app/src-tauri/src/link/driver.rs` | 72 F/Thread, 116 F/Thread, 368 D/Thread |
| `apps/dyno/app/src-tauri/src/shell.rs` | 404 F/Thread, 442 F/Thread, 720 F/Thread |
| `apps/dyno/app/src-tauri/src/tray_away.rs` | 34 F/Thread |
| `apps/dyno/app/src-tauri/src/tray_host.rs` | 308 F/Thread |
| `apps/dyno/crates/dyno-agent/src/bench_child.rs` | 244 F/Thread, 284 F/Thread |
| `apps/dyno/crates/dyno-agent/src/disk/volumes.rs` | 53 F/Thread |
| `apps/dyno/crates/dyno-agent/src/extras/finder.rs` | 122 D/Thread |
| `apps/dyno/crates/dyno-agent/src/extras/mod.rs` | 462 F/Thread |
| `apps/dyno/crates/dyno-agent/src/handlers.rs` | 236 F/Thread |
| `apps/dyno/crates/dyno-agent/src/helper_readers/counters.rs` | 292 D/Thread |
| `apps/dyno/crates/dyno-agent/src/helper_readers/processes.rs` | 513 D/Thread |
| `apps/dyno/crates/dyno-agent/src/inventory.rs` | 20 F/Thread |
| `apps/dyno/crates/dyno-agent/src/network.rs` | 182 F/Thread, 271 F/Thread |
| `apps/dyno/crates/dyno-agent/src/server.rs` | 117 F/Thread |
| `apps/dyno/crates/dyno-platform/src/process.rs` | 200 F/Thread |
| `apps/pitcrew/app/src-tauri/src/channels.rs` | 134 F/Thread |
| `apps/pitcrew/app/src-tauri/src/events.rs` | 113 F/Thread, 173 F/Thread |
| `apps/pitcrew/app/src-tauri/src/handoff.rs` | 185 D/Process |
| `apps/pitcrew/app/src-tauri/src/lib.rs` | 109 F/Thread |
| `apps/pitcrew/app/src-tauri/src/service_client.rs` | 114 F/Thread |
| `apps/pitcrew/app/src-tauri/src/service_install.rs` | 366 F/Thread |
| `apps/pitcrew/crates/pitcrew-service/src/host.rs` | 53 F/Task |
| `apps/pitcrew/crates/pitcrew-service/src/ipc.rs` | 227 F/Task, 258 F/Task |
| `apps/pitcrew/crates/pitcrew-service/src/launch.rs` | 52 D/Thread, 216 D/Process |
| `apps/pitcrew/crates/pitcrew-service/src/supervisor.rs` | 1044 D/Thread |
| `apps/pitcrew/crates/pitcrew-service/src/supervisor/ops.rs` | 24 F/Thread, 32 F/Thread, 48 F/Thread, 61 F/Thread, 105 F/Thread |
| `apps/pitwall/app/src-tauri/src/app_menu.rs` | 61 F/Task |
| `apps/pitwall/app/src-tauri/src/commands.rs` | 400 D/Task |
| `apps/pitwall/app/src-tauri/src/lib.rs` | 77 F/Task, 82 F/Task, 213 F/Thread, 231 F/Task, 260 F/Task |
| `apps/pitwall/app/src-tauri/src/service_install.rs` | 412 F/Thread |
| `apps/pitwall/crates/pitwall-loopback/src/lib.rs` | 165 F/Task |
| `apps/pitwall/crates/pitwall-server/src/app.rs` | 353 F/Task, 550 F/Task |
| `apps/pitwall/crates/pitwall-server/src/cache_db.rs` | 132 F/Thread |
| `apps/pitwall/crates/pitwall-server/src/engine_setup.rs` | 97 F/Task |
| `apps/pitwall/crates/pitwall-server/src/engines.rs` | 650 D/wrapper, 655 F/Task |
| `apps/pitwall/crates/pitwall-server/src/http.rs` | 739 F/Task |
| `apps/pitwall/crates/pitwall-server/src/library.rs` | 349 F/Task |
| `apps/pitwall/crates/pitwall-server/src/mgmt.rs` | 509 F/Task |
| `apps/pitwall/crates/pitwall-server/src/service/watch.rs` | 110 F/Task |
| `apps/pitwall/crates/pitwall-server/src/serving.rs` | 791 F/Task, 1039 F/Task, 1067 F/Task, 1088 D/Task, 1208 F/Task |
| `apps/pitwall/crates/pitwall-server/src/updates.rs` | 414 F/Task |
| `apps/spotter/app/src-tauri/src/commands.rs` | 187 F/Task |
| `apps/spotter/app/src-tauri/src/lifecycle.rs` | 131 F/Task, 135 F/Task, 138 F/Task, 185 F/Task |
| `apps/spotter/app/src-tauri/src/notifier.rs` | 164 F/Task |
| `apps/spotter/crates/spotter-engine/src/check.rs` | 158 F/Task |
| `apps/spotter/crates/spotter-render/src/cdp.rs` | 89 F/Thread |
| `apps/spotter/crates/spotter-render/src/launch.rs` | 350 F/Thread |
| `apps/spotter/crates/spotter-sources/src/folder.rs` | 106 F/Thread |
| `crates/paddock-channels/src/control.rs` | 378 F/Thread |
| `crates/paddock-chromium/src/cdp.rs` | 104 F/Thread |
| `crates/paddock-chromium/src/launch.rs` | 817 F/Thread |
| `crates/paddock-diag/src/tauri.rs` | 384 F/Blocking |
| `crates/paddock-dialogs/src/probe.rs` | 474 F/Thread, 483 F/Thread |
| `crates/paddock-dialogs/src/registry.rs` | 44 F/Thread |
| `crates/paddock-download/src/engine.rs` | 879 D/Thread |
| `crates/paddock-guard/src/guard.rs` | 215 D/Thread |
| `crates/paddock-health/src/health.rs` | 130 D/Thread |
| `crates/paddock-health/src/tauri/native.rs` | 137 D/Process |
| `crates/paddock-ipc/src/child.rs` | 55 D/Thread |
| `crates/paddock-ipc/src/peer.rs` | 257 D/Thread |
| `crates/paddock-live/src/tauri.rs` | 117 D/Thread |
| `crates/paddock-log/src/child.rs` | 74 D/Thread |
| `crates/paddock-mcp/src/bridge.rs` | 157 F/Thread |
| `crates/paddock-mcp/src/server.rs` | 619 F/Thread |
| `crates/paddock-panels/src/tauri/native.rs` | 108 D/Thread |
| `crates/paddock-pitwall/src/service.rs` | 561 F/Thread |
| `crates/paddock-proc/src/run.rs` | 176 F/Thread, 245 F/Thread |
| `crates/paddock-service/src/backend/windows.rs` | 158 F/wrapper |
| `crates/paddock-service/src/system.rs` | 87 D/Process |
| `crates/paddock-shell/src/setup/tauri.rs` | 26 D/Thread |
| `crates/paddock-siblings/src/native.rs` | 21 D/Process |
| `crates/paddock-startup/src/tauri.rs` | 446 D/Thread, 661 F/Thread, 789 D/Thread |
| `crates/paddock-storage/src/tauri.rs` | 168 F/Blocking |
| `crates/paddock-updater/src/tauri/native.rs` | 25 D/Process |
| `crates/paddock-updater/src/updater.rs` | 593 F/Thread |
| `crates/paddock-window/src/tauri/manager.rs` | 974 D/Thread |
| `crates/paddock-window/src/tauri/plugin_hook.rs` | 38 F/Task |
| `crates/paddock-window/src/tauri/popover.rs` | 401 F/wrapper, 463 F/wrapper |
