# 16. Issues from the OS logs and the baseline of programs

- Status: accepted (2026-10-23)
- Related: ADR 0001, 0006, 0008, 0013; roadmap WP-06 and WP-07

## Context

Dyno must show what went wrong on the machine (crashes, hangs, bad shutdowns) and which programs are new, without elevated
rights, without verdicts and without reading what the user did not choose.

## Decision

1. **Observe-only sources behind a port.** `dyno_core::issues::IssueSource` reads one OS log per implementation
   (`dyno_platform::issues`): macOS `DiagnosticReports` of the current user and `log show` (niced, 60 to 120 s timeout), the
   Windows Event Log through `wevtutil` (Application, System, Windows Update; one table of curated events is both query and
   classifier), the journal and `coredumpctl`. Parsers are pure and compile on every OS; external tools go through a
   `ToolRunner` seam. A source that fails is an "unavailable: reason" status; no score is claimed when none can be read.
2. **Normalise once, store once.** One `Issue` type with a stable id (re-reading never duplicates), bounded plain text, kind
   and severity decided by the engine. Table `issues` in SQLite, pruned with the other long tiers.
3. **Statistics are pure.** The daily score, leaderboards, trend, crash days and crash loops are functions over issues with the
   local-day offset passed in. Crashes feed `history.crashes`, the health reason and the weekly report; serious kinds raise
   alarms (one per issue, recent only), a crash loop feeds the crash-loop alert.
4. **A baseline, not a blacklist.** `dyno_core::unusual` scores facts about a program with independent reasons and reports
   reasons in words ("unusual because it..."), never "malware". The baseline learns silently for a configurable period; the first
   look is always silent. Flags never trigger an action; the user's actions are Protect (protected list), Hide, Restore.
5. **Reading is off the sampler.** Two agent threads (issues every 15 minutes, programs every 30 s); signature checks of new
   programs are capped per look.

## Consequences

- Windows uses `wevtutil` instead of native Event Log FFI; a native reader can replace it behind the same port.
- Signatures need process details, so "unsigned" is flagged only where details exist (macOS, Linux today; Windows with G8).
- Linux and Windows readers are fixture-tested; their real tools run only on their CI runners.
