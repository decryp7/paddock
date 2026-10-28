# Manual tests

What automated tests cannot cover: a real desktop, a real OS service manager, real hardware. Run these before a release on
each OS you ship, with the release build. Dyno only watches, so nothing here changes the machine; use dummy
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

## Processes

| # | Steps | Expected |
| --- | --- | --- |
| 12 | Start `sleep 1000` (or `ping -t`); find it in Processes | Grouped under its parent, with details; no action on it is offered |

## Service and updates

| # | Steps | Expected |
| --- | --- | --- |
| 21 | `dyno-agent service status` after Launch at login | "installed, running" |
| 22 | Install a newer build over the old one | The agent restarts into the new build; the app reconnects; no manual step (macOS: re-registration happens by itself) |
| 23 | `kill` the agent | It comes back within seconds (macOS, Linux); on Windows the app starts it |
| 24 | Updater with a signed test release | Verified, installed, restarted; a tampered `.sig` is refused |

## Privileges

Dyno 1.0 has no privileged helper and no way to run elevated. These checks run once per OS on the release build, with a
standard (non-administrator) account.

| # | Steps | Expected |
| --- | --- | --- |
| P1 | Install as a standard user; start Dyno | No administrator prompt appears at install or at start (Linux `.deb`: only `apt` asks for root); the agent runs as you |
| P2 | Open Processes | Your own and the system's processes are listed one by one; other accounts' processes are one **Other users** row (a second account logged in, or fast user switching, makes it appear) |
| P3 | Look at a system process's path, owner, CPU, memory and disk (per the OS table in ADR 0022) | Every figure the OS withholds says *Unavailable* with a plain reason; none shows 0 |
| P4 | Linux: remount `/proc` with `hidepid=2` (a VM) | The list says in one line that the system hides other accounts' processes |
| P5 | Check the process owner of `dyno-agent` and the app, and look for services, LaunchDaemons and systemd units named Dyno | The agent runs as you; no service or daemon exists |

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
| 31 | Corrupt `settings.json` while stopped | Dyno starts with the backup or defaults and says so |
| 32 | Make the data folder read-only | Dyno keeps working with in-memory history and says so |
| 34 | Inspect the data folder permissions | `agent.json` is readable by the owner only |
| 35 | Search the logs for a file path or command line you used | None present |

## Extras and backup health (WP-13, WP-14)

| # | Steps | Expected |
| --- | --- | --- |
| 36 | Extras > Sleep while a video call or music plays (macOS, Linux) | The app appears under "Keeping this computer awake" with its reason; on Windows without administrator rights the tab says only an administrator can list blockers |
| 37 | Extras > Boot after a restart and after a second one | Two rows; the verdict appears from the fourth boot on. Linux: `systemd-analyze blame` entries; Windows: slow apps from the Diagnostics-Performance log |
| 38 | Extras > Files, choose a folder with two identical large files | Both listed as one duplicate group; nothing can be removed from Dyno |
| 40 | Extras > Inventory > Export report, open the file | Hardware and software lists; nothing else |
| 41 | Extras > Maintenance: a window that is closed now, wait/inspect | "Next window opens ..."; no log read or backup check starts outside it; Rescan on Issues still works |
| 42 | Backup health with Time Machine on (macOS) | Healthy with the last backup time; unplug the disk: no-backup/cannot-check sentence with a next step |
| 43 | Backup health on Windows with File History or Windows Backup enabled | Verify the event IDs used (4 and 201) against your PC; report mismatches |
| 44 | Install restic through Homebrew, export RESTIC_REPOSITORY and RESTIC_PASSWORD_FILE in the shell profile, restart the agent | Found through the login-shell PATH; healthy or stale |
| 45 | Set the stale limit to 1 hour with a day-old backup | One alarm "... backup is out of date"; a new backup clears it at the next check |

