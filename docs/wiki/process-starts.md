---
title: Process starts
verified: 2026-10-31
sources: [ADR 0031, ADR 0033, ADR 0029, rule 53, crates/paddock-proc/src/lib.rs, conformance/README.md, docs/lessons.md, handoff/merge-steward.md]
---
How a host judges a child's start (the start judge), where the check C51 stands, which check numbers are taken, and where process
identity is decided. Status as of origin/main `233c1339b`. Steward-log citations are `merge-steward.md:<line>`.

## Current state
- ADR 0031 decided how a start is judged (run state, CPU time, progress, a ready handshake) instead of a fixed wait. Source: [ADR 0031](../adr/0031-patience-not-wall-clocks.md).
- The implementation is `paddock_proc::StartJudge` and `StartWatch`, with `InstanceLock` (one instance per folder by a held OS lock)
  and `hint` (a discovery file is only a hint). Source: [paddock-proc lib.rs](../../crates/paddock-proc/src/lib.rs).
- The shared crate landed as `feat/paddock-proc-start-judge` (final head `d79ece0b8`, `merge-steward.md:355`); the chromium driver adopted it in
  `fix/chromium-start-judge`, approved at `merge-steward.md:682` and on main since `233c1339b` (`merge-steward.md:707`). Source: src: `merge-steward.md:355, 682, 707`.
- Why a fixed start window failed: Dyno doubled its agent on a cold disk, and a wait without a time limit still needs a visible cause and Retry. Source: [lesson, a start window on a wall clock](../lessons.md#a-start-window-on-a-wall-clock-doubled-dynos-agent-on-a-cold-disk-2026-10-30-dyno).
- The Debrief chromium driver kept the old fixed wait because a fix in the canonical implementation never reached its copy. Source: [lesson of 2026-10-31](../lessons.md#the-fixed-start-wait-hid-in-a-helper-named-connect-2026-10-31-debrief-chromium-driver); [rule 55](../rules.md#copies-user-2026-10-31-adr-0034-rule-40).
- C51 is report-only on main (warnings; 45 at first, `merge-steward.md:337`). The shape-based version `conformance/c51-polish` (`fcbef6e7c`, 21
  findings, warn) is approved and waits for the next tools batch, so it is not on main. Source: [conformance README, C51](../../conformance/README.md); src: `merge-steward.md:337, 711`.
- Real findings of the wider C51 still open: Spotter `install.rs` self test (asked of the Spotter lead) and four Debrief items
  (sessions `WAIT_FOR_OTHER`, event pipe connect, install self test, opener) for Debrief 1.2. Source: src: `merge-steward.md:698`.
- The shell opens the start, not the app: ADR 0029, with check C50 on main. Source: [ADR 0029](../adr/0029-the-shell-owns-the-start.md); [conformance README, C50](../../conformance/README.md).
- Process identity: [ADR 0033](../adr/0033-processes-by-identity-never-by-a-bare-pid.md) is the design, [rule 53](../rules.md#process-identity-user-2026-10-30) the rule, check C53 the enforcement. Source: [conformance README, C53](../../conformance/README.md).
- The Windows `ERR_ABORTED` flake was not pid reuse; it was the Chrome for Testing network-service sandbox, fixed on main. See [ci-and-runners](ci-and-runners.md). Source: [ADR 0033 header](../adr/0033-processes-by-identity-never-by-a-bare-pid.md); [lesson of 2026-10-30](../lessons.md#read-the-failing-environments-own-logs-before-building-a-reproduction-2026-10-30).
- Conformance check numbers. On main: C50, C51, C53, C54, C56, C57 (and the older ones). Claimed elsewhere: C49 provider-grants (Spotter line); C52
  Pacenotes cargo-metadata; C55 windowed test browser (live-browser branch); C58 copies (rule 55, follow-up); C59 updater guard; C60 busy-wait-read
  (`conformance/busy-wait-read`); C61 the Material bridge (`fix/ui-settings-density`, Pitcrew p8, s28); C62 pipefail-grep (setup-tools hardening); C63 to
  C65 logging (ADR 0036). C66 is the first free number; check the branches before taking it. Source: src: `merge-steward.md:150, 155, 566, 600, 641, 712`; src: coordinator note of 2026-10-31 (C49, C61, C66); [conformance README](../../conformance/README.md).

## See also
[test-isolation](test-isolation.md), [ci-and-runners](ci-and-runners.md), [open-decisions](open-decisions.md).
