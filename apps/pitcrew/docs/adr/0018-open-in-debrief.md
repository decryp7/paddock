# 18. Open a job's log in Debrief

- Status: accepted (2026-10-23)
- Related: code: `pitcrew_core::debrief_link`, `app/src-tauri/src/handoff.rs`, the Debrief button (Activity, Logs)

## Context

Debrief is the owner's log-analysis app; a runner log is exactly what it is for. Pitcrew should hand a job's log over in
one click.

## Decision

**Command-line hand-off.** Debrief opens the paths it is started with, and a running Debrief takes them over as a second
instance, so Pitcrew starts `<debrief program> <file>` detached. A deep link would need a registered URL scheme on each
OS and is not needed. The program's name is one constant, `debrief_link::DEBRIEF_CLI` (`debrief`; it was `imt` before the
rename, and finding only `imt` is reported as "Update Debrief to use Open in Debrief"); it is searched on `PATH` and in the usual install places per OS
(`install_candidates`), because a GUI app on macOS has a minimal `PATH`.

**The job, not the whole log.** The engine cuts the job's lines out of `runner.log` (and `runner.log.1` when the job is
older) by the `time=` of the lines, with 5 s around the job, lines without a time belonging to the line before, at most
32 MB; the newest 64 MB of the file are read. The extract goes to `debrief-handoff/<runner>-<label>.log` in the app's data
folder.

**Graceful when Debrief is missing.** The extract is still written; the answer is `notInstalled` with the file and a
sentence ("Install Debrief, or open this file in it by hand"); a log with no lines for the job (rotated away) is
`noLines`. Neither is an error screen, only a message.

## Consequences

- The extract is a copy of log text on disk in the app's data folder; it is overwritten by the next hand-off of the same
  job and is not cleaned automatically (the folder is small).
