# 0122. Every endings scenario declares its precondition, and the harness establishes and checks it

- Status: Accepted 2026-11-25 by the user: D1 = option C (set up and verify), D2 = an act gate in the endpoints. The redesign itself
  was decided the same day (ER-21). Revised the same day after an independent review (4 P1s, P2-1 to P2-8).
- Date: 2026-11-25
- Related: ADR [0072](0072-every-connection-end-says-why-in-order.md) §5 (the ending harness and its ratchet; this ADR amends it),
  ER-12, ER-21, ER-28, ER-33 (a check counts only if its precondition was verified), ER-37, ER-41 and rule 5 (report-only first),
  ER-44, ER-46 (lists only shrink), rule 55 (one implementation per mechanism).
- Number: 0122. On 2026-11-25 the highest ADR on any local branch or `origin` branch was 0121 (origin/main and
  `chore/conformance-flip-and-drop`).
- Evidence heads read (ADR 0091): `origin/main` 07dc7eb685c7acde6d76b199a7600b99ae79d54a; `fix/ipc-endings-i10` (local, not on origin)
  8f14beeeac4c5255e36b8ffd8cf8e60ce119f1b4. Code was read with `git show`, not checked out. Nothing here claims released code.

## The class

**An endings scenario depends on a state that the harness assumes but does not establish or check, so when the race goes the other way
the result is reported as a finding about the endpoint (a stale or a new ratchet entry), or as an unexplained rig failure.**

The invariant: before the harness lets the endpoint act, the scenario's precondition holds, and the harness has evidence that it held.
If it did not, the outcome is `rig failure: precondition X not met`, and the scenario's faults are not compared with the ratchet.

## Instances

| Run | Rig and scenario | Hidden precondition | What happened | Source |
|---|---|---|---|---|
| 8174 (Linux) | mini-reference, owner-close-while-peer-not-reading | the peer reads again before the reason deadline passes | the harness let the peer read after it advanced the clock; `drain-not-eof` once in about 3000 | docs/lessons.md, "Two shared-crate tests failed once each in run 8174"; fix: `mini::run_with` (origin/main, `crates/paddock-ipc/src/testing/endings/mini.rs`) |
| 9001 (Linux) | ipc-host, client-keeps-writing | the client's later bytes are already queued when the host refuses | hello and junk went in separate writes; the host read the hello and closed before the junk came; ordered end; "stale ratchet entry I10" | commit 8f14beeea on `fix/ipc-endings-i10` (a forced gate reproduced it 20 of 20; fixed order 200 of 200 and 400 of 400) |
| 8425 (Windows) | ipc-link-server, busy | the refused client's hello is written before the server refuses | "ipc-link-server busy: RIG FAILURE (write)", passed on retry | docs/lessons.md, "Fix items from Train A's full rounds"; cause **inferred from code, not reproduced**: `LinkServer` refuses busy right after `accept`, without reading (`crates/paddock-ipc/src/link/server.rs`, the accept loop calling `auth.refuse_busy`), while the rig writes the hello after `Stream::connect` returns (`crates/paddock-ipc/tests/it/endings.rs`, `link::scenario`), so the write can meet a pipe the server already closed |

8174 and 9001 were confirmed real failures by a reproduction (lessons and commit body). 8425 is a real FLAKY failure (try 1 red); its
cause is the inference above.

Every rig and scenario with such a state, on origin/main plus `fix/ipc-endings-i10` (8 rigs: ipc-host, ipc-link-server,
ipc-launcher, mini, dyno-agent, dyno-helper, mcp-loopback, pitcrew-service):

| Precondition | Rig / scenario | Status |
|---|---|---|
| input queued when refused | ipc-host wrong-account, busy (`account_scenario` writes before `accept_raw`) | fine by order, not verified |
| | ipc-host, ipc-link-server, dyno-agent client-keeps-writing (`ipc::black_box`) | was broken (9001); fixed on `fix/ipc-endings-i10` |
| | mcp-loopback, pitcrew-service client-keeps-writing | fixed on `fix/ipc-endings-i10` (same helper) |
| | mini client-keeps-writing (writes, then `accept_exactly`) | fine by order, not verified |
| | ipc-link-server busy | broken (8425, inferred) |
| | mcp-loopback busy (connect, then write) | at risk if the endpoint refuses before it reads (the 8425 order); not checked |
| | pitcrew-service busy | does not apply: the service never refuses busy (ratchet I26: it waits on its semaphore on Linux and macOS, stops serving on Windows) |
| | dyno-helper busy | does not apply: its client writes nothing; the helper drops the 17th connection unanswered (ratchet I16 `no-reason`) |
| peer not reading at the close | mini owner-close-while-peer-not-reading | was broken (8174); fixed |
| | ipc-host owner-close-while-peer-not-reading (32 pushes of 256 KiB, "the buffers fill") | at risk: assumed, never checked |
| every place taken before the next client | dyno-helper busy (`Stream::connect` x16 returns at the backlog, before the server counts them) | at risk, not reproduced; out of this ADR until a reproduction exists |

