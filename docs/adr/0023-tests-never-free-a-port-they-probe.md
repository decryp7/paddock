# 23. Tests never free a port they probe

- Status: Accepted (2026-10-28, after an independent review). The user decided on 2026-10-28 that the apps' sites stay
  waived until 2026-12-31, one waiver per file
- Date: 2026-10-28
- Related: rule 40 (recurring bugs mean a missing design rule), rules 42/43 (share first, shared behaviour tested once),
  ER-12 and ER-14 (deterministic tests that do not depend on the machine); ADR 0021 (portable tests, `paddock-testkit`);
  ADR 0004 (`paddock-pitwall`); check C39; `crates/paddock-testkit/src/net.rs`
- Evidence: CI run 6512 (`ci.yml`, job "crate (windows-latest)"): `paddock-pitwall::contract
  a_link_that_has_not_seen_pitwall_says_what_is_missing` failed at `contract.rs:1466` with `left: Running, right: Absent`;
  2509 of 2510 tests passed. Reproduced on macOS by starting a fake Pitwall on the port the test had freed (same
  assertion, every run). Measured on macOS (2026-10-28): connecting to a port held by a socket that is bound but not
  listening times out instead of being refused.

## Context

To test "nothing listens there", tests read a port from a listener and dropped the listener:

```rust
let free = {
    let l = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    l.local_addr().unwrap().port()
};
```

The port is free again as soon as the block ends. Before the test connects, the OS can give it to another process. On a
shared CI runner, nextest runs each test in its own process, and many of them bind ephemeral ports at once (every
`FakePitwall`, every fake HTTP or SMTP server). Windows hands out ephemeral ports in sequence, so a port that was just
freed is likely to come back soon. In run 6512 the Pitwall link probed the freed port as Pitwall's fallback port, found
another test's fake Pitwall there, received a valid health answer, and reported `Running`.

The product code was right: without a discovery file it tries the fallback port, and anything that answers Pitwall's
health route with the right API version counts as Pitwall. The test was wrong: it relied on the machine.

The same shape appears in 21 places across the shared crates and four apps (survey below). Each one can fail the same
way, with an error kind other than the one asserted, and fixing them one at a time does not stop new ones.

No held port can refuse a connection on every OS. A socket that is bound but not listening is refused on Linux and
Windows, but macOS drops the attempt and the client times out (measured here). Tests that need "refused" therefore
cannot use a real port at all.

## Decision

1. **One mechanism: `paddock_testkit::net::HeldPort`.** It holds a loopback port for as long as the guard lives, so no
   other process can be given that port. Every connection is accepted and ended at once: `HeldPort::hang_up()` closes
   it normally (the client reads the end of the stream), and `HeldPort::reset()` sets `SO_LINGER` to 0 (the client gets
   "connection reset"). Dropping the guard stops the accept loop and joins its thread. The guard is tested once, in the
   kit: no other server can bind the port, a hang-up port never answers, and a reset port resets every connection.
2. **"Refused" through a connector or the mapping, never through a port.** Where a test needs a refused connection,
   the code under test gets a small connector port (`paddock_pitwall::loopback::Connector`, with `Tcp` as the real
   one, `Client::with_connector`), and the test injects one that returns `ErrorKind::ConnectionRefused`. Where the
   behaviour is only the mapping of a library error (paddock-net, paddock-channels, paddock-download), a unit test
   builds the refused error and checks how it is mapped: paddock-net's wire mapping (refused, reset and aborted, since
   Windows may report an abort) and paddock-download's `classify` build `ConnectionRefused`; paddock-channels takes
   paddock-net's result and tests its next step. The tests that used a freed port now use a reset port and are named
   for it ("a reset connection is a connect error"): the connection fails with the same error kind, without depending on
   the machine.
3. **One check: C39 "Tests never free a port they probe"** (conformance, error level, every Rust file). It reports:
   - a chain that reads the port of a temporary listener: `TcpListener::bind(..).unwrap().local_addr()`, also with `?`,
     `.expect(..)`, `.unwrap_or_else(..)`, `.map_err(..)`, `.context(..)`, `.with_context(..)` or `.await`, and under a
     `use .. TcpListener as Alias` name;
   - a block or fn body that binds `let l = ..bind(..)` (or a socket2 `Socket::new(..)` that it binds and sets up) and
     ends with `l.local_addr()..`, using `l` for nothing else;
   - `drop(l)` when a value made from `l.local_addr()` is used after the drop.

   A justified case carries `// conformance: freed-port — <reason>`. A waiver without a reason is itself an error. The
   check uses its own Rust masker that understands raw strings and char literals; C30's masker does not, and a `'"'`
   earlier in a file hid a later site. On `origin/main` the check reports all 11 shared-crate sites, including the line
   that failed in run 6512. On this branch it reports none in the shared crates. CI's conformance job runs it.

