# 81. Process liveness is three-valued: "could not look" is never "gone", proof tokens before destructive acts, one check over every look

- Status: Proposed (2026-11-10). Draft for the user's decision (ER-21); not implemented.
- Date: 2026-11-10
- Related: ER-21 / rule 40 (recurring bugs mean a missing design rule), rule 53 and ADR
  [0033](0033-processes-by-identity-never-by-a-bare-pid.md) (identity before acting: *which* process), ER-27 (logs explain
  every failure), ER-33 / rule 71 (a check counts only if its probe can see the thing), rule 5 / ER-11 (failing test
  first), rule 55 and ADR [0034](0034-one-implementation-per-mechanism.md) (copies fixed together), ADR 0072 (ordered
  connection ends, on `origin/docs/adr-0072-ordered-close`), ADR 0076 (owned background work, on
  `origin/docs/adr-0076-owned-background-work`), the user's decision of 2026-11-10 to move the family to sysinfo 0.40.
- Amends: ADR 0033 §1 (the identity API gains a three-valued look and proof tokens) and the text of rule 53.
- Number: 0081 was free on every `origin` branch on 2026-11-10 (`git log --all -- 'docs/adr/0081*'` is empty; the highest
  family ADR on any branch is 0080 on `origin/ci/bundle-kept-clone` and `origin/train/2026-11-09-3`). Check numbers are
  assigned on acceptance.
- Evidence read at `origin/feat/sysinfo-0-40` `2ea59896b` (the branch that found the class) unless a row names another
  ref. Every row below was read in the source; "unverified" marks what was not.

## Pattern

**A failed look at the process table is read as "the process is gone".** Whenever code asks "does process P still run?"
through a function that answers `bool` (or `Option`), the answer folds three facts into two: *running*, *gone*, and *could
not look* (the process table, the start time or a process handle could not be read). Which way "could not look" folds is
decided once, inside the helper, but the right way depends on what the caller does next:

| Next action of the caller | Safe only on | Folding "unknown" into the other value does |
| --- | --- | --- |
| delete a file or folder of P, start a second copy, take a lock over, remove program files | proven **Gone** | deletes, duplicates or takes over while P may run |
| report "stopped", "done", "exited", "crashed" | proven **Gone** | a false report (ER-33) |
| signal, end or "kill after the grace" | proven **Running** (and identity, rule 53) | kills a pid that may now be someone else, or kills blind |
| keep waiting, keep a file, show "unknown" | either | nothing harmful |

