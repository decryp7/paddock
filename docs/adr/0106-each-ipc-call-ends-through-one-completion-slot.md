# 0106. Each IPC call ends through one completion slot that its caller and its closer share

- Status: Accepted 2026-11-17 (user, relayed by the coordinator: "each pending call gets one completion slot that the caller
  and the closer share, so nothing can be registered after the close has fired", about 1 agent-day). Built on
  `refactor/ipc-call-completion-slot`, on top of the fix `fix/plugin-host-restart-flake` (4cb546db5, train 13).
- Date: 2026-11-17
- Related: ER-21 / rule 40 (a recurring class gets a redesign), ER-28, ER-37 / rule 76, rule 55 (`docs/known-copies.toml`,
  mechanism `ipc-frames-peer`); ADR 0031 and 0041 (limits on the injected clock), ADR 0072 (every connection end says why).
- Number: 0106. On 2026-11-17 the highest ADR on `origin/main` was 0103; 0104 is on `origin/docs/adr-safe-by-default-under-tests`
  and 0105 on `origin/feat/scroller-no-auto-scroll` and `local/train-14` (`git ls-tree docs/adr/` over every branch).

Evidence heads read (ADR 0091), re-read before the commit: `origin/main` 97176067c, `origin/train/2026-11-15-13` 1ee5d2be6,
`fix/plugin-host-restart-flake` 4cb546db5 (local; not pushed, its push waits on the Pitwall deny fix). Shipped code: none of the
commits cited below is in a tag (`git tag --contains` of 5528ea2b2, 123235de7, 381ca0d70, 5bdefa19d and 4cb546db5 is empty);
the code read is 4cb546db5's `crates/paddock-ipc/src/peer.rs`.

## Context: the class

A `paddock_ipc::Peer` connection ends in one of several ways (an answer, a failure, a close by either side, a frame that
breaks the protocol), and more than one thread may see the end (the caller, the reader, the writer, a second closer). Four
times in nine days a waiter missed an end, or saw one too early, because **the end was recorded in more than one place, at
different moments, and the waiter read a place the ender had not written yet (or had already left)**:

| # | Where | Evidence (checked) | What a waiter saw | Fixed by |
|---|---|---|---|---|
| 1 | Served calls and the write queue | CI run 7761, job 32105 (`crate (windows-latest)`, conclusion failure): `paddock-ipc::it peer::closing_fails_pending_calls_on_both_sides` FAIL; 1 of 2,000 on the Windows Arm VM | `close` cancelled the served calls before it shut the outgoing queue, so a handler's "cancelled" answer went out before the end: the caller got `Remote(cancelled)`, not `Closed` | 5528ea2b2, test 123235de7 (red 3 of 3 with the old order) |
| 2 | The close state | 1 of 336 in a paddock-ci:v2 Linux container run (local, not CI), `closing_ends_calls_that_were_still_waiting_in_the_queue`; 44 of 200 with the window widened | the close was visible as soon as its reason was recorded, before its calls were removed: `close` returned and a queued call still ran | 381ca0d70 (`Closing::Open/Started/Done`) |
| 3 | The close state, second closer and a failing close | review finding on 381ca0d70 (not an incident run); its tests failed 20 of 20 and at their 30 s guard with the defect put back | a second closer returned early; a close that unwound left `wait_closed` waiting for ever | 5bdefa19d (`DoneOnDrop`) |
| 4 | **Pending calls (this side's calls)** | CI run 8214, job 40588 (`crates`, Linux): `paddock-plugin-host::it faults::a_crashing_plugin_is_restarted_after_the_back_off_and_then_quarantined` TMT at 60 s, passed in 0.708 s on its retry; 1 of 78 crates jobs | the caller registered its wake-up through the pending map after the close had taken the call out, so the close cancelled an old token and the caller waited for its whole timeout (for ever on a fake clock) | 4cb546db5 (a patch at the look-up) |

Instance 4 was made by 381ca0d70 (the fix of instance 2): it moved the posting of the end out of the pending map's lock, and
the caller's wake-up lived in the map entry, a second place beside the event channel. 4cb546db5 patched the look-up, but the
shape stays: a wake-up token in the map entry and the event in an `mpsc` channel are two records of one end. One more hole of
the same shape was still open at 4cb546db5: a close that panics after taking the calls out drops their channel senders
without cancelling the token a caller already waits on, so that caller waits for its whole timeout (test
`a_close_that_panics_after_taking_the_calls_out_still_ends_a_waiting_caller`, red at 4cb546db5, below).

Instances 1 to 3 are on the served side and in the close state, which already have one record each since their fixes (each
served call's `Served { state: Mutex, cv }`, the one `Closing` state). This side's calls were the part without one.

## Decision

**Each call this side makes has one completion slot** (`crates/paddock-ipc/src/slot.rs`): a mutex over a queue of events, an
`ended` flag and the current wake-up token, plus a condition variable. The caller keeps `CallSlot`; the pending map keeps
`SlotSender` (inside `Pending`), which whoever ends the call uses: `deliver` (items, the answer, a failure), `mark_closed`,
`on_skipped`. The invariants, all under the slot's one lock:

1. **One completion.** After a last event nothing more is accepted: `post` returns false for a late answer after a close or a
   close after the answer (the `tokio::sync::oneshot` contract).
2. **No lost wake-up.** A wait on the clock takes its token from the slot while it sees the slot empty, under the lock every
   post takes, and every post cancels the token in the slot. A post is therefore either before the look (the wait takes the
   event) or after it (it cancels the token the wait holds). Nothing is looked up in the pending map to be woken.
3. **An end even without a post.** A `Pending` dropped without a last event (only a close that took the calls out and then
   failed does that) logs one line with the call id and the peer's label (ER-27) and ends the call with the reason the close
   recorded; `SlotSender`'s own drop puts `Closed("the connection is gone")` as the backstop, like a channel whose sender hung
   up. So a close that panics half way still ends every caller, with or without a time limit. A call its own caller gave up
   (timed out, a dropped stream, a send that failed) is abandoned quietly instead.

Waits stay on the injected clock (ADR 0031, ADR 0041): `take_within(clock, timeout)`; a wait without a limit blocks on the
slot's condition variable (`take`). Nothing else in the peer changes: the close order of 381ca0d70 (calls out of the maps,
the close reported done, then the ends posted) and the served side stay as they are; the slot only makes the order of the
posting irrelevant to the caller.

### Debrief's copy (rule 55)

`apps/debrief/crates/debrief-ipc/src/peer.rs` (registered copy of `ipc-frames-peer`, `delete_by = "debrief-v1.2.0"`) is
**deleted in favour of the shared crate, by Debrief 1.2 PH1 as already planned** (ADR 0031 line 406, ADR 0076 M5), not moved
onto the slot now. Why: its calls wait with `std::sync::mpsc::Receiver::recv_timeout` on the real clock, and its
`mark_closed` sends each call's end while it holds the pending map's lock (`for (_, p) in lock(&self.inner.pending).drain()`),
so a caller's channel is the one record of the end, which is the common practice below; it has no wake-up token to lose
(instance 4) and no injected clock to honour. Moving it would change a copy that PH1 deletes, for no defect measured in it.
Not measured: whether the copy has instance 2's shape (it sets `closed` before it drains the calls); ADR 0072's waiver (§9)
already covers its close order until PH1.

## Common practice (ER-37)

- **tokio's oneshot** (<https://docs.rs/tokio/latest/tokio/sync/oneshot/index.html>, read 2026-11-17): "A one-shot channel
  is used for sending a single message between asynchronous tasks", and "If the sender is dropped without sending, the
  receiver will fail with error::RecvError." Invariants 1 and 3 are this contract.
- **std's mpsc** (<https://doc.rust-lang.org/std/sync/mpsc/struct.Receiver.html>, read 2026-11-17): `recv` "will always block
  the current thread if there is no data available and it's possible for more data to be sent"; if the sender "disconnects
  while this call is blocking, this call will wake up and return Err"; "messages sent before the disconnect will still be
  properly received". One channel per call is the shape Debrief's copy uses; the end and the wake-up are one record.
- **gRPC's completion queue** (<https://grpc.io/docs/languages/cpp/async/> and
  <https://grpc.github.io/grpc/cpp/classgrpc_1_1_completion_queue.html>, read 2026-11-17): each operation carries a unique tag
  that "will be delivered out of the completion queue by a call to Next", and after `Shutdown` "Next will not return false
  until ... all pending tags have been drained": completions are delivered through the queue the waiter reads, never
  registered later on a side table.

All three give the waiter one place that both holds the end and wakes it. The slot is that with one addition the standard
channels cannot give: its timed wait runs on the injected clock (a `FakeClock` in tests), which `recv_timeout` cannot.

**Cost.** The slot as built: about 1 agent-day (this ADR 0.25, the slot and the peer change 0.25, tests and the stress runs
0.5). The cheaper common-practice option, offered as decision D1: keep 4cb546db5's patch and move the wake-up token and the
channel into one `Arc<Mutex<..>>` only when the next instance appears (0 days now), or a plain `std` channel per call posted
under the map lock as Debrief does (about 0.5 day, but every timed wait then leaves the injected clock, against ADR 0041,
and the plugin host's fake-clock tests would need real time). The slot costs about 0.5 day more than
the channel and keeps the clock; the user approved it (above).

## Decisions

- **D1** (taken by the user, 2026-11-17): the completion slot, not the patch alone and not a real-clock channel.
- **D2** (taken here): Debrief's copy is deleted by PH1, not moved now (above).

## Tests (each through the mechanism)

| Instance | Test | Red on old code |
|---|---|---|
| 1 | `paddock-ipc::it peer::closing_fails_pending_calls_on_both_sides` (the caller's `Closed` comes through its slot) and the lib test of 123235de7 | 3 of 3 with the old close order (123235de7) |
| 2 | `peer::tests::a_close_is_not_seen_until_it_has_ended_the_calls`, `closing_ends_calls_that_were_still_waiting_in_the_queue` | 44 of 200 widened (381ca0d70) |
| 3 | `a_second_close_returns_only_when_the_close_under_way_is_done_and_no_queued_call_runs`, `a_close_that_panics_while_ending_the_calls_is_still_reported_done`, new `a_close_that_panics_after_taking_the_calls_out_still_ends_a_waiting_caller` (the caller also gets the close's recorded reason) and new `a_call_ended_by_a_dropped_pending_is_logged_once_with_its_id_and_peer` (ER-27) | 20 of 20 (5bdefa19d); the panic test red when run on 4cb546db5 ("Err(Timeout) with 1 waiter(s) on the clock") |
| 4 | new `a_call_the_close_took_out_before_its_caller_waited_is_still_ended_by_that_close` | the new test was not run on old code (its `before_wait` seam does not exist there). What was red is 4cb546db5's own regression test, `a_call_the_close_took_out_before_its_caller_looked_it_up_is_still_woken_by_that_close`, at 1ee5d2be6 (its 30 s guard, one waiter left on the fake clock; 4cb546db5's message); the new test replaces it with the same order of events |
| slot | `slot::tests`: an end before the wait; an end after the wait took its token, before it blocks (the 8214 shape, the clock never moved); an end while blocked; one completion; a dropped sender ends a wait without a limit; a spent token is replaced so the next wait waits | new mechanism |

Stress, on the slot: the e2e test of instance 4 and the two new peer tests, counts in the commit (Linux CI-image container with
`ulimit -n 1024`, and macOS).

## Risks

- A blocking `take` without a time limit now relies on invariant 3 (a dropped sender ends it) instead of the channel's
  disconnect. Every `Pending` holds a `SlotSender`, and `Pending` leaves the map only by drop, so the end always comes.
- The served side and the close state keep their own records (fixed in instances 1 to 3); a fifth instance there would extend
  this ADR to a served-call slot.