## Sites

Fixed in this change (shared crates):

| File | Test | Now |
|---|---|---|
| `crates/paddock-pitwall/tests/contract.rs` | `a_link_that_has_not_seen_pitwall_says_what_is_missing` (failed in run 6512) | `HeldPort::hang_up()` |
| `crates/paddock-pitwall/tests/contract.rs` | `the_server_gone_is_not_running_and_a_foreign_listener_is_a_protocol_error` | a refusing `Connector` |
| `crates/paddock-pitwall/src/loopback.rs` | `refused_garbage_and_header_injection` | `send_via` with a refusing `Connector` (port 1: it never connects) |
| `crates/paddock-pitwall/src/loopback.rs` | the cancel test ("already cancelled: no socket") | `HeldPort::hang_up()` |
| `crates/paddock-net/tests/it/real.rs` | `a_reset_connection_is_a_connect_error_...`, `a_proxy_that_resets_the_connection_...` | `HeldPort::reset()` |
| `crates/paddock-channels/tests/it/network.rs` | `a_reset_connection_is_a_connect_error`, `smtp_reset_by_the_server_is_a_connect_error` | `HeldPort::reset()` |
| `crates/paddock-download/tests/it/gaps.rs` | the closed-port case at the end | `HeldPort::reset()` |
| `crates/paddock-download/tests/it/http.rs` | `a_reset_connection_is_a_network_error_after_the_attempts`, the proxy that resets | `HeldPort::reset()` |

Left to each app, under one dated C39 waiver per file in `paddock.waivers.toml` (expires 2026-12-31), so a new freed
port in an app is still reported:

| App | Site | Note |
|---|---|---|
| Debrief | `crates/debrief-ai-worker/src/child.rs:174` `free_port()` | production: picks a port for `llama-server` and retries on a taken one; an inline waiver with that reason is enough |
| Debrief | `crates/debrief-ai-worker/src/http.rs:494`, `crates/debrief-ai/src/engine/pitwall/http.rs:454`, `crates/debrief-ai/tests/it/net_tls.rs:112` | refused: connector or mapping test |
| Pitcrew | `crates/notify-channels/src/tests.rs:379`, `:453` | refused: reset port or mapping test |
| Pitwall | `app/src-tauri/src/server_client.rs:491`, `crates/pitwall-cli/tests/cli.rs:272`, `crates/pitwall-loopback/src/lib.rs:316` | refused: connector or reset port |
| Spotter | `crates/spotter-sources/tests/http_sources.rs:163` | `HeldPort::reset()` |

## Consequences

- A test that wants "no server there" says which kind it needs: one that hangs up, one that resets, or a refusal. The
  first two are real ports that are held. The third never touches the network.
- `paddock-testkit` gains `socket2` (already in the lockfile through the workspace hack). `paddock-pitwall` gains a
  public `Connector` trait, `Tcp` and `send_via`. `send` and `send_as` keep their signatures.
- `paddock-net` and `paddock-channels` gain a dev-dependency on `paddock-testkit`.

## Risks

- C39 is a heuristic over masked text. Known gaps: a listener kept in a struct field or a collection and dropped
  later; a helper that takes the listener by value and returns only its port; a port read through `peer_addr` or
  `as_raw_fd` tricks; a socket2 socket whose port is read without a `let` binding. The kit's documentation and this ADR
  name the pattern, so a reviewer can catch what the check misses.
- The SMTP refusal mapping of paddock-channels (`mail::classify`) has no constructed-error test: lettre's error
  constructors are crate-private. Any network error falls through to `SendError::Connect`, the branch the reset-port
  test reaches.
- On some OS a reset port could produce an error the code maps differently from a refusal. The three net-stack crates
  map every I/O failure that is not a timeout or TLS to "connect", so the assertions hold. The changed tests ran 50 times
  in a row on macOS. Linux and Windows are covered by CI.
