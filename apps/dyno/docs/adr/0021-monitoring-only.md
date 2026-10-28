# 21. Monitoring only; any privileged helper is read-only

- Status: accepted (2026-10-28); decision of 2026-10-27, confirmed 2026-10-28. Point 4 is amended the same day: Dyno 1.0 ships
  without the helper (ADR 0022)
- Related: supersedes ADR 0004 (CPU taming), ADR 0005 (cleanup safety) and the write parts of ADR 0006 and ADR 0016;
  amends ADR 0007 (packs, plugins) and ADR 0018 (where it describes writes); ADR 0019 (watched-folder sweeps) and ADR 0020
  (taming proof, file removal in Extras) lose their writing parts with them; commit 952990c57 ("Dyno is a read-only
  monitor")

## Context

Dyno grew from a monitor into a tool that also calms programs, cleans files and tunes settings. Every one of those writes
needs its own safety story (protected list, previews, quarantine, undo, an elevated helper that applies changes), and
together they are most of the code that can hurt a user's machine and most of what is still unverified on real systems.
The user decided on 2026-10-27, and confirmed on 2026-10-28, that Dyno 1.0.0 is a monitor: "Watches your PC, read-only:
live and history, alarms, a disk map and benchmarks."

## Decision

1. **Dyno 1.0.0 only watches.** It never ends, suspends, caps, deletes, moves, restores, tunes or changes a setting of the
   machine. What Dyno writes is its own data: history, settings, alarm acknowledgements, mutes and notes, snapshots,
   baselines, and the reports and exports the user saves.
2. **Kept (read-only).** Live and history; process details, lockers, open location, snapshots and diffs, why-slow; health
   (reasons only, no fix buttons); startup items (view only); issues and statistics; unusual processes (view); the weekly
   report; throttling, lifespan and baselines; alarms that notify; web search, VirusTotal, network view, IP lookup and host
   names (they only open the browser); counters; extras; find files (view only); backup health; the flight recorder;
   diagnostics; inventory; the disk map; benchmarks (on click); the CLI's `status`, `why-slow`, `top`, `alarms`,
   `snapshot` and `counters`; Dyno's own launch agent and settings; plugins as sensors only.
3. **Removed.** The helper's Apply and every write path; taming rules, profiles and pause; process actions (Act,
   ActOnGroup, Undo: end, suspend, cap); startup-state changes; health "fix" buttons; cleanup (categories, scan and run,
   quarantine and restore); watched-folder cleanup; tuning (apply and revert); RemoveFoundFiles; UnusualAct; the plugin
   `CleanupProvider` and `ActionProvider`; the CLI's `end`, `cap` and `undo`.
4. **Any privileged helper is read-only, and there is none in 1.0.** Decided 2026-10-28: Dyno 1.0.0 ships without the helper.
   It runs with the user's own rights and installs per user; a figure the OS withholds is `Unavailable` with a plain reason,
   other accounts' processes are one "Other users" row, and **Run as administrator** restarts the agent elevated for one
   session (UAC, the macOS password dialog, `pkexec`) without installing anything. The helper (Info, Traffic, Audit and the
   rest of ADR 0016's read-only requests), "start as administrator at login" and the planned process start and exit events
   (Linux proc connector, Windows ETW; user, 2026-10-27) come in Dyno 1.1 under ADR 0022. When it comes back, nothing it
   accepts can change the machine, apart from installing or removing itself and, as ADR 0022 decides, installing a signed,
   newer Dyno.
5. **Packs and plugins (amends ADR 0007).** TOML packs, when built (WP-15), carry read-only data only (backup detectors,
   known helpers, sensors); there are no cleanup, tuning or rule packs. Plugins are sensors only.
6. **Baselines and episodes (amends ADR 0018).** Throttling, lifespan and baselines stay as observation; nothing offers to
   act on a finding.
7. **Dropped, not later.** The removed features are not on the roadmap. Bringing one back needs a new ADR. WP-15, 16, 17,
   30 and 31 stay planned as later work, after 1.0.

## Consequences

- The protected list, undo records, the activity-with-undo log, the quarantine and the control-state file have no job; the
  code is deleted by separate changes. The Activity page becomes a history of what Dyno saw, not of what it did.
- The safety story shrinks to: read-only OS access with timeouts, no helper in 1.0 (and a read-only one later), and the user's own data.
- Settings says plainly that Dyno only watches.
- Docs describe only the kept features; the dropped ones are listed in the roadmap with a link to this record.
