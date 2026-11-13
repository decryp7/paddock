# 0102. A landing push takes the heavy slot first, and its budget counts run time

Status: accepted by the user on 2026-11-13 (the redesign was approved in the brief; this records it). Rules: ER-24, ER-12, ER-27,
ER-33, ER-37.

## Problem

Landing pushes (trains 8 and 9, WP-10) only passed the 15-minute pre-push gate after the coordinator paused every other agent by
hand, three times on 2026-11-13. The gate's steps (clippy, xwin, Linux) queue for the machine-wide heavy slot behind other agents'
builds, and the 15 minutes counted that wait, so a push could be "not measured" because other agents were busy. ER-24 says tests
never crowd out a release; nothing enforced it.

## Decision

1. **Priority lane.** `heavy run --high` already makes a run announce itself with a marker and go first; normal runs yield while a
   live marker exists. The pre-push gate now passes `--high` for its heavy steps when the pushed remote ref is `refs/heads/train/*`,
   `refs/heads/main` or a release tag (`vX.Y.Z`, `<app>-vX.Y.Z`; `budget::is_landing_ref`). Running steps are never killed. With
   several slots (opt-in) the marker now stays until the priority run releases, so a free slot is not taken beside it; with one slot
   the lock itself does that and the marker goes when the lock is taken, as before and as the shell plock does.
2. **The lane is bounded.** A priority marker older than `HEAVY_PRIORITY_MAX` (1800 s), or a normal waiter that has itself waited
   that long, stops yielding, with a `heavy.priority` warning in the log and a line on stderr. So a hung priority run, or a stream
   of landings, cannot hold everyone for good. 1800 s is twice the 15-minute run budget of the gate that holds the lane: a healthy
   priority run is always inside it. It is derived from that budget, not yet measured; the queue times now recorded (below) are the
   measurement to revisit it with (ER-12: a new limit, with its named basis, none raised).
3. **Budget counts run time.** `heavy` takes `--queue-max S` (no slot in S seconds: exit 79, the command never ran) and
   `--run-max S` (the command is ended after S seconds of running: exit 80), and prints `<agent> timing queued Q s, ran R s` for
   every run (queue and load gate, then the run). The gate reads that line (`budget::parse_heavy_timing`) and `BudgetClock` credits
   the time a step queued back to the 15-minute budget, except the part in which a sibling gate of the same push held the slot (that
   is the push's own run time). The table and the JSON show both per step (`3.0 s run, 12.0 s queued`; `run_secs`, `queued_secs`).
4. **Never a pass without a slot.** A step that got no slot within `PADDOCK_QUEUE_MAX` (default 1800 s) is "not measured" with its
   queue time and the next step (`heavy status`); the push is blocked (exit 6), never passed (ER-33).

## Common practice (ER-37)

- Buildkite agents' job priority and the Jenkins Priority Sorter plugin order a queue by priority and do not preempt a running job:
  the same shape as 1. GitLab's merge trains and Zuul's gate pipeline (which outranks the check pipeline) put what is about to land
  ahead of speculative runs. GitHub Actions has concurrency groups only (cancel or queue, no priority). None bounds the priority;
  starvation protection is by aging in Jenkins' sorter and in most schedulers (Slurm's age factor), which 2. imitates with a hard bound.
- Queue time is reported apart from run time by Buildkite (wait time vs run time) and GitHub Actions (queued vs in progress); job
  timeouts count run time only. 3. does the same for the push gate.

## Cost, and the cheaper options

- Do nothing: the coordinator pauses agents by hand; no build cost, about 3 interventions and about 15 minutes of everyone's time a day
  on a landing day, and the gate stays flaky for landings.
- Only pass `--high` (about 0.25 agent-day): a landing goes first, but the 15 minutes still count queue time behind a running step,
  and nothing bounds the lane.
- This design: about 1 agent-day (heavy 0.4, pre-push 0.4, ADR and checks 0.2). It is the one asked for; the cheaper option is the
  second one above.

## Consequences

- `tools/heavy`: `HEAVY_PRIORITY_MAX`, `--queue-max`, `--run-max`, exit codes 79 and 80, the timing line. The shell plock does not
  know the bound or the limits; it ignores the new flags it never receives (mixed use is unchanged: the marker format is the same).
- `tools/paddock-dev/src/pre_push`: `budget.rs` (pure), `exec.rs` (the flags, the credit), `mod.rs` (the ref decision, the table).
- Train 10 changes `pre_push/mod.rs`'s base choice; the overlap is the `RealExec { .. }` literal (two new fields) and `render`.
