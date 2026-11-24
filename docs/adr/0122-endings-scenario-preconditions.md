# 0122. Every endings scenario declares its precondition, and the harness establishes and checks it

- Status: Proposed 2026-11-25. The user decided on 2026-11-25 to redesign now (ER-21); the options and costs below wait for the user's
  choice (ER-37, rule 76).
- Date: 2026-11-25
- Related: ADR [0072](0072-every-connection-end-says-why-in-order.md) §5 (the ending harness and its ratchet; this ADR amends it),
  ER-12, ER-21, ER-28, ER-33 (a check counts only if its precondition was verified), ER-37, ER-41 and rule 5 (report-only first),
  ER-44, rule 55 (one implementation per mechanism).
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

Every scenario and rig with such a state, on origin/main plus `fix/ipc-endings-i10` (8 rigs: ipc-host, ipc-link-server,
ipc-launcher, mini, dyno-agent, dyno-helper, mcp-loopback, pitcrew-service):

| Precondition | Rig / scenario | Status |
|---|---|---|
| input queued when refused | ipc-host wrong-account, busy (`account_scenario` writes before `accept_raw`) | fine by order, not verified |
| | ipc-host, ipc-link-server, dyno-agent client-keeps-writing (`ipc::black_box`) | was broken (9001); fixed on `fix/ipc-endings-i10` |
| | mcp-loopback, pitcrew-service client-keeps-writing | fixed on `fix/ipc-endings-i10` (same helper) |
| | mini client-keeps-writing (writes, then `accept_exactly`) | fine by order, not verified |
| | ipc-link-server busy | broken (8425, inferred) |
| | mcp-loopback busy, pitcrew-service busy (connect, then write) | at risk if the endpoint refuses before it reads (the 8425 order); not checked |
| peer not reading at the close | mini owner-close-while-peer-not-reading | was broken (8174); fixed |
| | ipc-host owner-close-while-peer-not-reading (32 pushes of 256 KiB, "the buffers fill") | at risk: assumed, never checked |
| | ipc-host stall (waits for the clock's pending send) | fine, checked by the clock |
| every place taken before the next client | dyno-helper busy (`Stream::connect` x16 returns at the backlog, before the server counts them) | at risk: not reproduced |
| | ipc-link-server, mcp-loopback, pitcrew-service busy (held clients finish their handshake first) | fine by order |

## Common practice

- **A barrier or hook before the act.** Go's `net/http` exports test hooks that order internal events in its own tests
  (`testHookReadLoopBeforeNextRead`, `SetTestHookServerServe`, `SetEnterRoundTripHook`:
  <https://raw.githubusercontent.com/golang/go/master/src/net/http/export_test.go>, read 2026-11-25). turmoil, a Rust network
  simulator, runs hosts on one thread and offers `hold` and `release` of in-flight messages and `partition` and `repair`
  (<https://docs.rs/turmoil/latest/turmoil/>, read 2026-11-25).
- **A script that fixes the order and checks socket state.** Google's packetdrill runs timestamped script lines in order, injects
  packets and checks what the stack does (<https://github.com/google/packetdrill>, read 2026-11-25).
- **An unmet precondition is not a product failure.** JUnit 5 throws `TestAbortedException` when an assumption fails: the test is
  aborted, not failed (<https://docs.junit.org/current/writing-tests/assumptions.html>, read 2026-11-25). Here it stays red (ER-33: a
  check that did not see its case does not pass), but it is reported apart from the endpoint's faults.
- **The OS can tell how many bytes are unread.** Linux `unix(7)`: `SIOCINQ` "returns the number of unread bytes in the receive buffer"
  (<https://man7.org/linux/man-pages/man7/unix.7.html>); macOS `getsockopt(2)`: `SO_NREAD`, and `FIONREAD`; Windows `PeekNamedPipe`
  returns `lpTotalBytesAvail` for a named pipe handle, server or client
  (<https://learn.microsoft.com/en-us/windows/win32/api/namedpipeapi/nf-namedpipeapi-peeknamedpipe>); Winsock `ioctlsocket`
  (<https://learn.microsoft.com/en-us/windows/win32/api/winsock/nf-winsock-ioctlsocket>; the `FIONREAD` meaning is on the linked
  Winsock IOCTLs page, **not read**).
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

The common pattern is: **order the steps with a barrier or a script, and check the state only where the tool can see it.** Checking
the OS queue at the endpoint is what packetdrill does; ordinary test suites stop at the barrier.

## Options

| | Option | Cost | What it leaves |
|---|---|---|---|
| A | **Cheapest.** Fix each instance when it shows up, as 8f14beeea did (an order fix and a regression test) | about 0.5 agent-day per instance; 8425 next | the next hidden state is found by CI as a stale or new ratchet entry |
| B | **Common practice.** Declared preconditions, the harness establishes each with a barrier (one write, a gate before the endpoint acts, a fixed step order) and checks only what the rig itself can see (its own steps, its own socket) | about 3 agent-days | "input queued when refused" is established but not verified (no product change) |
| C | **Recommended.** B, plus the endpoint reports how many received bytes it had not read when it refused (`queued=` in its end line) and the rig checks the peer's buffer is full at the close | about 4.5 agent-days (1.5 beyond B) | nothing in this class; pitcrew-service has no end line until ADR 0072 WP-7, so its "queued" check is `not measured` until then |
| D | A simulated transport (turmoil-like) instead of real sockets | rejected | the harness exists to see the real OS ending (resets) below the library (ADR 0072 §5); a simulation hides it |

## Decision (option C)

1. **The precondition is part of the scenario.** In `crates/paddock-ipc/src/testing/endings/mod.rs`:

   ```rust
   pub enum Precondition {
       /// The client's bytes after its first frame are in the endpoint's receive queue when it refuses.
       InputQueuedWhenRefused,
       /// The peer reads nothing and its receive buffer is full when the owner closes.
       PeerNotReadingAtClose,
       /// Every place of the endpoint is taken before the refused client connects.
       PlacesTaken,
   }
   impl Scenario { pub fn preconditions(self) -> &'static [Precondition] }
   ```

   `WrongAccount`, `ClientKeepsWriting` -> `InputQueuedWhenRefused`; `Busy` -> `PlacesTaken`, `InputQueuedWhenRefused`;
   `OwnerCloseWhilePeerNotReading`, `Stall` -> `PeerNotReadingAtClose`; the others none. It is per scenario, not per rig, so every
   rig of a scenario must meet it. It replaces `Expect::unread_input` and `with_unread_input` (6 call sites).

2. **The harness establishes it before the endpoint acts.**
   - Input queued: the first frame and the first junk go in one write (`raw::send_then_keep_writing`, from `fix/ipc-endings-i10`).
     For an endpoint that acts on `accept` without reading (busy), the rig holds an **act gate**: an injected port, like the clock,
     `Option<Arc<dyn Fn()>>` in the server's config, called after `accept` and before the endpoint refuses or reads. The rig opens it
     after its write returned. Sites: `paddock-ipc`'s `Authenticator` (covers ipc-host, ipc-link-server, dyno-agent), mcp-loopback's
     accept, pitcrew-service's host, dyno-helper's server. Default `None`; no cost in production.
   - Peer not reading: the rig does not read until a named step, and logs its own steps (`rig step=<name>`) into the same
     `LogCapture` as the endpoint, so the order is on record. The 8174 order (peer reads, then the clock passes the deadline) becomes
     a declared step order checked from the capture; `mini::run_with`'s hook folds into it.
   - Places taken: held clients finish their handshake (or, for dyno-helper, the rig waits for the server's session count from its
     log) before the refused client connects.

3. **The harness checks that it held, per OS.**
   - Input queued: the endpoint logs `queued=<n>` in its end line: bytes received but not consumed at the moment it decides to refuse
     (before it writes the reason; measured there, not at the close, because a right endpoint drains after refusing, ADR 0072 §2).
     n = the OS queue plus the endpoint's own read buffer (mcp reads through a `BufReader`). The OS part is one helper,
     `paddock_ipc::endpoint::queued_bytes` (rule 55): `FIONREAD` on Unix sockets (Linux `SIOCINQ` is the same request), `PeekNamedPipe`
     `lpTotalBytesAvail` on the server's pipe handle on Windows, `ioctlsocket(FIONREAD)` for loopback TCP on Windows (mcp; needs the
     `Win32_Networking_WinSock` feature of `windows-sys`). A failed query logs `queued=?`. Held: n > 0. Missed: n = 0.
   - Peer not reading: the rig asks its own peer socket how much is waiting (the same helper on the rig's side) before it lets the
     owner close; held when it is at least what the rig measured as the buffer's capacity in that scenario (the buffer is full, so the
     endpoint's writer is blocked). Plus the step order of item 2.
   - Places taken: the held clients' handshakes returned, or the endpoint's log shows the count.
   - Unknown (no end line, `queued=?`, a probe that failed): `not measured`, never held (ER-33).

4. **The judge and the ratchet report it apart.** `Outcome::RigFailure` takes a typed cause:
   `RigFailure::Drive(String)` (today's) or `RigFailure::Precondition { which, evidence }`. The summary reads
   `ipc-host client-keeps-writing: RIG FAILURE (precondition input-queued-when-refused not met: end line queued=0)`. `Ratchet::check`
   prints `rig failure: ... precondition ... not met; the scenario's faults were not compared with the ratchet (no entry is new or
   stale)` and skips that scenario's entries in the stale check. A missed precondition was already red (as a stale or new entry), so
   this blocks nothing new. `not measured` is printed as a warning in a report-only phase (rule 5: a `[[report_only]]` entry in
   `docs/gates.toml`, `review_by` at most 14 days), and becomes a rig failure after the first green run on Linux, macOS and Windows.

## What it removes

- The false findings of the class: "stale ratchet entry" and "new failure" lines that were really the rig (9001), and the unnamed
  `RIG FAILURE (write)` (8425).
- `Expect::unread_input`, `Expect::with_unread_input` and its 6 call sites; `mini::run_with`'s one-off hook (it becomes the step order).
- Per-incident lessons for each new ordering race in these rigs.
- Adds: the enum, one probe helper, one end-line field, one act gate per endpoint family (4 sites), the step lines.

## Work packages

Prerequisite: `fix/ipc-endings-i10` (8f14beeea) lands first (it holds `send_then_keep_writing`; not on origin/main today).

| WP | What | Agent-days | Option |
|---|---|---|---|
| 1 | `Precondition`, `Scenario::preconditions`, typed `RigFailure`, judge, `run_all`, `Ratchet::check` and summaries; remove `unread_input`; tests 1, 5 | 1.0 | B, C |
| 2 | Act gate in `Authenticator`, mcp-loopback, pitcrew-service, dyno-helper; busy rigs use it (fixes 8425); test 2 | 1.0 | B, C |
| 3 | Rig step lines and the declared step order; fold `mini::run_with`; dyno-helper waits for its session count; test 3 | 0.5 | B, C |
| 4 | Migrate the 8 rigs to declared preconditions; the `[[report_only]]` entry; flip after the first green run on three OS | 0.5 | B, C |
| 5 | `endpoint::queued_bytes` (Unix, Windows pipe, Windows TCP) and the end line's `queued=`, set at the refusal in `paddock-ipc`, mcp-loopback, dyno-helper; probe self-test per OS; test 4 | 1.0 | C |
| 6 | The peer-side buffer check for `PeerNotReadingAtClose` (mini, ipc-host); test 6 | 0.5 | C |

Option B: WPs 1-4, about 3 agent-days. Option C: WPs 1-6, about 4.5 agent-days. Windows legs run on the Windows CI runner.

## Tests (each starts red; ER-11, ER-33)

1. **Positive control, input queued.** A broken rig that writes the junk only after the endpoint's end line (the gate of 8f14beeea)
   against ipc-host: the report is `RIG FAILURE (precondition input-queued-when-refused not met ...)`, and `Ratchet::check`'s text has
   no `stale ratchet entry`. Red today: on Linux it says "stale ratchet entry I10".
2. **Positive control, busy.** A link-server rig that opens the act gate before it writes the hello: precondition not met on every
   OS, never `RIG FAILURE (write)` or a ratchet line. With the gate used right: 100 of 100 per OS.
3. **Positive control, step order.** The mini rig that advances the deadline before the peer reads again (today's `run_with` slow
   path): rig failure naming the step order, not `drain-not-eof`.
4. **Probe self-test.** `queued_bytes` on a socket pair or pipe with n bytes written reads n, after reading k reads n - k, and a closed
   or invalid handle gives `None` (so `not measured`), on Linux, macOS and Windows.
5. **Not measured never passes.** A rig whose endpoint logs `queued=?`: the outcome is `not measured`, never a pass; in the report-only
   phase it is a warning line, after the flip a rig failure.
6. **Positive control, peer reading.** An ipc-host rig whose peer drains before the owner closes: precondition
   `peer-not-reading-at-close` not met.
7. **The endpoint faults stay visible.** The existing mini broken-endpoint tests (`MiniFault`) still find each fault: a precondition
   never hides an endpoint fault (`crates/paddock-ipc/tests/it/endings.rs`).

## Decisions for the user

- **D1 (ER-37, more than 1 agent-day beyond common practice):** (a) option C, 4.5 agent-days, recommended: the user's decision asks
  that the harness checks the precondition held, and only the endpoint side can see unread input on macOS (measured above); (b) option
  B, 3 agent-days: established by barriers, checked only where the rig can see; (c) option A, about 0.5 agent-day per instance.
- **D2:** the act gate (an injected hook in 4 endpoint families) for busy, recommended; or relax busy: a failed hello write and either
  ending are accepted, and the busy `reset` entries (I9) become OS-and-race dependent.

## Consequences

- A rig change that breaks the order is red with its cause named, and no ratchet edit can hide it.
- The end line gains a field that also explains a reset in production logs (ER-27); it holds a count, no content.
- Rollback: revert WPs 5-6 to fall back to option B; WPs 1-4 stand alone.
