# Changelog

## [1.0.0] - Unreleased

### Added

- Finding large and duplicate files, reading the system logs, checking the backups and hashing a program's file are jobs: starting one answers at once, the page shows the phase with its own count (a growing count while it walks, then the comparison, then the bytes read), "Still working" when a step takes long, and Cancel in every phase. A cancelled or failed job changes nothing, and no timeout ever abandons a running one. The file finder is bounded in files (500,000), folder depth (48 levels) and bytes compared (64 GiB) and says which limit stopped it and what to do. Jobs run on the shared `paddock-jobs`; the agent protocol is version 2 (`StartJob`, `Jobs`, `CancelJob`, `JobResult` replace `FindFiles`, `RescanIssues`, `RefreshBackup` and `FileHash`).
- Dyno measures your PC and explains why it is slow; it only looks and changes nothing on your computer (ADR 0021).
- Runs on the Paddock app shell: one frame, notifications and service health, jobs with Cancel, settings, About, updates and What's New, and the tray popover.
- Every dialog is on the shared dialog API (ADR 0026): the alert rule, the lookups and the lockers list are dialog windows with their contracts declared in Rust, deleting an alert rule asks with the shared confirmation, and the buttons follow each system's order.