A `bool` cannot be safe in both rows at once. `paddock_proc::is_alive` folded unknown into "not alive" (right for "kill
only if alive", wrong for "delete if not alive"); app copies fold it the other way (right for deletes, wrong for "kill
after the grace"). The same helper was then called from both kinds of caller.

**The invariant that should hold.** A look at a process answers `Running`, `Gone` or `Unknown`. A destructive act or a
"stopped/done" report needs a proof that only a `Gone` answer can produce; a signal needs a proof that only a `Running`
answer can produce (and ADR 0033's identity check at the moment of acting). `Unknown` produces neither: the caller must
write what it does then (wait, keep, say "not confirmed"), and logs it once. No public function folds the three into two.

## Context: the incidents (2026-11-10)

sysinfo 0.40 made `System::new()` fallible (the family moved to it on `origin/feat/sysinfo-0-40` after the macOS
null-iterator panic). Reviews of that branch asked, for each `System::new()` that can now fail, what its callers do with
the failure, and found six call sites that treated "cannot read the process table / start time" as "gone":

| # | Where (fixed in) | Destructive act on "unknown" |
| --- | --- | --- |
| 1 | `paddock-ipc` `link/discovery.rs` `is_running_by` (81f7eca72, 2ea59896b) | deleted the companion's discovery file |
| 2 | `paddock-ipc` `link/client.rs` (via #1, `config.is_alive`) | started a second companion beside a live one |
| 3 | `paddock-service` `privileged/install.rs:513` uninstall wait (81f7eca72) | removed program files while the process might hold them |
| 4 | Dyno `app/src-tauri/src/quit.rs:322` `confirmed_gone` (81f7eca72) | reported "Quit and stop measuring" as confirmed |
| 5 | `tools/paddock-dev/src/wait.rs:392` `PidProbe` (81f7eca72, 2ea59896b) | reported `./dev wait --pid` done |
| 6 | Debrief `debrief-agent/src/lock.rs:152` run lock (81f7eca72, 2ea59896b) | took the run lock over |

The branch added `paddock_proc::Presence { Running, Gone, Unknown }` (`crates/paddock-proc/src/tree.rs:217`),
`presence(pid)`, `ProcessId::presence()` and `ProcessId::recorded_presence(pid, written_ms)` (`identity.rs:77`, `:100`),
`Terminated::Unknown` (`tree.rs:19`), and routed those callers. It kept the `bool` forms beside them, with doc comments
that say which direction each folds: `is_alive` (unknown is not alive, "for code that ends or waits") and `may_be_alive`
(unknown is alive, "for sweeps and discovery files"). Earlier instances of the same shape: Dyno's stale `agent.json`
(lessons "A discovery file is a hint, never proof", 2026-10-30), and the per-tick `Option<bool>` that Dyno's keep-awake
machine already uses (`dyno-agent/src/power.rs:514`).

A doc comment that names the direction is a reminder, not a mechanism (recurring-bug-adr pitfall 1): the survey below
finds the class again on the same branch, in shared code (`paddock-startup`, `paddock-mcp`, `paddock-proc` itself) and in
app copies the reviews did not reach.

## Instances: every liveness look, `origin/feat/sysinfo-0-40` `2ea59896b`, 2026-11-10

**Method.** `git grep` over all `*.rs` of the branch, outside `vendor/`, for: the names `is_alive`, `may_be_alive`,
`pid_alive`, `process_alive`, `is_running`, `presence(`, `recorded_presence`; per-pid sysinfo lookups
(`ProcessesToUpdate::Some`, `.process(`); `libc::kill(pid, 0)`; `OpenProcess` with `WaitForSingleObject`;
`ProcessId::of(..)` used as a test; and `try_wait()` folded into a `bool`. Each hit was read and classified by what the
`false`/`None` answer leads to. Name-only hits that are not about processes (`ollama.rs` `keep_alive`, ETW session
`is_running`, capture-table `is_running(id)`, `widgets/build.rs` downloads, Pitcrew's controller `process_alive()` over a
held child) are excluded. Survey commands and the raw hit list: scratch copy, not in the repo.

**Class key.** *Fixed*: routed through `Presence` on the branch. *Broken*: unknown already leads to a destructive act or a
false report. *At risk*: correct today only through the direction its helper happens to fold, or a bare pid. *Held*: a
handle of our own child (`try_wait`), where unknown is an `Err` the site folds. *Test*: a test that asserts "gone" with a
fold that passes on unknown (a probe that cannot see, ER-33).

### Shipped code

| # | Site | Look | On unknown | Class |
| --- | --- | --- | --- | --- |
| S1 | `paddock-ipc` `link/discovery.rs:82-112` `is_running` | `recorded_presence` then `may_be_alive` | running | Fixed (incident 1, 2) |
| S2 | `paddock-pitwall` `discovery.rs:87-93` `pid_alive` | `presence` then `may_be_alive` | alive, file kept | Fixed |
| S3 | `paddock-chromium` `launch.rs:408` profile sweep | `may_be_alive` | profile kept | Fixed |
| S4 | `paddock-service` `privileged/install.rs:513` | `presence() == Gone` | waits, says why | Fixed (incident 3) |
| S5 | `paddock-service` `backend/mod.rs:426` `process_presence` | `presence()` | stop not confirmed | Fixed |
| S6 | Dyno `quit.rs:322` | `presence() == Gone` | "not confirmed" | Fixed (incident 4) |
| S7 | `tools/paddock-dev` `wait.rs:392`, `:611` | `Presence` match | pending, then `::error::` | Fixed (incident 5) |
| S8 | Debrief `debrief-agent/src/lock.rs:152` | own `Look { Gone, Unknown, Started }` | held | Fixed (incident 6); a second tri-state type |
| S9 | Debrief `debrief-ai/src/engine/pitwall/discovery.rs:194-216` `pid_alive` | own sysinfo copy | alive, logged once | At risk: an app copy of S2 (rule 55) |
| S10 | `paddock-proc` `tree.rs:300` `terminate`, inside the grace loop | `!id.is_alive()` | **`Terminated::Gracefully`** | **Broken**: a table that becomes unreadable during the grace reports a polite stop that was never seen |
| S11 | `paddock-startup` `lifecycle.rs:51` `os_probe` (`Option<u64>`: `None` for unreadable *and* gone), used by `prune_instances` (`:180-206`) | `probe(pid).is_some()` | **gone** | **Broken**: removes the state folder of a separate instance that still runs |
| S12 | same `os_probe` in `previous_run` (`:150-157`), through `run_marker::begin` (Dyno `lib.rs:643`, `dyno-agent/src/cli.rs:278`) | `alive(&m, probe)` | **gone** | **Broken (report)**: says the previous run crashed |
| S13 | Debrief `app/src-tauri/src/lifecycle.rs:173-206` (`os_probe`, `alive`, `prune_instances`): a copy of S11 and S12 | as S11 | **gone** | **Broken**: same deletion in Debrief's own copy |
| S14 | Debrief `activity/report/recovery.rs:25` | `os_probe(..).is_some_and(..)` | "not alive" in the diagnostic report | At risk (report) |
| S15 | `paddock-mcp` `listener.rs:48` `writer_alive`, used by `client/connect.rs:202` | `ProcessId::of(pid).is_some_and(..)` (`of` is `None` when the start time cannot be read) | "the app is not open" | At risk (false report; the connect is refused, nothing deleted) |
| S16 | Debrief `debrief-etw/src/session.rs:166` `process_alive` (Windows), used by `stop_orphans` (`:191`) | `OpenProcess(SYNCHRONIZE)`; any `Err`, access denied included, is `false` | **gone** | **Broken**: stops the ETW session of a Debrief that runs elevated or as another user; also a bare pid (rule 53) |
| S17 | Debrief `debrief-plugin-dotnet/src/etw/session.rs:230`, `debrief-plugin-native/src/etw.rs:218` | S16's `process_alive` | "The process exited during the recording" | At risk (false report; the capture ends early) |
| S18 | Pitcrew `pitcrew-service/src/probe.rs:142` `alive`, used by `process_watch.rs:197` through `supervisor/processes.rs:28`, decided by `pitcrew-core` `process_track.rs:441` `next_step(still_alive: bool)` | own sysinfo lookup; no `System` is `true` | **running**, so after the stop timeout `StopStep::Kill` | **Broken**: ends a pid it cannot see, through `OsTerminator::end` by bare pid (`terminate.rs`, ADR 0033 WP-6 still open) |
| S19 | Pitcrew `supervisor.rs:2036` `adopted_exit` | own sysinfo lookup | still running | At risk (safe direction today, by the helper) |
| S20 | Pitwall `pitwall-server/src/system.rs:121` `pid_alive`, used by `serving.rs:395-398` (release loop) and `:1348` (`running_exited`) | own sysinfo lookup | alive | At risk: the release loop then forces `paddock_proc::end` by identity (guarded by ADR 0033), `running_exited` stays false |
| S21 | Debrief `storage.rs:236` `process_alive` (store folders) and `engine/mod.rs:398` `sweep_scratch` | own sysinfo lookup, bare pid | alive / sweep skipped | At risk: safe direction, but a bare pid keeps a recycled pid's folder (rule 53) |
| S22 | Dyno `dyno-agent/src/power.rs:514` `alive -> Option<bool>` into `dyno-core` `power/awake.rs:499` | sampler table | `None`, the hold stays | Fine: a third local tri-state |

### Test code (a fold that passes on unknown)

| # | Site | Assertion |
| --- | --- | --- |
| T1 | `paddock-proc` `login.rs:287-290` | `!p.is_alive()` for every leftover |
| T2 | `tools/paddock-dev` `input_hash.rs:672` | `!ProcessId::of(pid).is_some_and(is_alive)` |
| T3 | Dyno `dyno-platform/src/process.rs:432` | `!paddock_proc::is_alive(pid)` |
| T4 | `paddock-chromium` `launch/tests.rs:127`, `tests/it/real_browser.rs:489` | `!is_alive(pid)` |
| T5 | `paddock-ipc` `tests/it/child.rs:21` | `!is_alive(pid)` |
| T6 | Debrief `chromium-driver/src/process.rs:71` `process_exists` | `kill(pid, 0) == 0` (EPERM reads as gone) |
| T7 | Debrief `debrief-ipc/src/process.rs:697` `running` | `kill(pid, 0)` plus zombie check |
| T8 | Pitcrew `pitcrew-service/src/process.rs:428` (Windows, test) | `System::new().expect(..)` then `is_some_and` |

### Held handles (`try_wait` folded into a `bool`)

14 sites: `paddock-proc` `supervised.rs:340`, `:385`, `:448`; `paddock-service` `native.rs:994`; Debrief
`chromium-driver/src/process.rs:27`, `debrief-ipc/src/process.rs:250`, `:302`, `:520`, `:525`, `:579`, `contain.rs:269`;
Pitcrew `pitcrew-service/src/process.rs:265`, `supervisor.rs:1950`, `:1957`. `try_wait` on our own child fails only on
an OS error, rarely; each site still folds `Err` one way silently.

**Counts.** 22 shipped look sites in 20 files: 8 fixed on the branch, **5 broken** (S10, S11, S13, S16, S18; S12 a false
report), 8 at risk, 1 fine. Two local tri-states exist besides `Presence` (Debrief's `Look` in S8, Dyno's `Option<bool>`
in S22), and six app-local sysinfo liveness helpers (Debrief S9, S13, S21; Pitcrew S18, S19; Pitwall S20). 8 test sites assert "gone" with a
fold; 14 held-handle folds.

## Decision

### 1. One three-valued look, and no public fold

In `paddock-proc` (amends ADR 0033 §1):

```text
pub enum Presence { Running(Alive), Gone(Departed), Unknown(Why) }   // #[must_use], not #[non_exhaustive]
pub struct Departed { id: Ident, seen: Instant }    // proof: the OS said nothing holds this identity
pub struct Alive    { id: ProcessId, seen: Instant } // proof: the OS said this identity runs (rule 53 re-checks at the act)
pub enum Why { TableUnreadable(String), StartTimeUnreadable, HandleRefused(u32 /* OS code */) }
pub enum Ident { Recorded { pid: u32, written_ms: i64 }, Exact(ProcessId), Pid(u32) }

impl ProcessId { pub fn look(pid: u32) -> Look }      // replaces `of`: Found(ProcessId) | Gone | Unknown(Why)
impl ProcessId { pub fn presence(self) -> Presence }
impl ProcessId { pub fn recorded_presence(pid: u32, written_ms: i64) -> Presence }
pub fn presence(pid: u32) -> Presence                  // bare-pid look; Departed carries Ident::Pid
```

- **Removed (D1):** `paddock_proc::is_alive`, `paddock_proc::may_be_alive`, `ProcessId::is_alive`,
  `Presence::may_be_alive`, and `ProcessId::of` returning `Option`. Paddock is before v1: no shims.
- **Proof tokens (D2).** `Departed` and `Alive` have private fields and no public constructor; only a look builds them.
  "Gone" is monotonic for an identity (a pid plus start time never runs again), so a `Departed` stays true; "running" is
  not, so `Alive` is only an admission ticket: `ask`, `end`, `terminate` and `kill_tree` take it and still re-check
  identity at the act (ADR 0033 §2). A `Departed` built from `Ident::Pid` (no start time) is accepted only by acts that
  are harmless when the pid was recycled (a cache entry), never by a delete; the type says which (`Departed::exact()`
  returns `Option<ExactDeparted>`).
- **`terminate`** (S10): the grace loop matches the look; `Unknown` mid-grace returns `Terminated::Unknown`, never
  `Gracefully`.
- **Logs (ER-27).** A look that answers `Unknown` logs once per process and reason (`ProcLook=unknown Why=... Pid=...`),
  the pattern the branch already uses (`warn_once` in Pitwall and Pitcrew), moved into `paddock-proc` so the apps' copies
  go.

### 2. Destructive acts take a proof

Every shared helper that deletes, takes over, starts a second copy or reports an end takes the token, so the wrong
direction does not compile:

| Helper | Takes |
| --- | --- |
| discovery file removal (`paddock-ipc` `link/discovery.rs`, `paddock-pitwall` `discovery.rs`, `paddock-mcp` `listener.rs`) | `ExactDeparted` |
| `paddock-startup` `prune_instances`, `previous_run` (`Probe` becomes `&dyn Fn(u32) -> Presence`; `PreviousRun` gains `Unknown`, shown as "could not tell whether the last run ended cleanly") | `ExactDeparted` to delete or to say `Crashed` |
| `paddock-chromium` profile sweep | `ExactDeparted` |
| `paddock-service` uninstall wait and stop read-back | `ExactDeparted` to report stopped or remove files |
| `paddock_proc::{ask, end, terminate, kill_tree}` | `Alive` |
| `paddock_testkit::proc::{assert_gone, wait_gone}` (new) | fails on `Unknown` with the reason (T1-T8) |

An app's own destructive decision (Debrief's run lock, ETW orphan stop, store and scratch sweeps; Pitcrew's stop watch)
matches the three arms; `next_step` in `pitcrew-core` takes `Presence` and gains `StopStep::Unconfirmed` (no kill, the
row says "Could not check whether it stopped").

### 3. The check: every look is enumerated (number on acceptance; D3)

1. **Clippy `disallowed-methods`** in `lint/clippy.toml` (every app copies the preset, plan X11): `sysinfo::System::process`,
   `sysinfo::System::processes` and `sysinfo::System::refresh_processes_specifics` with reason "a per-pid look: use
   `paddock_proc::presence` / `ProcessId::look`; a sampler that only displays carries `#[expect(clippy::disallowed_methods,
   reason = "liveness: sampler, display only")]`". `paddock-proc` itself is exempt. This is ADR 0033's mechanism, applied to
   looks instead of kills. On the branch, 33 non-test files outside `paddock-proc` call a `.process(` method and 18 of them
   name sysinfo (samplers included; WP-0 reads the other 15); each gets its `expect` or is migrated.
2. **Conformance check (shapes Clippy cannot see)**, in the C53 scanner (comment- and string-masked Rust, `maskRust`):
   `kill(<pid>, 0)` / `kill(<pid>, None)`, `OpenProcess(..SYNCHRONIZE..)` with `WaitForSingleObject(.., 0)`,
   `GetExitCodeProcess` against `STILL_ACTIVE` on a handle opened by pid, a `fn` whose name matches
   `alive|running|exists` that takes a `u32` pid and returns `bool` or `Option<bool>`, `try_wait()` folded by
   `matches!`/`is_some`/`.ok()` (the held class), and, on `Presence`, `!=`/`!matches!` against `Running` or a `_ =>`
   arm (the fold reappearing). Every hit outside `paddock-proc` must be in the register; tests are in scope (T-class).
3. **The register `docs/liveness.toml`**: one entry per allowed site with `path`, `kind` (`sampler` | `held` | `test-probe`
   | `migrating`), `reason`, and for `migrating` a `delete_by` tag (an app's next tag). Shrink-only, like C71's baseline:
   an entry the merge base lacks fails, a stale entry warns. The register starts with S9, S11-S21 and T1-T8 as
   `migrating` and the 14 held sites as `held`.
4. **Positive controls (ER-33).** Fixtures that must fail: a pid helper returning `bool`, `kill(pid, 0)`, a `Presence`
   wildcard arm, an unregistered `.process(` call; and `paddock-proc` unit tests that drive every look through a failing
   `System` constructor (the branch's `presence_with` seam) and a refused handle on Windows.

### 4. Apps with their own checks (Pitwall server, Pitcrew, Debrief)

They delete their helpers and call `paddock_proc` (all three already depend on it): Pitwall `system::pid_alive` (S20) and
its `warn_table_once`; Pitcrew `probe.rs::alive` and `adopted_exit` (S18, S19), with `OsTerminator` taking `Alive` (closes
ADR 0033 WP-6 for the stop watch); Debrief's lifecycle copy (S13, S14: use `paddock-startup`'s, a registered copy until
then), `debrief-ai` discovery (S9: use `paddock-pitwall`'s `Locator`), `debrief-etw` `process_alive` (S16, S17: a
`Presence` look by handle where `ERROR_ACCESS_DENIED` is `Unknown` and only the OS's "no such process" answer is `Gone`;
which code that is, `ERROR_INVALID_PARAMETER` by common report, is unverified and confirmed in WP-0), store and
scratch sweeps (S21: `ExactDeparted` from the folder's recorded start time), and the run lock's `Look` (S8) becomes
`Presence`. Dyno's sampler tri-state (S22) stays, registered as `sampler`. Each copy found here goes into
`docs/known-copies.toml` until it is deleted (rule 55).

### 5. Relation to ADR 0033, ADR 0072 and ADR 0076

- **ADR 0033** answers *which* process an act reaches (identity at the moment of acting). This ADR answers *whether we
  know* it runs. They compose: a look yields `Alive` or `Departed`; an act on `Alive` still checks identity (0033 §2); a
  `Departed` is the only way to "forget" an identity. C53 (kills) and this check (looks) share one scanner and run side
  by side; rule 53 gains one sentence (§7).
- **ADR 0072** (connection ends say why): a client that decides from a discovery file that its companion is gone and
  starts or replaces one (`paddock-ipc` `link/client.rs`, `Replace::Ask`) needs `ExactDeparted`; when the look is
  `Unknown` it does not replace and reports the existing 0072 refusal (`Busy` or the transport end) with "could not tell
  whether the companion still runs" in the detail. 0072's ending harness adds an "unreadable process table" fault to its
  scripted faults. No new 0072 reason is needed.
- **ADR 0076** (owned background work): the reaper and `Supervised` look at children through held handles; their
  "ended" reports need the same proof, so 0076's leak guard uses `paddock_testkit::proc::assert_gone` (fails on
  `Unknown`). The held-handle sites of this survey are 0076 territory and are registered, not rewritten, here.

### 6. Parallelism and cost (rule 51)

No new OS work: every look is the same per-pid refresh it is today (one `System` and one `ProcessesToUpdate::Some`), and
the token types are zero-sized wrappers around data already read. What changes is behaviour under failure: loops that
used to end on a failed look now keep waiting until their own bound or report "not confirmed", so a broken process table
can make a wait last to its bound instead of ending early; each such wait already has a bound (ADR 0031 patience, the
`./dev wait --max`). The migration is file-disjoint per app and can run as one implementer per app after the shared
change lands; it is not on any hot path, so no benchmark is quoted (rule 48).

### 7. Rule text (rule 53 amendment, on acceptance)

> A look at a process answers running, gone or unknown; a failed look is unknown, never gone. Only a proven gone deletes,
> takes over, starts a second copy or reports an end; only a proven running is signalled. Checked by Clippy
> `disallowed-methods` on per-pid lookups and the liveness check with its register `docs/liveness.toml` (ADR 0081).

## Work packages (raw estimates, one agent)

| WP | Content | Days |
| --- | --- | --- |
| WP-0 | Re-read every row of the survey on the then-current branch (sysinfo 0.40 landed), fill `docs/liveness.toml`, confirm S16 with a refused-handle test on the Windows VM | 0.5 |
| WP-1 | `paddock-proc`: `Presence` with tokens, `ProcessId::look`, removal of the bool forms, `terminate` fix (S10), once-per-reason logging, failing-constructor and refused-handle seams; 99 % line coverage | 1.5 |
| WP-2 | Shared consumers: `paddock-startup` (S11, S12), `paddock-mcp` (S15), `paddock-ipc`, `paddock-pitwall`, `paddock-chromium`, `paddock-service` onto tokens; `paddock-testkit::proc` (`assert_gone`, `wait_gone`) | 1.5 |
| WP-3 | The check: Clippy entries in `lint/clippy.toml` and every app's copy, the conformance shapes in the C53 scanner, the register with its shrink-only rule, fixtures and positive controls | 1.5 |
| WP-4 | Debrief: S8, S9, S13, S14, S16, S17, S21, T6, T7 | 2 |
| WP-5 | Pitcrew: S18 (`next_step` three-valued, `Unconfirmed`, `OsTerminator` on `Alive`), S19, T8 | 1.5 |
| WP-6 | Pitwall server: S20 | 0.5 |
| WP-7 | Dyno and tools: S22 registered, T3; T1, T2, T4, T5 onto `assert_gone` | 0.5 |
| WP-8 | Rule 53 text, lesson, ADR 0033 amendment note, README row | 0.25 |
| | **Mechanism (WP-0 to WP-3, WP-8)** | **5.25** |
| | **Adoption (WP-4 to WP-7)** | **4.5** |
| | **Total** | **9.75** |

**Order.** `feat/sysinfo-0-40` lands first (the user's decision of 2026-11-10; it already routes the six incidents).
WP-1 to WP-3 change `crates/` and `lint/`, which every release builds from, so they land in the first shared window the
coordinator names, outside a release freeze (ADR 0049). WP-3's check is report-only for registered sites and an error for
new ones from the day it lands. Each app's WP lands inside its own batches, before its next tag (D5): Debrief 1.2 (S13,
S16 delete or stop), Pitcrew 1.1 (S18 kills), Pitwall 1.1 (S20), Dyno's next tag (S12 via `paddock-startup`).

## Decisions for the user

| # | Question | Options | Recommendation |
| --- | --- | --- | --- |
| D1 | The `bool` and `Option` forms | (a) remove them outright; (b) keep them behind Clippy `disallowed-methods` and the register | **(a)**: a fold that exists will be called; before v1 no shims (user, 2026-11-06) |
| D2 | Proof tokens | (a) `Departed`/`ExactDeparted`/`Alive` required by shared destructive helpers; (b) a plain three-arm `match` everywhere, checked only by lint | **(a)**: makes the wrong direction a compile error in shared code; apps still match three arms |
| D3 | Where the check lives | (a) Clippy on per-pid sysinfo lookups plus a new conformance check sharing C53's scanner, own register; (b) fold the shapes into C53 | **(a)**: C53 is about kills and has its own waivers; one register per question keeps both readable |
| D4 | `ProcessId::of` | (a) replace by `ProcessId::look -> Found | Gone | Unknown`; (b) keep `of` and add `look` | **(a)**: `of`'s `None` is the same fold (S15) |
| D5 | Release gating | (a) the broken rows (S10-S13, S16, S18) block the owning app's next tag; (b) fix in normal order | **(a)**: each deletes, stops, kills or reports falsely on a real user's machine when the table cannot be read |
| D6 | Held-handle `try_wait` folds (14) | (a) register as `held`, make the `Err` arm explicit when touched; (b) rewrite now | **(a)**: rare, owned by ADR 0076's mechanism |
| D7 | Lock probes (`dyno-agent::instance::is_held`, the "held" re-check of lessons 2026-10-30) | (a) list as candidates in WP-0, decide after reading; (b) include now | **(a)**: not surveyed for this draft |

## Consequences

- A failed process-table read can no longer delete, take over, start a duplicate, kill or report an end anywhere the
  check sees. Some waits that used to end on a failed read now run to their bound and say "not confirmed" (wanted: ER-33).
- `paddock-proc`'s public API breaks (no shims); every workspace changes in the same landing as WP-1 and WP-2 (rule 43).
- Five app-local liveness helpers and two copies (Debrief's lifecycle, Debrief's Pitwall discovery) go.
- Tests that asserted "nothing is left" with a fold now fail on a host where they cannot look, instead of passing.

## Alternatives rejected

- **Keep `is_alive` and `may_be_alive` with better doc comments** (the branch's state): the survey found the fold again in
  `paddock-proc`'s own `terminate` and in three shared crates the reviews did not reach.
- **Pick one fold direction for the whole family** (unknown is always alive): makes "kill after the grace" kill blind
  (S18) and "report done" never finish; no single direction is safe for every caller.
- **Retry the read until it succeeds**: a `System::new()` that fails (sysinfo 0.40) fails for a reason that usually
  lasts (permissions, a sandbox); retrying only delays the same decision.
- **Lint only, no tokens**: leaves shared destructive helpers callable with a `bool` computed anywhere.

## The failing tests the fixes start with (rule 5 / ER-11; not written in this change)

- `paddock-proc`: `terminate_says_unknown_not_gracefully_when_the_table_becomes_unreadable_during_the_grace` (a scripted
  look: `Running`, then `Unknown`; today `Gracefully`).
- `paddock-startup`: `prune_instances_keeps_a_live_instance_folder_when_the_process_table_cannot_be_read` and
  `previous_run_is_unknown_not_crashed_when_the_table_cannot_be_read` (through `os_probe` with a failing constructor;
  today the folder is removed and the run is `Crashed`). The same pair in Debrief's `lifecycle.rs` until its copy goes.
- Debrief `debrief-etw`: `stop_orphans_keeps_a_session_whose_owner_refuses_the_handle` (an `OpenProcess` seam answering
  access denied; today the session is stopped).
- Pitcrew `pitcrew-service`: `a_stop_whose_process_cannot_be_looked_at_is_never_ended` (`advance` past the timeout with
  an unreadable table; today `Terminator::end` is called).
- `paddock-mcp`: `connect_says_it_cannot_tell_when_the_writer_cannot_be_identified` (today `NotOpen`).

## Rollback

The check can return to report-only by one line; Clippy entries can be removed from `lint/clippy.toml`. The API change is
not rolled back piecemeal: reverting WP-1 reverts WP-2 with it.
