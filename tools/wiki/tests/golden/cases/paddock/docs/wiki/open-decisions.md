---
title: Open decisions
verified: 2026-10-31
sources: [handoff/merge-steward.md, handoff/windows-runner-update-policy.md, handoff/research-llm-wiki-2026-10-31.md]
---
Decisions and open items that the steward log and the hand-offs still show as undecided or unfinished on 2026-10-31, and a short
list of things that look open but are settled. The authority for each is the linked ADR, rule or log line; this page only says where
things stand. Re-check against the log before acting on any line. Citations are `merge-steward.md:<line>`.

## Settled (do not re-derive)
- Offline mode: decided against. There is no shared Offline mode; the user removed Pitwall's, and the removal is approved (final head
  `2e7745e5b`). "Local first" means the AI models run locally. Source: src: `merge-steward.md:560, 561, 586, 672`.
- ADR 0036 (logging for root cause) is approved at `021dc3c8d` and queued in the docs batch. See [logging-and-diagnostics](logging-and-diagnostics.md). Source: src: `merge-steward.md:715`.
- The Windows `ERR_ABORTED` cause is known and fixed. See [ci-and-runners](ci-and-runners.md). Source: src: `merge-steward.md:188, 210`.

## Current state
- Rules 57 and 58 (root-cause method) and the CI flaky-is-red change are written (`origin/docs/rca-rules` at `9fba4291f`, `origin/ci/flaky-is-red` at
  `10ab50d10`) and in review. See [test-isolation](test-isolation.md). Source: src: `merge-steward.md:710, 716`.
- Dyno 1.0 residuals the login verdict left open: the 30 s connect latency (cause unknown beyond the fixed SMAppService route), a
  failed save keeping an unsaved toggle, and a blank window after a web process kill. See [releasing](releasing.md). Source: src: `merge-steward.md:510, 697`.
- Widget retarget across real Tauri webviews (BroadcastChannel) is unverified outside tests and is on the Dyno VM checklist. Source: src: `merge-steward.md:640`.
- Debrief 1.2 carry-overs from the wider C51: four fixed-wait sites to move onto the start judge. See [process-starts](process-starts.md). Source: src: `merge-steward.md:698`.
- Follow-ups after the tags: a uniform wire shape for `Progress::Unknown` (`merge-steward.md:664`), the shared quit title (`merge-steward.md:304`), a tray route through
  the shell queue (`merge-steward.md:237`), and an endpoint-leak ADR for Debrief 1.2 so `debrief_ipc` registers its own `atexit` removal (`merge-steward.md:692`). Source: src: `merge-steward.md:237, 304, 664, 692`.
- About 1000 leaked test temp folders sit in the macOS TMPDIR from old runs; the sweep was blocked by the tool's classifier and the user is to be asked. Source: src: `merge-steward.md:657`.
- Windows CI PC: the policy that keeps optional and preview updates off, and a report to Microsoft about the listener state change, were
  prepared; their application is not recorded. Source: src: `windows-runner-update-policy.md` (header).
- Engineering rules catch-up: a rules-sync pass should bring the general Paddock rules and lessons (49 to 55, the heavy lock tool, cleanup paths,
  busy-wait loops, the timer leak) into engineering-rules after ER-27 landed. Source: src: `merge-steward.md:700`.
- The wiki itself: the user concluded the pilot and made the wiki official: ER-30 and `skills/wiki` are on engineering-rules `docs/er-30-wiki`, and
  Paddock rule 59 is on `docs/rule-wiki`; neither is merged yet. The wiki tooling is to become one Rust `wiki` CLI (branch `tools/wiki`) and the Node lint
  retires after parity. Until then `scripts/wiki-lint.mjs` stays the check. Source: src: `merge-steward.md:719, 721`; [eval](eval.md) (kept as an optional measure).

## See also
[ci-and-runners](ci-and-runners.md), [releasing](releasing.md), [index](index.md).
