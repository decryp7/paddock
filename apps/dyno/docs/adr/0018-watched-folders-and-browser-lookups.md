# 18. Watched folders sweep into the quarantine only; web and hash searches are browser hand-offs

- Status: accepted (2026-10-23; the number is allocated at merge time, renumber on a clash)
- Related: ADR 0005 (cleanup safety), 0015 (IP lookup), `dyno_core::{watch, weblinks, netchange}`, `dyno_agent::{watch, handlers_watch, netwatch}`

## Context

WP-10 deletes the user's files on a timer, which is the riskiest thing Dyno does. WP-09 and WP-33 send text and a hash toward the
web. WP-35 reads the network interfaces in the background.

## Decision

1. **Watched folders move files to the 7-day quarantine and nowhere else.** Never a permanent delete, never the Trash, so
   every sweep is one activity entry with an undo (the same quarantine restore as cleanup).
2. **The person chooses the folder in the OS dialog.** The shell's `add_watched_folder` command opens it; the generic `dyno`
   command refuses `AddWatchedFolder`, so a compromised webview cannot name a folder.
3. **Configured, previewed, then on.** A folder starts off. Turning it on (or "Move now") needs a preview of the current settings;
   a shorter keep-for time or turning subfolders on resets that. The engine enforces it, not the UI.
4. **Same admission as cleanup.** `dyno_core::watch::plan` uses `cleanup::admit`: inside the chosen folder, no links, no `..`,
   personal folders only when the chosen folder is itself inside one. `validate_root` refuses the home folder and its
   ancestors, drives, system folders and Dyno's data folder; folders may not overlap. Each file is re-checked just before it
   moves (still a plain file, unchanged since listed, real parent inside the real root, not in use). Partial downloads and
   housekeeping files are never moved. A file counts from the later of its creation and modification time.
5. **A periodic sweep, no file-change notifications.** FSEvents/inotify/`ReadDirectoryChangesW` add three platform code paths and
   a dependency for a keep-for time measured in minutes at best; a sweep every tenth of the keep time (1 to 15 minutes)
   is exact enough and also catches anything an event would miss. Revisit if sub-minute times are wanted.
6. **Searches open the default browser on a click.** The agent builds the address (`weblinks`) from a typed value: a
   redacted, user-edited text, or a 64-digit hash. The shell opens only `https://` addresses. Dyno fetches and uploads
   nothing; the file hash is computed locally and shown before anything opens.
7. **Network change events poll slowly.** Interfaces every 20 s (one sysinfo call), the Wi-Fi tool at most once a minute with a
   timeout. IPv6 churn, link-local addresses and virtual interfaces never produce events; the first look is only a baseline.

## Consequences

- Space is freed when the quarantine expires, as for cleanup. The page says so.
- macOS may hide the Wi-Fi name without Location access: events then read "Joined Wi-Fi" without a name.
- "Signed" in the startup filter means the platform reported a publisher; macOS launch agents report none yet, so there only
  the OS-owned filter hides entries.