`Stall` is not in the table: its rig already waits for the send to wait on the clock before it advances it (`peer_scenario`), which is
the barrier and the evidence this ADR asks for.

## Common practice

- **A barrier or hook before the act.** Go's `net/http` exports test hooks that order internal events in its own tests
  (`testHookReadLoopBeforeNextRead`, `SetTestHookServerServe`, `SetEnterRoundTripHook`:
  <https://raw.githubusercontent.com/golang/go/master/src/net/http/export_test.go>, read 2026-11-25). turmoil, a Rust network
  simulator, runs hosts on one thread and offers `hold` and `release` of in-flight messages and `partition` and `repair`
  (<https://docs.rs/turmoil/latest/turmoil/>, read 2026-11-25).
- **A script that fixes the order and checks socket state.** Google's packetdrill runs timestamped script lines in order, injects
  packets and checks what the stack does (<https://github.com/google/packetdrill>, read 2026-11-25).
- **An unmet precondition is not a product failure.** JUnit 5 throws `TestAbortedException` when an assumption fails: the test is
  aborted, not failed (<https://docs.junit.org/current/writing-tests/assumptions.html>, read 2026-11-25). Here it is red after the flip
  (ER-33: a check that did not see its case does not pass), but it is reported apart from the endpoint's faults.
- **The OS can tell how many bytes are unread.** Linux `unix(7)`: `SIOCINQ` "returns the number of unread bytes in the receive buffer"
  (<https://man7.org/linux/man-pages/man7/unix.7.html>); macOS `getsockopt(2)`: `SO_NREAD`, and `FIONREAD`; Windows `PeekNamedPipe`
  returns `lpTotalBytesAvail` for a named pipe handle, server or client
  (<https://learn.microsoft.com/en-us/windows/win32/api/namedpipeapi/nf-namedpipeapi-peeknamedpipe>); Winsock `FIONREAD` on a stream
  socket "returns the total amount of data that can be read in a single receive operation; this is normally the same as the total
  amount of data queued on the socket (since a data stream is byte-oriented, this is not guaranteed)"
  (<https://learn.microsoft.com/en-us/windows/win32/winsock/winsock-ioctls>, read 2026-11-25). The Windows TCP probe therefore only
  claims n > 0 or a lower bound, never an exact count.
- **Why unread input matters.** RFC 1122 §4.2.2.13: a host that closes "while received data is still pending in TCP ... SHOULD send a
  RST to show that data was lost" (<https://www.rfc-editor.org/rfc/rfc1122.txt>). AF_UNIX on Linux does the same (commit 8f14beeea
  cites `unix_release_sock`; kernel source **not re-read** here).

Measured here with synthetic sockets (2026-11-25, a Python probe, no product code):

- macOS (this Mac): on an AF_UNIX pair after one write of 1,029 bytes, the reading side's `FIONREAD` and `SO_NREAD` say 1029, then 1024
  after it reads 5, then 0. The writing side's `SO_NWRITE` stays 0, so **on macOS only the endpoint's side can see what it has not
  read**. Loopback TCP: the reading side's `FIONREAD` says 1029.
- Linux (the CI image `paddock-ci-local:v3-arm64`, OrbStack kernel 7.2.12, not a CI runner's kernel): the reading side's `SIOCINQ`
  says 1029, then 1024. The writer's `SIOCOUTQ` is non-zero while the peer has unread bytes, but counts buffer memory (3072, then 2304),
  not bytes. A close with 1,024 bytes unread makes the other side's read fail with `ECONNRESET`; a close with nothing unread ends in
  order after the reason.
- Windows: not measured.

The common pattern is: **order the steps with a barrier or a script, and check the state only where the tool can see it.** Here the
endpoint's own handle is the only place that sees unread input on every OS, and a test-only gate reaches it without a product change.

## Options

| | Option | Cost | What it leaves |
|---|---|---|---|
| A | **Cheapest.** Fix each instance when it shows up, as 8f14beeea did (an order fix and a regression test) | about 0.5 agent-day per instance; 8425 next | the next hidden state is found by CI as a stale or new ratchet entry |
| B | **Common practice.** Declared preconditions, established by barriers (one write, a test-only act gate, the clock's pending wait), checked only where the rig sees it (its own steps) | about 2.75 agent-days | "input queued when refused" is established but not verified |
| C | **Chosen (D1).** B, plus the act gate measures the endpoint's own handle (`queued_bytes`) before it lets the endpoint act | about 3.75 agent-days (1.0 beyond B) | nothing in this class among the scenarios that apply |
| D | A simulated transport (turmoil-like) instead of real sockets | rejected | the harness exists to see the real OS ending (resets) below the library (ADR 0072 §5); a simulation hides it |

## Decision (option C)

1. **Preconditions are declared per rig and scenario, with the scenario's default as the fallback.** In
   `crates/paddock-ipc/src/testing/endings/mod.rs`:

   ```rust
   pub enum Precondition {
       /// The client's bytes after its first frame (for busy: its first frame) are in the endpoint's receive queue
       /// before the endpoint reads or refuses.
       InputQueuedWhenRefused,
       /// The owner's send waits on the clock (the peer reads nothing) before the owner closes.
       PeerNotReadingAtClose,
       /// The peer reads again before the clock passes the reason deadline (the 8174 order; mini).
       PeerReadsBeforeReasonDeadline,
   }
   impl Scenario { pub fn default_preconditions(self) -> &'static [Precondition] }
   ```

   Defaults: `WrongAccount`, `Busy`, `ClientKeepsWriting` -> `InputQueuedWhenRefused`; `OwnerCloseWhilePeerNotReading` ->
   `PeerNotReadingAtClose` (mini's rig adds `PeerReadsBeforeReasonDeadline`); the others none. A rig narrows a default only through a
   `[[precondition_exempt]]` entry in `docs/ipc-endpoints.toml` (endpoint, scenario, precondition, reason), read by the harness's
   `applicability` loader and by C102; the list only shrinks (ER-46). The entries at the start: dyno-helper busy
   (`InputQueuedWhenRefused`: its client writes nothing, I16) and pitcrew-service busy (`InputQueuedWhenRefused`: not applicable until
   ADR 0072 WP-11, I26). `Expect.preconditions` replaces `Expect::unread_input`, removing 5 `with_unread_input()` calls and 3
   `unread_input: false` literals.

2. **The harness establishes each before the endpoint acts.**
   - Input queued: the first frame and the first junk go in one write (`raw::send_then_keep_writing`, from `fix/ipc-endings-i10`).
     The endpoint holds at a test-only **act gate** after `accept` and before it reads or refuses (item 4).
   - Peer not reading: the existing barrier is the setup. For ipc-host, `OwnerCloseWhilePeerNotReading` gets the `PeerOptions` clock and
     the small `write_queue` that `Stall` uses, and the rig pushes until `clock.wait_for_pending(1, ..)` returns before it closes the
     owner. Mini's peer is the rig's own sink (`limit_peer`), so it is set up by construction.
   - The 8174 order is its own barrier: mini's rig makes the peer read (`limit_peer(0, usize::MAX)`) and waits until the sink has taken
     bytes again before it advances the clock past the reason deadline. Barriers, not log order. `mini::run_with` stays: it is the
     positive control's hook that runs the steps in the wrong order.

3. **The harness checks that it held.**
   - Input queued: inside the act gate, the rig calls `queued_bytes` on the endpoint's own accepted handle and waits, with a bound
     (`READ_GUARD`), until n exceeds what must be there (busy: n > 0; client-keeps-writing: n > the first frame's length), then opens the
     gate. The probe is one test-only helper, `paddock_ipc::testing::endings::queued_bytes` (rule 55): `FIONREAD` on Unix sockets (Linux
     `SIOCINQ` is the same request), `PeekNamedPipe` `lpTotalBytesAvail` on a pipe handle on Windows, `ioctlsocket(FIONREAD)` for loopback
     TCP on Windows (mcp). The gate runs before the endpoint reads, so the endpoint has no read buffer yet: the evidence is the OS queue
     alone, printed as `os_queue=<n>`; anything that later logs a queue size keeps the OS part and a buffer part apart. Held: the bound
     was met. Missed: the bound passed (the gate then opens, so the scenario ends). A failed query is `not measured`.
   - Peer not reading: `clock.wait_for_pending(1, ..)` returned true **before** the close (ipc-host). A wait after the close is the
     endpoint's flush cap and stays the endpoint's finding (mini's `FlushWithoutCap` is still reported as `hang`, test 7). Mini: the
     sink answered `WouldBlock` at least once before `close_owner`.
   - The 8174 order: the sink's count of bytes taken after `limit_peer(.., usize::MAX)` is above zero before the advance.

4. **The act gate is test-only.**
   - `paddock-ipc`: a field and setter under `#[cfg(any(test, feature = "testing"))]` on `Host` and `link::ServerConfig`, typed
     `Arc<dyn Fn(&Stream) + Send + Sync>`, called by `Authenticator` after `accept` (covers ipc-host, ipc-link-server, dyno-agent).
   - `paddock-mcp` and `pitcrew-service`: a new `testing` feature that only their integration tests enable (`required-features` on the
     ending test target). mcp calls the gate on its accept thread. pitcrew-service runs on tokio: its accept task calls the gate through
     `tokio::task::spawn_blocking` with the stream's raw handle and awaits it, so no runtime worker blocks; the task keeps the stream
     until the gate returns.
   - dyno-helper needs no gate: none of its scenarios that apply has an input precondition (busy is exempt above). Its in-crate rig
     would use plain `#[cfg(test)]` if one is added later.
   - The Winsock probe needs `windows-sys`'s `Win32_Networking_WinSock`, enabled only by `paddock-ipc`'s `testing` feature
     (`testing = ["windows-sys?/Win32_Networking_WinSock"]`).
   - **Release check:** no existing check proves a `testing` feature is off in a release build (`./dev locks guard` watches only
     `serde_json`, from `cargo metadata`, which includes dev-dependencies). WP-2 adds one: for each release binary that links
     `paddock-ipc`, `paddock-mcp` or `pitcrew-service`, `cargo tree -e normal,features -i <crate>` must not list their `testing`
     feature; it also catches unification through the workspace hack. It starts report-only like any new check (rule 5).

5. **The judge and the ratchet report it apart.** `Outcome::RigFailure` takes a typed cause: `RigFailure::Drive(String)` (today's) or
   `RigFailure::Precondition { which, evidence }`. The summary reads `ipc-host client-keeps-writing: RIG FAILURE (precondition
   input-queued-when-refused not met: os_queue=10 after 20 s)`. `Ratchet::check` prints `rig failure: ... precondition ... not met; the
   scenario's faults were not compared with the ratchet (no entry is new or stale)` and skips that scenario's entries in the stale
   check. `judge` folds `NoReason` into `Reset` exactly when `InputQueuedWhenRefused` is declared for that rig and scenario and its
   evidence is held; when it is exempt, missed or not measured, `NoReason` stays.

6. **The whole precondition verdict starts report-only.** Missed and not measured both print a warning line, and the scenario is
   judged and ratcheted as today, under the id `test:endings-preconditions` in `docs/gates.toml` `[[report_only]]`: owner `ADR 0122`,
   `since` the day it lands on main, `review_by` at most 14 days later (rule 5), `approved = "user"`. The register has no run-id field
   (`tools/paddock-dev/src/gates/register.rs`, `Stamp`: reason, owner, since, review_by, approved), so the first green run id goes in
   `reason`, as the C128 and C138 entries do. The flip criterion: a complete green CI run on Linux, macOS and Windows in which every
   declared, not exempt precondition was held; pitcrew-service busy is outside it through its exemption. **The flip to blocking is its
   own later step** (WP-5): remove the entry, and a miss or not measured becomes a `RigFailure`.

## What it removes

- The false findings of the class: "stale ratchet entry" and "new failure" lines that were really the rig (9001), and the unnamed
  `RIG FAILURE (write)` (8425).
- `Expect::unread_input`, `Expect::with_unread_input`, its 5 calls and the 3 `unread_input: false` literals.
- Per-incident lessons for each new ordering race in these rigs.
- Adds: the enum, one test-only probe helper, one test-only gate (3 endpoint families), the `[[precondition_exempt]]` table, the
  release-feature check. No product code path or log line changes.

## Work packages

Prerequisite: `fix/ipc-endings-i10` (8f14beeea) lands first (it holds `send_then_keep_writing`; not on origin/main today).

| WP | What | Agent-days |
|---|---|---|
| 1 | `Precondition`, `Scenario::default_preconditions`, `[[precondition_exempt]]` (the `applicability` loader and C102), `Expect.preconditions`, typed `RigFailure`, the judge's fold, `run_all`, `Ratchet::check`; remove `unread_input`; tests 1, 5 | 1.0 |
| 2 | First the probe self-test on all three OS (test 4; Windows pipe and TCP first, on the Windows runner), then `queued_bytes`; the test-only act gate in `paddock-ipc`, mcp and pitcrew-service (tokio via `spawn_blocking`); the `testing` features; the release-feature check; busy and client-keeps-writing rigs use the gate (fixes 8425); test 2 | 1.5 |
| 3 | ipc-host `OwnerCloseWhilePeerNotReading` on the clock with a small `write_queue`; mini's sink evidence and the 8174 barrier; `run_with` kept as the control hook; tests 3, 6, 7 | 0.5 |
| 4 | Move the 8 rigs to declared preconditions with the two exemptions; the `[[report_only]]` entry | 0.5 |
| 5 | After the first green run on three OS: record its id, remove the entry, misses and not measured become rig failures | 0.25 |

Option C (chosen): WPs 1-5, about 3.75 agent-days, down from 4.5 in the first draft (no end-line field, no production WinSock feature,
no peer-buffer probe). Option B, the common-practice baseline, is the same without the probe and the gate's measurement: about 2.75
agent-days. C costs 1.0 agent-day beyond B.

## Tests (each starts red; ER-11, ER-33)

1. **Positive control, input queued.** A rig against ipc-host that writes the hello alone and the junk only after the endpoint's end
   line. Before the flip: a warning line naming `input-queued-when-refused`. After it: `RIG FAILURE (precondition ... not met ...)` and
   no `stale ratchet entry` line. Red today: on Linux it says "stale ratchet entry I10".
2. **Positive control, busy.** A link-server rig whose client writes nothing before the gate's bound: precondition not met on every OS,
   never `RIG FAILURE (write)` or a ratchet line. With the hello written: held, 100 of 100 per OS.
3. **Positive control, the 8174 order.** Mini through `run_with`, with the deadline advanced before the peer reads again: precondition
   `peer-reads-before-reason-deadline` not met, not `drain-not-eof`.
4. **Probe self-test** (first in WP-2). `queued_bytes` on a socket pair, a loopback TCP pair and a named pipe with n bytes written
   reads n (Windows TCP: at least 1), after reading k reads n - k (Windows TCP: still above 0 while bytes remain), and an invalid
   handle gives `None` (`not measured`); on Linux, macOS and Windows.
5. **Not measured never passes.** A gate whose probe returns `None`: `not measured`, never held; a warning before the flip, a rig
   failure after it.
6. **Positive control, peer reading.** An ipc-host rig whose peer drains before the owner closes: the send never waits on the clock
   before the close, so `peer-not-reading-at-close` is not met.
7. **The endpoint faults stay visible.** The existing mini broken-endpoint tests (`MiniFault`) still find each fault; `FlushWithoutCap`
   is still `hang`, not a rig failure (`crates/paddock-ipc/tests/it/endings.rs`).

## Decisions for the user

None open. D1 and D2 were taken on 2026-11-25 (Status). Option C is 1.0 agent-day beyond the common-practice option B, within ER-37's
1-day limit; the user chose it with both costs shown in the first draft (4.5 against 3); the revised costs are 3.75 against 2.75.

## Consequences

- After the flip, a rig change that breaks the order is red with its cause named, and no ratchet edit can hide it.
- The gate is a test seam in three endpoint families; a release build cannot carry it (the release-feature check).
- dyno-helper's "places taken" stays an open risk until it is reproduced.
- Rollback: drop the probe from the gate (option B); WPs 1, 3 and 4 stand alone.
