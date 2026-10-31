---
title: Logging and diagnostics
verified: 2026-10-31
sources: [crates/paddock-log/src/lib.rs, crates/paddock-diag/src/lib.rs, rule 56, ADR 0036, handoff/merge-steward.md]
---
What the two shared logging crates do on main, what is only on a branch (rule 56, ADR 0036), and the work packages that move the
apps onto them. Status as of origin/main `233c1339b`.

## Current state
- `paddock-log` is the shared app and service log: one CLEF JSON-lines file per process per day in the OS log folder, `tracing`
  and `log` in one stream, a redactor, error dedupe once per minute, breadcrumbs, and a crash marker with a panic hook. Source: [paddock-log lib.rs](../../crates/paddock-log/src/lib.rs).
- `paddock-diag` builds the Report-a-problem zip that the user reviews file by file and saves where they choose; it has no network
  dependency. Source: [paddock-diag lib.rs](../../crates/paddock-diag/src/lib.rs).
- Rule 56 (logs explain every failure) is not in `docs/rules.md` on main. It is on branch `docs/rule-logs-explain-failures` at `ef8f52349`
  (all AGENTS copies, a block and a lesson), approved by the steward and queued for the next shared batch. Source: src: `git show origin/docs/rule-logs-explain-failures:docs/rules.md` (line 284); src: `handoff/merge-steward.md` (entries of 2026-10-31 06:35 and 21:30).
- ADR 0036 (logging for root cause) is on branch `docs/adr-logging-root-cause` at `021dc3c8d` with status Accepted dated 2026-10-31; it is not on main.
  Source: src: `git show origin/docs/adr-logging-root-cause:docs/adr/0036-logging-for-root-cause.md`. The steward log's last mention still said the ADR needed a dependency-cycle fix before acceptance; the commit message of the branch head says accepted. Source: src: `handoff/merge-steward.md` (entry of 2026-10-31 22:55).
- The audit behind the ADR found that no shipped app's bundle could find a root cause from its logs; the user decided: Dyno gets
  D6 in 1.0, a 20 MB, 7 day, 4 MB bundle budget, Info as the default level, and each app replaces its own logger before its next tag. Source: src: `handoff/merge-steward.md` (entry of 2026-10-31 21:45).
- ADR 0036 lists work packages WP-1 to WP-11 (dedupe, OS banner, proc and ipc events, jobs events, 4 MB log window WP-6a, secret scanner,
  conformance checks C63 to C65, per-app adoption WP-9, cost measurement WP-10, merging rule 56 first WP-11). Source: src: `git show origin/docs/adr-logging-root-cause:docs/adr/0036-logging-for-root-cause.md` (Work packages).
- The user also asked for logs for CI and tooling failures; rule 56 and ER-27 were extended, and a CI diagnostics audit was started.
  ER-27 to ER-29 are on engineering-rules main at `a8064e1`. Source: src: `handoff/merge-steward.md` (entries of 2026-10-31 06:40 and 22:40).
- Debrief has its own ADR 0036 (dump symbols by PDB identity), numbered per app; the family ADR 0036 above is a different document. Source: src: `git ls-tree origin/docs/adr-logging-root-cause apps/debrief/docs/adr`.

## See also
[open-decisions](open-decisions.md), [test-isolation](test-isolation.md), [ADR index](../adr/README.md).
