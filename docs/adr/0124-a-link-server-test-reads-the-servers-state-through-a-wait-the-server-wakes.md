# 0124. A link-server test reads the server's state through a wait the server wakes, never at a moment the client chose

- Status: Accepted 2026-11-26 by the user (D1 = C: option B plus the Clippy ban with a positive control), relayed by the coordinator.
  The investigation and this redesign ADR were approved the same day (ER-21). It took number 0124 as train 2026-11-26-t landed
  (the `NEW-` rule of AGENTS.md).
- Date: 2026-11-26
- Related: ADR [0122](0122-endings-scenario-preconditions.md) (the endings rigs' preconditions and act gate: the other direction of
  the same class, accepted 2026-11-25), ADR [0072](0072-every-connection-end-says-why-in-order.md) §5, ER-11, ER-12 (no sleeps or longer
  timeouts), ER-21, ER-28 (a cause is confirmed by a run without it and with it back), ER-33, ER-37, ER-41 (report-only first), ER-44,
  rule 55 (one implementation per mechanism).
- Evidence heads read (ADR 0091): `origin/main` 4ee7d98fc8330acf3c466af1ce84910e54a7dc65, re-read before commit at
  43c28c456bcbe5b59f55a7e9206bafed0573e208 (only `crates/paddock-ipc/fuzz/Cargo.lock` changed under paddock-ipc); `fix/ipc-link-flakes` (this branch, from that
  commit); `feat/adr-0122-wp2-probe-gate` at 24df86f03 (on origin/main) **plus its uncommitted worktree diff** (28 files, read with
  `git diff`, not checked out). No claim here is about released code: `git tag --contains 24df86f03` lists no tag.

## The class

**A link-server test treats an OS event on the client's side as the barrier for a step of the server's accept or handshake thread, but
nothing orders the two, so under CI load the test sees the server one step early (a record not yet made) or one step late (a stream
already closed).**

The invariant: a test that needs the server to have done X waits for X through a signal the server raises after X (bounded by a hang
guard); a test that needs the server not to act before the client has done Y holds the server at a gate until Y is measured (ADR 0122).
The client's own observation (an answer, an end of stream, a returned `connect`) is never the barrier for server state.

## Instances

| Run | OS, job | Test | Direction | What happened | Status |
|---|---|---|---|---|---|
| 9150 | macOS, `crate (macos-latest)` job 51219, main at 4ee7d98fc | `edges::a_link_server_with_no_descriptor_left_to_accept_tells_the_waiting_client_why` | server one step early | `edges.rs:470` "the server records why: refused: []" in 0.095 s; passed on retry | **reproduced and fixed on this branch** (below) |
| 9156 | Windows, `crate (windows-latest)` job 51298, `train/2026-11-26-s` at 43c28c456 | `endings::ipc_link_server_ends_every_connection_right_or_is_in_the_ratchet` | server one step late | "ipc-link-server busy: RIG FAILURE (write)" plus "stale ratchet entry WP-2: busy: drain-not-eof:none no longer happens"; passed on retry | ADR 0122 instance; fixed by its WP-2 (in progress, uncommitted) |
| 8425 | Windows, job 43164 | the same endings test | server one step late | the same "RIG FAILURE (write)" | ADR 0122's instance row; lessons "Fix items from Train A's full rounds" |
| 8174 | Windows (per lessons) | the same endings test | server one step late | named as the first Windows instance in docs/lessons.md ("Second Windows instance after run 8174"); the log was not re-read | not re-checked |

Logs were read in full from the Gitea API into the agent's scratch folder (job 51219 and 51298); only the failure blocks are quoted above.

### 9150: the cause, with evidence

- **The path.** `link_out_of_files` (`crates/paddock-ipc/src/bin/test_child.rs`) fills the child's descriptor table, prints `ready`,
  and after the test's line `done` prints `server.refused()`. The test's client connects, reads its answer (on macOS: the stream ends),
  writes `done`, and asserts that the printed list names "Too many open files". The accept thread (`LinkServer::start`,
  `crates/paddock-ipc/src/link/server.rs`) calls `accept`, gets EMFILE, and only then calls `Shared::refuse`.
