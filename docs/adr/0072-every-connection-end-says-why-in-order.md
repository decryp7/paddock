# 72. Every connection end says why, in order: one ordered shutdown for `paddock-ipc` and the MCP transport

- Status: Proposed (2026-11-09)
- Date: 2026-11-09
- Related: ER-21 / rule 40 (recurring bugs mean a missing design rule), ER-27 (logs explain every failure), ER-28 (a cause
  is confirmed only by measurement), rule 5 / ER-11 (failing test first), rule 53 (reusable OS names), rule 54 (release
  path builds installers), rule 55 and ADR 0034 (one implementation per mechanism), ADR 0001 (the IPC: private endpoint,
  token, peer-pid handshake), ADR 0007 (MCP servers, the loopback key line), ADR 0041 (deterministic timing: the injected
  `Clock`), ADR 0049 §4 (wire versions derived from the schema), ADR 0023 (tests never free a port they probe).
- Number: 0072 was free on every branch of `origin` on 2026-11-09 (family `docs/adr/` ends at 0062 across all branches;
  Debrief's own folder uses 0063 to 0070; nothing anywhere uses 0071 or 0072).

## Pattern

Three teardown bugs in one night (2026-11-08 to 2026-11-09), in three different places, one shape:

| # | Where | What the peer saw | Why | State |
| --- | --- | --- | --- | --- |
| 1 | `paddock-ipc` `Peer::close` (`crates/paddock-ipc/src/peer.rs:385`) | `Remote(cancelled)` instead of `Closed` (run 7761, Windows, 1 in 2,000 on the Windows 11 Arm VM) | `close` marked the connection closed, which cancels the served calls, and only then shut the outgoing queue; a handler that answered its cancel at once queued a "cancelled" reply that went out before the end of the stream. | Fixed by ordering on `fix/main-after-7761` (5528ea2b2): `close` now goes through `fail`, which shuts the queue first. |
| 2 | `paddock-mcp` loopback (`crates/paddock-mcp/src/loopback.rs`, `serve_loopback`) | "Connection reset by peer" (os error 54) after reading the refusal (run 7761, macOS) | A refused client's key line was left unread when its socket closed; a TCP socket closed with unread received data sends a reset. Drains shared the cap of the places (1), so a client that reconnected at once found no drain. | Fixed by a separate drain pool of 16 on `fix/main-after-7761` (29990de5f). |
| 3 | Dyno agent (`apps/dyno/crates/dyno-agent/src/server.rs`), through `paddock-ipc` `handshake.rs` | "the host closed the connection during the handshake" (run 7763, Linux container; not reproduced on macOS) | Open. Candidates (lessons.md on `origin/train/2026-11-08-1`, entry "Dyno helper_seam ... cause open"): the handshake watchdog could not start and the stream was dropped without an answer (`handshake.rs:100-112`); the read of the `Hello` failed and the host returned without a `Fail`; the 10 s watchdog fired; a rig artefact. This ADR adds one more found while reading: a listener dropped while a client waits in its backlog. | Under investigation on `fix/dyno-agent-handshake-close`. |

Earlier instances of the same shape are in `docs/lessons.md` (origin/main): line 430 (Dyno's helper "drops the client unread
and the client sees a reset"), line 844 (a fake server dropping a client mid-handshake, Windows os error 10053, fixed in one
copy only), and the 2026-11-07 MCP field case quoted in `loopback.rs` ("the reset can reach the client before the
refusal").

**The pattern:** a connection ends without the peer learning why, or in the wrong order, whenever one side refuses, closes,
times out or stops. Each code path that ends a connection decides on its own whether to say why, in which order to stop
the outgoing queue, cancel work and close the socket, and whether to read what the peer already sent; most paths do not.

**The invariant that should hold:** every end of a connection that one side decides is announced on the same stream by
exactly one typed reason, which is the last thing that side writes; nothing produced by the teardown itself (cancel
answers, pushes) follows it; and the side then reads and discards the peer's input under a bound before it releases the
socket, so the reason arrives instead of a reset. A connection the side never accepted (still in the backlog when the
listener stops) is accepted and refused the same way, within a bound.

## Instances found (origin/main, 2026-11-09)

Every place that ends a connection in the two shared transports and their consumers. "Broken" means a path that ends a
connection with no reason, out of order, or with the peer's data unread; "at risk" means correct today only by an
unchecked ordering or a timing margin.

| # | File and place | Path | State |
| --- | --- | --- | --- |
| I1 | `paddock-ipc` `peer.rs:385` `Peer::close` | owner closes | Broken on main (order); fixed by 5528ea2b2 on a branch; still sends no reason (the peer reads only "x closed the connection"). |
| I2 | `peer.rs:474` `Peer::fail` (stall, protocol error, write failure, reader end) | local failure | No reason frame: the peer of a connection closed for a protocol error or a stall sees a bare end. |
| I3 | `peer.rs:917-942` `read_loop` end | remote end | Correct (nothing to announce), but the end is reported as an untyped `Closed(String)`: callers cannot tell "the peer said why" from "the stream broke". |
| I4 | `handshake.rs:94-120` `bounded` | watchdog fires | Broken: the watchdog closes the stream; the client reads "the host closed the connection during the handshake". |
| I5 | `handshake.rs:100` and `:112` (`stream.closer()?`, watchdog spawn failure) | resource failure | Broken: `?` returns and drops the stream with no answer (candidate cause of #3). |
| I6 | `handshake.rs:94-120` | watchdog fires after `Welcome` was written | Broken: the host returns `Timeout` and drops a connection the client believes is open; the client's first call fails with `Closed`. |
| I7 | `handshake.rs:265`, `:270`, `:294` (client left, first frame not a hello, hello not valid) and the `read_body` I/O error path | malformed or failed read | Broken: no `Fail` written. |
| I8 | `handshake.rs:210-217` `check_account` refusal | refuse before reading | At risk: the refusal is written, then the stream drops with the client's `Hello` unread (the reset-before-refusal shape of #2; whether AF_UNIX resets is measured in WP-0). |
| I9 | `handshake.rs:173` `refuse_busy` (used by `link/server.rs:185`) | busy | At risk: same as I8. |
| I10 | `handshake.rs` version and auth refusals | refuse after reading | Correct order today; no drain of anything sent after the `Hello`. |
| I11 | `Host` drop (`handshake.rs`, `Listener`) | listener stops | Broken: clients in the backlog are dropped unaccepted, with no answer. |
| I12 | `link/server.rs:202` (`Ok(peer) if stop => peer.close()`), `:247` stop | stop | No reason ("stopping"). |
| I13 | `launch.rs:201`, `:213` child close | owner closes | No reason (I1). |
| I14 | Dyno agent `server.rs:61-78` (inline handshake, `sleep(50 ms)` after any refusal), `:70` (`connect` failure after `Welcome`: the peer may be left running, unclosed), `:80` (stop) | refuse, internal failure, stop | Broken: the error path drops the stream; the 50 ms sleep delays every other client; a connect failure after `Welcome` gives the client an open connection that dies later. |
| I15 | Dyno helper `apps/dyno/crates/dyno-helper/src/server.rs:222` (caller not identified), `:235` ("too many connections, one was dropped"), `:250-251` (`SESSION_MAX` expiry) | refuse, busy, expiry | Broken: dropped with no answer; expiry closes with no reason. |
| I16 | Dyno app link `apps/dyno/app/src-tauri/src/link/driver.rs:299`, `:305`, `:344`, `:362` | owner closes | No reason (I1). |
| I17 | `paddock-mcp` `loopback.rs` `handshake` refusals (`key`, `handshake`) | refuse after the key line | Correct line and order; anything the client sent after its key line (a pipelined `initialize`) is left unread: reset risk. |
| I18 | `loopback.rs` busy refusal beyond `DRAIN_PLACES` (16) | busy under load | At risk: "beyond this bound a refused connection is closed at once", key line unread, reset. |
| I19 | `loopback.rs` `Serving::shutdown` (`shutdown(Shutdown::Both)` on paired connections) | stop | No reason after pairing: the bridge reads a bare end. |
| I20 | `paddock-mcp` `bridge.rs` | reads the end | Cannot say why the app ended a paired connection (it has only "It may be restarting"). |
| I21 | `apps/debrief/crates/debrief-ipc/src/peer.rs:231` (registered copy of `ipc-frames-peer`, delete by `debrief-v1.2.0`) | owner closes | Order correct (`Out::Shutdown` before `mark_closed`), no reason. |

21 instances: 14 broken, 4 at risk, 3 correct in order but without a reason or a drain. None is checked by a test that
enumerates endpoints.

## Decision

### 1. One closed vocabulary of reasons

`paddock_ipc::ending` (a new module; pure, no I/O) declares why a connection ends:

```text
enum Refusal   { Version { ours, theirs }, Auth(AuthFailure), Busy, Stopping,
                 Timeout { phase: Phase }, Malformed(ProtocolError), Internal(InternalCause) }
enum Closing   { ByOwner(OwnerReason /* Stopping | Replaced | SessionExpired | Done */),
                 Protocol(ProtocolError), Stalled { secs }, Internal(InternalCause) }
enum Reason    { Refused(Refusal), Closing(Closing) }
enum Side      { Local, Remote, Transport }          // Transport: the stream ended with no reason frame
struct Closure { side: Side, reason: Option<Reason>, detail: String }   // reason is None only for Transport
```

`Reason::code()` derives the wire code (`version`, `auth`, `busy`, `stopping`, `timeout`, `protocol`, `stalled`,
`internal`, `closed`); nobody passes a free string for a connection end (call failures keep their free `RemoteError`
codes). `IpcError::Closed(String)` becomes `IpcError::Closed(Closure)`: a caller can tell "the peer said why" from "the
stream broke" without parsing text. This is a breaking change of `paddock-ipc`'s public API with no shim (Paddock is before
v1; the 7 sites outside the crate that build or match it are migrated in WP-8).

### 2. One ordered shutdown, the same for every end

A connection is in exactly one state, held in one place and changed by one pure function
`advance(state, event) -> (state, actions)` (the ADR 0056 pattern):

```text
Handshaking ──welcome──▶ Open ──end(reason)──▶ Sealed ──writer flushed──▶ Draining ──eof | bytes cap | time cap──▶ Released
     │                                           ▲
     └──refuse(refusal)──────────────────────────┘
```

The actions of `end(reason)` are fixed in this order, and no other code path closes a connection:

1. **Seal.** The outgoing queue takes the reason frame as its last frame and refuses every later frame (`Push::Sealed`).
   `WriteQueue::shutdown()` without a reason is removed, so a close without a reason does not compile.
2. **Fail and cancel.** Pending calls fail with the `Closure`; served calls see their cancel. Their answers find the queue
   sealed (bug #1 cannot recur: the order is the transition's, and the model test checks that nothing is queued after the
   reason).
3. **Flush and half-close.** The writer sends everything up to the reason, then shuts its write half (`shutdown(Write)` on
   sockets; on Windows the pipe's write side ends at the writer's last write, and `DisconnectNamedPipe` is never called
   while the client may still read, because it discards unread data,
   [Microsoft Learn](https://learn.microsoft.com/en-us/windows/win32/api/namedpipeapi/nf-namedpipeapi-disconnectnamedpipe)).
4. **Drain.** The reader keeps reading and discards input into a fixed 8 KiB buffer until the peer's end, the drain's byte
   cap or its time cap on the injected clock, whichever comes first.
5. **Release.** The socket or pipe is closed; the outcome (`eof`, `bytesCap`, `timeCap`, `poolFull`) is logged (§6).

`Peer::close` returns at once (its caller never waits for a drain); `closed()` reports the `Closure` from step 2; a new
`Peer::wait_released(timeout)` waits for step 5 (tests and orderly stops). The peer's reader, which already exists, does
the drain: no new thread for an open connection.

On receipt, a `Fail { id: 0 }` after the handshake is the peer's reason: the reader records `Closure { side: Remote,
reason }`, fails pending calls with it and goes to `Draining` itself (the peer has said its last word).

### 3. The handshake: a verdict before the stream can be dropped

- **The accepted stream is a typestate.** `Host::accept*` and `LinkServer` hand out `Accepted` (owning the `Stream`), whose
  only exits are `welcome(self) -> Connection` and `refuse(self, Refusal)`. `Drop` of an `Accepted` without a verdict
  refuses with `Internal(DroppedWithoutVerdict)` and logs at error level, so every early return and `?` still answers
  (I5, I7). The ending harness (§5) fails on that log line, so the fallback is a safety net, not a design.
- **No watchdog thread.** The `Hello` is read in slices of the stream's own read timeout (both endpoints already support one:
  `endpoint/unix.rs` polls, `endpoint/windows.rs` passes `read_timeout` to the overlapped read), checking the
  `Deadline` on the injected clock between slices, as `paddock-mcp`'s `Watched` reader already does. A handshake that runs
  out is refused with `Timeout { phase: Hello }` by the thread that owns the stream (I4), no thread spawn can fail (I5), and
  the verdict is one value, so `Welcome` and a timeout cannot both happen (I6). The client side does the same for
  `Welcome`.
- **Refuse, then drain.** Every refusal follows §2's order: the refusal frame, half-close, drain under the refusal bounds,
  release (I8 to I10). A refusal before the `Hello` is read (account check, busy) drains the `Hello` too.
- **The listener stops in order.** `Host::stop` (and `Drop`) accepts what waits in the backlog without blocking, up to a
  bound, refuses each with `Stopping` and drains it in the drain pool; only then is the listener closed (I11). Clients
  beyond the bound are closed by the OS as today, counted and logged.

### 4. The MCP transport on the same core

`paddock-mcp`'s loopback uses `paddock_ipc::ending` (the reasons, the state machine, the drain and the drain pool; not the
frames): its refusal lines stay as they are (`{"ok":false,"code":...}`), each refusal drains under the refusal bounds, and a
paired connection that the app ends (stop, `McpControl` off) writes one JSON-RPC notification
`{"jsonrpc":"2.0","method":"notifications/paddock/closing","params":{"code":...,"message":...}}` as its last line before it
half-closes and drains (I17 to I19). MCP defines no shutdown message and leaves the end to the transport
([MCP lifecycle, Shutdown, 2025-06-18](https://modelcontextprotocol.io/specification/2025-06-18/basic/lifecycle)); a
JSON-RPC notification has no reply ([JSON-RPC 2.0, §4.1](https://www.jsonrpc.org/specification#notification); read, not
re-fetched today). The bridge (`bridge.rs`) consumes that line, never forwards it to the stdio host (a third-party client
must not see a Paddock-only method), and turns it into its `BridgeError` text (I20). Busy refusals beyond the drain pool
still write the refusal before closing; the pool and the places stay separate (29990de5f).

### 5. The check: an ending harness over every endpoint, with a positive control (C102, number assigned at landing)

- **A register that enumerates.** `docs/ipc-endpoints.toml` lists every place that listens or accepts in non-test code
  (one entry per endpoint: crate, constructor, ending test). The static part of the check (no build; it runs in the push
  check) finds every listening site in non-test Rust code (`Host::listen*`, `Listener::bind*`, `LinkServer::start`,
  `serve_loopback`, `TcpListener::bind`, a raw `Listener::accept`) and fails on a site that has no entry, or an entry whose
  test does not exist. A new endpoint is therefore covered by construction. Today it lists at least: the plugin host, the
  link server, the child launcher, the Dyno agent's app endpoint, the Dyno helper's endpoint, the Dyno plugin host, and the
  MCP loopback.
- **One harness, every path.** `paddock_ipc::testing::endings` (behind the crate's `testing` feature) drives an endpoint
  through: wrong token, wrong version, wrong account (Unix), busy, a silent client (handshake timeout on a `FakeClock`), a
  malformed first frame, a client that keeps writing after it is refused, an owner close with calls in flight whose handler
  answers its cancel at once, a stall, a listener stop with a client in the backlog, and a stop with an open connection. It
  asserts that the client reads the expected typed reason (never `Side::Transport`, never `Remote(cancelled)` after an owner
  close, never a reset), that the endpoint logs one end line with the same code (§6), and that no `DroppedWithoutVerdict`
  line appears. The ordering race of #1 is made deterministic with a test seam between "seal" and "cancel" (ADR 0041), not by
  repetition; the stress count is a second line of defence only.
- **The positive control.** The harness ships `testing::endings::Broken` endpoints, one per fault: drop without a reason,
  cancel before seal, release without drain, drop the backlog. A test asserts that the harness fails on each, with the
  scenario named. If a harness change stops seeing a fault, that test goes red (ER-33).
- **All three OS.** `paddock-ipc`'s and `paddock-mcp`'s tests already run on the Linux, macOS and Windows crate legs (run
  7761 ran them on Windows and macOS); each app's ending test runs on the OS legs its workflow has. The check must be red on
  today's main for I4, I5, I11, I14 and I15 (WP-0 records the red run).

### 6. Logs (ER-27)

Every connection end logs one line on each side, no content: connection id and fingerprint (as the Dyno agent's
`AgentRun`), side (`local`, `remote`, `transport`), phase (`hello`, `open`), reason code, the closure's detail, how long
the handshake and the open phase lasted, frames still queued at the seal, pending calls failed, served calls cancelled, and
for the drain its outcome (`eof`, `bytesCap`, `timeCap`, `poolFull`), bytes discarded and duration. Refusals log the cause
the peer was told and the cause for the log (never a token or a key, as `loopback.rs` does today). Each endpoint logs a
periodic summary of refusals by code (the Dyno helper's `ServedLine` pattern). A client logs the reason it received and the
next step its message names. Rigs that start a product print the product's log on failure (the lesson of run 7763).

### 7. Bounds (no new denial of service)

| Bound | Default | Why |
| --- | --- | --- |
| Drain after a refusal | 64 KiB or 2 s on the clock | A `Hello` or key line is under 1 KiB; a client that keeps writing is cut off. |
| Drain after an open connection ends | 1 MiB or 2 s | A peer in mid-call may have one frame in flight; beyond that it is not listening. |
| Drain pool per endpoint (connections with no thread of their own) | 16 | As 29990de5f; beyond it the refusal is still written, then the socket closes at once (counted, logged). |
| Backlog refused at stop | 64 | Bounded accept-and-refuse; the rest are closed by the OS. |
| Reason message | 512 bytes, truncated | Always fits `MIN_FRAME` (1 KiB). |
| Handshakes in progress | the existing `LinkServer` cap; the Dyno agent moves to it | One silent client never holds the accept loop (I14). |

The drain allocates nothing per byte (one fixed buffer), runs on a thread that already exists for open connections, and
on the bounded pool for refusals. The Dyno agent's `sleep(50 ms)` after any refusal goes: it delayed every client and
limited nothing. These are protocol bounds, set in `PeerOptions` and the `Host` builder with these defaults, not user
Settings (rule 21 is for tunables a user changes).

### 8. Wire and protocol versions (ADR 0049)

- **No frame layout change.** The reason is the existing `Frame::Fail { id: 0, error }`. Frame `VERSION` stays 2.
- **Old peers degrade to today's behaviour.** A reader of today's code ignores a `Fail` for an id it is not waiting for
  (`Peer::deliver`, `peer.rs:626-631`: "a late answer to a call that timed out or was cancelled"), and call ids start at 1
  (`peer.rs`, `next: AtomicU64::new(1)`), so id 0 is never pending: an old peer drops the reason and sees the end of the
  stream, as now. An old client in a handshake already maps a `Fail` with an unknown code to `IpcError::Remote`
  (`handshake.rs`, `connect_on`), which says more than "the host closed the connection". Verified by reading the code; the
  harness adds a mixed-version case (a reader built from the last tag's `peer.rs` behaviour, as a test double).
- **Derived wire versions do not move.** ADR 0049's schemas cover the bodies (`app-agent`, `agent-helper`, `host-plugin`
  roots), not `Frame`; no body type changes, so no derived version is bumped, no installed Dyno helper is refused, and no
  wire note is needed for them. The new codes are documented in `codes` and in `paddock-ipc`'s CHANGELOG.
- **The frame itself becomes a declared wire** (D1): `crates/wire.toml` declares `ipc-frame` (`roots = ["Frame"]`,
  constant `frame::VERSION`), so a later change of the frame (for example a dedicated `Close` frame) is versioned by C80
  instead of by memory.
- **MCP:** the refusal lines do not change; the closing notification is new and only our bridge reads it.

### 9. Consumers and migration

Consumers of `paddock-ipc` (by `Cargo.toml`): Dyno (`dyno-agent`, the app's `src-tauri`, `dyno-cli`, `dyno-helper`,
`dyno-helper-service`, `dyno-plugin-api`, `dyno-plugin-host`), `paddock-plugin-api`, `paddock-plugin-host`,
`paddock-plugin-package`, `paddock-plugin-test`, `paddock-view`. Consumers of `paddock-mcp`: `paddock-shell`, Dyno's
app and `dyno-cli` (the bridge), `shell-demo`. Debrief uses its registered copy `debrief-ipc` (delete by
`debrief-v1.2.0`); Pitwall, Pitcrew and Spotter use neither crate today.

Order (rule 43: the shared change and its adoption land together; rule 54: Dyno's release path, so its installers build
before landing):

1. WP-0 lands the harness and the register with the red scenarios marked expected-to-fail per instance (the check's
   ratchet lists them), so the class is checked from day one.
2. The core, the peer and the handshake (WP-1 to WP-4) land with the migration of every consumer's call sites (WP-8) in one
   change; each expected failure leaves the ratchet as its instance turns green.
3. MCP (WP-5) and the logs (WP-7) follow; then the ratchet is empty and stays empty.
4. Debrief's copy: waived (D5) with `Copies-waived: ipc-frames-peer apps/debrief/crates/debrief-ipc/src/peer.rs: ...`
   until Debrief 1.2 moves onto `paddock-ipc` (PH1 to PH4), or ported.

**Rollback:** each step is one merge; reverting the peer and handshake change restores today's behaviour on the wire (no
format changed), and the harness's ratchet is restored with it.

## Parallelism and cost

- **Threads.** Fewer than today: the per-handshake watchdog thread goes (one fewer thread per connection attempt). The drain
  of an open connection runs on its existing reader thread. Refusals that have no thread use the bounded pool (16 per
  endpoint). Nothing is CPU-bound; nothing runs at a starvable QoS (rule 52: closing is live work, a person waits on a
  reconnect).
- **Bytes.** One reason frame per end (under 600 bytes) and at most the drain caps read and discarded.
- **Latency.** `close` returns at once; release waits for the peer's end, normally within milliseconds, at most 2 s. A
  client that reconnects at once is not blocked by the previous connection's drain (separate pool, as 29990de5f).
- **Cheaper, not only parallel (rule 51).** The handshake becomes cheaper (no thread spawn and join per attempt); the
  Dyno agent's serial inline handshake with a 50 ms sleep after errors is replaced by the link server's threaded
  handshakes with a cap.
- **To measure and record (rule 48, `docs/benchmarks.md`):** close-to-release time (p50, p99) and connect-refuse churn per
  second on the three OS before and after, in the harness.

## Work packages (raw estimates, one person)

| WP | Content | Days |
| --- | --- | --- |
| WP-0 | Harness skeleton, register, static check with a ratchet; red runs on main on the three OS; measure what each OS does when a socket or pipe closes with unread data (AF_UNIX on Linux and macOS, named pipe `CloseHandle` on Windows: not yet verified) | 2 |
| WP-1 | `ending`: reasons, `Closure`, `advance`, bounds; exhaustive model test (every event in every state, the order of actions) | 2 |
| WP-2 | `WriteQueue::seal`, `Peer` end sequence, reader drain, receiving `Fail{0}`, `wait_released`, `IpcError::Closed(Closure)` | 2.5 |
| WP-3 | Handshake: `Accepted` typestate and its `Drop`, sliced reads on the clock (no watchdog), one verdict, refusal drains, client side | 3 |
| WP-4 | Listener stop with backlog refusal; drain pool; `LinkServer` on it | 1.5 |
| WP-5 | MCP loopback and bridge on the core: refusal drains, closing notification, stop | 2 |
| WP-6 | Full harness scenarios, mixed-version case, deterministic seal/cancel seam, `Broken` positive controls, 3-OS runs | 2.5 |
| WP-7 | ER-27 end lines, refusal summaries, rigs print the product log on failure | 1 |
| WP-8 | Consumers: Dyno agent (threaded capped handshakes, no sleep, close the peer when `connect` fails), helper (busy, unidentified, expiry reasons), app link, CLI, plugin host and test fake, shell-demo, the 7 `IpcError::Closed` sites outside the crate; each endpoint's ending test; Dyno installers built (rule 54) | 3 |
| WP-9 | Debrief copy: the waiver (0.5) or a port (2) | 0.5 |
| WP-10 | Proposed rule text, lessons entry, ADR index, CHANGELOGs | 0.5 |
| | **Total (with the waiver)** | **20.5** |

WP-0 can start at once and helps the open Dyno investigation (its rig prints the agent's log, and the backlog and
no-verdict scenarios test two of its candidates). WP-1 and WP-6's controls can run in parallel with WP-2/WP-3; WP-5 needs
WP-1; WP-8 needs WP-2 to WP-4.

## Decisions for the user

| # | Question | Options | Recommendation |
| --- | --- | --- | --- |
| D1 | How does the reason travel? | (a) The existing `Fail { id: 0 }`, no frame change, old peers degrade to today; declare the frame as wire `ipc-frame` under C80. (b) A new `Frame::Close`, frame `VERSION` 3, both sides must update together. | (a): no compatibility cost now; the declared frame wire makes (b) a versioned step later if wanted. |
| D2 | The handshake timer | (a) Sliced reads on the injected clock, no watchdog thread. (b) Keep the watchdog, but let it only signal and let the stream's owner answer. | (a): removes a thread, a spawn failure and the welcome/timeout race at once. |
| D3 | Where the shared core lives | (a) `paddock_ipc::ending`; `paddock-mcp` depends on `paddock-ipc`. (b) A new small crate `paddock-ending` both depend on. | (a): `paddock-mcp` already depends on `paddock-jobs` and `paddock-proc`, which `paddock-ipc` uses; one crate less. (b) if MCP must stay free of the frame code. |
| D4 | Bounds | The defaults in §7 (64 KiB / 2 s, 1 MiB / 2 s, pool 16, backlog 64, 512-byte messages). | Accept them as protocol defaults in `PeerOptions`/`Host`, not Settings. |
| D5 | Debrief's copy (rule 55) | (a) Waive until `debrief-v1.2.0` (its close order is already right; it is replaced in Debrief 1.2). (b) Port the reason and drain now (+1.5 days). | (a). |
| D6 | MCP after pairing | (a) The `notifications/paddock/closing` line, consumed by the bridge. (b) Nothing; stay with the spec's bare transport end. | (a): the bridge can then say why; third parties never see it. |
| D7 | A binding rule | Propose a Paddock rule (number from the coordinator) and an ER candidate: "every end of a connection says why, last, after the outgoing queue is sealed and before cancellations are answered, and drains the peer's input under a bound; every endpoint is in the register and passes the ending harness". | Yes, with the check (C102 or the next free number). |
| D8 | When | (a) WP-0 now, the rest after the current foundation items and before the next Dyno line lands. (b) All after Debrief 1.2's foundation. | (a): the class keeps producing red CI runs, and WP-0 helps the open Dyno investigation. |

## Consequences

- Every refusal, close, timeout and stop of both transports tells the peer why, in order, and the peer can show it; a bare
  end of stream (`Side::Transport`) then means a crash or a broken link, which is what it should mean.
- `IpcError::Closed` changes shape for every consumer (one migration, no shim, before v1).
- Closing costs at most one small frame and a bounded drain; handshakes cost one thread less.
- The class is checked: a new endpoint cannot land without an entry and an ending test, and the harness proves it can see
  each fault.

## Alternatives rejected

- **Fix each instance where it is found** (what the three fixes did): each was right, none stopped the next one; 14 broken
  instances remain.
- **`SO_LINGER` with a timeout or a zero linger:** a zero linger turns every close into a reset (the opposite of the goal);
  a linger timeout blocks the closing thread and does not exist for named pipes
  ([Microsoft Learn, graceful shutdown](https://learn.microsoft.com/en-us/windows/win32/winsock/graceful-shutdown-linger-options-and-socket-closure-2)
  recommends an explicit `shutdown` and reading until the peer's end instead, which is §2).
- **Retry on the client after a bare end:** hides the cause, and retried refusals load the server; the client should know
  the reason and act on it.
- **A watchdog that writes the timeout refusal itself:** two writers on one stream can interleave a `Welcome` and a
  refusal; one owner of the stream is simpler.
