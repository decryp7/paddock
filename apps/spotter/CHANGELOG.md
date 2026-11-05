# Changelog

## [1.0.0] - 2026-10-30

### Added

- The tray popup shows the widgets you choose, on the shared widget area: watched sites and how each is doing, recent changes, the next check, quiet hours, failing watches with Retry, and whether notifications are on or paused. Each one has a loading, an empty, a working and an error state; status is always an icon and a word. Choose and order them in Settings > Tray popup, with a live preview, undo and keyboard reordering; "Customize widgets…" in the popup's more menu opens it. The layout is kept in `spotter.toml`; one that cannot be read gives the defaults and is kept until you change it.
- Help > Report a problem lists every file of a diagnostics report with its size, lets you read what is in it and saves a zip where you choose; nothing is sent. Addresses keep only their host (a feed's path, query values and fragment are removed, since they can hold a private token), and passwords, tokens, your user name and your machine's name are removed as well. Settings > Logging sets how much Spotter writes to its log.
- The log says what Spotter did: how long the engine took to open, one line for every check (the watch's id, the kind of source, the outcome and, for a failure, its class, the reason and what to do next), what the tray menu was asked to do and how it ended, and how long quitting took. The lines carry no watch names; a reason from the network can name an address, and a report cuts every address to its host.
- Spotter watches folders and shares, HTTP listings, JSON APIs, forges, feeds and web pages (including JavaScript pages) and tells you the moment a release appears.
- Downloads (release files, the browser for JavaScript pages) go through the Paddock downloader: up to four connections per host over byte ranges, waits out "too many requests", resumes a cut or cancelled download, checks the publisher's checksum while writing and the free disk space first, and shows phases and Cancel in the status bar.
- Runs on the Paddock app shell: one frame, notifications and service health, jobs with Cancel, settings, About, updates and What's New.
- Every dialog is on the shared dialog API (ADR 0026): the Settings window's contract is declared in Rust, deleting a watch asks with the shared confirmation, and the buttons follow each system's order.
- A browser connection for JavaScript pages survives a stopped and continued process: a page no longer reports closed when the browser was stopped for a while.
- On Windows the browser Spotter downloads for JavaScript pages starts without Chrome's testing field trials, so pages no longer fail to load with a network error when its network service cannot start sandboxed.
- A start that does not get to its window is logged with where it stopped (the shell's start guard, from the shell plugin's set-up on), after about 20 s and then every minute, instead of leaving the log empty.
- A browser for JavaScript pages that macOS or an app such as App Tamer stops while it starts is waited for, not ended as stuck, and the job says "The browser is paused by macOS or another app; Spotter waits".
- Launch at login refuses to register a copy that runs from a mounted disk image or a quarantined download and says to move Spotter to Applications first.
- Starting and closing the browser for JavaScript pages is judged by whether the browser is working, not by a fixed time: a slow start or quit on a busy machine is waited for, a stuck one is ended and reported as such (ADR 0031).
- The Copy menu copies the text you selected after a right click, and it stays open in WebKit (macOS) until you choose Copy or press Escape.
