---
title: Test isolation
verified: 2026-10-31
sources: [docs/lessons.md, rule 49, ADR 0031, handoff/merge-steward.md]
---
Why UI specs time out together, what guards exist and where they are. Rule 49 is the authority for the spec rules and is linked,
not restated. Status as of origin/main `233c1339b`.

## Current state
- The Angular unit-test builder runs Vitest with `isolate: false`, so specs of one bundle share one page: anything a spec leaves behind
  reaches the next file. Source: [rule 49](../rules.md#test-isolation-user-2026-10-29); [lesson, Test isolation](../lessons.md#test-isolation-debrief-2026-10-29).
- The timer-leak class: a spec that spies on `setTimeout` under fake timers left a dead fake clock, and later files hung at the 5000 ms
  default (first seen as `reference-app.spec.ts` health.repair timeouts and the panel-snapshot timeouts, runs 7274 and 7276). The
  cause was `settings-close.spec`, which exists only on the Pitwall line s23b. Source: src: `handoff/merge-steward.md` (entries stamped 2026-11-01 02:10, 03:45 and 04:00, written on 2026-10-31).
- The guard is a shared test-kit timer guard at `bb10e52a3` plus a spec fix that counts timers with `getTimerCount`; it is on the
  branches `fix/panel-snapshot-spec-cost` and `steward/s23b-pitwall-1.1`, not on main. The review of `bb10e52a3` returned CHANGES with a P1 (a throwing
  `afterEach` skipped TestBed teardown) and went back to the author; s23b `cf4262769` merged the kit head of that moment. Source: src: `git branch -r --contains bb10e52a3`; src: `handoff/merge-steward.md` (entries stamped 2026-11-01 04:25 and 04:55).
- main is not affected by that exact leak, because s26b has no `settings-close.spec`; the panel-snapshot fix `d5eda8680` was approved
  but was not the cause (pipeline 1 to 4 ms). Source: src: `handoff/merge-steward.md` (entries stamped 2026-11-01 02:50 and 04:00).
- Earlier members of the same family: the lazy-dialog wait counted event-loop turns (fixed by waiting on the condition) and a prototype
  patch that was not restored. Source: [lesson, Test isolation](../lessons.md#test-isolation-debrief-2026-10-29).
- Rules 57 and 58 (the root-cause method rules the user decided on 2026-10-31 22:05, with ER-28 and ER-29) are not in `docs/rules.md` on
  main and no origin branch with their text was found when this page was written; ER-28 and ER-29 are on engineering-rules main.
  Source: src: `handoff/merge-steward.md` (entry of 2026-10-31 22:05, 22:40); src: `git branch -r` (no `rca` branch).
- Wait on conditions in tests, never on a count of turns or a fixed time: ADR 0031 is the design for waits in the product and tests. Source: [ADR 0031](../adr/0031-patience-not-wall-clocks.md).

## See also
[ci-and-runners](ci-and-runners.md), [process-starts](process-starts.md), [open-decisions](open-decisions.md).
