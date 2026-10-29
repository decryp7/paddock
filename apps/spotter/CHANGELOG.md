# Changelog

## [1.0.0] - 2026-10-30

### Added

- Spotter watches folders and shares, HTTP listings, JSON APIs, forges, feeds and web pages (including JavaScript pages) and tells you the moment a release appears.
- Downloads (release files, the browser for JavaScript pages) go through the Paddock downloader: up to four connections per host over byte ranges, waits out "too many requests", resumes a cut or cancelled download, checks the publisher's checksum while writing and the free disk space first, and shows phases and Cancel in the status bar.
- Runs on the Paddock app shell: one frame, notifications and service health, jobs with Cancel, settings, About, updates and What's New.
- Every dialog is on the shared dialog API (ADR 0026): the Settings window's contract is declared in Rust, deleting a watch asks with the shared confirmation, and the buttons follow each system's order.
