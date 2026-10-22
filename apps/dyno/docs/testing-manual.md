# Manual tests

What automated tests cannot cover: a real desktop, a real OS service manager, real hardware. Run these before a release on
each OS you ship, with the release build. Never run them on a machine where you cannot afford to lose a process: use dummy
programs (`yes > /dev/null`, `ping -t`, `sleep 1000`) as targets.

Record: date, OS and version, Dyno version, load average at the time, result per row.

## Install and start

| # | Steps | Expected |
| --- | --- | --- |
| 1 | Install from the release artifact; open Dyno | Splash, then the dashboard within a few seconds; no blank window |
| 2 | Quit; `dyno-agent status` | Either "running" (agent kept measuring) or "No agent is running." with a clear next step |
| 3 | Open Dyno again with no agent | The app starts one; tiles fill in |
| 4 | Settings > Launch at login on, log out and in | Dyno is in the tray; `dyno-agent status` says running; history has no gap longer than the logout |
| 5 | Settings > Launch at login off | The login registration is removed (macOS Login Items, `systemctl --user is-enabled dyno-agent`, the Run value) |
| 6 | Delete `dyno.db` while stopped, start | History starts again; an alert or note says so |

## Live data

| # | Steps | Expected |
| --- | --- | --- |
| 7 | Run a CPU burner (`yes > /dev/null`) | CPU tile and the app's row rise within a few seconds; stop it and they fall |
| 8 | Unplug the power (laptop) | The battery tile shows; Dyno's intervals double (Dyno CPU drops) |
| 9 | Desktop without a battery or sensor | The tile says why ("no battery"), never zero |
| 10 | Leave the window closed for 10 minutes, reopen | History has a continuous line for the gap |
| 11 | Watch "Dyno: x % CPU, y MB" with the dashboard open for 5 minutes | Stays near the targets in the roadmap; note the load average |

## Processes and actions

| # | Steps | Expected |
| --- | --- | --- |
| 12 | Start `sleep 1000` (or `ping -t`); find it in Processes | Grouped under its parent, with details |
| 13 | Lower its priority, then Undo in Activity | Priority changes and is restored; Activity shows both |
| 14 | Cap it at 20 % (with a CPU burner as target) | CPU falls to about 20 % of a core; Release restores it |
| 15 | Suspend, then Resume | State shows suspended, then running |
| 16 | Try a protected process (`launchd`, `systemd`, `lsass.exe`, the antivirus) | Locked, with the reason; no action possible |
| 17 | Kill the agent with `kill -9` while a cap is active (Unix) | The target stays stopped until the agent restarts; after restart it is continued |
| 18 | Same on Windows with `taskkill /f` | The named job survives; after restart the cap is released |
| 19 | Start a process, note its pid, let it exit and reuse the pid if you can; act on the old row | "has exited"; the new process is untouched |
| 20 | End a process that owns a window | It is asked to close first, then terminated after the grace period |

## Service and updates

| # | Steps | Expected |
| --- | --- | --- |
| 21 | `dyno-agent service status` after Launch at login | "installed, running" |
| 22 | Install a newer build over the old one | The agent restarts into the new build; the app reconnects; no manual step (macOS: re-registration happens by itself) |
| 23 | `kill` the agent | It comes back within seconds (macOS, Linux); on Windows the app starts it |
| 24 | Updater with a signed test release | Verified, installed, restarted; a tampered `.sig` is refused |

## Window, tray, native feel

| # | Steps | Expected |
| --- | --- | --- |
| 25 | Close the window; tray Open | A fresh window, not blank; macOS Dock reopen works |
| 26 | Linux without a tray host, close the window | The app quits |
| 27 | Right-click chrome; press F5, Ctrl +/- , Ctrl+P; select text in a path | No browser menu, no reload, no zoom, no print; the path can be selected and copied; Copy only inside the selection |
| 28 | Drop a file on the window | Nothing navigates |
| 29 | Dark and light themes, system switch | Follows the OS; no unreadable text |
| 30 | Resize to the minimum (900 x 600) | No clipped controls |

## Safety and robustness

| # | Steps | Expected |
| --- | --- | --- |
| 31 | Corrupt `settings.json` and `rules.json` while stopped | Dyno starts with the backup or defaults and says so |
| 32 | Make the data folder read-only | Dyno keeps working with in-memory history and says so |
| 33 | Enable a rule in `rules.json` while the agent is stopped (the CPU taming screen is not built yet), start it, burn CPU with a dummy | Steps happen in order; quitting the app or stopping the agent releases them |
| 34 | Inspect the data folder permissions | `agent.json` is readable by the owner only |
| 35 | Search the logs for a file path or command line you used | None present |
