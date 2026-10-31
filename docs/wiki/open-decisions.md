---
title: Open decisions
verified: 2026-10-31
sources: [handoff/merge-steward.md, handoff/windows-runner-update-policy.md, handoff/research-llm-wiki-2026-10-31.md]
---
Decisions and open items that the steward log and the hand-offs still show as undecided or unfinished on 2026-10-31. The
authority for each is the linked ADR, rule or log entry; this page only says where things stand. Re-check against the log
before acting on any line.

## Current state
- Offline mode: the user wants a shared Offline mode in the current releases and a designer drafts `docs/adr-offline-mode`; the log noted
  that no production code calls the egress `set_offline`, so the user-facing mode may not exist in the apps yet. Source: src: `handoff/merge-steward.md` (entries of 2026-10-31 13:53 and 14:29).
- ADR 0036 (logging for root cause): accepted on its branch, not yet on main; rule 56 and the Dyno logging set D1 to D7 are the first
  steps. See [logging-and-diagnostics](logging-and-diagnostics.md). Source: src: `git show origin/docs/adr-logging-root-cause` (commit `021dc3c8d`).
- Rules 57 and 58 (root-cause method) and the CI "flaky is red" change: decided by the user on 2026-10-31 22:05, text not found on an origin branch.
  See [test-isolation](test-isolation.md). Source: src: `handoff/merge-steward.md` (entry of 2026-10-31 22:05).
- Dyno 1.0 residuals the login verdict left open: the 30 s connect latency (cause unknown beyond the fixed SMAppService route), a
  failed save keeping an unsaved toggle, and a blank window after a web process kill. See [releasing](releasing.md). Source: src: `handoff/merge-steward.md` (entries of 2026-10-31 05:50 and 11:27).
- Widget retarget across real Tauri webviews (BroadcastChannel) is unverified outside tests and is on the Dyno VM checklist. Source: src: `handoff/merge-steward.md` (entry of 2026-10-31 20:50).
- Debrief 1.2 carry-overs from the wider C51: four fixed-wait sites to move onto the start judge. See [process-starts](process-starts.md). Source: src: `handoff/merge-steward.md` (entry stamped 2026-11-01 06:00).
- Follow-ups after the tags: a uniform wire shape for `Progress::Unknown`, the shared quit title, a tray route through the shell queue, and an
  endpoint-leak ADR for Debrief 1.2 (`debrief_ipc` registering its own `atexit` removal). Source: [lesson of 2026-10-31](../lessons.md#the-fixed-start-wait-hid-in-a-helper-named-connect-2026-10-31-debrief-chromium-driver); src: `handoff/merge-steward.md` (entries of 2026-10-30 21:52 and 22:20, 2026-10-31 00:30).
- About 1000 leaked test temp folders sit in the macOS TMPDIR from old runs; the sweep was blocked by the tool's classifier and the user is to be asked. Source: src: `handoff/merge-steward.md` (entry of 2026-10-31 23:25).
- Windows CI PC: the policy that keeps optional and preview updates off, and a report to Microsoft about the listener state change, were
  prepared and not applied. Source: src: `handoff/windows-runner-update-policy.md` (header).
- Engineering rules catch-up: a rules-sync pass should bring the general Paddock rules and lessons (49 to 55, the heavy lock tool, cleanup paths,
  busy-wait loops, the timer leak) into engineering-rules after ER-27 landed. Source: src: `handoff/merge-steward.md` (entry stamped 2026-11-01 06:15).
- The wiki itself: ER-30 is added to engineering-rules only after this pilot passes (pass: at least 8 of 10 eval questions right and sourced, fewer
  tokens and less time, and no case where an agent followed a wiki page against a rule or ADR). Source: src: `handoff/research-llm-wiki-2026-10-31.md` (section 4); [eval](eval.md).

## See also
[ci-and-runners](ci-and-runners.md), [releasing](releasing.md), [index](index.md).
