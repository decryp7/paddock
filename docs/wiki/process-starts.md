---
title: Process starts
verified: 2026-10-31
sources: [ADR 0031, ADR 0033, ADR 0029, rule 53, crates/paddock-proc/src/lib.rs, conformance/README.md, docs/lessons.md, handoff/merge-steward.md]
---
How a host judges a child's start (the start judge), where the check C51 stands, and the identity rules for ending processes.
Status as of origin/main `233c1339b`.

## Current state
- ADR 0031 decided that a start is judged on the child's run state, CPU time, progress and a ready handshake, not on a fixed
  wait, and that a paused process is waited for and said so. Source: [ADR 0031](../adr/0031-patience-not-wall-clocks.md).
- The implementation is `paddock_proc::StartJudge` and `StartWatch`, with `InstanceLock` (one instance per folder by a held OS lock)
  and `hint` (a discovery file is only a hint). Source: [paddock-proc lib.rs](../../crates/paddock-proc/src/lib.rs).
- The shared crate landed as `feat/paddock-proc-start-judge` (final head `d79ece0b8`); the chromium driver adopted it in
  `fix/chromium-start-judge`, merged into s26b and on main since `233c1339b`. Source: src: `handoff/merge-steward.md` (entries of 2026-10-31 00:07, 03:30 and 21:30).
- Why a fixed start window failed: Dyno doubled its agent on a cold disk after a fixed 5 s wait, and the follow-up adds that a wait
  without a time limit must show a cause and a Retry. Source: [lesson, a start window on a wall clock](../lessons.md#a-start-window-on-a-wall-clock-doubled-dynos-agent-on-a-cold-disk-2026-10-30-dyno).
- A fix in a canonical implementation must reach its registered copies: the Debrief chromium driver kept the old fixed wait because
  the fix did not reach the copy. Source: [lesson of 2026-10-31](../lessons.md#the-fixed-start-wait-hid-in-a-helper-named-connect-2026-10-31-debrief-chromium-driver); [rule 55](../rules.md#copies-user-2026-10-31-adr-0034-rule-40).
- C51 is report-only (warnings) and finds fixed waits inside functions named like a start. It reported 45 warnings at first; the wider
  shape-based version `conformance/c51-polish` (`fcbef6e7c`, 21 findings, warn) is approved and waits for the next tools batch,
  so it is not on main. Source: [conformance README, C51](../../conformance/README.md); src: `handoff/merge-steward.md` (entries of 2026-10-31 23:07 and 22:10).
- Real findings of the wider C51 that are still open: Spotter `install.rs` self test (asked of the Spotter lead) and four Debrief items
  (sessions `WAIT_FOR_OTHER`, event pipe connect, install self test, opener) for Debrief 1.2. Source: src: `handoff/merge-steward.md` (entry stamped 2026-11-01 06:00).
- The shell opens the start, not the app: ADR 0029, with check C50 on main. Source: [ADR 0029](../adr/0029-the-shell-owns-the-start.md); [conformance README, C50](../../conformance/README.md).
- Processes are named by identity (pid plus start time) and ended through `paddock_proc`; ADR 0033 is the design, rule 53 the rule,
  check C53 the enforcement. Pid reuse was disproved as the cause of the Windows `ERR_ABORTED` flake. Source: [ADR 0033](../adr/0033-processes-by-identity-never-by-a-bare-pid.md); [rule 53](../rules.md#process-identity-user-2026-10-30); [conformance README, C53](../../conformance/README.md).
- Which conformance numbers are taken: C49 was left free by ADR 0031 and has no check; C52 and C55 were assigned to branches; C58 is the copies
  check named in rule 55 (follow-up); C59 is the updater guard and C60 another check, both on branches; C62 and C63 to C65 are claimed by the tooling
  and logging work. The log shows no claim on C49 or C61, and none above C65, so C66 is the first number past every claim; check the branches before taking any. Source: src: `handoff/merge-steward.md` (entries of 2026-10-30 NUMBERING, 2026-10-31 08:00 to 09:25, 21:05 and 22:20); [ADR 0031](../adr/0031-patience-not-wall-clocks.md) (header).

## See also
[test-isolation](test-isolation.md), [ci-and-runners](ci-and-runners.md), [open-decisions](open-decisions.md).