- **macOS drops the client inside the failed accept.** XNU's `accept_nocancel` takes the connection off the queue, then `falloc` fails
  and it calls `soclose(so)`: "Don't put this back on the socket like we used to, that just causes the client to spin. Drop the
  socket." (`bsd/kern/uipc_syscalls.c`, <https://github.com/apple-oss-distributions/xnu/blob/main/bsd/kern/uipc_syscalls.c>, read
  2026-11-26). Measured on this Mac with a synthetic probe (Python, AF_UNIX, no product code): the client's read stays pending for
  300 ms before the accept, the server's one `accept` fails with EMFILE, and the client then sees EOF while the server does nothing
  more (3 of 3 runs). So the client's "answer" is complete before `accept` returns to user space, and the comment above the record
  ("Recorded before the client is answered, so `refused()` never trails what a client was told") holds only where `turn_away` answers
  (Linux).
- **Hypotheses (ER-28) and what ruled each out:**

  | Cause | Observation that rules it out | Result |
  |---|---|---|
  | Race between the failed accept and the record (above) | the failure stops when the child waits for the record, and comes back without the wait | **ruled in**: 7 of 3,600 before, 0 of 10,800 after, 7 of 3,600 with the wait removed again |
  | A descriptor was free, so the accept succeeded | the client would wait for its handshake limit (seconds); the failed try took 0.095 s, like a pass | ruled out |
  | The accept thread never ran before `done` | the client would stay queued (the probe: pending until accept) and hit its read guard; 0.095 s | ruled out |
  | Another error was recorded | the list printed was empty, not a different error | ruled out |
  | Timing-dependent assertion, shared port or path between parallel tests | the child has its own temp folder and socket; the assertion compares content, not time | ruled out by the code |
  | Shutdown order (`drop(fillers)` before the print) | the fix leaves that order as it was and is green | ruled out |

- **Counts** (this Mac, `stress.sh`: 12 parallel loops of the `it` binary's one test plus 10 busy-loop CPU burners, through
  `./dev throttle`, 15-minute load 6.5 to 13.5): before the fix 1 of 1,200 (4 burners) and 7 of 3,600; with the fix 0 of 3,600, 0 of
  3,600, 0 of 3,600 (interleaved with the next row); with the cause put back (the child reads without waiting) 7 of 3,600. At p = 7/3,600
  about 1,540 green runs give 95 % confidence (bug-fix skill, step 6); 10,800 were run. Without burners: 0 of 300.

### 9156 / 8425: the cause (inferred, as in ADR 0122)

The busy rig (`tests/it/endings.rs`, `link::scenario`, `Scenario::Busy`) connects a raw stream and then writes its hello. The accept
thread refuses busy right after `accept`, without reading (`auth.refuse_busy`), and drops the stream. On Windows the accept waits on
`ConnectNamedPipe` with an event (`crates/paddock-ipc/src/endpoint/windows.rs`), so it wakes at once and can close the pipe before the
client's `WriteFile`; a write to a pipe whose server end is closing fails (`ERROR_NO_DATA`, 232, "The pipe is being closed":
<https://learn.microsoft.com/en-us/windows/win32/debug/system-error-codes--0-499->, from memory, not re-read), which the rig reports as
`RIG FAILURE (write)`. On Unix the accept polls every 5 ms (`Listener::accept` in `endpoint/unix.rs`), so the client's write almost
always comes first, which fits the Windows-only record. Not reproduced: there is no Windows machine here (rule 101).

## Every place with the shape (origin/main plus this branch)

