# 76. Owned background work: every thread, task and child process has an owner that ends it within a bound and says so

- Status: Accepted 2026-11-09 by the user: D1–D10 as recommended (D7 = c). Rule 74. History: Proposed (2026-11-09);
  revision 1 (`368696bbd`) reviewed independently (CHANGES, five P1s, six P2s); revision 2 (`282a73305`) answered them
  (the survey's classifier fixed and rerun with a hand check of every class, a non-owning handle for call contexts, D1
  re-weighed on the real dependencies, the soundness of stop, drop and the reaper with a model test each, the estimate
  recomputed with per-release days, the coupling to ADR 0072 and D7 option (c)); the re-review said ACCEPTABLE, and
  revision 3 applies its P2s (per-binding handle tracking with shadowing, two more discards, `run_bounded`'s drain and
  waiter threads kept in the ratchet with their ending test, children ended inline when the reaper cannot start, the
  self-join example, `Subscriptions` and the Debrief waiver's `ctx.peer()` uses, the latch wording).
- Trimmed 2026-11-11 (user, comparable-practice review): TRIM: keep WP-X0 (crash fix), WP-0's `leak-timeout` part only (set to fail), a small `TaskGroup` from WP-1, Debrief incident 2 and start-up sweeps from WP-7, Pitwall's `shutdown_timeout` and tokio groups (WP-6, X5), WP-4's `Peer` split only if a kept item needs it; dropped C42 and its register (WP-2, X2), the testkit leak `Guard` (WP-3), `./dev burn` and `./dev leftovers` (WP-9), the generic exit framework (X1), the Dyno, Pitcrew and Spotter site sweeps.
- Rule: **rule 74**, assigned on acceptance (2026-11-09). `docs/rules.md` on `origin/main` ends at rule 71; the open
  rule branches use 56–59, 65 and 66; family ADR 0057's branch claims 72 and ADR 0073 took 73 (`a467cdd2e`); ADRs 0071,
  0072, 0074 and 0075, accepted the same day, assign none. WP-10 adds the text (D9) to `docs/rules.md`.
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
  0072's ordering. It amends 0072's `OwnerReason` by one variant (D6) and lands its `paddock-ipc` part inside 0072's
  merges (§9).
- Not in scope, by design: start waits (**ADR 0071**, ready or exit), shared test machines and their lease holders
  (**ADR 0074**, `./dev machine lease` with `--owner-pid`; this ADR only requires that the lease holder is a registered
  detached process, §6), test doubles (ADR 0073).
- Number: 0076 was free on every `origin` branch on 2026-11-09 (family ADRs on branches end at 0074; 0075 is being drafted
  for copy waivers). Check number: this ADR **builds C42** as ADR 0024 numbered it, and needs no new number.
- Evidence read at `origin/main` `b51c72509` unless a row names a branch. The survey is a syn-based prototype of the check
  (scratch: `/Volumes/d7sst7/dev-targets/scratch/adr-0076/scan`, revision 2's output `sites3.tsv` next to it); its cost is
  recorded in [benchmarks.md](../benchmarks.md#2026-11-09-a-syn-scan-of-every-spawn-site-in-the-monorepo-docsadr-0076-owned-background-work).

## Pattern

**Background work outlives what started it.** Whenever a component starts a thread, an async task or a child process, it
decides on its own whether to keep the handle, whether to signal and wait for the work when it stops or is dropped, and
for how long. Most do not keep the handle at all; many keep it but have no stop path that waits; a few wait without a
bound. The work then runs on after its owner is gone, holding files, descriptors, folders, locks, CPU or a connection,
and nothing in the log says so.

**The invariant that should hold.** Every thread, task and child process started by Paddock code is held by exactly one
owner. The owner's explicit stop signals the work and waits for it to end within a declared bound, then reports what
ended, how long it took and what overran. Dropping the owner without a stop does the same without blocking: it signals,
and hands the handles to one reaper that waits within the same bound and logs the outcome. Work that must outlive its
owner (a relaunched app, a document opened in another program, a call into the OS that cannot be cancelled) is detached
only through a declared kind with a registered reason and a cap. A test that builds and ends an owner finds no thread,
task, child process or descriptor of it left.

## Context: the incidents (2026-11-01 to 2026-11-09)

| # | Where | What happened | Why | State |
| --- | --- | --- | --- | --- |
| 1 | `paddock-ipc` `Peer`/`BodyPeer`, seen in Dyno's `helper_seam` tests (CI run 7763, Linux) | "the host closed the connection during the handshake"; measured in the CI image: each dropped `BodyPeer` kept 6 descriptors and 5 threads, about 250 connections in 5 s, the in-process agent hit EMFILE (lessons on `origin/train/2026-11-08-1`, `docs/lessons.md:1551-1573`) | `Peer` is `Clone` around `Arc<Inner>` (`crates/paddock-ipc/src/peer.rs:242-244`) and has no `Drop`; its reader, writer and push threads are started by `spawn(...).map(drop)` (`peer.rs:254-260`), each holding a clone of `Inner`, so dropping the last user handle frees nothing and nothing ever joins them. `BodyPeer` (`body.rs:96`) has no `Drop` either. `CallCtx::peer()` (`peer.rs:1038-1048`) hands handlers an owning `Peer` minted on the reader thread, and handlers keep clones of it (`crates/paddock-plugin-api/src/serve.rs:142`, `PeerHost(self.0.peer().clone())` inside an `Arc<dyn Host>`). | Fixed in the test rig only (`adc28dd0a`: the rig closes each connection). The lesson ends "Class: a handle whose drop does not release its resource (`BodyPeer`, `Peer`); reported for an ER-21 decision." This ADR is that decision. |
| 2 | Debrief `debrief-store-duck` tests | "Directory not empty" from `remove_dir_all` at `tests/it/segments.rs:240` (the moved store) and `tests/it/search.rs:220` (`index` right after `drop(store)`), once each under a full nextest run; 30 of 30 green alone (lessons on `origin/fix/debrief-lint-1-temp-client`, `docs/lessons.md:1503`) | **Not measured.** Lesson's probable cause: the store's index threads still write after `drop`. Candidates found for this ADR by reading (ER-28 list in "The failing tests the fixes start with"): (a) the FTS service joins its own thread on drop (`src/fts.rs:685-693`) but its loop leaves on `stop` without `drop_writer` when a turn just worked or segments are pending (`fts.rs:820-834`), and the error path drops the writer with `*s.writer = None` (`fts.rs:828`); the Tantivy `IndexWriter` is then dropped with the shared state, and Tantivy 0.27's `Drop for IndexWriter` only kills the segment updater and joins the indexing workers (`index_writer.rs:821-828`), while merges already scheduled on its `merge_thread_{i}` pool (`segment_updater.rs:312-313`, `:541`) keep writing into `index/` until done; only `wait_merging_threads` waits for them (`index_writer.rs:355-376`). (b) the SQL console's timer thread is bound, inspected for `Err` and dropped, never joined (`src/console/mod.rs:215-222`). | Open; WP-7 starts with the failing test in "The failing tests the fixes start with". |
| 3 | Test children | A test that fails before it kills its children leaves them running: `paddock-proc` `sys.rs:607-613` starts three `sleep 60` as plain `std::process::Child` in a `Vec` (no guard), Dyno `app/src-tauri/src/quit.rs:831-836` one `sleep 60` (or `ping -n 60`); `std::process::Child` has no `Drop` and is not waited for ([std docs](https://doc.rust-lang.org/std/process/struct.Child.html), read 2026-11-09). The interim VM lock's holder (`scripts/vm-lock.sh` on `origin/fix/vm-smoke-take-vm-lock`) is a detached process that lives as long as an owner folder, which a killed script leaves until the next acquirer removes it. | No owner kills them on a failed assert; the holder's life is tied to a folder, not to a process identity. | The VM holder is replaced by ADR 0074's lease (`--owner-pid`, by identity); the test children are covered by §5. |
| 4 | Agents | 2026-11-01: a flake researcher's 20 unbounded `sh -c 'while :; do :; done'` burners drove the Mac's load to 130 while the macOS runner ran Dyno's release tests, which timed out (coordinator memory `agent-concurrency.md`, entry "2026-11-01 incident"; outside the repository). 2026-11-09: an agent started one busy process per vCPU in the Windows test VM over SSH; sshd stopped answering and the clean-up in the same session never ran (lessons on `origin/train/2026-11-08-1`, `docs/lessons.md:1543-1549`). | The load had no owner, no deadline of its own and no recorded identity. | §7. |
| 5 | Debrief start-up sweeps and other detached threads | `apps/debrief/app/src-tauri/src/lib.rs:649` (old logs pruned) and `:687` (scratch stores swept and removed) run in `std::thread::spawn` whose handles are dropped; Quit cuts them wherever they are. 200 fire-and-forget starts in shipped code (survey). | No owner; at best idempotent by luck. | At risk, not seen failing. |

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
`run_bounded`, also inside macro arguments (parsed as expressions; no macro that mentions `spawn` failed to parse).

The handle's fate is an enum, not a string, in revision 2:

| Disposition | Meaning | Class |
| --- | --- | --- |
| `Discarded` | `spawn(..);`, `let _ = ..`, `.map(drop)`, `.map(|_| ())`, `.ok().map(|h| h.thread().id())` (the handle only looked at), `if let Err(e) = spawn(..)` | fire-and-forget |
| `HeldUnused` | `let _x = spawn(..)`, `_x` never used | fire-and-forget |
| `BoundNotJoined` | bound to a name that is only inspected (`if let Err(e) = x`) or `drop(x)`ped, never joined or stored, in the statements after the binding up to a `let` that shadows it | fire-and-forget |
| `JoinedAtOnce` | `.join()`, `.await`, `.wait()` on the spawn itself | owned and joined |
| `BoundJoined` | bound (also through `if let Ok(h) = x` or `match x { Ok(h) => .. }`) and joined, awaited, waited for or terminated in the function | owned and joined |
| `BoundStored`, `Returned`, `InExpr` | stored in a field or list, returned, or passed on | "join in the file" if the file joins or waits anywhere (to read), else owned but detached |

`thread::scope` spawns, `Supervised`, `run_bounded` and `.output()`/`.status()` are owned and joined by construction.

**Revision 1's bug (P1-1).** Revision 1 matched dispositions as substrings, and "joined in fn" also matched "not joined in
fn", so 57 never-joined sites counted as joined; `.map(drop)` was read as a returned handle; spawns in macro arguments
were not visited; and `if let Err(e) = spawn(..)` and `drop(x)` counted as stored. The prototype's fixture now has a case
for each (bound and `if let Err`, bound and `if let Ok` then stored, `match` then joined, `.map(drop)`, `drop(x)`, a
spawn inside `vec![..]`, `if let Err` on the spawn itself, `if let Ok(h)` on the spawn itself) and the reviewer's own
fixture; both report every case as expected. The reviewer's named sites now read: `paddock-ipc/src/link/server.rs:197`,
Dyno `quit.rs:321`, `debrief-store-duck/src/console/mod.rs:215`, `paddock-mcp/src/loopback.rs:398` and `:411`
(fire-and-forget, the first four; stored in a list, the last two, to read), `paddock-health/src/notify/writer.rs:56` and
`paddock-service/src/control.rs:260` (`.map(drop)`: fire-and-forget), `paddock-ipc/src/peer.rs:257` (fire-and-forget).

**Revision 3 (the re-review's P2).** A binding is followed only through the statements after it in its block, up to a
`let` that shadows the name (whose own initializer still sees the old binding, `let c = c.join()`), instead of the whole
function body; `.map(|_| ())` and `.ok().map(..)` that only look at the handle are discards. The fixture gained the
shadowed cases (a joined name shadowed by a dropped one and the reverse, a handle joined by its own shadowing `let`) and
both new discards: 24 cases, all as expected. Eight shipped sites changed class, all read and correct:
`paddock-guard/src/guard.rs:215`, `paddock-download/src/engine.rs:879`, `paddock-panels/src/tauri/native.rs:108`,
Dyno `dyno-agent/src/disk/engine.rs:102`, Debrief `debrief-core/src/plugin/watchdog.rs:51` and
`app/src-tauri/src/health/hub.rs:363`, Pitcrew `app/src-tauri/src/handoff.rs:185` (to fire-and-forget: `.map(|_| ())` or
`.ok().map(|_| dog)`), and one test-support site.

**Hand check of every class (revision 2).** 24 sites, 6 per class drawn at random (seed 763) from the final output, read:
all 24 are in the right class; one has the wrong kind (`pitwall-server/src/engines.rs:650` is a child process started by
Pitwall's own command helper, counted as a wrapper call). An earlier draw of 32 (seeds 761 and 762) found the three
misreadings that the `if let Err`, `if let Ok` and `drop(x)` rules now fix (`dyno/app/.../link/driver.rs:368`,
`debrief/app/.../settings.rs:571`, `pitcrew-service/src/launch.rs:52`).

**Remaining limits** (WP-2 removes them): "join in the file" means a join, wait or kill exists somewhere in the file, not
that it is this handle's (66 sites to read); a call of a project's own wrapper named `spawn` (`self.spawn(..)`,
`captures.spawn(..)`) is a site of unknown kind (10); one wrapper may start several threads per call (`peer.rs:254`, three
per connection); inline `#[cfg(test)]` modules are not counted.

**Counts.** 615 sites: 442 in shipped code (249 files, 64 crates), 129 in tests, 29 in test support, 13 in dev tools,
2 vendored (`apps/dyno/vendor/tray-icon`). (Revision 1 reported 647 found: that total also counted the 32
`thread::scope(..)` calls themselves, which start nothing; the 442 shipped sites are the same sites in both revisions,
reclassified.)

| Shipped, by kind | Owned and joined | Owned, a join in the file (to read) | Owned but detached | Fire-and-forget | Total |
| --- | --- | --- | --- | --- | --- |
| Threads (`thread::spawn`, `Builder::spawn`) | 8 | 59 | 8 | 147 | 222 |
| Scoped threads (`thread::scope`) | 30 | – | – | – | 30 |
| Blocking tasks (`spawn_blocking`) | 81 (awaited) | – | – | 6 | 87 |
| Async tasks (tokio, Tauri `async_runtime`) | – | – | 3 | 33 | 36 |
| Child processes: `.spawn()` | 19 | 7 | – | 8 | 34 |
| Child processes: `Supervised`, `run_bounded`, `.output()`/`.status()` | 23 | – | – | – | 23 |
| Calls of a project wrapper named `spawn` | 1 | – | 3 | 6 | 10 |
| **Total** | **162** | **66** | **14** | **200** | **442** |

| Shipped, by owner | Owned and joined | A join in the file | Owned but detached | Fire-and-forget | Total | To migrate or register (detached + fire-and-forget) |
| --- | --- | --- | --- | --- | --- | --- |
| Shared crates (`crates/`) | 50 | 16 | 5 | 38 | 109 | 43 |
| Dyno | 19 | 22 | 2 | 50 | 93 | 52 |
| Pitwall | 15 | 2 | 3 | 23 | 43 | 26 |
| Debrief | 65 | 23 | 4 | 61 | 153 | 65 |
| Pitcrew | 4 | 3 | – | 18 | 25 | 18 |
| Spotter | 8 | – | – | 10 | 18 | 10 |
| workbench-demo | 1 | – | – | – | 1 | – |
| **Total** | **162** | **66** | **14** | **200** | **442** | **214** |

Notes on the classes:
- **Only 8 plain threads are joined where they are started.** Most threads that are joined at all are joined from a
  `Drop` or a stop method elsewhere in the file (the 59 to read).
- **"Owned and joined" is not yet "ended within a bound".** 81 of the 162 are `spawn_blocking(..).await`: if the awaiting
  future is dropped (a cancelled command), the blocking work runs on ([tokio `JoinHandle`](https://docs.rs/tokio/latest/tokio/task/struct.JoinHandle.html):
  "tasks spawned using `spawn_blocking` cannot be aborted"; read 2026-11-09). Most joins have no bound.
- **Detached by design, legitimately** (to be registered, §6), from reading: relaunching the app (`debrief/app/.../lifecycle.rs:727`,
  `updater.rs:471`, `paddock-updater/src/tauri/native.rs:25`), opening a document or URL in another program
  (`paddock-files/src/open.rs:309-328`, `debrief/app/.../opener.rs:231-233`, `paddock-health/src/tauri/native.rs:137`,
  `paddock-siblings/src/native.rs:21`), and calls the OS cannot cancel (Spotter `spotter-sources/src/folder.rs:106`, a
  hung network share by design; Dyno `dyno-agent/src/network.rs:271`, a name lookup; `dyno-agent/src/extras/finder.rs:122`,
  `abandonable`). Watchdogs (`paddock-chromium/src/launch.rs:817` and its copies in `chromium-driver` and `spotter-render`)
  are not detached by nature: they become members of the browser's group (§6).
- **Already right, and the model for the mechanism:** `paddock_proc::Supervised` ends its whole tree on drop and registers
  it for `end_all` (`supervised.rs:1-9`, `:486`); `debrief-store-duck`'s `CatalogueService` and `FtsService` signal and
  join on drop (`catalogue.rs:206-214`, `fts.rs:685-693`); `paddock_proc::cleanup::join` logs a panicked join
  (`cleanup.rs:27`). None of them bounds the join or reports its duration.
- **ADR 0024's count** (raw grep of thread spawns, 2026-10-28): shared 71, Debrief 127, Dyno 49, Pitcrew 24, Spotter 10,
  Pitwall 7. The syn count of threads only (222) is lower, because it skips inline test modules; it is higher for
  Pitwall because it also counts tokio tasks.

The full list of the 214 fire-and-forget and owned-but-detached shipped sites is appendix A; the 66 "join in the file"
sites are appendix B and are read in WP-0.

**Known entries that stay in the ratchet for now.** `paddock-proc`'s `run_bounded` starts a drain thread per pipe
(`src/run.rs:176`) and a waiter thread (`:245`) and drops their handles: when a grandchild inherited the pipe, the drain
thread blocks until that grandchild exits, after `run_bounded` returned. They stay ratchet entries (they hold a pipe:
tag-blocking under D7 (c)) until WP-5 makes them members of a group that `run_bounded` ends with the tree; the ending
test starts a child that leaves a grandchild holding stdout, asserts that `run_bounded` returns at its limit, and that the
guard finds neither the grandchild nor the drain thread afterwards.

## Decision

### 1. The ownership rule

1. **Every start has an owner.** Production code starts a thread, task or child process only through an owner: a
   `TaskGroup` (§2), `paddock_proc::Supervised` (children), `std::thread::scope` (joined by construction), or a
   registered detached kind (§6). Raw `thread::spawn`, `Builder::spawn`, `tokio::spawn`, `task::spawn_blocking`,
   `tauri::async_runtime::spawn*` and `Command::spawn` are allowed only inside the mechanism's own module and
   `paddock-proc`.
2. **Stop is a command, and it waits within a bound.** `stop(bound)` signals every member (its `StopToken`; its
   `on_stop` hooks unblock I/O: close a socket, interrupt a DuckDB query), then waits for them until the bound, and
   returns a `StopReport`: each member's role, outcome (`ended`, `panicked`, `overran`), duration. Members that overran
   are handed to the reaper (§3) and logged as such; `stop` never waits past its bound (ER-32).
3. **Drop does the same without blocking.** Dropping an owner that was not stopped only sets its token and hands the
   group to the reaper; it never waits, kills a child or runs a hook inside `Drop` (safe on a UI thread and during an
   unwind, as ADR 0072's `Accepted` fallback). The hand-off logs one line at `warn` (a stop was missing), which the leak
   harness (§5) treats as a finding unless the test declares it.
4. **No detached work without a registered reason** (§6), and none without a cap.
5. **The handle the user holds is not the handle the threads hold.** A type whose threads need shared state keeps the
   user's handle separate from the threads' state (`Peer { handle: Arc<PeerHandle> }` with `Drop for PeerHandle`, the
   threads holding only `Arc<Inner>`), so dropping the last user handle is observable and ends the work (incident 1).
6. **Work running inside an owner gets a non-owning handle to it** (P1-2). A handler, callback or member thread never
   receives an owning handle minted from inside: `CallCtx::peer()` returns a `PeerRef` backed by `Weak<Inner>`. A
   `PeerRef` can call, notify and stream while the connection is open; after the release every call fails at once with
   `IpcError::Closed(closure)` (ADR 0072's `Closure`, the one the connection ended with). It does not keep the connection
   open, is not counted as a user handle, and cannot be turned into a `Peer`. Code that needs to own a connection gets it
   from whoever opened it (the launcher's `Peer`, held next to its child: `debrief-ai` `worker/frames.rs:326` and Dyno
   `dyno-plugin-host/src/launcher.rs:66` keep the launcher's handle with the child they own, which is allowed); a plugin
   host's `PeerHost` (`paddock-plugin-api/src/serve.rs:142`) holds a `PeerRef`. The same rule holds for every owner type:
   a context handed into its members is `Weak`-backed.

### 2. One shared mechanism: `TaskGroup`

In `paddock-jobs`, module `tasks`, next to the `Clock`, `Deadline` and `CancelToken` it needs and where ADR 0020 puts the
executor (D1 weighs the alternatives on the real dependencies):

```text
pub struct TaskGroup     // one owner's background work; not Clone
  fn new(owner: &'static str, bound: Duration, reaper: &Reaper) -> TaskGroup   // the group uses the reaper's clock
  fn thread(&mut self, role: &'static str, f: impl FnOnce(StopToken) + Send + 'static) -> io::Result<()>
  fn child(&mut self, role: &'static str, child: paddock_proc::Supervised)     // feature "proc": ended with the group
  fn task(&mut self, role, fut)            // feature "tokio": stop hook first, abort after the grace (§2a)
  fn blocking(&mut self, role, f)          // feature "tokio": spawn_blocking with a StopToken, waited at stop
  fn on_stop(&mut self, hook: impl FnOnce() + Send + 'static)   // must not block (§2a); unblocks I/O
  fn stop_token(&self) -> StopToken
  fn stop(self, bound: Option<Duration>) -> StopReport           // blocking threads only (§2a)
  async fn stop_async(self, bound: Option<Duration>) -> StopReport // async contexts (§2a)
impl Drop for TaskGroup                    // set the token; hand everything to the reaper; never blocks

pub type StopToken = CancelToken          // ADR 0020's token (cancel and stop are one mechanism, rule 55)
pub struct StopReport    // owner, members [(role, Outcome{Ended, Panicked, Overran}, duration)], handed_to_reaper
pub struct Reaper        // one per clock: Reaper::global() (system clock) or Reaper::new(clock) in tests
  fn pending(&self) -> Vec<Pending>; fn settle(&self, bound) -> Settled
pub enum Detached        // §6: Relaunch, OpenElsewhere, UncancellableCall{what}, ProcessLifetime{what}
pub fn detached(kind: Detached, role: &'static str, f) -> io::Result<()>   // capped, counted, logged
```

- **What moves (P1-3):** nothing. `Clock`, `SystemClock`, `FakeClock`, `Deadline` and `CancelToken` (whose
  `CancelReason` derives `serde`) stay in `paddock-jobs` (`src/time.rs`, `src/cancel.rs`); `tasks` uses them in place.
  `paddock-jobs` gains two optional features: `proc` (a dependency on `paddock-proc` for `TaskGroup::child`) and `tokio`
  (for `task`, `blocking` and `stop_async`). `paddock-proc` does not depend on `paddock-jobs`, so there is no cycle.
- **Time comes from the reaper's clock** (ADR 0041): a group takes its reaper, and with it its clock, at construction, so
  a group's bound and the reaper that may finish its wait use the same clock; tests build `Reaper::new(fake_clock)`.
- **Panics** are caught per member and reported as `Panicked` with the role (the containment ADR 0020 asks of
  `paddock_guard::catch`).
- **Process children** stay `paddock_proc::Supervised` (it already ends the whole tree, by identity, ADR 0033); the group
  holds them so one `stop` ends threads and children in one report. `paddock_proc` keeps its `Registry`/`end_all` for a
  crash or panic hook.
- **ADR 0020's executor, when it is built**, returns its `lease` and `submit` handles as members of the caller's
  `TaskGroup` (a lease is a member with role and cap; `lease_abandonable` is `Detached::UncancellableCall`). Until then
  `TaskGroup` starts plain named threads at the creator's priority (rule 52 unchanged: shutdown is live work).

#### 2a. Soundness: what stop, drop and the reaper may and may not do (P1-4)

Each rule has a model test in `paddock-jobs` on a `FakeClock` (WP-1), named in brackets.

1. **A completion latch per member, then a join.** `std::thread::JoinHandle` has no timed join. Each member's body runs
   inside a guard whose `Drop` (so also on a panic) sets the member's latch (a mutex and a condvar, or a closed channel)
   as the last thing the member's code does. `stop` waits on the latches with the clock's deadline; it calls `join` only
   on a member whose latch is set, which then returns nearly at once (the thread still runs its thread-local destructors
   after the latch; the join waits for those, and a thread-local destructor that blocks is a bug the model test's
   blocking-destructor case shows as an overrun of the join, logged with the role). A member whose latch is not set at the bound is `Overran` and
   its `JoinHandle` goes to the reaper. [`stop_never_waits_past_its_bound`, `a_panicking_member_is_reported_and_joined`]
2. **One reaper thread, many deadlines.** The reaper keeps one queue of handed-off groups ordered by deadline and waits on
   one condvar until the earliest deadline or a new hand-off (a fresh wake token per wait, as ADR 0020's scheduler). At a
   group's deadline it logs what is still running and moves it to `pending()` (it never kills a thread). It never polls.
   [`the_reaper_waits_for_many_groups_with_one_thread`, `a_new_hand_off_wakes_the_reaper`]
3. **Whose clock.** A group's deadline is computed with the clock of the reaper it was built with; there is one reaper
   per clock, so a reaper never compares deadlines from two clocks. [`a_group_uses_its_reapers_clock`]
4. **Children and hooks are ended on the reaper, not in `Drop`.** `Supervised::drop` ends the tree and waits for it
   (`supervised.rs:486`), which can block. In `TaskGroup::drop` the children and the `on_stop` hooks are moved to the
   reaper, which runs the hooks, terminates each child with its grace and records the outcome. In an explicit `stop`
   (the caller accepted a wait) they run inline, within the bound. If the reaper cannot be started (item 5), a dropped
   group's children are ended inline instead (`Supervised::drop`, which may block for its grace): a blocked drop is the
   lesser harm than a leaked process tree. Hooks must not block (closing a socket, setting a flag,
   `InterruptHandle::interrupt`); the reaper measures each hook and logs any that takes over 10 ms on its clock, naming the
   owner and role. [`drop_returns_while_a_child_is_still_ending`, `a_slow_hook_is_named_in_the_log`]
5. **The reaper may fail to start.** It starts at the first hand-off (a library may never need one) or, in an app, at
   composition (ADR 0037), so a failure shows at start. If the thread cannot be spawned (EMFILE, a thread limit), the
   hand-off logs one `error` line naming the owner and the OS error, ends the group's children inline (item 4), counts its
   threads as leaked, and returns; nothing panics; the next hand-off tries again. [`a_reaper_that_cannot_start_logs_and_leaks_without_panicking`,
   with an injected spawn failure]
6. **No self-join.** A member that stops or drops its own group (a `LinkServer`'s accept loop that stops the server
   when its listener fails; a callback run on a member thread that drops the owner, such as a push callback that drops
   the app's last handle to its own link) is recognised by its thread id: `stop` does not wait for the calling member
   (it is reported `Ended` when it returns) and hands the others to the reaper if the caller cannot wait; `Drop` from a
   member is a plain hand-off. The reaper itself is never a member of a group it reaps. [`a_member_dropping_its_own_group_does_not_deadlock`]
7. **`settle` never runs on the Tauri main thread.** Blocking the main thread freezes the windows and can stall webview
   teardown. The Quit path runs the top-level stops and `settle` on a background thread and lets the main thread exit
   when that thread reports (ADR 0037's composition root owns this); a debug build fails fast when `settle` is called on
   the main thread. [`settle_refuses_the_main_thread_in_debug`; the composition test of §5.3]
8. **Async contexts never block.** In a tokio context `stop` (sync) would block a runtime worker; there the owner calls
   `stop_async(bound).await`, which waits on the members' latches as futures and yields. A sync `stop` called inside a
   runtime context does not `block_on`: it signals, hands the waiting to the reaper with the latches, and returns a report
   marked `Handed`. A `task` member gets its stop hook and the token first, and is aborted only when the grace part of the
   bound has passed (an abort drops the future at its next await point and runs no clean-up of its own).
   [`stop_inside_a_runtime_hands_off_instead_of_blocking`, `a_task_sees_its_stop_before_its_abort`]

#### 2b. Composition with ADR 0072

`Peer` holds its reader, writer and push threads in a `TaskGroup`; 0072's ordered close (seal, fail and cancel, flush,
half-close, drain, release) runs on those threads exactly as 0072 specifies; `Peer::close` stays non-blocking;
`Peer::wait_released(timeout)` becomes "the group's members all ended within the timeout"; and dropping the last user
handle (`PeerHandle::drop`) calls `close(Closing::ByOwner(Dropped))` and drops the group, which hands it to the reaper.
The reaper's bound for a peer is 0072's own worst case (flush cap + reason deadline + drain cap, 4.25 s), so a dropped
peer releases its descriptors and threads within it, never later. A `PeerRef` held by a handler (§1.6) does not delay
this. `LinkServer`, the MCP loopback and the Dyno helper hold their accept loop and per-connection groups the same way;
0072's drain pool is a `TaskGroup` with a member cap.

### 3. The reaper

One reaper per clock; in production one per process (`Reaper::global()`, a registered `Detached::ProcessLifetime`
thread). It receives groups dropped without a stop, runs their hooks, ends their children, waits for their members until
the group's deadline (§2a), and logs one line per group: owner, roles, outcome, how long it ran after the drop. A member
still running at its deadline is logged at `error` with its role and left running (a thread cannot be killed safely);
`pending()` lists it, the diagnostics bundle includes the list, and the leak harness fails on it. `settle(bound)` waits
until nothing is pending or the bound passes. The app's Quit path (ADR 0037's composition root) stops its top-level
groups, then settles with the app's quit bound off the main thread, so a Quit ends every owned thread and child or names
the one that did not.

### 4. The check: C42, built as a syn pass, with a register and a ratchet

C42 as ADR 0024 numbered it, specified here so it sees ownership, built like ADR 0072's C102 (syn in `paddock-dev`,
part of the push check over changed files and of CI over the whole tree):

- **What it finds.** Every spawn primitive of the survey method, in non-test code, including macro arguments, with the
  `use` paths of each file resolved (a renamed import is still found) and each handle followed to its join, store or
  drop (not the file heuristic of the prototype). Inside `paddock_jobs::tasks` and `paddock-proc` raw spawns are allowed;
  elsewhere every raw start is a finding unless it is a `std::thread::scope` spawn or is in the register.
- **Compile-time half.** The lint preset (`lint/clippy.toml`) and the seven workspace copies (`crates/`,
  `apps/{debrief,dyno,pitcrew,pitwall,spotter}/`, `tools/paddock-dev/`; C13 checks that the copies keep every preset
  path) add `disallowed-methods` for `std::thread::spawn`, `std::thread::Builder::spawn`, `std::process::Command::spawn`,
  `tokio::spawn`, `tokio::task::spawn`, `tokio::task::spawn_blocking`, `tauri::async_runtime::spawn` and
  `tauri::async_runtime::spawn_blocking` (with `allow-invalid` where a workspace lacks the dependency), so the compiler
  resolves paths the syn pass might miss. Each allowed site carries
  `#[expect(clippy::disallowed_methods, reason = "detached: <register id>")]`, and C42 checks that the id exists. Whether
  clippy resolves a method path such as `Builder::spawn` reliably is verified in WP-2 against a fixture; if it does not,
  the syn pass alone is the check (D3).
- **The register** `docs/background-work.toml`: `[[detached]]` (site, `Detached` kind, reason, owner) and `[[owner]]`
  (each type that holds a `TaskGroup`, with the name of its ending test, §5). A site in neither, or an entry whose site
  no longer exists, fails the check. There is no exemption section: ADR 0020's declared exemptions (real-time loops, the
  exit watchdog, a process that is itself the loop) are exempt from the executor's routing, not from ownership; each is a
  member of an owner's group or a `ProcessLifetime` thread.
- **The ratchet** `conformance/baselines/C42.json` lists today's findings by file plus a hash of the whitespace-collapsed
  line (the C71 method, lessons 2026-11-08), each with what it holds (`fd`, `folder`, `connection`, `child`, `none`,
  filled in WP-0) and a `blocks_tag` (D7). It only shrinks; a new raw spawn fails the push check at once; the tag check
  that already refuses a tag for an expired copy (ADR 0034, C58) refuses a matching tag while an entry remains (as ADR
  0072 D8).
- **Positive control** (ER-33). A fixture tree with one production-style site per primitive and disposition (each import
  style; the cases of the survey's fixture, macro arguments included) and one inside a `#[cfg(test)]` module; a test
  asserts the pass reports each production site with its disposition and skips the test one, so a primitive or a
  disposition the parser stops seeing turns the test red. The scratch prototype and its fixture are the starting point.

### 5. The test-harness leak check

Three layers, from cheapest:

1. **Children holding the test's output.** Every `.config/nextest.toml` (seven today) sets
   `leak-timeout = { period = "500ms", result = "fail" }` in the default and CI profiles, so a test whose children still
   hold its stdout or stderr after it ended fails as LEAK instead of passing ([nextest leaky tests](https://nexte.st/docs/features/leaky-tests/),
   read 2026-11-09: the `result = "fail"` form exists since 0.9.95; the machine has 0.9.151; nextest sees only children
   that inherited stdout or stderr). C14's hermetic wrapper keeps killing and reporting survivors of a whole run.
2. **Per-owner ending tests.** `paddock-testkit::leaks::Guard` snapshots, in the test's own process: the thread list with
   names, the open descriptors (handles on Windows), the child processes by identity (`paddock_proc::ProcessId`, pid plus
   start time, started after the guard) and the reaper's `pending()`. `guard.check(bound)` first settles the test's reaper
   (a condition wait, never a sleep; ER-12), then fails with the names of every new thread, descriptor path and child
   command line. **It assumes one test per process**, which nextest gives (each test runs in its own process); under
   `cargo test` (many tests as threads of one process) the snapshot would see other tests' threads, so the guard refuses
   to run there with a message naming nextest (the environment variable nextest sets is chosen and verified in WP-3).
   Every `[[owner]]` in the register names an ending test that builds it, uses it, stops it and drops it, and asserts the
   guard; a second test drops it without a stop and asserts the reaper's line and a clean guard. The probes per OS (Linux
   `/proc/self/task` and `/proc/self/fd`; macOS the task's thread list and `proc_pidinfo` descriptor list; Windows a
   Toolhelp thread snapshot and `GetProcessHandleCount`) are chosen and measured in WP-3; the descriptor count on
   Windows is a count, not a list. Third-party threads that live for the process (a tokio runtime's workers, whose idle
   blocking threads stay for 10 s by default, [tokio Builder](https://docs.rs/tokio/latest/tokio/runtime/struct.Builder.html),
   read 2026-11-09) are declared per test by name prefix, never by a blanket allow.
   For `paddock-ipc`, one ending test has a handler that keeps `ctx.peer()` (a `PeerRef`) in a static after its call: the
   owner drops its `Peer`, the guard is clean within 0072's bound, and a later call through the kept `PeerRef` fails with
   the connection's `Closure`.
3. **The app's Quit.** ADR 0037's composition test (its step 5, "Quit, and check that the process tree is gone") also
   asserts the reaper settled with nothing pending.

**Positive controls** (ER-33): `leaks::Broken` owners, one per fault: a detached thread, a leaked descriptor, a child
left running, a group whose member ignores its stop and overruns the reaper's deadline, a dropped `Peer` without the §1.5
split, and a call context that mints an owning `Peer` (the shape of `CallCtx::peer()` today) kept by a handler. A test
asserts the guard fails on each with the fault named. A test with a child left on purpose (holding stdout) proves the
nextest setting fails it, once per OS leg.

### 6. Detached kinds (closed set, each capped and logged)

| Kind | For | Cap | Ends |
| --- | --- | --- | --- |
| `Relaunch` | the app starting its next instance or its updater | 1 per process | never (it is meant to outlive us); logged with the child's `ProcessId` |
| `OpenElsewhere` | a document, folder or URL opened in another program | none needed (the OS owns it; the child is not waited for); counted | never; logged |
| `UncancellableCall { what }` | a blocking OS or plugin call with no cancel (`getaddrinfo`, a hung network share, a stuck counter read); ADR 0020's `lease_abandonable` | 8 per process in all kinds of `what` together (ADR 0020's written-off cap, D4 there); beyond it the call is refused with a message naming the cap | when the call returns; the caller got its error at its deadline |
| `ProcessLifetime { what }` | one thread for the whole process (the reaper, the log writer, the tray loop), and ADR 0074's lease holder process | 1 per `what` | at process exit; listed in the leak guard's per-process allowance |

Anything else is a `TaskGroup` member, watchdogs included (a browser's watchdog is a member of the browser's group and
ends with it). A kind is added only by amending this table.

### 7. Agents: load and helpers have owners too

- `./dev burn --cores <n> --max <duration> [--target vm:<name>]` (`paddock-dev`): starts the load as children it holds,
  records their identities and refuses more than cores minus one (the 2026-11-09 lesson). **Each burner also ends on its
  own**, so a killed `./dev burn` or a dropped session cannot leave it: it is started with its own deadline (it exits at
  `--max` by its own clock) and with parent-death delivery (Linux `PR_SET_PDEATHSIG`; macOS a kqueue `EVFILT_PROC`
  `NOTE_EXIT` watch on its parent; Windows a kill-on-close job object held by `./dev burn`). In a test VM it runs through
  ADR 0074's `lease run`, so the guest-side load has the lease as its owner and the next holder's restore ends what is
  left. These OS mechanisms are from the platforms' documentation as known, not re-read for this draft; WP-9 verifies
  each with a test that kills the parent.
- A PreToolUse hook (as `.claude/hooks/no-pgrep-wait.sh`, with its `.test.sh`) refuses the known unbounded shapes
  (`yes > /dev/null &`, `while :; do :; done &`, `stress-ng` without `--timeout`, `nohup ... &` or `disown`) and names
  `./dev burn` as the next step. **The hook can be bypassed** (a script file, another spelling): it catches the common
  shape, as the `pgrep` hook does (ER-32), and the backstop is `./dev leftovers`.
- `./dev leftovers` lists processes whose command line or working folder names the agent's worktree or scratch folder
  (the method of C14's hermetic wrapper), by identity; the agent-cleanup checklist ends with it, and `--stop` ends them
  through `paddock_proc` (rule 53).

### 8. Logs (ER-27)

- Each `stop` and each reaper hand-off logs one line: owner, roles, outcome per member, duration, bound, overruns,
  whether the stop was explicit or a drop. No content.
- `detached(...)` logs the kind, role, reason id and the running count per kind; reaching a cap is an `error` line
  naming the kind and the next step. A reaper that cannot start logs the OS error once per attempt (§2a.5).
- The reaper logs a summary every 10 minutes while anything is pending, and at Quit; a slow hook is logged with its owner
  and role (§2a.4).
- The diagnostics bundle (the family's `paddock-diag`) gains the reaper's `pending()`, the detached counts and the live
  thread names of the process.

### 9. Migration, ordered by release (rule 1; ADR 0049 small merges; rule 43 a shared change lands with its adoption)

| Merge | Content | Gate (D7 (a); D7 (c) narrows it to entries that hold something) |
| --- | --- | --- |
| M0 | C42 with the register and the ratchet (every finding of today listed with what it holds and its `blocks_tag`; new raw spawns fail the push check), the prototype's fixture as positive control, nextest `leak-timeout` in all seven configs (red tests fixed or ratcheted first) | none; changes no product |
| M1 | `paddock_jobs::tasks` (`TaskGroup`, reaper, `Detached`, features `proc` and `tokio`), `paddock-testkit::leaks` with its `Broken` controls, on three OS | none |
| M2 | `paddock-ipc`: `Peer`/`BodyPeer` user-handle split, `PeerRef` for call contexts, threads in a group, `wait_released` on the group, `LinkServer` and launch; `paddock-mcp` loopback. **Lands inside ADR 0072's merges** (below) | `dyno-v*` |
| M3 | **Dyno** (next release after 1.1.0): app, agent, helper, platform, plugin host; the shared crates Dyno ships (`paddock-proc` runners, `paddock-service`, `paddock-window`, `paddock-startup`, `paddock-health`, `paddock-shell`, `paddock-updater`, `paddock-watch`, `paddock-files`, `paddock-dialogs`, `paddock-chromium`) | `dyno-v*` |
| M4 | **Pitwall 1.1**: `pitwall-server` tokio tasks into groups (`task`, `stop_async`), the app's tray and menu tasks, `pitwall-loopback`; shared crates it ships that M3 did not cover | `pitwall-v1.1*` |
| M5 | **Debrief 1.2**: store-duck (incident 2, failing test first), app start-up sweeps into an app-lifetime group stopped at Quit, plugin crates, `debrief-ai`, `chromium-driver`; `debrief-ipc` is a registered copy (rule 55) waived until it is replaced by `paddock-ipc` in 1.2 PH1-PH4. The waiver names the owning `ctx.peer()` uses of that copy in `debrief-plugin-api/src/serve.rs` (lines 118, 155, 156, 179, 193, 252, 264, 282, 304, 319: `HostFile`, `RemoteView` and tool contexts), which become `PeerRef`s with the move to `paddock-ipc` | `debrief-v1.2*` |
| M6 | Pitcrew 1.1 and Spotter (after its 1.0 tag): the remaining sites | `pitcrew-v1.1*`, `spotter-v1.1*` |
| M7 | C42 to error mode for every app whose ratchet is empty; rule text, lessons, ADR index | – |

**Coupling to ADR 0072 (WP-4).** M2 edits the files ADR 0072's M2 to M4 edit (`peer.rs`, `body.rs`, `handshake.rs`,
`link/server.rs`, `launch.rs`, the Dyno agent and helper servers) and uses 0072's `Closure` and the `Closing` vocabulary
from its M1. So WP-4 starts after 0072's WP-1 (its core types), and its commits land inside 0072's M2 (peer close) and M3
(accepting), not as a separate merge; if 0072's M2 to M4 slip, M2 here slips with them, and M3 (Dyno) can still proceed for
every site outside `paddock-ipc`. Its 1.5 days come on top of 0072's 27.5.

Each merge removes its sites from the ratchet. **Rollback:** each merge reverts alone; no wire or file format changes.

## Parallelism and cost (rule 51)

- **Threads: none added** in steady state. One reaper thread per process, idle on a condvar (no polling). `TaskGroup`
  adds one latch and one `Arc` per member. A `StopToken` wait is a condvar wait: a stop wakes the member at once instead
  of at its next poll.
- **Stop and Quit latency:** bounded by design. Groups stop in parallel (each signals at once, the waits overlap), so a
  Quit costs the slowest member up to the app's quit bound (default 2 s, D4), not the sum. Today some stops never end
  (incident 1) and some are cut by process exit (incident 5).
- **Cheaper, not only bounded:** stop hooks unblock I/O at once (close the socket, interrupt the query) instead of
  waiting for a poll slice; 0072's sliced reads stay where a hook is impossible.
- **The check:** the syn pass (revision 1's prototype) took 5.90-6.04 s for the whole tree on the Mac mini at load 5
  ([benchmarks.md](../benchmarks.md#2026-11-09-a-syn-scan-of-every-spawn-site-in-the-monorepo-docsadr-0076-owned-background-work));
  revision 2 adds a macro-argument parse and per-binding rules and was not re-timed; the push check runs it on changed
  files only, CI on the whole tree.
- **The leak guard:** a process-table pass for children costs 13.6-14.7 ms on macOS (ADR 0033's recorded snapshot); thread
  and descriptor lists are one directory read or one system call each. It runs in the ending tests only (about one per
  owner type), not around every test.
- **To measure and record (rule 48):** Quit time p50/p99 per app before and after M3 to M6; the guard's cost per OS in
  WP-3; the revised scan's time in WP-2; reaper hand-offs per hour in a day of normal use (expected 0 after migration).

## Work packages (raw estimates, one agent; recomputed from revision 2's survey)

| WP | Content | Sites | Days |
| --- | --- | --- | --- |
| WP-0 (M0) | Read the 66 "join in the file" sites and the 10 wrapper calls; fill what each ratchet entry holds; write the register and the ratchet with `blocks_tag`; nextest `leak-timeout` in seven configs and fix or ratchet what it turns red | 76 to read | 2 |
| WP-1 (M1) | `paddock_jobs::tasks`: `TaskGroup` with latches, `stop`/`stop_async`, the reaper (one queue, one condvar, hooks and children on the reaper, start failure, self-join), `Detached` with caps, features `proc` and `tokio`, log lines; the model tests of §2a on a `FakeClock`; crate coverage gate | – | 3.5 |
| WP-2 (M0/M1) | C42 in `paddock-dev`: the syn pass from the prototype (import resolution, macro arguments, per-handle tracking instead of the file heuristic), the register and ratchet comparison, the fixture positive control; the clippy lines in the preset and the seven workspace `clippy.toml` files, and their fixture (D3) | – | 2 |
| WP-3 (M1) | `paddock-testkit::leaks::Guard` on Linux, macOS and Windows (probe choice measured), the nextest-only guard, settle, the `Broken` controls, the per-test allowance for runtime threads | – | 2 |
| WP-4 (M2) | `paddock-ipc` and `paddock-mcp` owners: user-handle split, `PeerRef` for `CallCtx` and its adopters (`paddock-plugin-api` `serve.rs`; `paddock-ipc` `link/server.rs:263` `Subscriptions`, which today keeps the subscribing `Peer` from `ctx.peer()` and becomes a table of `PeerRef`s that forgets a released one), groups, `wait_released`; their ending tests, the kept-context test; inside ADR 0072's merges | ~15 | 1.5 |
| WP-5 (M3) | Dyno and the shared crates it ships: 52 Dyno sites and about 35 shared sites to migrate or register, 22 and about 13 to finish from WP-0's reading; ending tests per owner type; Dyno installers built (rule 54) | ~87 | 3.5 |
| WP-6 (M4) | Pitwall 1.1: 26 sites (mostly tokio tasks, `stop_async`); ending tests | 26 | 1 |
| WP-7 (M5) | Debrief 1.2: 65 sites plus 23 to finish; incident 2 with its failing test first; start-up sweeps; plugin crates | 65 | 3 |
| WP-8 (M6) | Pitcrew 1.1 (18 sites, 0.6 days) and Spotter 1.1 (10 sites, 0.4 days); ending tests | 28 | 1 |
| WP-9 | Agents: `./dev burn` with deadlines and parent-death per OS, `./dev leftovers`, the hook with its test, the checklist line | – | 1.25 |
| WP-10 (M7) | Rule 74's text in `docs/rules.md` and the ER candidate, lessons entry, ADR index, CHANGELOGs, ADR 0020/0024/0072 amendment notes | – | 0.5 |
| | **Total** | **214 + 76 to read** | **21.25** |

**Range 19 to 24 days.** Revision 1 said 17.5 for 158 sites; revision 2's survey has 213 sites to migrate or register
(revision 3: 214; 55 more than revision 1, the sites the classifier bug had counted as joined) and the soundness work of §2a. The low end (19.25) holds if
D3 takes clippy alone (WP-2 to 1 day) and D8 keeps agents to the checklist (WP-9 to 0.25); the high end if the Windows
leak probes need more than one iteration or Pitwall's runtime shutdown needs its own design.

**Days added per release** (on top of the foundation WP-0 to WP-3 and WP-10, 10 days, which come first and change no
product):

| Release | WPs | Days added | Under D7 (c) |
| --- | --- | --- | --- |
| Dyno, next after 1.1.0 | WP-4, WP-5 | 5 | WP-4 in full (it is the incident class); WP-5 only for entries that hold something, the rest in Dyno's following release; WP-0 states the split |
| Pitwall 1.1 | WP-6 | 1 | the entries that hold something |
| Debrief 1.2 | WP-7 | 3 | incident 2 and the entries that hold something |
| Pitcrew 1.1 | WP-8 (part) | 0.6 | the entries that hold something |
| Spotter 1.1 | WP-8 (part) | 0.4 | the entries that hold something |
| Not tied to a release | WP-9 | 1.25 | same |

**The first cut** (WP-0 to WP-5 and WP-10: the mechanism, the check, the leak harness, `paddock-ipc` and Dyno) is
**15 days** by this table (revision 1 said "about 10" against its own 12; that was wrong). Under D7 (c) WP-5 shrinks to
the Dyno and shared entries that hold a descriptor, folder, connection or child; their number is known only after WP-0,
so the first cut is then 12.5 to 15 days.

WP-0 and WP-1 run in parallel; WP-2 and WP-3 follow WP-1's API; WP-4 follows 0072's WP-1 and lands in 0072's merges;
WP-5 to WP-8 follow the release order, one app at a time.

## Decisions for the user

| # | Question | Options | Recommendation |
| --- | --- | --- | --- |
| D1 | Where `TaskGroup` lives (re-weighed in revision 2 on the real dependencies: `paddock-ipc` and `paddock-mcp` already depend on both `paddock-jobs` and `paddock-proc`; of the 64 crates with shipped sites, 23 depend directly on `paddock-jobs` and 18 on `paddock-proc`; `TaskGroup` needs `Clock`, `Deadline` and `CancelToken`, all in `paddock-jobs`, `CancelReason` with `serde`) | (a) A module `paddock_jobs::tasks`, with `child()` behind a `proc` feature and the async parts behind `tokio`; nothing moves; ADR 0020's executor is built next to it. The 41 crates with sites that do not depend on `paddock-jobs` directly gain it (and `croner`, `jiff`, `sha2`, `serde` with it, unless their workspace already builds them, which WP-0 checks with `cargo tree`). (b) A new crate `paddock-tasks`, moving `Clock`, `SystemClock`, `FakeClock`, `Deadline` and `CancelToken` (with `serde` behind a feature) down into it and re-exporting them from `paddock-jobs`: lighter for the 41, but it moves the family's clock port and token, which many crates and tests import, for a dependency cost `paddock-ipc` and `paddock-mcp` already pay. (c) `paddock-proc`: 46 of the 64 crates do not depend on it, it pulls in `sysinfo`, and it would need the clock and token moved into it. (d) `paddock-runtime`: the pinned-download crate; the name would mislead. | (a): no move, one token for cancel and stop (rule 55), the executor's home. Take (b) only if WP-0's `cargo tree` shows that `paddock-jobs` adds dependencies to a workspace that does not build them today. |
| D2 | What `Drop` of an owner does | (a) Set the token, hand everything (members, hooks, children) to the reaper, never block, log at `warn` (§2a). (b) Signal and join with the bound inside `Drop` (blocks a UI thread or an unwind for up to the bound; `Supervised::drop` can block on its own). (c) Signal only. | (a). |
| D3 | The static check | (a) C42 as a syn pass in `paddock-dev` plus the clippy `disallowed-methods` lines in the preset and its seven copies, the register and the ratchet. (b) The syn pass only. (c) Clippy only (no register, no handle disposition). | (a), falling back to (b) if WP-2 shows clippy cannot match `Builder::spawn` or tokio's re-exports. |
| D4 | Bounds | Group stop bound 2 s by default (per group, set where the group is made); a `Peer` group 0072's 4.25 s; the reaper's error line at the deadline; the app's quit bound 2 s after the last group stop; `UncancellableCall` cap 8 per process (ADR 0020's written-off cap); a hook over 10 ms is logged. Protocol defaults in code, not user Settings (no user tunes them). | Accept. |
| D5 | The detached kinds | §6's closed set of four kinds, a register entry per site, amended only by amending this ADR; no exemption section (ADR 0020's exemptions are owned members). | Accept. |
| D6 | ADR 0072 amendment | Add `OwnerReason::Dropped` to 0072's closing vocabulary, so a peer whose last handle was dropped ends with a reason (`stopping`-class code on the wire, `Dropped` in the log) instead of a bare end. No frame change. | Yes. |
| D7 | Release gating | (a) Every ratchet entry blocks its app's next listed tag: Dyno's next, Pitwall 1.1, Debrief 1.2, Pitcrew 1.1; Spotter 1.0 is not blocked (it is in flight; its entries block `spotter-v1.1*`). Strictest; adds the full per-release days above. (b) Report only, no tag blocks: fastest, and the class stays open, as it is today. (c) Now: the push check fails every **new** raw spawn from M0 on, so the class cannot grow. Tags are blocked only by entries that hold a descriptor, folder, connection or child (the incident class: EMFILE, "Directory not empty", orphaned children; WP-0 marks each entry); entries that hold nothing (a short thread that logs, a tray menu call) block the same app's following release instead. Faster for each next release by the share of entries that hold nothing (known after WP-0), at the cost of those threads still being cut at Quit for one more release. | (c), since you want speed: it closes the harmful part in each next release, stops new instances at once, and keeps every remaining entry on a dated path to zero. (a) if you prefer one release per app with the whole class gone. |
| D8 | Agents | (a) `./dev burn` (own deadline, parent-death), `./dev leftovers` and the hook (WP-9). (b) The checklist line only. | (a): two incidents left nothing to clean up with; a hook is what held for `pgrep` waits (ER-32), and `leftovers` covers what the hook misses. |
| D9 | A binding rule | Rule 74 (assigned on acceptance) and an ER candidate: "every thread, task and child process has one owner that holds its handle; stop signals and waits within a bound and reports; drop hands off to the reaper without blocking; work inside an owner gets only a non-owning handle to it; detached work only through a registered kind with a cap; every owner type has an ending test that finds nothing left". | Yes, with C42 and the leak guard as its checks. |
| D10 | Scope now | (a) The whole migration, each part in its app's release (21.25 days). (b) The first cut now (15 days; 12.5 to 15 under D7 (c)), the rest in each app's phase. | (b), with D7 (c). |

## Consequences

- A dropped owner releases its threads, children and descriptors within a known bound, or the log names the member that
  did not; "Directory not empty" after a drop and EMFILE from dropped peers become test failures with a name.
- A handler that keeps its call context can no longer keep a connection alive.
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
  tests, which share one process for the length of a test (incidents 1 and 2).
- **Only the executor of ADR 0020:** it is large and not started; the ownership rule is small, needed now, and the
  executor can be built on it.
- **A leak check around every test:** runtime threads and lazily started process-wide threads would need broad
  allowances that hide real leaks; per-owner ending tests plus nextest's leak setting cover the class with less noise.
- **A grep-based check, or a syn check that matches strings:** a grep cannot tell inline test modules from production
  code or a kept handle from a dropped one, and revision 1's own substring classification miscounted 57 sites; the check
  uses an enum of dispositions with a fixture per disposition.

## The failing tests the fixes start with (rule 5 / ER-11; not written in this change)

- **Incident 1, `paddock-ipc`** (`tests/it/peer.rs` or the endings harness of ADR 0072):
  `a_dropped_body_peer_releases_its_threads_and_descriptors`: open an in-process pair, take a `leaks::Guard`, start a
  `BodyPeer`, make one call, drop it without `close()`; assert the host side sees the connection end within 0072's bound
  and `guard.check(bound)` finds no new thread or descriptor. Fails on `origin/main`: the three peer threads and the
  descriptors stay (the measured 5 threads and 6 descriptors per connection include both ends). And
  `a_handler_that_keeps_its_context_does_not_keep_the_connection`: as §5.2.
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

## Appendix A: shipped sites classified fire-and-forget (F) or owned but detached (D)

Line numbers at `origin/main` `b51c72509` (revision 3's classifier); kind `Thread`, `Task` (tokio or Tauri async),
`Blocking` (`spawn_blocking`), `Process`, `wrapper` (a call of a project function named `spawn`). WP-0 confirms each row.

| File | Sites (line, class/kind) |
| --- | --- |
| `apps/debrief/app/src-tauri/src/after_first_paint.rs` | 66 F/Thread, 81 F/Thread |
| `apps/debrief/app/src-tauri/src/ai/folder_setup.rs` | 28 F/Thread |
| `apps/debrief/app/src-tauri/src/ai/pitwall.rs` | 114 F/Thread |
| `apps/debrief/app/src-tauri/src/ai/update_commands.rs` | 164 F/Thread |
| `apps/debrief/app/src-tauri/src/browser/sessions.rs` | 701 F/Thread |
| `apps/debrief/app/src-tauri/src/drag_ghost/mod.rs` | 401 F/Thread |
| `apps/debrief/app/src-tauri/src/health/commands.rs` | 235 F/Thread |
| `apps/debrief/app/src-tauri/src/health/hub.rs` | 363 F/Thread |
| `apps/debrief/app/src-tauri/src/jobs/service.rs` | 509 F/wrapper, 538 F/Thread, 593 F/Thread |
| `apps/debrief/app/src-tauri/src/lib.rs` | 649 F/Thread, 687 F/Thread |
| `apps/debrief/app/src-tauri/src/lifecycle.rs` | 523 F/Thread, 600 F/Thread, 727 F/Process |
| `apps/debrief/app/src-tauri/src/live.rs` | 425 F/Thread, 481 F/Thread |
| `apps/debrief/app/src-tauri/src/names.rs` | 130 F/Thread |
| `apps/debrief/app/src-tauri/src/opener.rs` | 233 F/Thread |
| `apps/debrief/app/src-tauri/src/plugin_packages/commands.rs` | 167 F/Blocking |
| `apps/debrief/app/src-tauri/src/query_cancel.rs` | 43 F/Thread |
| `apps/debrief/app/src-tauri/src/settings.rs` | 571 F/Thread |
| `apps/debrief/app/src-tauri/src/startup_watchdog.rs` | 246 F/Thread |
| `apps/debrief/app/src-tauri/src/tools.rs` | 98 F/Thread |
| `apps/debrief/app/src-tauri/src/tray.rs` | 303 F/Blocking |
| `apps/debrief/app/src-tauri/src/updater.rs` | 258 D/Task, 453 F/Thread, 471 F/Process |
| `apps/debrief/app/src-tauri/src/video/media.rs` | 78 F/Blocking |
| `apps/debrief/app/src-tauri/src/watched.rs` | 170 F/Thread |
| `apps/debrief/crates/chromium-driver/src/cdp.rs` | 80 F/Thread |
| `apps/debrief/crates/chromium-driver/src/launch.rs` | 393 F/Thread |
| `apps/debrief/crates/debrief-ai-worker/src/child.rs` | 396 F/Thread |
| `apps/debrief/crates/debrief-ai/src/engine/pitwall/mod.rs` | 382 F/Thread |
| `apps/debrief/crates/debrief-ai/src/llm/lazy.rs` | 99 F/Thread, 177 D/Thread |
| `apps/debrief/crates/debrief-ai/src/worker/frames.rs` | 93 F/Thread, 258 F/Thread |
| `apps/debrief/crates/debrief-collect/src/runner.rs` | 62 F/Thread |
| `apps/debrief/crates/debrief-core/src/events.rs` | 209 F/Thread |
| `apps/debrief/crates/debrief-core/src/guard.rs` | 496 F/Thread |
| `apps/debrief/crates/debrief-core/src/plugin/watchdog.rs` | 51 F/Thread |
| `apps/debrief/crates/debrief-core/src/workspace/tail/watch.rs` | 349 F/Thread |
| `apps/debrief/crates/debrief-ipc/src/child.rs` | 75 F/Thread |
| `apps/debrief/crates/debrief-ipc/src/peer.rs` | 185 F/Task, 186 F/Task, 413 F/Blocking |
| `apps/debrief/crates/debrief-ipc/src/process.rs` | 240 F/Thread, 287 F/Thread |
| `apps/debrief/crates/debrief-ipc/src/runtime.rs` | 31 F/Task |
| `apps/debrief/crates/debrief-plugin-api/src/forward.rs` | 79 F/Thread, 97 F/Thread |
| `apps/debrief/crates/debrief-plugin-dotnet/src/eventpipe/mod.rs` | 61 F/Thread, 133 F/Thread, 173 F/Thread |
| `apps/debrief/crates/debrief-plugin-dotnet/src/plugin/tool.rs` | 78 F/Thread |
| `apps/debrief/crates/debrief-plugin-host/src/builtins.rs` | 205 F/Thread |
| `apps/debrief/crates/debrief-plugin-native/src/tool.rs` | 68 F/Thread, 145 D/wrapper, 368 D/wrapper |
| `apps/debrief/crates/debrief-plugin-windows/src/command.rs` | 50 F/Thread |
| `apps/debrief/crates/debrief-store-duck/src/console/mod.rs` | 215 F/Thread |
| `apps/debrief/crates/debrief-store-worker/src/server.rs` | 345 F/Thread |
| `apps/dyno/app/src-tauri/src/crash_reports.rs` | 257 F/Thread |
| `apps/dyno/app/src-tauri/src/lib.rs` | 492 F/wrapper, 501 F/Thread, 507 F/Thread, 518 F/Thread, 527 F/Thread |
| `apps/dyno/app/src-tauri/src/link/driver.rs` | 72 F/Thread, 116 F/Thread, 368 F/Thread |
| `apps/dyno/app/src-tauri/src/link/mod.rs` | 410 F/Thread, 574 F/Thread |
| `apps/dyno/app/src-tauri/src/link/ports.rs` | 431 F/Thread |
| `apps/dyno/app/src-tauri/src/login.rs` | 200 F/Thread |
| `apps/dyno/app/src-tauri/src/quit.rs` | 321 F/Thread |
| `apps/dyno/app/src-tauri/src/shell.rs` | 404 F/Thread, 442 F/Thread, 686 F/Thread, 720 F/Thread, 856 F/Thread |
| `apps/dyno/app/src-tauri/src/tray_away.rs` | 34 F/Thread |
| `apps/dyno/app/src-tauri/src/tray_host.rs` | 192 F/Thread, 308 F/Thread |
| `apps/dyno/app/src-tauri/src/tray.rs` | 917 F/Thread, 924 F/Thread |
| `apps/dyno/crates/dyno-agent/src/bench_child.rs` | 244 F/Thread, 284 F/Thread |
| `apps/dyno/crates/dyno-agent/src/bench.rs` | 835 F/Thread |
| `apps/dyno/crates/dyno-agent/src/cli.rs` | 64 F/Thread |
| `apps/dyno/crates/dyno-agent/src/counters.rs` | 219 F/Thread, 395 F/Thread |
| `apps/dyno/crates/dyno-agent/src/disk/centre.rs` | 603 F/Thread, 705 F/Thread |
| `apps/dyno/crates/dyno-agent/src/disk/engine.rs` | 102 F/Thread |
| `apps/dyno/crates/dyno-agent/src/disk/volumes.rs` | 53 F/Thread, 94 F/Thread |
| `apps/dyno/crates/dyno-agent/src/extras/finder.rs` | 122 D/Thread |
| `apps/dyno/crates/dyno-agent/src/extras/mod.rs` | 462 F/Thread |
| `apps/dyno/crates/dyno-agent/src/firewall.rs` | 265 F/Thread |
| `apps/dyno/crates/dyno-agent/src/handlers.rs` | 236 F/Thread |
| `apps/dyno/crates/dyno-agent/src/hardware.rs` | 350 F/Thread |
| `apps/dyno/crates/dyno-agent/src/helper_readers/counters.rs` | 292 D/Thread |
| `apps/dyno/crates/dyno-agent/src/helper_readers/processes.rs` | 513 F/Thread |
| `apps/dyno/crates/dyno-agent/src/inventory.rs` | 20 F/Thread |
| `apps/dyno/crates/dyno-agent/src/network.rs` | 182 F/Thread, 271 F/Thread |
| `apps/dyno/crates/dyno-agent/src/server.rs` | 117 F/Thread |
| `apps/dyno/crates/dyno-helper/src/orphan.rs` | 128 F/Thread |
| `apps/dyno/crates/dyno-helper/src/serve.rs` | 271 F/Thread |
| `apps/dyno/crates/dyno-helper/src/server.rs` | 247 F/Thread |
| `apps/dyno/crates/dyno-platform/src/process.rs` | 200 F/Thread |
| `apps/dyno/crates/dyno-platform/src/windows/indirect.rs` | 74 F/Thread |
| `apps/dyno/crates/dyno-platform/src/windows/wake_timer.rs` | 76 F/Thread |
| `apps/pitcrew/app/src-tauri/src/channels.rs` | 134 F/Thread |
| `apps/pitcrew/app/src-tauri/src/events.rs` | 113 F/Thread, 173 F/Thread |
| `apps/pitcrew/app/src-tauri/src/handoff.rs` | 185 F/Process |
| `apps/pitcrew/app/src-tauri/src/lib.rs` | 109 F/Thread |
| `apps/pitcrew/app/src-tauri/src/service_client.rs` | 114 F/Thread |
| `apps/pitcrew/app/src-tauri/src/service_install.rs` | 366 F/Thread |
| `apps/pitcrew/crates/pitcrew-service/src/host.rs` | 53 F/Task |
| `apps/pitcrew/crates/pitcrew-service/src/ipc.rs` | 227 F/Task, 258 F/Task |
| `apps/pitcrew/crates/pitcrew-service/src/launch.rs` | 52 F/Thread |
| `apps/pitcrew/crates/pitcrew-service/src/process.rs` | 82 F/Process |
| `apps/pitcrew/crates/pitcrew-service/src/supervisor.rs` | 1044 F/Thread |
| `apps/pitcrew/crates/pitcrew-service/src/supervisor/ops.rs` | 24 F/Thread, 32 F/Thread, 48 F/Thread, 61 F/Thread, 105 F/Thread |
| `apps/pitwall/app/src-tauri/src/app_menu.rs` | 61 F/Task |
| `apps/pitwall/app/src-tauri/src/commands.rs` | 400 D/Task |
| `apps/pitwall/app/src-tauri/src/lib.rs` | 77 F/Task, 82 F/Task, 213 F/Thread, 231 F/Task, 260 F/Task |
| `apps/pitwall/app/src-tauri/src/quit.rs` | 116 F/Thread |
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
| `crates/paddock-download/src/engine.rs` | 879 F/Thread |
| `crates/paddock-files/src/open.rs` | 309 F/wrapper |
| `crates/paddock-guard/src/guard.rs` | 215 F/Thread |
| `crates/paddock-health/src/health.rs` | 130 F/Thread |
| `crates/paddock-health/src/notify/writer.rs` | 56 F/Thread |
| `crates/paddock-health/src/tauri/native.rs` | 137 F/Process |
| `crates/paddock-health/src/tauri/os.rs` | 243 F/Thread |
| `crates/paddock-ipc/src/child.rs` | 55 D/Thread |
| `crates/paddock-ipc/src/link/server.rs` | 197 F/Thread |
| `crates/paddock-ipc/src/peer.rs` | 257 F/Thread |
| `crates/paddock-live/src/tauri.rs` | 117 D/Thread |
| `crates/paddock-log/src/child.rs` | 74 D/Thread |
| `crates/paddock-mcp/src/bridge.rs` | 157 F/Thread |
| `crates/paddock-mcp/src/server.rs` | 619 F/Thread |
| `crates/paddock-panels/src/tauri/native.rs` | 108 F/Thread |
| `crates/paddock-pitwall/src/service.rs` | 561 F/Thread |
| `crates/paddock-proc/src/run.rs` | 176 F/Thread, 245 F/Thread |
| `crates/paddock-service/src/backend/windows.rs` | 158 F/wrapper |
| `crates/paddock-service/src/control.rs` | 260 F/Thread, 295 F/Thread |
| `crates/paddock-service/src/native.rs` | 493 F/Thread |
| `crates/paddock-service/src/system.rs` | 87 F/Process |
| `crates/paddock-shell/src/setup/tauri.rs` | 26 F/Thread |
| `crates/paddock-siblings/src/native.rs` | 21 F/Process |
| `crates/paddock-startup/src/tauri.rs` | 446 D/Thread, 661 F/Thread, 789 D/Thread |
| `crates/paddock-storage/src/tauri.rs` | 168 F/Blocking |
| `crates/paddock-updater/src/tauri.rs` | 462 F/Thread |
| `crates/paddock-updater/src/tauri/native.rs` | 25 F/Process |
| `crates/paddock-updater/src/updater.rs` | 593 F/Thread |
| `crates/paddock-window/src/tauri/manager.rs` | 974 F/Thread |
| `crates/paddock-window/src/tauri/plugin_hook.rs` | 38 F/Task |
| `crates/paddock-window/src/tauri/popover.rs` | 401 F/wrapper, 463 F/wrapper |

## Appendix B: shipped sites with a join or wait somewhere in their file (to read in WP-0)

Stored, returned or passed on; the file joins, waits for or ends something, maybe this handle.

| File | Lines |
| --- | --- |
| `apps/debrief/app/src-tauri/src/browser/research.rs` | 432 |
| `apps/debrief/app/src-tauri/src/config_store.rs` | 238 |
| `apps/debrief/app/src-tauri/src/jobs/service.rs` | 889 |
| `apps/debrief/crates/chromium-driver/src/process.rs` | 17 |
| `apps/debrief/crates/debrief-agent/src/agent.rs` | 156 |
| `apps/debrief/crates/debrief-ai-worker/src/child.rs` | 231, 263, 264 |
| `apps/debrief/crates/debrief-ai/src/runtimes/report.rs` | 95 |
| `apps/debrief/crates/debrief-core/src/ports/session.rs` | 35, 52 |
| `apps/debrief/crates/debrief-core/src/workspace/tail/watch.rs` | 265 |
| `apps/debrief/crates/debrief-ipc/src/process.rs` | 384 |
| `apps/debrief/crates/debrief-plugin-dotnet/src/etw/session.rs` | 175 |
| `apps/debrief/crates/debrief-plugin-dotnet/src/recording.rs` | 148 |
| `apps/debrief/crates/debrief-plugin-host/src/builtins.rs` | 78 |
| `apps/debrief/crates/debrief-plugin-native/src/captures.rs` | 161 |
| `apps/debrief/crates/debrief-plugin-tools/src/monitor_tool.rs` | 72 |
| `apps/debrief/crates/debrief-plugin-windows/src/eventlog/follower.rs` | 488, 507 |
| `apps/debrief/crates/debrief-store-duck/src/catalogue.rs` | 188 |
| `apps/debrief/crates/debrief-store-duck/src/fts.rs` | 595 |
| `apps/debrief/crates/debrief-store-duck/src/progress.rs` | 67 |
| `apps/dyno/app/src-tauri/src/link/ports.rs` | 162 |
| `apps/dyno/app/src-tauri/src/supervise/mod.rs` | 188, 214 |
| `apps/dyno/crates/dyno-agent/src/cache/mod.rs` | 236 |
| `apps/dyno/crates/dyno-agent/src/counters.rs` | 259 |
| `apps/dyno/crates/dyno-agent/src/extras/mod.rs` | 224 |
| `apps/dyno/crates/dyno-agent/src/lib.rs` | 246, 268, 278, 292, 316, 326, 336, 346, 356, 366, 373, 383, 397, 410 |
| `apps/dyno/crates/dyno-platform/src/process.rs` | 41, 189 |
| `apps/pitcrew/crates/pitcrew-service/src/host.rs` | 37 |
| `apps/pitcrew/crates/pitcrew-service/src/launch.rs` | 216 |
| `apps/pitcrew/crates/pitcrew-service/src/process.rs` | 48 |
| `apps/pitwall/crates/pitwall-server/src/service/exec.rs` | 25 |
| `apps/pitwall/crates/pitwall-server/src/service/windows.rs` | 254 |
| `crates/paddock-assistant/src/service.rs` | 310 |
| `crates/paddock-files/src/open.rs` | 328 |
| `crates/paddock-ipc/src/link/client.rs` | 234 |
| `crates/paddock-ipc/src/link/server.rs` | 172 |
| `crates/paddock-jobs/src/pool.rs` | 72 |
| `crates/paddock-mcp/src/loopback.rs` | 359, 398, 411 |
| `crates/paddock-proc/src/supervised.rs` | 176 |
| `crates/paddock-service/src/control.rs` | 377 |
| `crates/paddock-service/src/native.rs` | 1001 |
| `crates/paddock-service/src/watch.rs` | 161 |
| `crates/paddock-startup/src/session.rs` | 288 |
| `crates/paddock-watch/src/source.rs` | 90 |
| `crates/paddock-window/src/exit.rs` | 72 |
| `crates/paddock-window/src/tauri/popover.rs` | 137 |
