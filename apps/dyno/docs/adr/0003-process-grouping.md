# 3. Process grouping: one application, not forty processes

- Status: accepted (2026-10-22)
- Related: ADR 0001, ADR 0004 (rules act on groups), `dyno_core::grouping`

## Context

A modern browser, editor or game launcher is dozens of processes (`chrome.exe` ×40, helper apps inside a `.app`
bundle, language servers). A task manager that lists them flat hides which *application* is heavy, and a rule such as
"cap Chrome when it uses too much CPU" must see the whole application's total, not each renderer on its own. Yet naive
grouping by name merges unrelated programs, and grouping by parent merges everything under a shell or `launchd`.

## Decision

1. **Grouping is a pure function** (`grouping::group(&[ProcessSample]) -> Vec<AppGroup>`) in the core, run by the
   agent on every process tick and by tests on fixtures. The UI receives groups; it never groups.
2. **A process joins a group by, in order:**
   1. its macOS app bundle: the outermost `*.app` in its path, so helper apps inside a bundle join their app;
   2. its parent's group, when its executable lives under the parent's install folder (VS Code's `rg`, a game's
      launcher children) **and** the parent is not a shell, launcher or system host;
   3. its executable path (case-insensitive for Windows paths);
   4. its name, when the path is unreadable (other users' processes without rights).
3. **Parents that never adopt** are a fixed list (`NEVER_ADOPT`): `explorer.exe`, `cmd.exe`, `powershell.exe`,
   `pwsh.exe`, `windowsterminal.exe`, `wininit.exe`, `services.exe`, `svchost.exe`, `launchd`, shells, `terminal`,
   `iterm2`, `userinit.exe`, `winlogon.exe`, `taskhostw.exe`, `sihost.exe`, `steam.exe`. Without this, everything
   started from a terminal would join the terminal.
4. **A group has a stable key** (`AppGroup::id`: bundle, executable path or name, normalised) so history, rules and the
   UI's selection survive restarts. Its members are `ProcessRef`s (pid plus start time); its totals (CPU, memory, disk)
   are sums of the members. A friendly name comes from a table of well-known executables (`FRIENDLY`) and otherwise
   from the executable or bundle name.
5. **Actions on a group** (`Request::ActOnGroup`) expand to one action per member, each checked against the protected
   list (ADR 0004; protected members are skipped and counted) and each recorded with its inverse so the undo restores
   every member.

## Consequences

- Windows `conhost.exe` children and Linux threads are not applications: the Linux adapter drops threads from the
  process list (`thread_kind()` in sysinfo), and grouping treats a console host like any other child.
- Processes that join a group later (a new browser renderer) are picked up on the next tick, so a rule that is already
  holding applies its steps to them too.
- The friendly-name table is data in the core; extending it is an edit with a test, not new logic. A future process
  rule pack (ADR 0007) can add names and known helpers without code.
