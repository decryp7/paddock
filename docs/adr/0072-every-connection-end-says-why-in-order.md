# 72. Every connection end says why, in order: one ordered shutdown for the family's transports

- Status: Accepted (2026-11-09). Accepted 2026-11-09 by the user: D1–D9 as recommended (D3 = small pure crate, or decide
  after WP-1 as the ADR states). Revision 2 answered the first independent review (P1-1 flush policy, P1-2 typestates,
  P1-3 register check and Pitcrew); revision 3 the re-review's four P2s (listen calls, Pitcrew's busy handling,
  client-side drains, the enforced D8 gate).
- Trimmed 2026-11-11 (user, comparable-practice review): DEFERRED, keeping the logging part: keep WP-7 (ER-27 end lines and summaries) and Pitcrew's busy refusal (WP-11) only if the bug recurs; deferred WP-1 to WP-6, WP-8 to WP-10 and the rest of WP-11. Dyno's tag gate is now only the kept logging part (WP-7); M2 to M4 no longer gate any tag.
- Date: 2026-11-09
- Related: ER-21 / rule 40 (recurring bugs mean a missing design rule), ER-27 (logs explain every failure), ER-28 (a cause
  is confirmed only by measurement), ER-33 / rule 71 (a check counts only if its probe can see the thing), rule 5 / ER-11
  (failing test first), rule 43 (a shared change and its adoption land together), rule 53 (reusable OS names), rule 54
  (release path builds installers), rule 55 and ADR 0034 (one implementation per mechanism), ADR 0007 (MCP servers, the
  loopback key line), ADR 0023 (tests never free a port they probe), ADR 0041 (deterministic timing: the injected `Clock`),
  ADR 0049 (small merges; §4 wire versions derived from the schema), Pitcrew ADR 0004 (its app-service transport).
- Amends: ADR 0001 and [ipc-wire.md](../ipc-wire.md) §5 and §9 (the handshake's watchdog becomes sliced reads; a
  `Fail { id: 0 }` after the handshake is the closing reason). It does not change ADR 0001's frame version 3 plan
  (capabilities in the handshake, package 2.6d); §8 says how the two meet.