| Where | Reads | Barrier today | Status |
|---|---|---|---|
| `bin/test_child.rs` `link_out_of_files` | `refused()` after the client's end | none | **broken (9150)**; fixed here with `wait_refused` |
| `tests/it/edges.rs` `a_link_server_refuses_a_wrong_token_and_keeps_serving` | `refused()` after the client's Fail | a `yield_now` spin with a guard | fine; moved to `wait_refused` here |
| `tests/it/clocked.rs` `a_silent_client_...`, `the_refusal_list_keeps_the_latest_64` | `refused()` | `wait_until` spin | fine; moved to `wait_refused` here |
| `tests/it/clocked.rs` (busy refusal, around line 223) | `refused()` | `wait_until` spin | fine; moved to `wait_refused` (WP-1) |
| `tests/it/clocked.rs`: `wait_until` on `server.peers().len() == 1` and on `.is_empty()` | `peers()` | `wait_until` spin | fine; moved to `wait_peers`, `wait_until` deleted (WP-1) |
| `tests/it/link.rs` around line 197: `assert_eq!(first.peers().len(), 1)` after the client's call was answered | `peers()` | **none**: the handshake thread pushes the peer after `Peer::start`, so the peer can answer a call before it is in the list | **at risk**, not seen failing; moved to `wait_peers` (WP-1), and the same read in `edges.rs` `a_link_server_refuses_a_wrong_token_and_keeps_serving` |
| endings busy rig (`link::scenario`, Busy) | the server must not refuse before the hello is queued | none | **broken (9156, 8425)**; ADR 0122 WP-2's act gate |
| endings `client-keeps-writing`, `wrong-account` rigs | input queued before the refusal | one write / order | ADR 0122's table (fixed or fine by order) |

No app uses `LinkServer` yet (`git grep LinkServer -- apps` finds none), so `refused()` and `peers()` have no product reader today.

## Common practice

- **Wait on a condition the producer signals, with a bound.** Rust's `Condvar::wait_timeout_while` blocks "until the provided condition
  ... is false" or the timeout passes (<https://doc.rust-lang.org/std/sync/struct.Condvar.html#method.wait_timeout_while>); Java's
  Awaitility asserts asynchronous state by awaiting a condition with a maximum, never by sleeping
  (<https://github.com/awaitility/awaitility>); this crate's own test helpers already do the spin form (`wait_until` in `clocked.rs`,
  the loop in `edges.rs`).
- **A test hook that orders the server's internal steps.** Go's `net/http` exports hooks for its own tests (cited in ADR 0122). That is
  ADR 0122's act gate, the inverse direction.
- **Do not infer server state from what the client saw.** The OS is free to answer the client before the server's user space runs
  (XNU above; Linux's own `turn_away` order is a property of this code, not of the OS).

## Options

| | Option | Cost | What it leaves |
|---|---|---|---|
| A | **Cheapest.** Fix 9150's child (done on this branch) and let ADR 0122 WP-2 fix the busy rig | done here, 0 more | the at-risk `link.rs` read, and the next test that reads `refused()`/`peers()` at a client-chosen moment |
| B | **Common practice (recommended).** A, plus `wait_peers` beside `wait_refused`, every test read of `refused()`/`peers()` moved to the waits, and the spin helpers removed | about 0.5 agent-day | nothing found today; a new direct read is caught only in review |
| C | B, plus a check: `LinkServer::refused` and `LinkServer::peers` in `crates/clippy.toml` `disallowed-methods` with the reason "a snapshot; wait with wait_refused / wait_peers", so a direct read needs a written `#[expect(clippy::disallowed_methods, reason = ...)]` | about 0.75 agent-day (0.25 beyond B) | nothing; product code that wants a snapshot states why |
| D | A simulated transport (turmoil-like) | rejected | ADR 0122 option D: the tests exist to see the real OS endings |

## Decision (option C, accepted)

1. **`LinkServer::wait_refused(max, until) -> Result<Vec<IpcError>, Vec<IpcError>>`** (on this branch): the refusal list gains a
   `Condvar` that `Shared::refuse` notifies after each record; the wait uses `Condvar::wait_timeout_while` and returns `Err` with the
   last list when `max` (OS time, a hang guard) passes. `refused()` stays, documented as a snapshot that can trail what a client saw.
2. **`LinkServer::wait_peers(max, until) -> Result<Vec<Peer>, Vec<Peer>>`** over the open peers: a `Condvar` beside the peer list,
   woken when the handshake thread lists a peer (`Shared::add_peer`) and when a listed peer's close is done (`stop` takes `&mut self`,
   so no waiter can be woken by it). `until` runs under the list's lock, so it must not call the server or close a peer; the same holds
   for `wait_refused`. Each wake site has a deterministic test that parks the wait first (`listing_a_peer_wakes_a_parked_wait_for_it`,
   `a_listed_peer_closing_wakes_a_parked_wait_for_it`, in `link/server.rs`). The close wake is a crate-private `Peer::when_closed` watcher (`crates/paddock-ipc/src/peer.rs`): run once by `report_done`
   after the close is reported, with no lock of the peer held, or at once if the peer is already closed; it holds the server only
   weakly. `stop` now takes the peers out of the list before it closes them, because the watcher takes the list's lock. `add_peer`
   reads the stop flag under the list's lock and closes the peer when the server has stopped: `stop` does not join the handshake
   threads, so a handshake that passed its own stop check could list a peer after `stop` emptied the list and leave it open
   (found in review, older than this ADR; `a_peer_listed_after_stop_is_closed_not_left_open`).
