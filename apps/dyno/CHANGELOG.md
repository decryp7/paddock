# Changelog

## [1.0.0] - Unreleased

### Added

- Finding large and duplicate files, reading the system logs, checking the backups, hashing a program's file and the sleep, boot, app-usage and software reads are jobs on the shared `paddock-jobs`: starting one answers at once, the agent pushes every status to the shell (`paddock://jobs`), so the Jobs view, the status item and the pages show it with the phase's own count (a growing count while it walks or streams, then the comparison, then the bytes read), "Still working" at the shell's threshold, and Cancel in every phase. A cancelled or failed job changes nothing, and no timeout of the UI ever abandons a running one (the UI has no timer at all; a shared check over the app's sources keeps it so). Cancelling a system-log read kills the `log show` process at once; a log read or backup check asked for while the scheduled one runs waits for it. The file finder is bounded in files (500,000), folder depth (48 levels) and bytes compared (64 GiB) and says which limit stopped it and what to do. The agent protocol is version 2 (`StartJob`, `Jobs`, `CancelJob`, `JobResult` replace `FindFiles`, `RescanIssues`, `RefreshBackup`, `FileHash`, `SleepBlockers`, `BootAnalysis`, `AppUsage` and `SoftwareInventory`; `Message::Job` pushes a status).
- Dyno measures your PC and explains why it is slow; it only looks and changes nothing on your computer (ADR 0021).
- Runs on the Paddock app shell: one frame, notifications and service health, jobs with Cancel, settings, About, updates and What's New, and the tray popover.
- Every dialog is on the shared dialog API (ADR 0026): the alert rule, the lookups and the lockers list are dialog windows with their contracts declared in Rust, deleting an alert rule asks with the shared confirmation, and the buttons follow each system's order.