- Number: 0072 was free on every branch of `origin` on 2026-11-09 (family `docs/adr/` ends at 0062 across all branches;
  Debrief's own folder uses 0063 to 0070; nothing anywhere uses 0071 or 0072).

## Pattern

Three teardown bugs in one night (2026-11-08 to 2026-11-09), in three different places, one shape:

| # | Where | What the peer saw | Why | State |
| --- | --- | --- | --- | --- |
| 1 | `paddock-ipc` `Peer::close` (`crates/paddock-ipc/src/peer.rs:385`) | `Remote(cancelled)` instead of `Closed` (run 7761, Windows, 1 in 2,000 on the Windows 11 Arm VM) | `close` marked the connection closed, which cancels the served calls, and only then shut the outgoing queue; a handler that answered its cancel at once queued a "cancelled" reply that went out before the end of the stream. | Fixed by ordering on `fix/main-after-7761` (5528ea2b2): `close` goes through `fail`, which shuts the queue first. |
| 2 | `paddock-mcp` loopback (`crates/paddock-mcp/src/loopback.rs`, `serve_loopback`) | "Connection reset by peer" (os error 54) after reading the refusal (run 7761, macOS) | A refused client's key line was left unread when its socket closed; a TCP socket closed with unread received data sends a reset. Drains shared the cap of the places (1), so a client that reconnected at once found no drain. | Fixed by a separate drain pool of 16 on `fix/main-after-7761` (29990de5f), not yet on main. |
| 3 | Dyno agent (`apps/dyno/crates/dyno-agent/src/server.rs`), through `paddock-ipc` `handshake.rs` | "the host closed the connection during the handshake" (run 7763, Linux container; not reproduced on macOS) | Open. Candidates (lessons.md on `origin/train/2026-11-08-1`, entry "Dyno helper_seam ... cause open"): the handshake watchdog could not start and the stream was dropped without an answer (`handshake.rs:100-112`); the read of the `Hello` failed and the host returned without a `Fail`; the 10 s watchdog fired; a rig artefact. One more found for this ADR: a listener closed while a client waits in its backlog (I11). | Under investigation on `fix/dyno-agent-handshake-close`. |

Earlier instances of the same shape are in `docs/lessons.md` (origin/main): line 430 (Dyno's helper "drops the client unread
and the client sees a reset"), line 844 (a fake server dropping a client mid-handshake, Windows os error 10053, fixed in one
copy only), and the 2026-11-07 MCP field case quoted in `loopback.rs` ("the reset can reach the client before the
refusal").

**The pattern:** a connection ends without the peer learning why, or in the wrong order, whenever one side refuses, closes,
times out or stops. Each code path that ends a connection decides on its own whether to say why, in which order to stop
the outgoing queue, cancel work and close the socket, and whether to read what the peer already sent; most paths do not.

**The invariant that should hold:** every end of a connection that one side decides is announced on the same stream by
exactly one typed reason, which is the last thing that side writes; nothing produced by the teardown itself (cancel
answers, pushes) follows it; the time spent sending what was queued before it is bounded; and the side then reads and
discards the peer's input under a bound before it releases the socket, so the reason arrives instead of a reset. A
connection the side never accepted (still in the backlog when the listener stops) is accepted and refused the same way,
within a bound.

## Instances found (origin/main, 2026-11-09, unless a row names a branch)

Every place that ends a connection in the family's transports: `paddock-ipc`, `paddock-mcp`, their consumers, and
Pitcrew's own app-service transport (a third, hand-rolled transport with the same class; in scope, see §9). "Broken" means
a path that ends a connection with no reason, out of order, or with the peer's data unread; "at risk" means correct today
only by an unchecked ordering or a timing margin.

| # | File and place | Path | State |
| --- | --- | --- | --- |
| I1 | `paddock-ipc` `peer.rs:385` `Peer::close` | owner closes | Broken on main (order); fixed by 5528ea2b2 on a branch; still sends no reason (the peer reads only "x closed the connection"). |
| I2 | `peer.rs:474` `Peer::fail` (stall, protocol error, write failure, reader end) | local failure | Broken: no reason frame; and `closer.close()` right after `mark_closed` can cut off frames queued before the shutdown that the writer has not sent yet (also after 5528ea2b2). |
| I3 | `peer.rs:917-942` `read_loop` end; `frame.rs:403-406` folds `ConnectionReset`/`ConnectionAborted` into `FrameError::Closed` | remote end | Correct (nothing to announce), but untyped: a reset, a clean end and an announced end all read as `Closed(String)`. |
| I4 | `handshake.rs:94-120` `bounded` | watchdog fires | Broken: the watchdog closes the stream; the client reads "the host closed the connection during the handshake" (`connect_on`, `:502-504`). |
| I5 | `handshake.rs:100` and `:112` (`stream.closer()?`, watchdog spawn failure) | resource failure | Broken: `?` returns and drops the stream with no answer (candidate cause of #3). |
| I6 | `handshake.rs:94-120`, `:316-321` host side; `:488` client side | watchdog fires after `Welcome` | Broken both ways: the host returns `Timeout` and drops a connection the client accepted; the client's own `bounded` can time out after the host welcomed it. |
| I7 | `handshake.rs:265`, `:267`, `:270`, `:294` (client left, `read_body` error, first frame not a hello, hello not valid) | malformed or failed read | Broken: no `Fail` written. |
| I8 | `handshake.rs:210-217` `check_account` refusal | refuse before reading | At risk: the refusal is written, then the stream drops with the client's `Hello` unread (the shape of #2; whether AF_UNIX resets is measured in WP-0). |
| I9 | `handshake.rs:173` `refuse_busy` (used by `link/server.rs:185`) | busy | At risk: as I8. |
| I10 | `handshake.rs` version and auth refusals | refuse after reading | Correct order today; nothing sent after the `Hello` is drained. |
| I11 | `Listener` drop (`endpoint/unix.rs:147-157`; `endpoint/windows.rs:241-283`) | listener stops | Broken. **Unix:** closing a listening AF_UNIX socket resets the connections queued in its backlog (Linux gives them ECONNRESET), which paddock-ipc reports as `Closed`: "the host closed the connection during the handshake" (fits #3). **Windows:** the endpoint keeps at most one pending pipe instance, so the "backlog" is that one connected client (dropped unanswered); other clients wait in `WaitNamedPipe` and see the name vanish. |
| I12 | `link/server.rs:202` (`Ok(peer) if stop => peer.close()`), `:247` stop | stop | Broken: no reason ("stopping"). |
| I13 | `link/server.rs:208-211` | handshake thread fails to spawn | Broken: the closure that owns the stream is dropped; the client gets no answer. |
| I14 | `launch.rs:201`, `:213` child close | owner closes | Broken: no reason (I1). |
| I15 | Dyno agent `server.rs:64-78` (inline handshake on the accept loop, `sleep(50 ms)` at `:76` after any refusal), `:70` (`connect` fails after `Welcome`: when the push thread cannot start, the `Sub` (`:103`) and the started peer (`:98`) stay alive, unclosed), `:80` (stop) | refuse, internal failure, stop | Broken: the error path drops the stream; the sleep delays every other client; a connect failure leaves a live peer the agent no longer tracks. |
| I16 | Dyno helper `apps/dyno/crates/dyno-helper/src/server.rs:222` (caller not identified), `:235` ("too many connections, one was dropped"), `:249` (`BodyPeer::start` fails), `:250-251` (`SESSION_MAX` expiry), `:255` (session thread fails to spawn) | refuse, busy, failure, expiry | Broken: dropped or closed with no answer. The helper uses the raw `endpoint::Listener::accept` and `Connection::raw` in production (`:215`, `:248`): no handshake, no typestate. |
| I17 | Dyno app link `apps/dyno/app/src-tauri/src/link/driver.rs:299`, `:305`, `:344`, `:362` | owner closes | Broken: no reason (I1). |
| I18 | `paddock-mcp` `loopback.rs` `handshake` refusals (`key`, `handshake`) | refuse after the key line | At risk: correct line and order; anything the client sent after its key line (a pipelined `initialize`) is left unread. |
| I19 | `loopback.rs` busy refusal: **on main** the drains share `max_connections` (`:394`), so with every place taken the refused key line is left unread (bug #2); **on `fix/main-after-7761`** (29990de5f) drains have their own `DRAIN_PLACES = 16`, and beyond them the connection "is closed at once" | busy under load | Broken on main, at risk on the branch. |
| I20 | `loopback.rs:283-288` | `try_clone`/`Watched::new` fails after a place is taken | Broken: dropped with no refusal. |
| I21 | `loopback.rs:361-363` and the listener's drop | a client accepted after `stop` is set; clients still in the TCP backlog | Broken: dropped unanswered; the backlog gets a reset when the listener closes (I11 for MCP). |
| I22 | `loopback.rs` `Serving::shutdown` (`shutdown(Shutdown::Both)`, `:169` on the branch) | stop after pairing | Broken: the bridge reads a bare end. |
| I23 | `paddock-mcp` `bridge.rs` | reads the end | Broken in effect: it cannot say why the app ended a paired connection (only "It may be restarting"). |
| I24 | Pitcrew `apps/pitcrew/crates/pitcrew-service/src/ipc.rs:86-88` | hello timeout or unreadable hello | Broken: `_ => return`, no answer. |
| I25 | Pitcrew `ipc.rs:101`, `:113-115`, `:119`, `:129` | version refusal, malformed request, read error, service stopping | At risk: the error line is written (except the read error, `:115`), then the stream drops with nothing drained. |
| I26 | Pitcrew `ipc.rs:148-163` `subscribed`, `:168-260` `serve` | subscription end, stop, 32 connections | Broken: a subscription ends silently; at 32 connections the accept loop waits on the semaphore after accepting, so the next client waits unanswered instead of being refused `busy`; a stop drops everything. |
| I27 | `apps/debrief/crates/debrief-ipc/src/peer.rs:231` (registered copy of `ipc-frames-peer`, delete by `debrief-v1.2.0`) | owner closes | Correct order (`Out::Shutdown` before `mark_closed`); no reason. Out of scope by waiver (§9). |

27 instances: 20 broken, 4 at risk, 3 correct in order but without a reason or a drain. None is checked by a test that
enumerates endpoints. Pitcrew's transport is also an **unregistered copy** of the `ipc-endpoint` and `ipc-frames-peer`
mechanisms (`docs/known-copies.toml` lists only Debrief's copies), so rule 55 applies to it (§9).

## Decision

### 1. One closed vocabulary of reasons, and a closure that cannot lie

The pure core (§10, D3 decides its home) declares why a connection ends:

```text
enum Refusal  { Version { ours, theirs }, Auth(AuthFailure), Busy, Stopping,
                Timeout { phase: Phase }, Malformed(ProtocolError), Internal(InternalCause) }
enum Closing  { ByOwner(OwnerReason /* Stopping | Replaced | SessionExpired | Done */),
                Protocol(ProtocolError), Stalled { secs }, Internal(InternalCause) }
enum Reason   { Refused(Refusal), Closing(Closing) }
enum By       { Local, Remote }
enum Closure  { Said { by: By, reason: Reason, detail: String },   // a reason was sent or received
                Transport { detail: String } }                     // the stream ended with no reason
impl Closure  { fn side(&self) -> Side }                           // derived, never stored
```

A closure without a reason that is not a transport end cannot be built. Each transport maps a `Reason` to its own codes:
`paddock-ipc` to `version`, `auth`, `busy`, `stopping`, `timeout`, `protocol`, `stalled`, `internal`, `closed`; MCP to its
existing `key`, `handshake`, `busy` refusal codes plus the closing line's codes; Pitcrew to its `ErrorCode`. Nobody passes
a free string for a connection end (call failures keep their free `RemoteError` codes). `IpcError::Closed(String)` becomes
`IpcError::Closed(Closure)`: a caller can tell "the peer said why" from "the stream broke" without parsing text. This is a
breaking change of `paddock-ipc`'s public API with no shim (Paddock is before v1).

### 2. One ordered shutdown, with a flush policy per reason

A connection is in exactly one state, held in one place and changed only by one pure function
`advance(state, event) -> (state, actions)` (the ADR 0056 pattern). The table is the specification of the exhaustive model
test; a cell not listed is a no-op that the model test asserts.

| State \ event | `welcome` | `refuse(r)` / `end(r)` | reason received (`Fail{0}`) | transport end (EOF, reset) | write fails | flush done / flush cap | drain: EOF / byte cap / time cap / no budget | hello deadline |
| --- | --- | --- | --- | --- | --- | --- | --- | --- |
| Handshaking | → Open | → Sealed(r), policy Abort | → Draining (host: protocol refusal) | → Released, `Transport` | → Released, `Transport` | – | – | → Sealed(Timeout), policy Abort |
| Open | – | → Sealed(r), policy by `r` | → Draining(`Said{Remote}`): fail pending, cancel served, discard queue, half-close own write | → Released, `Transport`: fail pending, cancel served | → Released, `Transport` | – | – | – |
| Sealed | – | ignored (first reason wins; simultaneous close from both sides: each side's own reason is what it sent, the one it receives is logged) | record the peer's reason in the log; stay Sealed | → Released (the peer left; nothing to drain) | → Released, outcome `reasonUnsent` | → Draining | – | – |
| Draining | – | ignored | ignored | → Released, outcome `eof` | – | – | → Released, outcome `eof` / `bytesCap` / `timeCap` / `poolFull` | – |
| Released | – | ignored | – | – | – | – | – | – |

The actions of entering `Sealed(r)` are fixed in this order, and no other code path closes a connection:

1. **Seal.** The outgoing queue refuses every later frame (`Push::Sealed`) and takes the reason as its last frame.
   `WriteQueue::shutdown()` without a reason is removed, so a close without a reason does not compile.
2. **Fail and cancel.** Pending calls fail with the `Closure`; served calls see their cancel. Their answers find the queue
   sealed (bug #1 cannot recur: the order is the transition's, and the model test checks that nothing is queued after the
   reason).
3. **Flush by policy** (P1-1 of the review):
   - **`Graceful`** (for `Closing::ByOwner` and `Refusal::Stopping` on an open connection): the writer sends the frames
     queued before the seal, under the **flush cap** on the injected clock (2 s); at the cap it drops what is left (outcome
     `flushCap`) and goes on with the reason. Frames queued before the seal are delivered within the cap (the model test
     covers it; this also closes I2's "cut off before the writer sent them").
   - **`Abort`** (for `Closing::Stalled`, `Closing::Protocol`, `Closing::Internal` and every handshake refusal): the queued
     frames are dropped at once; only the reason is tried.
   - In both, the reason is written under the **reason deadline** (250 ms on the clock, the write made non-blocking or
     sliced); if it cannot be written, the closer closes the stream (which unblocks the writer, as `fail` does today) and
     the outcome is `reasonUnsent`. A peer that stopped reading therefore holds a thread and a connection for at most the
     flush cap plus the reason deadline.
4. **Half-close.** The writer shuts its write half (`shutdown(Write)` on sockets). Named pipes have no half-close: on
   Windows the writer's last write is the end of its output, and `DisconnectNamedPipe` is never called while the client
   may still read, because it discards unread data
   ([Microsoft Learn](https://learn.microsoft.com/en-us/windows/win32/api/namedpipeapi/nf-namedpipeapi-disconnectnamedpipe)).
5. **Drain.** The reader reads and discards input into a fixed 8 KiB buffer until the peer's end, the byte cap, the time
   cap on the injected clock, or immediately when the drain budget is used up (§7).
6. **Release.** The socket or pipe is closed; the outcome is logged (§6).

**The receiving side ends promptly.** A side that receives `Fail{0}` (or a refusal line, or Pitcrew's error-and-close)
records `Closure::Said { by: Remote }`, fails its pending calls with it, discards its own queue, half-closes its write side
and releases after its own short drain. The closer's drain then ends with `eof` in milliseconds, not at its time cap.

**Client-side drains need no budget.** A client drains only connections it opened itself, so its drains are bounded by
the number of connections the process opens, which no remote side can raise; the budgets of §7 are for the accepting
side, where the other side chooses how many connections to open.

`Peer::close` returns at once (its caller never waits for a flush or a drain); `closed()` reports the `Closure` from step 2;
`Peer::wait_released(timeout)` waits for step 6 (tests and orderly stops). The peer's reader, which already exists, does the
drain: no new thread for an open connection.

### 3. Accepting: a verdict before the stream can be dropped

- **Every accept returns a typestate** (P1-2). `endpoint::Listener::accept` (and so `Host::accept*`, `Host::accept_raw` and
  `LinkServer`) returns `Accepted`, which owns the `Stream`. Its only exits are:
  - `authenticate(self, &Authenticator) -> Result<Connection, Refused>` (the handshake; a refusal goes through §2);
  - `refuse(self, Refusal)`;
  - `serve_without_handshake(self, why: &'static str) -> RawConnection` for an endpoint whose trust comes from the OS (the
    Dyno helper reads the caller's identity from the OS; Dyno ADR 0027). A `RawConnection` ends through the same
    `Peer` sequence, and every call of it is listed in the register (§5).
- `Connection::raw` and `Connection::stream_mut` move behind the crate's `testing` feature (in-process pairs and tests that
  write bytes a peer never would). Production code cannot make a connection from a bare stream.
- **The `Drop` fallback never blocks.** Dropping an `Accepted` without a verdict hands its stream to the endpoint's drain
  pool with `Refusal::Internal(DroppedWithoutVerdict)` (a non-blocking hand-off; with the pool full the stream is closed at
  once and counted as `poolFull`) and logs at error level. It never writes or drains inside `Drop`, so it is safe on the
  accept thread and during an unwind. The ending harness (§5) fails on that log line: the fallback is a safety net.
- **No watchdog thread** (D2). The `Hello` is read through a `Read` wrapper that slices each blocking read by the stream's
  own read timeout (both endpoints support one: `endpoint/unix.rs` polls; `endpoint/windows.rs` passes `read_timeout` to
  the overlapped read and ends a timed-out slice with `CancelIoEx`, `windows.rs:129`) and checks the `Deadline` on the
  injected clock between slices, as `paddock-mcp`'s `Watched` reader does. A slice timeout never reaches `read_body`, so a
  partly read frame is never lost. A handshake that runs out is refused with `Timeout { phase: Hello }` by the thread that
  owns the stream (I4); no thread spawn can fail (I5); the verdict is one value, so `Welcome` and a timeout cannot both
  happen on either side (I6).
- **Refuse, then drain.** Every refusal follows §2 with policy `Abort`; a refusal before the `Hello` is read (account check,
  busy) drains the `Hello` too (I8, I9).
- **The listener stops in order** (I11, I21). On Unix and for TCP, `stop` (and `Drop`) accepts what waits in the backlog
  without blocking, up to the backlog bound, refuses each with `Stopping` through the drain pool, and only then closes the
  listener. On Windows the endpoint holds at most one pending pipe instance: if a client has connected to it, it is refused
  the same way; clients waiting in `WaitNamedPipe` see the name disappear, which the client already maps to "not running".
  Clients beyond the bound are closed by the OS as today, counted and logged.

### 4. The MCP transport on the same core

`paddock-mcp`'s loopback uses the pure core (the state machine, the flush and drain policy, the drain pool, the log fields;
not the frames and not the IPC codes). Its refusal lines stay as they are (`{"ok":false,"code":"key"|"handshake"|"busy"}`);
each refusal drains under the refusal bounds; the places and the drains stay separate (29990de5f); the paths of I20 and
I21 answer. A paired connection that the app ends (stop, `McpControl` off) writes one JSON-RPC notification
`{"jsonrpc":"2.0","method":"notifications/paddock/closing","params":{"code":...,"message":...}}` as its last line before it
half-closes and drains (I22). MCP defines no shutdown message and leaves the end to the transport
([MCP lifecycle, Shutdown, 2025-06-18](https://modelcontextprotocol.io/specification/2025-06-18/basic/lifecycle)); a
JSON-RPC notification gets no reply ([JSON-RPC 2.0, §4.1](https://www.jsonrpc.org/specification#notification); read, not
re-fetched today).

**Version mismatch.** An older bridge (`bridge.rs:190-209`) forwards every line, so an older `dyno-cli` paired with a newer
app would pass the closing line to the third-party host. Hosts are expected to ignore an unknown notification, but that is
not verified for every host. So the app sends the line only to a bridge that asked for it: the bridge's key line gains an
optional `"caps":["closing"]` (the server ignores unknown fields today, `loopback.rs` `handshake` reads only `key`), and a
bridge that sends it consumes the line, never forwards it, and turns it into its `BridgeError` text (I23). Older bridges get
today's bare end. The `ok` line does not change.

### 5. The check: a register that enumerates, an ending harness, a positive control (C102, number assigned at landing)

**The register** (`docs/ipc-endpoints.toml`) has two sections:
- `[[endpoint]]`: every place in non-test code that listens or accepts, with its crate, the constructor and the name of its
  ending test.
- `[[out_of_scope]]`: every other listen site, each with a reason, an owner and, where it applies, a delete-by or
  adopt-by tag. Day-one entries (from the review's sweep; WP-0 confirms the list): `pitwall-server/src/main.rs` (HTTP: it
  keeps HTTP's own close semantics), `fake_llama_server`, `chromium-driver/src/fake.rs`, `paddock-chromium/src/fake.rs`
  (test fakes), `paddock-testkit/src/net.rs` (test support, ADR 0023), `debrief-ai-worker/src/child.rs:181` (a port probe,
  not a server), and `apps/debrief/crates/debrief-ipc/**` (waived until `debrief-v1.2.0`, §9).

**The static check** (no build; part of the push check) is syn-based, in `paddock-dev`: it parses every non-test Rust file,
skips `#[cfg(test)]` items, `tests/`, `benches/` and `examples/`, resolves each file's `use` paths, and finds every listen
primitive: `std::net::TcpListener::bind`, `std::os::unix::net::UnixListener::bind`, `tokio::net::{TcpListener,
UnixListener}::bind`, tokio's `TcpListener::from_std` and `UnixListener::from_std`, tokio's
`named_pipe::ServerOptions::create` and `create_with_security_attributes_raw` (Pitcrew, `ipc.rs:207`), socket2's
`Socket::listen`, `CreateNamedPipeW`, `paddock_ipc::endpoint::Listener::bind*`, `Host::listen*`, `LinkServer::start`,
`serve_loopback`, and `Accepted::serve_without_handshake`. The check's own positive control is a fixture tree with one
production-style site per listen call above (and one inside a `#[cfg(test)]` module that must be skipped); a test asserts
that the check reports every fixture site by its call, so a listen call the parser stops seeing turns the test red. A site in neither section,
an entry whose test does not exist, or an out-of-scope entry without a reason fails the check. A new endpoint is therefore
covered by construction, and a grep's confusion between test modules and production code does not arise.

**The harness.** `testing::endings` (behind the `testing` feature) drives an endpoint through: wrong token, wrong version,
wrong account (Unix), busy, a silent client (handshake timeout on a `FakeClock`), a malformed first frame, a client that
keeps writing after it is refused, an owner close with calls in flight whose handler answers its cancel at once, an owner
close while the peer is not reading (flush cap and reason deadline, P1-1), a stall, a listener stop with a client in the
backlog, a stop with an open connection, and a mixed-version peer that ignores `Fail{0}`. It asserts:
- on the **raw stream**, below `paddock-ipc`'s folding of resets into `Closed` (`frame.rs:403-406`): the expected reason
  frame or line arrives, then an orderly end, and the read never fails with `ConnectionReset`/`ConnectionAborted`;
- on the typed API: never `Closure::Transport` in these scenarios, never `Remote(cancelled)` after an owner close;
- on the **endpoint's own end line** (§6): the code, the flush outcome, the drain outcome `eof` with the bytes read, and no
  `DroppedWithoutVerdict` line.

The ordering race of #1 is made deterministic with a test seam between "seal" and "cancel" (ADR 0041), not by repetition.

**The positive control** (ER-33). `testing::endings::Broken` endpoints, one per fault: drop without a reason, cancel before
seal, flush without a cap, release without drain, drop the backlog. A test asserts the harness fails on each, with the
scenario named. A dropped drain may have no effect a client can see on AF_UNIX (Linux delivers queued data before the
ECONNRESET) or on a pipe closed with `CloseHandle`; so the `release without drain` control is judged on the endpoint's end
line everywhere, and on the raw stream only on TCP and on the OS where WP-0 shows a reset.

**All three OS.** `paddock-ipc`'s and `paddock-mcp`'s tests run on the Linux, macOS and Windows crate legs (run 7761 ran
them on Windows and macOS); each app's ending test runs on the OS legs its workflow has. A ratchet lists the scenarios
expected to fail per instance; WP-0 records the red run on main, and the ratchet only shrinks.

### 6. Logs (ER-27)

Every connection end logs one line on each side, no content: connection id and fingerprint (as the Dyno agent's
`AgentRun`), the closure (`said by local`, `said by remote`, `transport`), phase (`hello`, `open`), reason code, detail, how
long the handshake and the open phase lasted, the flush policy and outcome (`flushed`, `flushCap`, `dropped n frames`), the
reason outcome (`sent`, `reasonUnsent`), pending calls failed, served calls cancelled, and the drain outcome (`eof`,
`bytesCap`, `timeCap`, `poolFull`) with bytes discarded and duration. Refusals log the cause the peer was told and the cause
for the log (never a token or a key, as `loopback.rs` does today). Each endpoint logs a periodic summary of ends by code
and outcome (the Dyno helper's `ServedLine` pattern). A client logs the reason it received and the next step its message
names. Rigs that start a product print the product's log on failure (the lesson of run 7763).

### 7. Bounds (no new denial of service)

| Bound | Default | Why |
| --- | --- | --- |
| Flush cap (`Graceful` only) | 2 s on the clock | An owner close does not wait behind up to 32 MiB of pushes to a slow reader. |
| Reason deadline | 250 ms | A peer that stopped reading cannot hold the closer; the stream is closed instead. |
| Drain after a refusal | 64 KiB or 2 s | A `Hello` or key line is under 1 KiB; a client that keeps writing is cut off. |
| Drain after an open connection ends | the connection's frame limit + 64 KiB, or 2 s | The peer may be in the middle of one frame (up to the frame limit) plus control frames when it learns the end; more means it is not listening. The time cap is the real bound; the byte cap stops a fast writer early. |
| Refusal drain pool per endpoint (connections with no thread of their own) | 16 | As 29990de5f; beyond it the refusal is still written, then the socket closes at once (`poolFull`). |
| Open-drain budget per endpoint (P2-2) | `max_connections`, at least 16 | A drain runs on the connection's reader but no longer counts against the places (LinkServer drops closed peers; MCP gives the place back before the end, on purpose). Without a budget, a key holder who provokes server-side closes could keep connection rate × 2 s readers alive. Beyond the budget the connection is released at once (`poolFull`). |
| Backlog refused at stop | 64 (Unix and TCP); the one pending instance (Windows pipes) | Bounded accept-and-refuse. |
| Reason message | 512 bytes, truncated | Always fits `MIN_FRAME` (1 KiB). |
| Handshakes in progress | the existing `LinkServer` cap; the Dyno agent and Pitcrew move to it | One silent client never holds the accept loop (I15, I26). |

**Per-OS option for pipes (D4).** If WP-0 shows that a named pipe closed with `CloseHandle` (not `DisconnectNamedPipe`) keeps
the data already written readable by the client, pipes skip the drain: the server releases right after the reason. That also
removes the Windows mixed-version delay (§8).

The drain allocates nothing per byte (one fixed buffer). The Dyno agent's `sleep(50 ms)` after any refusal goes: it delayed
every client and limited nothing. These are protocol bounds, set in `PeerOptions` and the `Host` builder with these
defaults, not user Settings (rule 21 is for tunables a user changes).

### 8. Wire and protocol versions

- **No frame layout change.** The reason is the existing `Frame::Fail { id: 0, error }`. Its meaning is set by the phase:
  before `Welcome` it is a handshake refusal (as today); after it, the closing reason. Frame `VERSION` stays 2.
- **Old peers, every OS.** A reader of today's code ignores a `Fail` for an id it is not waiting for (`Peer::deliver`,
  `peer.rs:626-631`), and call ids start at 1 (`peer.rs:306`, `next: AtomicU64::new(1)`), so an old peer drops the reason and
  sees the end of the stream, as now. An old client in a handshake maps a `Fail` with an unknown code to `IpcError::Remote`
  (`connect_on`, `:498`), which says more than "the host closed the connection". Verified by reading; the harness's
  mixed-version scenario keeps it true.
- **Not exactly today's behaviour on Windows** (P2-1). A pipe has no half-close; an old peer ignores the reason and sees no
  end until the new side's drain reaches its time cap (2 s), and its calls in that window hang instead of failing at once.
  This matters for the separately installed Dyno helper service. Two ways out, both in D4: no drain on pipes if WP-0
  confirms `CloseHandle` keeps written data readable; or, once ADR 0001's frame version 3 (capabilities in the handshake,
  package 2.6d) lands, a capability bit `reasons` so a new side drains only for peers that understand the reason.
- **ADR 0049's derived versions do not move.** Its schemas cover the bodies (`app-agent`, `agent-helper`, `host-plugin`
  roots), not `Frame`; no body type changes, so no derived version is bumped and no installed Dyno helper is refused.
- **Declaring the frame as a wire waits for C80.** C80 and `wire.toml` do not exist on any branch yet (ADR 0049 WP-4), and
  ADR 0049 §4 declares wires per app (`apps/<app>/wire.toml`). D1 proposes that a shared crate declares its wire in its own
  folder (`crates/paddock-ipc/wire.toml`, `ipc-frame`, `roots = ["Frame"]`, constant `frame::VERSION`) when C80 lands, as an
  amendment of ADR 0049 §4; until then a golden-encoding test of `Frame` in `paddock-ipc` guards the layout.
- **MCP:** the refusal lines do not change; the closing line goes only to bridges that asked for it (§4).
- **Pitcrew:** its frames (JSON, `pitcrew_core::protocol`) do not change; its error-and-close answers already exist and are
  made consistent (§9).

### 9. Consumers, copies and migration

Consumers of `paddock-ipc` (by `Cargo.toml`): Dyno (`dyno-agent`, the app's `src-tauri`, `dyno-cli`, `dyno-helper`,
`dyno-helper-service`, `dyno-plugin-api`, `dyno-plugin-host`), `paddock-plugin-api`, `paddock-plugin-host`,
`paddock-plugin-package`, `paddock-plugin-test`, `paddock-view`. Consumers of `paddock-mcp`: `paddock-shell`, Dyno's app and
`dyno-cli` (the bridge), `shell-demo`. Pitcrew's service and app use Pitcrew's own transport. Debrief uses its registered
copy `debrief-ipc`. Pitwall and Spotter use none of these.

**Copies (rule 55).** The change touches four registered mechanisms, each with a Debrief copy, so four waivers (D5):
`Copies-waived: ipc-endpoint apps/debrief/crates/debrief-ipc/src/endpoint.rs: ...`, `ipc-frames-peer
apps/debrief/crates/debrief-ipc/src/{frame,peer,wire}.rs`, `ipc-child-launch apps/debrief/crates/debrief-ipc/src/{launch,child}.rs`,
`companion-link apps/debrief/crates/debrief-ipc/src/link.rs`, each with the reason "replaced by `paddock-ipc` in Debrief 1.2
PH1 to PH4, delete by `debrief-v1.2.0`". Pitcrew's transport is registered in the same change as a copy of `ipc-endpoint`
and `ipc-frames-peer` (owner `pitcrew`, with the delete-by D9 sets) and fixed through the pure core (§10, WP-11).

**Merges** (ADR 0049: small merges; rule 43: each migrates its consumers; rule 54: each builds Dyno's installers):

| Merge | Content | Wire effect |
| --- | --- | --- |
| M0 | WP-0: harness skeleton, register, static check, ratchet with the red scenarios | none |
| M1 | The core's types and `IpcError::Closed(Closure)` (the API break), consumers' match sites | none (no behaviour change) |
| M2 | Peer: seal, reason, flush policy, drain, receiving `Fail{0}`; consumers' close sites (I1, I2, I12, I14, I17) | compatible (`Fail{0}` after the handshake) |
| M3 | Accepting: `Accepted` typestate, sliced reads, `Connection::raw` behind `testing`; Dyno agent and helper (I4 to I10, I13, I15, I16) | none |
| M4 | Listener stop with backlog refusal (I11) | none |
| M5 | MCP loopback and bridge (I18 to I23) | additive (`caps`, closing line) |
| M6 | Pitcrew's transport through the core (I24 to I26) | none |

Each merge moves its instances out of the ratchet. **Rollback:** each merge is reverted alone; no frame format changed.

### 10. Where the pure core lives (D3)

The three transports share the state machine, the flush and drain policy, the bounds and the log fields; they do **not**
share codes or framing (IPC: `version`/`auth`/...; MCP: `key`/`handshake`/`busy`; Pitcrew: its `ErrorCode`), and Pitcrew is
async (tokio) while the other two are threads. The options are in D3.

## Parallelism and cost

- **Threads.** Fewer than today: the per-handshake watchdog thread goes (one fewer thread per connection attempt). The drain
  of an open connection runs on its existing reader thread, within the open-drain budget. Refusals with no thread use the
  bounded pool (16 per endpoint). Nothing is CPU-bound; nothing runs at a starvable QoS (rule 52: closing is live work, a
  person waits on a reconnect).
- **Bytes.** One reason frame per end (under 600 bytes) and at most the drain caps read and discarded into one buffer.
- **Latency.** `close` returns at once; a connection is released within the flush cap + the reason deadline + the drain
  time cap at worst (4.25 s), normally within milliseconds because the receiving side ends promptly (§2). A client that
  reconnects at once is not blocked by the previous connection's drain (separate pool, as 29990de5f).
- **Cheaper, not only parallel (rule 51).** The handshake becomes cheaper (no thread spawn and join per attempt; a sliced
  read is one poll or one `CancelIoEx` per idle slice); the Dyno agent's serial inline handshake with a 50 ms sleep after
  errors is replaced by the link server's threaded, capped handshakes.
- **To measure and record (rule 48, `docs/benchmarks.md`):** close-to-release time (p50, p99) and connect-refuse churn per
  second on the three OS before and after, in the harness.

## Work packages (raw estimates, one person)

| WP | Content | Days |
| --- | --- | --- |
| WP-0 (M0) | Harness skeleton, endpoint register with the out-of-scope triage, syn-based static check with its positive-control fixtures, ratchet with `blocks_tag`; red runs on main on the three OS; the OS measurements below. Plan in "M0 plan" | 3.5 |
| WP-1 | The core: reasons, `Closure`, the state × event table as `advance`, policies, bounds; exhaustive model test (every event in every state, action order, frames queued before the seal delivered under the cap) | 2 |
| WP-2 | `WriteQueue::seal`, flush policy with the cap and the reason deadline, `Peer` end sequence, reader drain with the budget, receiving `Fail{0}` and ending promptly, `wait_released` | 3 |
| WP-3 | `Accepted` on every accept, `RawConnection`, `Connection::raw` behind `testing`, the non-blocking `Drop` hand-off, the sliced `Read` wrapper (no watchdog), one verdict, refusal drains, client side | 3 |
| WP-4 | Listener stop with backlog refusal, per OS; drain pool and open-drain budget; `LinkServer` on it | 1.5 |
| WP-5 | MCP loopback and bridge on the core: refusal drains, I20 and I21, `caps` and the closing line, stop | 2 |
| WP-6 | Full harness scenarios on the raw stream and the end line, mixed-version case, deterministic seal/cancel seam, `Broken` controls (per OS where WP-0 shows a reset), 3-OS runs | 3 |
| WP-7 | ER-27 end lines, summaries, rigs print the product log on failure | 1 |
| WP-8 | Consumers across M1 to M4: Dyno agent (onto the `LinkServer` accept model, no sleep, close the peer when `connect` fails), helper (a per-machine service: busy, unidentified, expiry, spawn failures; `serve_without_handshake`), app link, CLI, plugin host and test fake, shell-demo, the `IpcError::Closed` sites; each endpoint's ending test; Dyno installers built for each merge (rule 54) | 4.5 |
| WP-9 | The four Debrief waivers | 0.5 |
| WP-10 | Proposed rule text, lessons entry, ADR index, CHANGELOGs, the ADR 0001 / ipc-wire.md amendment | 0.5 |
| WP-11 | Pitcrew: register its copies; its transport through the core (hello timeout answered, `busy` refusal at 32 connections instead of waiting on the semaphore, subscription and stop reasons, drains); its ending test on its OS legs. **The app's side of `busy`:** the refusal arrives as the answer to the hello, which `service_client.rs:103` already turns into a `ServiceError` and `dto.rs:292` into `CommandErrorCode::Busy`; reads retry it with the shared bounded backoff of `withRequestStatus` (three tries, 250 ms, 1 s, 4 s), commands that change state never retry and show "The Pitcrew service is busy with other connections. Try again in a moment." with Retry; the event subscription reconnects with its existing backoff; a test per path with a fake service that answers `busy` | 3 |
| | **Total** | **27.5** |

### M0 plan (WP-0; a plan only, nothing built yet)

M0 adds checks and measurements and changes no product behaviour. It builds:

1. **The endpoint register** `docs/ipc-endpoints.toml`: one `[[endpoint]]` per production listen site found by the sweep
   (plugin host, link server, child launcher, Dyno agent, Dyno helper, Dyno plugin host, MCP loopback, Pitcrew's service),
   each naming its ending test; one `[[out_of_scope]]` per other site with reason, owner and delete-by or adopt-by (§5).
2. **The static check** (C102, `paddock-dev`): the syn pass over non-test code for the listen calls of §5, the register
   comparison, the positive-control fixture tree and its test, and the ratchet file with `blocks_tag` per entry (D8). It
   runs in the push check; the tag-check hook for `blocks_tag` is wired with C58's.
3. **The harness skeleton** `testing::endings` behind `paddock-ipc`'s `testing` feature: the scenario list of §5 as named
   cases, a raw-stream client that reports the bytes it read and the exact `io::ErrorKind` of the end, a log capture that
   finds the endpoint's end line, and the `Broken` endpoints as positive controls. Before M1 the end line does not exist, so
   the scenarios that need it are ratchet entries; the raw-stream assertions run from the start.
4. **Each registered endpoint's ending test**, calling the harness; today's failures recorded in the ratchet per instance
   (I1 to I26), so the run is green with the ratchet and red without it.
5. **The OS measurements**, recorded in `docs/benchmarks.md` (rule 48) and the lessons file, each on Linux, macOS and
   Windows (the local VMs, never CI): what a client reads when the other side closes with unread received data (TCP,
   AF_UNIX, a named pipe closed with `CloseHandle`, and with `DisconnectNamedPipe`); whether a client of a `CloseHandle`d
   pipe can still read what was written before (decides D4's pipe option); what clients in a Unix listener's backlog see
   when it closes; and the baseline close-to-release time and connect-refuse churn per second.
6. **Rigs print the product's log on failure** where they start a product (the Dyno helper_seam lesson), so the open run
   7763 investigation gets the agent's own line on the next failure.

Done when: the check is green on main with the ratchet, red on each `Broken` fixture and each positive-control site
removed, and the measurements are recorded with their raw values.

**Range: 25 to 28 days.** The low end holds if WP-0 confirms that pipes need no drain (WP-2, WP-4 and WP-6 shrink) and the
out-of-scope triage is as listed; the high end if Pitcrew's async transport needs more than the pure core's adapter.

WP-0 can start at once and helps the open Dyno investigation (its rig prints the agent's log, and the backlog and
no-verdict scenarios test two of its candidates). WP-1 and WP-6's controls run in parallel with WP-2 and WP-3; WP-5 and
WP-11 need WP-1; WP-8 follows the merge it belongs to.

## Decisions for the user

| # | Question | Options | Recommendation |
| --- | --- | --- | --- |
| D1 | How does the reason travel? | (a) The existing `Fail { id: 0 }`, its meaning set by the phase; old peers ignore it; a golden test guards `Frame` now, and `crates/paddock-ipc/wire.toml` (`ipc-frame`) is declared when C80 lands, amending ADR 0049 §4 to allow a shared crate's wire. (b) A new `Frame::Close`, riding on ADR 0001's planned frame version 3. | (a): no compatibility cost now; (b) stays possible as a versioned step once C80 exists. |
| D2 | The handshake timer | (a) A `Read` wrapper that slices each read on the injected clock (as `Watched`), no watchdog thread; a slice timeout never reaches `read_body`; on Windows a timed-out slice is a `CancelIoEx`. (b) Keep the watchdog, but let it only signal and let the stream's owner answer. | (a): removes a thread, a spawn failure and the welcome/timeout race on both sides. |
| D3 | Where the pure core lives | (a) A module `paddock_ipc::ending`; `paddock-mcp` and Pitcrew depend on `paddock-ipc`. One crate fewer, but MCP (and the `dyno-cli` bridge) and Pitcrew would link endpoints, token and Windows pipe code they do not use, for a pure module whose codes they do not share (interface segregation). (b) A small pure crate (`paddock-ending`: states, `advance`, policies, bounds, log fields; no I/O, no runtime) that `paddock-ipc`, `paddock-mcp` and Pitcrew's service use, each mapping reasons to its own codes; the reviewer recommends this. (c) Defer the choice to the end of WP-1, when the shared surface is known. | (b), or (c) if you prefer to see the surface first. With Pitcrew (async) in scope, the shared part is clearly the pure policy, not the IPC; the first draft's (a) is weaker. |
| D4 | Bounds | §7: flush cap 2 s, reason deadline 250 ms, refusal drain 64 KiB / 2 s, open drain frame limit + 64 KiB / 2 s, refusal pool 16, open-drain budget `max_connections` (≥ 16), backlog 64, 512-byte messages; plus the per-OS option: no drain on named pipes if WP-0 confirms `CloseHandle` keeps written data readable (removes the 2 s Windows mixed-version delay). | Accept as protocol defaults in `PeerOptions`/`Host`, not Settings; take the pipe option if WP-0 confirms it. |
| D5 | Debrief's copies (rule 55) | (a) Four waivers until `debrief-v1.2.0` (`ipc-endpoint`, `ipc-frames-peer`, `ipc-child-launch`, `companion-link`; its close order is already right; it is replaced in Debrief 1.2). (b) Port the reason and drain into the copies now (+3 to 4 days). | (a). |
| D6 | MCP after pairing | (a) The closing line, sent only to a bridge that declared `"caps":["closing"]` in its key line; older bridges get today's bare end. (b) Always send it (an older bridge would forward it to the host). (c) Nothing; the spec's bare transport end. | (a): the bridge can say why, and third parties never see the line, whatever the versions. |
| D7 | A binding rule | Propose a Paddock rule (number from the coordinator) and an ER candidate: "every end of a connection says why, last; queued frames are flushed under a cap or dropped by the reason's policy; cancellations come after the seal; the peer's input is drained under a bound; every listen site is in the endpoint register and every endpoint passes the ending harness". | Yes, with the check (C102 or the next free number). |
| D8 | When, and for which release | (a) M0 now; M1 to M4 and M6 target **Pitcrew 1.1** (the first release on the order that ships an in-scope transport: Spotter 1.0 and Pitwall 1.1 use none), and Dyno takes them in its next release after 1.1.0; M5 lands with them. The gate is enforced, not remembered: every ratchet entry of C102 names the tag pattern it blocks (`blocks_tag = "dyno-v*"` for the Dyno instances of M2 to M4, `"pitcrew-v*"` for I24 to I26), and the tag check that already refuses a tag for an expired copy (ADR 0034, C58) refuses a matching tag while such an entry remains; a test of the tag check covers both patterns. *Amendment 2026-11-11 (the user's trim):* an entry for a fault the trim deferred says `blocks_tag = "none"` and names the deferred work package in `deferred_by` (it must contain `ADR nnnn`); it stays listed, blocks no tag, and an entry without that reason still blocks. (b) All after Debrief 1.2's foundation. | (a): the class keeps producing red CI runs, and M0 helps the open Dyno investigation. |
| D9 | Pitcrew's transport | (a) In scope now through the pure core (WP-11, 3 days), registered as a copy of `ipc-endpoint` and `ipc-frames-peer` with delete-by `pitcrew-v1.2.0`, when it moves onto `paddock-ipc` as ADR 0001 foresees. (b) Move Pitcrew onto `paddock-ipc` in 1.1 (its own ADR, larger). (c) Out of scope with a waiver to `pitcrew-v1.2.0` (the class stays open in Pitcrew 1.1). | (a). |

## Consequences

- Every refusal, close, timeout and stop of the family's transports tells the peer why, in order, within bounded time; a
  `Closure::Transport` then means a crash or a broken link, which is what it should mean.
- `IpcError::Closed` changes shape for every consumer (one merge, no shim, before v1); `Connection::raw` leaves production
  code.
- Closing costs at most one small frame and a bounded flush and drain; handshakes cost one thread less.
- The class is checked: a listen site cannot land without a register entry (or a reasoned out-of-scope entry), every
  endpoint passes the ending harness, and the harness proves it can see each fault.

## Alternatives rejected

- **Fix each instance where it is found** (what the three fixes did): each was right, none stopped the next one; 20 broken
  instances remain.
- **`SO_LINGER` with a timeout or a zero linger:** a zero linger turns every close into a reset (the opposite of the goal);
  a linger timeout blocks the closing thread and does not exist for named pipes
  ([Microsoft Learn, graceful shutdown](https://learn.microsoft.com/en-us/windows/win32/winsock/graceful-shutdown-linger-options-and-socket-closure-2)
  recommends an explicit `shutdown` and reading until the peer's end instead, which is §2).
- **Flush everything before the reason, always:** a peer that stopped reading would hold the closer without bound (P1-1).
- **Retry on the client after a bare end:** hides the cause, and retried refusals load the server; the client should know
  the reason and act on it.
- **A watchdog that writes the timeout refusal itself:** two writers on one stream can interleave a `Welcome` and a
  refusal; one owner of the stream is simpler.
- **A grep-based register check:** it cannot tell inline test modules from production code (about 70 `TcpListener::bind`
  sites, most in tests) and misses aliased imports; a syn pass is cheap and exact enough.
