---
title: Test isolation
verified: 2026-10-31
sources: [docs/lessons.md, rule 49, rule 57, rule 58, ADR 0031, handoff/merge-steward.md]
---
Why UI specs time out together, what guards exist and where they are. Rule 49 is the authority for the spec rules and is linked,
not restated. Status as of origin/main `233c1339b`. Steward-log citations are `merge-steward.md:<line>`.

## Current state
- Specs of one Angular bundle run in one page (Vitest with `isolate: false`), so what one spec leaves behind reaches later files; the
  rules for specs are [rule 49](../rules.md#test-isolation-user-2026-10-29) and the incident is [lesson, Test isolation](../lessons.md#test-isolation-debrief-2026-10-29).
- The timer-leak class: a spec that spied on `setTimeout` under fake timers left a dead fake clock, and later files hung at the 5000 ms
  default. The panel-snapshot timeout was first seen in run 7257 (`merge-steward.md:614`) and recurred in run 7274 (`merge-steward.md:673`);
  `reference-app.spec.ts` timed out in run 7276 (`merge-steward.md:683`). The cause was `settings-close.spec`, which exists only on the
  Pitwall line s23b (`merge-steward.md:685`). Source: src: `merge-steward.md:614, 673, 683, 685`.
- The guard is a shared test-kit timer guard plus a spec fix that counts timers with `getTimerCount`. The first head `bb10e52a3` came back
  CHANGES (`merge-steward.md:691`). The current head is `d459854a6` (it uses `onTestFinished`), only on `origin/fix/panel-snapshot-spec-cost` and in
  review; s23b (`cf4262769`) still carries the earlier `bb10e52a3`. None of it is on main. Source: src: `git branch -r --contains d459854a6` (one branch); src: `merge-steward.md:688, 691`.
- main is not affected by that exact leak, because s26b has no `settings-close.spec` (`merge-steward.md:685`); the panel-snapshot fix `d5eda8680` was
  approved but was not the cause (`merge-steward.md:678`). Source: src: `merge-steward.md:678, 685`.
- Earlier members of the same family: the lazy-dialog wait counted event-loop turns and a prototype patch was not restored. Source: [lesson, Test isolation](../lessons.md#test-isolation-debrief-2026-10-29).
- Rules 57 and 58 are on `origin/docs/rca-rules` at `9fba4291f`, and the CI flaky-is-red change is `origin/ci/flaky-is-red` at `10ab50d10`; both are in
  review and neither is on main. The user decided them with ER-28 and ER-29, which are on engineering-rules main. Source: src: `git show origin/docs/rca-rules:docs/rules.md`; src: `merge-steward.md:710, 713, 716`.
- Waits in the product and in tests are designed in [ADR 0031](../adr/0031-patience-not-wall-clocks.md).

## See also
[ci-and-runners](ci-and-runners.md), [process-starts](process-starts.md), [open-decisions](open-decisions.md).
