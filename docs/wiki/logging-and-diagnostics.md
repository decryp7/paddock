---
title: Logging and diagnostics
verified: 2026-10-31
sources: [crates/paddock-log/src/lib.rs, crates/paddock-diag/src/lib.rs, rule 56, ADR 0036, handoff/merge-steward.md]
---
What the two shared logging crates do on main, what is only on a branch (rule 56, ADR 0036), and the work packages that move the
apps onto them. Status as of origin/main `233c1339b`. Steward-log citations are `merge-steward.md:<line>`.

## Current state
- `paddock-log` is the shared app and service log: one CLEF JSON-lines file per process per day in the OS log folder, `tracing`
  and `log` in one stream, a redactor, error dedupe once per minute, breadcrumbs, and a crash marker with a panic hook. Source: [paddock-log lib.rs](../../crates/paddock-log/src/lib.rs).
- `paddock-diag` builds the Report-a-problem zip that the user reviews file by file and saves where they choose; it has no network
  dependency. Source: [paddock-diag lib.rs](../../crates/paddock-diag/src/lib.rs).
- Rule 56 is not in `docs/rules.md` on main; it is on branch `origin/docs/rule-logs-explain-failures` at `ef8f52349` and was approved for the next
  shared batch. Source: src: `git show origin/docs/rule-logs-explain-failures:docs/rules.md`; src: `merge-steward.md:702, 707`.
- ADR 0036 (logging for root cause) is approved at `021dc3c8d` on branch `origin/docs/adr-logging-root-cause` and queued in the docs batch; it is not on
  main yet. Source: src: `git show origin/docs/adr-logging-root-cause:docs/adr/0036-logging-for-root-cause.md`; src: `merge-steward.md:715` (approved, docs batch queued).
- The audit behind the ADR found that no shipped app's bundle could find a root cause from its logs; the user's decisions on
  the Dyno set, the bundle budget, the default level and the logger replacement are in the ADR's Decisions (user, 2026-10-31) section. Source: src: `git show origin/docs/adr-logging-root-cause:docs/adr/0036-logging-for-root-cause.md` (Decisions section); src: `merge-steward.md:708`.
- The ADR lists work packages WP-1 to WP-11 (dedupe, OS banner, proc and ipc events, jobs events, a 4 MB log window WP-6a, the secret scanner,
  the logging conformance checks (ADR 0036 section 7), per-app adoption WP-9, cost measurement WP-10, merging rule 56 first WP-11). Source: src: `git show origin/docs/adr-logging-root-cause:docs/adr/0036-logging-for-root-cause.md` (Work packages).
- The user also asked for logs for CI and tooling failures, and for rules 56 and ER-27 to cover them; ER-27 to ER-29 are on engineering-rules
  main at `a8064e1`. Source: src: `merge-steward.md:703, 713`.
- Debrief has its own ADR 0036 (dump symbols by PDB identity), numbered per app; the family ADR 0036 above is a different document. Source: src: `git ls-tree origin/docs/adr-logging-root-cause apps/debrief/docs/adr`.

## See also
[open-decisions](open-decisions.md), [test-isolation](test-isolation.md), [ADR index](../adr/README.md).
