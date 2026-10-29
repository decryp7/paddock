# 20. Extras, backup health checks, the login-shell PATH and the proof of taming

- Status: accepted (2026-10-23); taming proof and file removal superseded by ADR 0021 (2026-10-28); extras and backup health stand
- Related: ADR 0004, 0005, 0006, 0010, 0013, 0017; roadmap WP-13, WP-14, WP-36 and gap G11

## Context

Several small read-only features (what changed, why the Mac will not sleep, boot analysis, per-app network and energy, GPU per
process, an inventory report, a large and duplicate file finder, maintenance windows) and the backup health checks all run OS
tools or walk the file system. A service started by launchd or systemd has a minimal `PATH`, so a user's `restic` or `borg` is
not found. And a taming rule that acts must be able to show that it helped, expire, and give an app back the moment it comes
to the front.

## Decision

1. **Pure parsers, thin runners.** Every tool's output is parsed by a pure function in `dyno_core` (`extras::*`, `backup`)
   with fixture tests; `dyno_platform::extras` runs the tool behind the existing `ToolRunner` seam with a timeout and turns a
   refusal into an "unavailable: reason" sentence. The OS is a parameter, so every branch is tested on every host.
2. **The login-shell PATH resolver (G11).** `dyno_core::toolpath` validates and composes; `dyno_platform::toolpath` asks the
   user's login shell once (`$SHELL -l -c`, non-interactive so a prompt framework cannot hang it; 5 s timeout; marker-prefixed
   output; 32 KiB cap), caches the answer (also a failure) for 10 minutes and searches in this order: the per-tool override
   folders (validated: absolute, plain, at most 16), the login `PATH`, the process `PATH`, well-known folders. Programs run
   niced with that `PATH`. The same ask exports an allow-list of repository variables (`RESTIC_REPOSITORY`,
   `RESTIC_PASSWORD_FILE`, `BORG_REPO`, `BORG_PASSCOMMAND`, ...), never a password. A `PATH` built only in an interactive rc
   file is not seen; the override covers it.
3. **Backup health looks, it never manages.** Time Machine (`tmutil`), Windows Backup (event log, then `wbadmin`), File History
   (event log), restic and Borg (`snapshots`/`list` with the user's repository variables) are judged against a stale limit
   (default 48 h). A tool that is not installed produces nothing ("no backup tool found" is a normal state); a tool Dyno cannot
   read says why and what to do next. Checks run on a background thread, once at start and every 6 hours inside the maintenance
   windows, and on "Check now" (polled by the UI). A stale or missing backup is one alarm per tool, cleared when healthy.
   Duplicati is detected but not read yet. Declarative tier-2 detector packs wait for WP-15; the detectors are built in.
4. **Maintenance windows limit only what Dyno starts by itself** (the OS log reads of the Issues page, the backup checks and
   the change recording; scheduled cleanups will join when they exist). A click always runs. The windows are plain settings data (weekdays, local start and end, an end before
   the start runs past midnight), validated in Rust and refused with a sentence.
5. **The file finder deletes nothing by itself.** It walks a folder the person picked in the OS dialog (the shell remembers
   picked folders; the finder refuses others), bounded in entries and time, never following links, hashes only files that share
   a size (head, then whole file, SHA-256). Removal re-checks each selected file with the cleanup policy's `admit` (plain file,
   inside the chosen folder, not in a personal folder the person did not pick, unchanged since the scan, not locked) and moves
   it to the Trash or the 7-day quarantine; one copy of each duplicate always stays.
6. **"What changed" is recorded, not reconstructed.** A background pass compares the software and startup lists with the last
   ones (the first pass is a silent baseline) and stores the differences; setting changes, startup changes and cleanups come
   from the activity log. Boots are now recorded (they were not): the agent stores the boot time at start and the seconds until
   the CPU stayed quiet for 30 s.
7. **Taming proof (WP-36).** A rule may have `expiresAt`; the sampler switches an expired rule off, logs it, and the engine
   releases what it did (the engine keeps being ticked while it holds anything). The foreground app is never acted on and is
   given back at once when it comes to the front (this was already the rule; it now has a test). Twenty seconds after each
   step the engine emits `Measured { before, after }` from the group's CPU and the agent writes an activity entry, shown with
   the in-effect list.

## Consequences

- No new runtime: all of this is Rust plus the Angular UI. One new dependency, `sha2` (MIT or Apache-2.0, already in the tree).
- Windows event IDs for Windows Backup (4) and File History (201) and the Windows GPU counters are verified by fixtures only;
  they are checked on the Windows runner and by hand (testing-manual).
- Energy per app exists only on macOS (`top -o power`); network bytes per app are the OS's cumulative counters (macOS today)
  rather than minute rows in the history, so there is no per-app network history yet.
