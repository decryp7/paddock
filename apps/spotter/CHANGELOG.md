# Changelog

## [1.0.0] - 2026-10-30

### Added

- Spotter watches folders and shares, HTTP listings, JSON APIs, forges, feeds and web pages (including JavaScript pages) and tells you the moment a release appears.
- Downloads (release files, the browser for JavaScript pages) go through the Paddock downloader: up to four connections per host over byte ranges, waits out "too many requests", resumes a cut or cancelled download, checks the publisher's checksum while writing and the free disk space first, and shows phases and Cancel in the status bar.
- Runs on the Paddock app shell: one frame, notifications and service health, jobs with Cancel, settings, About, updates and What's New.
- Every dialog is on the shared dialog API (ADR 0026): the Settings window's contract is declared in Rust, deleting a watch asks with the shared confirmation, and the buttons follow each system's order.
- A browser connection for JavaScript pages survives a stopped and continued process: a page no longer reports closed when the browser was stopped for a while.
- On Windows the browser Spotter downloads for JavaScript pages starts without Chrome's testing field trials, so pages no longer fail to load with a network error when its network service cannot start sandboxed.
- A start that does not get to its window is logged with where it stopped (the shell's start guard, from the shell plugin's set-up on), after about 20 s and then every minute, instead of leaving the log empty.
- Launch at login refuses to register a copy that runs from a mounted disk image or a quarantined download and says to move Spotter to Applications first.
- Starting and closing the browser for JavaScript pages is judged by whether the browser is working, not by a fixed time: a slow start or quit on a busy machine is waited for, a stuck one is ended and reported as such (ADR 0031).
- The Copy menu copies the text you selected after a right click, and it stays open in WebKit (macOS) until you choose Copy or press Escape.
