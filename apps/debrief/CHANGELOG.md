# Changelog

What changed for people who use Debrief, one section per release. This file is curated by hand (the in-app "What's new"
panel reads it at build time); `git cliff` lists every commit if you need the full history.

## Unreleased

### Added

- **Names:** Searching for an abbreviation of a program name now finds it: "rptbldr" finds Report Builder and "netmon" finds Network Monitor, with the reason "abbreviation: letters of report builder", without any AI model

### Fixed

- **Search:** Closing a log store while its word-search index was still merging could leave the index's background merges writing into the store's folder after it was closed or deleted. Closing now waits for them to end

### Security

- **Start-up programs:** Debrief no longer lets environment variables choose which program it starts. `DEBRIEF_STORE_WORKER` (the store worker), `DEBRIEF_PLUGINS` (the plugins program) and `CHROMIUM_PATH` (the research browser) are now read only by development builds, and `DEBRIEF_BROWSER_NO_SANDBOX` (which started the research browser without its sandbox when Debrief ran as root) is ignored too. Before, any program running as you could set one of them, and Debrief, relaunched with administrator rights, would start the program it named. An installed Debrief now starts the store worker and the plugins program from its own install folder, and the research browser from the browser path in Settings, the one Debrief installed, or one found in the usual places; its log says once, by name, when it ignored a variable

### Removed

- **AI:** Pitwall is now the only AI engine. Debrief no longer downloads or runs its own models: the set-up wizard, the model manager, the inference runtimes, their update checks and manual downloads, the engine choice and the AI worker's memory cap are gone, and every AI feature runs on the model Pitwall serves. Install Pitwall and connect Debrief to it in Settings ▸ AI; Debrief's own model and runtime files stay on disk until you remove them. The AI download proxy and connection settings are gone too: the FFmpeg download now follows your system's proxy settings (HTTPS_PROXY, NO_PROXY or the operating system's)
- **Names:** Debrief no longer asks the AI model for human names in the background. "Learning human names" after an import, and the Settings ▸ AI switch "AI aliases for names", are gone, so opening logs no longer loads or runs a model. Aliases an earlier version already learned stay, labelled AI-generated, and you can still remove any of them

### Changed