3. **Every test read moves to the waits**: the `yield_now` loop in `edges.rs` and `clocked.rs`'s `wait_until` are deleted, and the
   at-risk reads in `link.rs` and `edges.rs` (`peers().len() == 1` right after an answered call) wait for the list.
4. **The other direction is ADR 0122's act gate**, unchanged: this ADR adds no second gate.
5. **The check:** `LinkServer::refused` and `LinkServer::peers` are in `crates/clippy.toml`'s `disallowed-methods` (with
   `allow-invalid`, since crates that do not depend on paddock-ipc cannot resolve the paths). Clippy runs with `-D warnings` on every
   crate change (built on origin/main), so no new gate. It lands blocking: no product reader exists and its first run on the real tree
   (the whole `crates` workspace, this branch) was green (ER-41). **Positive control:** `edges.rs` reads each snapshot once, in its own
   function under `#[expect(clippy::disallowed_methods)]`; when an entry stops biting, its expectation is unfulfilled and Clippy is red
   (seen on this branch: with the `peers` entry removed, "this lint expectation is unfulfilled" at `edges.rs:115`). The apps' own
   `clippy.toml` files do not carry the entries: no app uses `LinkServer` yet; the app that adopts it (Dyno M3) adds them.

## What it removes

- The 9150 class of flaky failures and its FLAKY retries on the macOS crate job.
- The two spin-wait copies (`edges.rs` loop, done; `clocked.rs` `wait_until`, WP-1).
- A false comment in the accept loop (corrected here).
- Adds: two `Condvar`s and two public wait methods on `LinkServer`, a crate-private close watcher on `Peer`, two Clippy entries and
  their controls. No log line or wire change; `stop` closes its peers after taking them out of the list instead of while holding it.

## Work packages

| WP | What | Agent-days | State |
|---|---|---|---|
| 0 | `wait_refused`, the 9150 fix in the test child, the edges and two clocked reads moved, the accept-loop comment corrected | 0.25 | **done on `fix/ipc-link-flakes`** |
| 1 | `wait_peers` and `Peer::when_closed`; move `link.rs`'s and `edges.rs`'s `peers()` reads and the remaining `clocked.rs` reads; delete `wait_until` | 0.5 | **done on `fix/ipc-link-flakes`** |
| 2 | the `disallowed-methods` entries with their reason; the positive controls | 0.25 | **done on `fix/ipc-link-flakes`** |
| — | the busy rig | — | ADR 0122 WP-2 (prerequisite for 9156/8425; held uncommitted in the `adr-0122-wp2` worktree) |

## Checks it relies on

- Clippy `-D warnings` on crate changes: built on origin/main (CI's crates job).
- ADR 0122's precondition verdict: WP-1 built on origin/main (24df86f03); the act gate is WP-2, a prerequisite, not on any branch yet.

## Decisions for the user

- **D1.** Taken 2026-11-26: C. None open.

## Consequences

- A test that needs server state waits for it; under load it is slower only by the time the server really takes.
- Rollback: the waits are additive; reverting a test to `refused()` brings back only that test's race.
