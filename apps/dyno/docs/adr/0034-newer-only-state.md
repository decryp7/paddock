# 34. Newer-only state: what is read under a lock is stored or pushed only if nothing newer was stored

- Status: Accepted (2026-11-19). Approved: user, 2026-11-19 (the redesign, the P1 and P2 fixes below and the cost).
- Date: 2026-11-19
- Related: [0027](0027-helper-for-1-1-0.md) (the helper's epoch), [0029](0029-saved-views-disk-map-cache.md) (the cache writer),
  [0031](0031-network-threat-lists-world-map-and-firewall-view.md) (lists and index), [0032](0032-power-keep-awake-wake-on-lan-idle-actions-energy.md)
  (power, Wake-on-LAN, wakes), engineering rule ER-21 (a class seen twice gets a redesign).

## Context

The same defect keeps showing up in the agent: state is read under a lock, the lock is let go, and the code later acts on it or
stores it, so a concurrent caller sees or applies state that is already stale. The review of the bulk-API branch and the power flake
of train 14 (run 8283) found it again; a sweep of the agent found 12 more places. Each was fixed (or would have been) as an instance;
the class needs one mechanism and a review line.

Instances (the two already fixed are in `power.rs`):

| # | Where | What went stale |
| --- | --- | --- |
| 0a | `PowerCentre::idle_tick` | the due timed entry was read, the lock let go, and the entry noted as run later (run 8283); fixed: one hold of the core lock |
| 0b | `PowerCentre::configure_idle` | the settings were read under the core lock and given to the idle machine after it; the older read could be applied last; fixed: one hold |
| 1 | `cache/mod.rs` `save`, `put_view` | `enabled` was checked by the caller; a save queued after `set_settings(off)` and its `ForgetAll` wrote a row |
| 2 | `IntelCentre::rebuild` | the index was built from lists read at the start; an edit during the build was lost because the job start answers the running job |
| 3 | `SaveSettings` | a window's whole copy of the settings overwrote what another window or request changed |
| 4 | `SaveSettings` effects | two saves could interleave their effects (disk limits, saved maps, idle machine, wakes) |
| 5 | `UnusualCentre::look` against `forget` | the look observed, the forget cleared the store, the look then stored rows of a baseline that was gone |
| 6 | `alarm_event`, `network::record` | the alarm changes were pushed after the core lock was let go; an older change could arrive last |
| 7 | power views | a view read earlier was pushed later than a newer one |
| 8 | `Inputs.process_alive` | read for the process the hold watched, fed after the hold had changed to another |
| 9 | `WolCentre::changed` | the list was read, then pushed after a newer change had pushed its own |
| 10 | `FirewallCentre::epoch` | waited for the lock a running read holds for its whole length, so `view()`'s 2 s bound did not hold |
| 11 | `ReadyGate::store` | the answer of a probe that began earlier replaced that of one that began later |
| 12 | `PageSweeps::for_page`, `request_reread` | two first pages kept the sweep that finished last; a re-read asked for while one ran was dropped, and one spawn bypassed the guard |

## Common practice

- **Versions and generations.** HTTP's `If-Match` with an entity tag (RFC 9110 section 13.1.1, a precondition that is checked
  against the current representation, answered 412 when it does not hold) is the standard form of "apply only if nothing newer".
  Databases do it as optimistic concurrency (a version column), Kubernetes as `resourceVersion`, etcd as revisions.
- **A single owner.** The actor model and a single owner thread (Redux-style stores, Chromium's sequenced task runners) let no one else
  hold state to be stale about.
- **Rerun if dirty.** Build systems and debouncers (a watcher, `make`) that get a request while one run is going mark it dirty and
  run once more when it ends, instead of dropping the request or running two at once.

## Existing guards in this repository

The class is already solved where someone met it first, each time with its own copy:

- `VolumesSlot` tickets: a volume list put with an older ticket than the newest is refused (`agent.rs`).
- `space_ticket` in the disk centre: the same for the free-space reading.
- The firewall epoch: the helper's readiness moves it, and a kept view of an older epoch is read again.
- `ReadyGate` generation: a probe that was in flight during a `forget` does not store its answer.
- `WakeCentre::exclusive`: one scheduled-wake change at a time, read through push.

## Decision

1. **One mechanism**, `dyno_core::newer`: a `Generation` counter (`next()` is greater than every earlier number) and `NewerSlot<T>`
   whose `put(generation, value)` keeps the value only when no newer generation was stored before (it remembers the newest generation
   after a `take`, so a late older value is refused then too). A generation is taken **in the same hold of the lock that reads the
   state**, so the order of the generations is the order of the states whichever thread stores or pushes last.
2. `LatestSlot`, `AlarmSlot` and `VolumesSlot` are built on it. An alarm change carries a `ticket` the alarm centre takes when the
   change is stored (not on the wire). Power views take their generation with both machines' locks held (`view_stamped`); the
   Wake-on-LAN list takes it in the hold that reads the list.
3. Where state is **installed** rather than pushed, the same slot is used (the intel index with the revision of the lists it was built
   from) and a build whose inputs moved on builds again.
4. Where a value is checked and then acted on, the check moves into the owner of the order (the cache writer reads `enabled` when it
   pops a job) or the lock is held across both (the unusual look, the settings apply mutex, the firewall's atomic).
5. A user-visible overwrite is refused, not merged: `Settings.revision` and `SaveSettings.base_revision`, answered `Stale`, as the
   widget layouts' `base_revision` already is. The Settings window reloads and says so. The alert rules stay agent-owned.
6. Protocol: `Settings.revision` and `SaveSettings.base_revision` are in version 8, which is unreleased (no further bump).

Each fix started with a test that fails without it, through a seam in the style of `scan_with` (`save_with`, `rebuild_with`,
`alarm_event_with`, `changed_with`, `look_with`): the seam runs the other caller at the moment between the read and the act.

## Review checklist line

Added to the review checklist of the agent: "State read under a lock and then stored, pushed or acted on after the lock was let go:
does it carry a generation (a `NewerSlot` or a ticket), is the read and the act one hold, or is the check made by the one thread that
orders the work? A new `Mutex` around a read-then-act needs a seam test that runs the other caller in between."

## Consequences

- The class is one mechanism with unit tests (including the positive control: an older generation put after a newer one is refused)
  plus one seam test per instance.
- Cost: about 1 to 1.5 agent-days for the mechanism and the 3 P1 and 9 P2 fixes through it, about 1 day beyond fixing each instance
  alone (approved with the decision; ER-37: the baseline is the per-instance fix, the common practice above is the same idea).
- A value pushed with a generation costs a counter and a compare; nothing waits for anything new.
- Not done here: a single-owner thread for the agent's state (a rewrite of the agent's locking, far beyond 1 agent-day); the
  mechanism removes the stale pushes without it.