- **Opened bundles:** A `.debriefz` bundle you open is now unpacked into Debrief's local data folder (`bundles`) instead of a fixed folder in the system temp folder, where another user of the same computer could have planted files. Copies not touched for 7 days are removed when Debrief starts (the bundle file itself is never touched); Settings ▸ Storage still lists and clears them. Throwaway browser profiles of the research browser live in the local data folder too, and ones left by a crashed run are removed after a day. If your user profile folder cannot be found, opening bundles, video support and the performance suggestions say so and stay unavailable instead of using a temporary folder.
- **Search:** The word-search index of a large import is split over up to four threads (at a lowered priority, so Debrief stays responsive) instead of two. In a first measurement on a busy machine, word searches for common words answered faster once it was built ([performance notes](docs/performance.md#index-writer-threads-and-heap-one-round-on-a-busy-machine-2026-10-30)); building it does not get faster yet. Settings ▸ Performance ▸ Search index threads (0 is Auto, the default) and Search index memory per thread (64 MB by default) set them; a value you chose for the old Search index threads setting is not kept

- **Database engine:** Debrief now runs on DuckDB 2.0. Searches for text in messages use about half the processor time and queries need less memory. The database of an existing project keeps its format, and a new project is written in a format the previous Debrief can still open. A project saved by a newer Debrief than yours now says so and asks you to update, instead of showing a database error
- **SQL console:** DuckDB 2.0 changed some SQL rules that affect your own queries: `~` and `!~` match anywhere in the text (a partial match), `!` binds tighter than other operators, `X'...'` and `B'...'` are binary and bit-string literals, integer division and remainder by zero are errors, and `SHOW name` prints a setting when no table has that name
- **SQL console:** `SHOW` statements are no longer accepted (DuckDB 2.0 lists databases and settings with them); use `DESCRIBE`, or query the views events, sources, fields and notes.
- **macOS:** Debrief now supports Apple silicon Macs only (DuckDB drops Intel Macs in 2.1 and Debrief follows it). Intel Macs can keep using Debrief 1.1.1, which no longer receives updates
- **Updates:** Debrief now updates from the Paddock repository's releases, signed with the Paddock family's update key. A copy installed before this release cannot update itself to it: download this release once by hand and install it over the old one; later releases update as usual

### Fixed

- **Event list:** After a large import the events could be left without their time order when the database ran out of memory, and the event list then missed events until the next import. The import now gives that step its memory, says so if it still fails, and orders the events again as a job when the project opens
- **Report a problem:** everything in the report except the description you type now has secrets and personal details removed: Debrief's own log, the logs of its helper processes, the steps to reproduce, the recent operations, the system information, the process list, the recovery information and the error details (tokens and passwords, your user name, home folder and computer name, e-mail, public IP and MAC addresses). Your own computer's address and private network addresses are kept, because they show how the setup looks and say nothing about you. The preview shows exactly what the file holds, and a report saved without screenshots says so when you open it instead of showing an error. Screenshots of the Debrief window are no longer in the report unless you tick Include screenshots in the dialog, because a screenshot can show the contents of the logs you have open
- **Browser on Windows:** the page browser Debrief downloads (Chrome for Testing) no longer starts its network service twice, which could make the first page it opened fail to load
- **Event Log follower:** the status no longer shows more queued events than the queue holds while the writer is busy
- **Research profile:** clearing the kept research profile right after the AI browser closed no longer fails with "in use"
- **Process monitor:** a new run's live charts wait until its output is open instead of briefly charting the earlier runs; when the output cannot be opened the panel and the monitor say so, and a failed first load stays shown until you retry
- **Process monitor:** every monitoring run (the debrief-monitor command and the agent's monitor action) now ends its log with "Monitoring stopped after … ms", the reason (stopped, or the error) and how many samples and stats events it wrote, so a short or empty monitor log says why
- **Clock offsets:** After you set a file's clock offset, search results came back in the old time order. The moved events are now indexed again with their new times: until that is done (the Store pane shows indexing) word search does not find them. In a project from an earlier Debrief, data files whose event ranges overlap (after an earlier compaction) keep their old times in search until the index is rebuilt

## 1.1.1 (2026-10-24)

### Added

- **AI:** When Pitwall runs on this computer, Debrief can use it as its AI engine: choose Use Pitwall in the AI set-up, approve the six-digit code in Pitwall, and one model serves all your apps without a second download. Settings ▸ AI ▸ AI engine chooses Automatic, Pitwall or Built-in; if Pitwall stops, Debrief says so and offers the built-in engine instead of switching by itself
- **Research:** Fewer "verify you are human" checks: research waits a short, varying pause before each search and each page (1.5 to 4 seconds by default), makes at most three searches per question, and reuses the results and pages one chat already fetched (the same question gets the earlier answer at once). Another research waits in the Jobs pane ("Waiting for the current research"). The pauses and searches per question are in Settings ▸ Web browser
- **Research:** Settings ▸ Web browser ▸ Keep a research profile (off by default) keeps cookies and consent between researches in a profile of research's own (never your browser's), with its size, a size limit and Clear research profile

### Changed

- **Notifications:** One failure is now one notification. A repeated message updates the one already on screen (with a count) instead of adding another, errors stay until you dismiss them or the problem is gone, and technical details are folded behind Details. Job results, update offers, live alerts, error snapshots and errors all appear in the same place
- **Services:** When the log store, the AI worker, a plugin, the research browser or a download stops, the status bar shows one chip with a plain explanation, the next step, the technical details and a Try again / Restart / Allow again / Install button. Every window shows the same state, including a window you open while a service is down
- **Memory limits:** When a memory limit stops the database worker, the AI worker or a plugin, the status chip says so once, with Try again and Raise the cap… (it opens the setting with a suggested value)
- **Panels:** A panel that needs the log store shows "Waiting for the log store" instead of its own red error, and loads by itself when the store is back; the assistant, the research browser, models and runtimes wait for their own service the same way

- **Layout:** Panes lay out by the room each one needs: what does not fit is tabbed or auto-hidden instead of squeezed (one notice with Undo), and returns when the window is large enough again; sizes you set by dragging are kept
- **Research browser:** When the research browser cannot start, the status chip says so with Open Settings… (Settings ▸ Web browser) or, when no browser is installed, Install
- **What's new:** The release notes are now a document in the window instead of a dialog. After an update it opens once in the background (it does not take the front or the keyboard focus), and one notification, "Updated to … · See what's new", brings it forward. *Help ▸ What's new* opens it at any time; close it like any other document. It shows the notes of the version that is running and does not open again on later starts of the same version
- **Notifications:** A notification's button that opens a view or a window (See what's new, Show in Jobs, View, Open the assistant, Open Settings…) now slides the Notifications pane in, and one that opens a view moves the keyboard focus to it; buttons that act in place (Try again, Reset, Later) keep the pane open

### Fixed

- **Start-up:** A window shows "Starting Debrief…" until it is ready; a window that does not finish starting is reloaded once (keeping your query, filters and selection), then Debrief asks whether to reload, restart in Safe Mode or keep waiting
- **Research browser:** Closing the research browser's window, quitting it or a crash no longer stops the next research; the browser opens on the display of the Debrief window that started it
- **Dialogs:** Every dialog is now one of four kinds (decision, form, viewer, tool) that decides its size, buttons and keys: result windows open at least 680 x 560 and only grow, questions are exactly as big as their text, and every window has a Close or Cancel button that stays in view (What's new no longer scrolls its Got it button out of sight)
- **Dialogs:** A dialog window may call only the commands it needs, so a bug in one window can no longer do what only the main window should; Restart to apply in Settings and the plugin windows, and Restart as administrator in the profiling and diagnostics windows, now restart through the window that opened them and keep its session
- **Dialogs:** Open recent project hands the choice back to the window you were working in, which asks about unsaved changes and opens it; What's new always shows the notes of the version that is running
- **Dialogs:** With several displays, a dialog always opens on the display of the window that opened it, at a size that fits that display; a remembered position is kept relative to that window and only used when the dialog then stays on its display, and short dialogs no longer jump in height after opening
- **Research browser:** The research browser closes again when research ends, unless you used it; it no longer opens an empty second tab
- **Profile editor:** Its buttons no longer jump to another line when the profile arrives just after the editor opens, so a click made at once (on Propose…, for example) is no longer lost; the profile's name and path truncate instead

### Known issues

- **Memory:** On Linux the log store's worker may use up to about 320 MB while querying (300 MB before the names catalogue); a later update makes the catalogue cheaper
- **Memory:** On macOS the log store's worker may use up to about 570 MB while querying (about 470 MB in 1.0.1); a later update looks into it

## 1.0.2 (2026-10-23)

### Added

- **Search:** The search box suggests "Did you mean CompanyA.Graphic.Builder.exe (ProcessName)?" when your words match a value in your logs, even with different separators, case or small typos
- **Tool panes:** Every tool pane has an auto-hide (pin) button in its header; right-click it to choose the left, right or bottom edge, remembered per pane. The tooltip shows your current shortcut
- **Command line:** Import progress by phase on a terminal, `--quiet` to turn it off, and Ctrl+C cancels the import and exits with code 130
- **Assistant:** It finds events and values by name, trying wider matches when the exact words are not there and saying which step found them
- **Jobs:** Every long task shows its phase (finding files, checking, importing, compacting, indexing, saving) with its own progress, in the toast, the Jobs pane and the status bar; opening a dump, making a redacted copy, installing the browser and downloading an update are jobs with byte progress and Cancel

### Fixed

- **Dialogs:** Dialog windows have no title-bar buttons and close from their content (Close or Cancel; `Esc` and `Cmd+W` act as Cancel, and "Close Debrief?" ignores them); the note, knowledge, parser and format editors, the incident summary, Search the web and Report export are tools you can keep beside the work
- **Dialogs:** Windows that fill in as they load or stream (Explain event, proposals, the incident summary, research, What's new) open at a usable size and only grow
- **AI setup:** Pick the model, press Download and set up, and the window closes; the download runs in the background (Jobs, status bar, the assistant pane) and a message says when the assistant is ready or why it failed, with Retry
- **About:** It shows the real version, fills its window with the notices scrolling inside and a pinned footer, and loads the 15,000-line third-party notices only when asked (no longer part of the app's JavaScript)
- **Research:** The research browser opens as a normal window and closes itself when research ends, unless you used it; it also closes when you quit
- **Research:** Closing the research browser window yourself no longer stops research from working: the next research opens a new window, and a research whose window you close stops with a plain message and Retry
- **AI setup:** Model and runtime downloads are checked while they are written, so the progress no longer restarts for a second pass
- **AI models:** Whether a model fits is judged from its weights plus the cache for the real context size, against the memory that is free now
- **Search:** A phrase such as "graphic builder" finds Graphic.Builder whatever the separators or capitalisation
- **Home:** Home reflows in a narrow pane, and long paths and file names shorten in the middle with the full value in a tooltip
- **Import:** Opening a very large folder reports finding, checking and importing with live counts from the first moment and walks the folders in parallel
- **Reload:** A cancelled reload says "Reload cancelled — previous data kept"
- **Monitor:** Linux threads are no longer counted as processes

## 1.0.1 (2026-10-23)

### Fixed

- **macOS:** The app opens after download instead of being reported as damaged

## 1.0.0 (2026-10-23)

### Added

- **Debrief:** The product has a new name, Debrief: the app, the command line (`debrief`), the project and plugin file formats, the SDK and the crates all carry the new name
- **Command line:** The `debrief` tool imports, queries, analyses, watches and reports headlessly with the same engine as the app, for scripts and CI gates
- **Outliers:** Error outliers show which hosts, builds or other fields are over-represented among errors compared with the rest of the same time span
- **Patterns:** Every message pattern has a trend sparkline on one shared scale, and a Values list shows what was behind each placeholder
- **Import:** Import only a time window (for example one hour of a 20 GB folder); everything outside it is skipped
- **Watched folders:** Debrief imports new files as they settle in folders you watch and announces known issues when they appear
- **Lenses:** Save a view as a lens (levels, search, filters, extra columns, highlights and the first chart) and switch between lenses from one menu in the filter row
- **Design:** A design pass over every screen: themed selects, switches and number fields, one tabs style, plugins as one table with a side panel, a one-line timeline legend with a popover and folded marker counts, hover row actions, calm empty states, a Home that fits a 1440 x 900 window and a colour-blind-safe pair for comparing A and B
- **Shortcuts:** Edit every key binding in Settings > Keyboard, with conflict warnings; the F1 cheat sheet shows your bindings
- **Themes:** A high contrast theme (WCAG AAA text, strong outlines and focus rings) that is chosen automatically when your system asks for more contrast; "Follow system" is the new default theme choice
- **What's new:** This panel, shown once after an update and always under Help > What's new
- **Parsers:** Generate a parser with AI, check it against your file and keep it in the parser library
- **Store:** A Store pane with reports, maintenance and a read-only SQL console
- **Settings:** Performance settings for memory caps, CPU use and presets, with suggested values
- **Home:** A dashboard of everything you opened: live tiles, the history of events by level (click or drag to set the time range), a health check with a button for each finding, recent activity and quick actions; it replaces the Overview view
- **Timeline:** A minimap above the histogram shows the whole data range; drag to pan, drag an edge to zoom, double-click to fit
- **Search:** Search everything (Ctrl/⌘+Shift+F) looks in events, sources, fields and their values, marker kinds, exception groups, notes and knowledge files, and opens the match
- **Perspectives:** Named window arrangements (Investigate, Profiling, Live, Diagnostics and your own) switched with Ctrl/⌘+1 to 9; every view opens in its own area and never over the timeline or the grid; Alt+Left and Alt+Right walk back and forward through events, ranges and views
- **Video:** Open a screen recording, let Debrief pick the frames that matter, align them to the logs and have the local model describe them; each video is a source with its own lane
- **Investigate:** Declare known issues in your knowledge files, build an incident summary report in one step and follow investigation playbooks (four templates included)
- **Encodings:** Debrief detects the text encoding of every source (Shift_JIS, GBK, EUC-KR, Big5, windows-125x and more), shows its confidence and lets you choose another one
- **Paste:** Edit > Paste as log adds pasted text, or text dropped on the window, to the project as a log
- **Share:** File > Share redacted copy writes a project copy built from redacted text, with a live preview of what changes and what is left out
- **Dumps:** Symbols are matched by the PDB's identity, not its name, and can be downloaded from symbol servers you enable; each module says why it has no names
- **Store:** Compact, rebuild the index and clear cached analyses from the Store pane; the SQL console keeps event ids, opens rows in the grid and exports results to CSV
- **Plugins:** One Plugins view with Installed and Health tabs; a Parsers view for the parser library
- **Live tail:** Alerts, the tail status, clock skew and the last event of a file are handled by the store, so they work on large projects too

### Changed

- **Menus:** Theme choices live under View > Theme; live tail moved to View > Live
- **Palette:** The command palette matches word starts and lists recent commands first
- **Layout:** The Focused and Full layouts are gone; perspectives take their place
- **Search:** One search field takes words or a query, and the engine tells which
- **Error snapshots:** The picture comes from the system's web view and no longer freezes the window

### Fixed

- **Time:** Dates without a year are read near the file's own date
- **Memory:** Memory is given back to the system after an import, an index build and each analysis
