# User guide

Dyno measures your PC, explains why it is slow and, when you ask, calms a program, cleans up or tunes. This guide covers
what works today; screens marked *placeholder* are described in [features.md](features.md) and [roadmap.md](roadmap.md).

## Glossary

| Term | Meaning |
| --- | --- |
| Agent | `dyno-agent`, the background program that measures and keeps your history, also when the window is closed |
| App | The Dyno window and tray icon |
| Application (group) | All processes of one program (Chrome and its 40 helpers) shown as one |
| Protected | A process Dyno never touches: critical OS processes, antivirus, the audio engine, Dyno itself, your own entries |
| Rule | A CPU taming rule: when an app uses too much CPU for a while, lower its priority, then use efficiency mode, then cap it |
| Profile | A set of active rules (Normal, Gaming, Meeting, On battery, Building code) |
| Activity | The log of everything Dyno did, each with an Undo |
| Observe-only | Dyno's default: it measures and explains; it changes something only from an enabled rule or your click |

## Install

Per OS in the [README](../README.md#quick-start); macOS in [macos.md](macos.md), Linux in [linux.md](linux.md). After
installing, open Dyno. The window starts with a short splash and then the dashboard. If no agent is running, the app starts
one; to keep measuring after you log in, turn on **Settings > Launch at login** (this registers the agent with the OS:
`SMAppService` on macOS, a systemd user unit on Linux, a per-user Run entry on Windows).

## First run

1. Open **Dashboard**. Tiles show CPU, memory, disk, network and, where your machine has them, GPU, battery, temperatures and
   fans. A tile that cannot be read says why (for example "no battery") instead of showing zero.
2. Press **Check now** to get the diagnosis: the likeliest cause of slowness, with evidence and suggestions. Nothing runs until
   you click a suggestion, and each one previews what it will do.
3. Look at **Processes** to see which applications use the most.
4. In **Settings**, choose the theme, what the tray icon shows and how long history is kept.

## Everyday use

- **Dashboard.** Live tiles with sparklines, the history chart (hover for min, average and max; short spikes stay visible),
  the top applications, the health score and "Dyno: x % CPU, y MB" at the bottom, which is what Dyno itself costs.
- **Processes.** Applications are grouped; expand one to see its processes. Select a row for details (command line, working
  directory, modules, connections, signer). Right-click or use the toolbar for end, suspend, resume, priority, efficiency mode and
  a CPU cap, on one process or the whole application. Dyno asks first for anything destructive. Protected processes show a
  lock and the reason; they cannot be changed. Every action appears in **Activity** with **Undo**.
- **Command palette.** Ctrl or Cmd + K jumps to a screen or action.
- **Tray.** The icon shows the metric you chose; its dropdown is a compact dashboard. Closing the window keeps Dyno in the
  tray and the agent measuring; **Quit** closes the app (the agent keeps measuring until you stop it).
- **Tuning.** Suggestions grouped by topic (power, sleep and hibernation, battery, developer folders, appearance). Nothing
  changes when you open the page. **Review...** shows exactly what Dyno will change and exactly how it will undo it; only
  **Apply** changes anything, and the change appears in **Activity** with **Undo**. If something else changed the setting
  afterwards, **Revert** asks before overwriting it (**Revert anyway**). Changes that need administrator rights are shown
  but cannot be applied yet.
- **CPU taming, Cleanup, Reports, Why slow** are placeholders for now. Their engine parts exist (see
- **Cleanup.** Tick what to scan and press Scan: nothing is deleted. Review the list (every item, its size and why it is offered),
  untick what you want to keep, press Clean selected and confirm. Items wait 7 days in the quarantine (Undo, or Put back in the
  list below) or go to the Recycle Bin / Trash. A scan older than an hour must be repeated; files in use are skipped and named.
- **CPU taming, Health and tuning, Reports, Why slow** are placeholders for now. Their engine parts exist (see
  features.md); rules ship disabled and cannot act until you enable them.
- **Why slow** explains the likeliest cause of a slow PC: what was measured, which apps are behind it, where to look closer and
  the fixes Dyno proposes. Fixes that change a process show what they will do and wait for your click. **Run again** checks anew;
  **Copy as text** puts the result on the clipboard.
- **Reports** shows a summary of the week (pick the week at the top): health score and trend, the busiest apps, disk space and
  how soon a disk fills, what needed attention and what changed. **Export** saves it as Markdown or a web page wherever you
  choose; nothing is sent anywhere. A week without history says "Nothing to report".
- **CPU taming, Cleanup and Health and tuning** may still be placeholders; rules ship disabled and cannot act until you enable
  them (see features.md).

- **Throttling** says whether the processor is running slower than it could while it is busy, and why (heat, a power limit,
  battery saver, or unknown), how much speed it costs and how sure Dyno is. Below it are the episodes of the last 30 days and a list of
  what your machine does and does not report; when it reports too little, the page says "cannot tell" rather than guess.
- **Lifespan** shows how long your SSD and battery should last as a range ("about 23 months, 17 to 31"), what the estimate stands on
  and how sure it is. A new install needs a couple of weeks of readings first. If Dyno cannot read your SSD's wear it says what to
  install (smartmontools) or that the helper will do it later. A replaced disk or battery starts a new forecast.
- **Is this normal?** appears on the dashboard once Dyno has learned your usual week (7 days by default). It lists only what is out
  of the ordinary right now, in words ("CPU: higher than usual for Tuesday mornings, 71% against about 14%"). Change the learning
  period or switch alarms on in **Settings, Hardware and baselines**.

## Undo

Everything Dyno does to a process, a startup entry or (later) a file or setting is logged in **Activity** with an **Undo**
button where the OS allows. Rule actions release themselves when the program calms down, when the rule stops applying, when you
quit the app and when the agent stops, including after a crash. A process Dyno stopped or capped is never left that way: the
next agent start releases leftovers.

## Settings

| Setting | Meaning |
| --- | --- |
| Theme | System, dark or light |
| Launch at login | Start the app in the tray and register the agent with the OS |
| Tray metric and style | What the tray icon shows |
| Sampling | System and process intervals (Dyno slows itself down when nobody is watching or on battery) |
| History retention | How long raw points and each rollup tier are kept |
| Alerts | Which conditions raise an alert |
| Hardware and baselines | Whether and how long Dyno learns what is normal, and which hardware findings (throttling, disk and battery life, far-above-usual values) may raise an alarm |

Settings are versioned and saved with a backup; a damaged file falls back to the backup and then to the defaults.

## Command line

`dyno-agent` ships with the app (next to it, or in `Dyno.app/Contents/MacOS` on macOS):

```sh
dyno-agent status                      # is an agent running? prints pid, version, protocol
dyno-agent stop                        # ask the running agent to exit
dyno-agent once                        # one measurement as JSON; changes nothing
dyno-agent bench 25 processes          # Dyno's own CPU and memory for 25 s: none | summary | processes
dyno-agent service install|uninstall|status|ensure   # register it to start at login
dyno-agent run                         # run in the foreground (what the service starts)
```

## Files and logs

- Data folder: `%LOCALAPPDATA%\Dyno`, `~/Library/Application Support/Dyno` or `~/.local/share/dyno`: `dyno.db` (history),
  `settings.json`, `rules.json`, `process-control.json`, `agent.json`, `logs/`.
- Logs: the app's `logs/app.log` (small, rotating); the agent writes to its standard error, which the service keeps:
  `~/Library/Logs/Dyno/agent.log` on macOS, `journalctl --user -u dyno-agent` on Linux. Logs never contain file contents or
  command lines.
- Deleting `dyno.db` is safe: history starts again.

## Troubleshooting

| Symptom | What to do |
| --- | --- |
| The window shows an error with **Retry** | The agent did not answer in 15 s. Retry; if it persists run `dyno-agent status`, then `dyno-agent run` in a terminal to see why |
| No history after a reboot | The agent was not registered to start at login: turn on Launch at login |
| A tile says "unavailable" | Read the reason; the OS or hardware does not provide it (no battery, no sensor, needs rights) |
| An action says "needs the helper" | The process belongs to another user or the system; Dyno never elevates by itself (the helper is planned) |
| An action says the process "has exited" | Its id was reused or it ended; Dyno refuses to act on a different process with the same id |
| macOS: the agent does not start after an update | Dyno registers it again by itself; or `dyno-agent service ensure`; see macos.md |
| Linux: no tray icon | Install a status-notifier host (GNOME: the AppIndicator extension) |
| Windows: details, locks and startup are empty | Those adapters are not built yet (roadmap G8) |
