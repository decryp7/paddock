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

## The privileged helper (WP-01)

Nothing in the automated suite installs the helper; it is verified here, once per OS, on a spare machine or VM (not a machine
you cannot afford to change). Use the release build, and keep the Settings > Helper page open to watch the state.

| # | Steps | Expected |
| --- | --- | --- |
| H1 | Fresh install; open Settings > Helper | "Not installed"; no prompt appeared at any point before this |
| H2 | Tuning > Review on "Turn off hibernation" (Windows) / a Low Power Mode or sleep item (macOS) / the charge limit (Linux with one) | The helper dialog explains first and names the prompt; **Not now** closes it and nothing changed |
| H3 | Same, **Install the helper**, dismiss the OS prompt (Cancel / close) | Dialog shows that the prompt was closed and Dyno works; Settings still says Not installed; nothing under the helper's folder exists |
| H4 | Same, approve the prompt | The dialog closes, the normal review opens; Settings says Ready with the version; macOS: `launchctl print system/net.decryptology.dyno.helper` is running; Linux: `systemctl status dyno-helper`; Windows: service `DynoHelper` runs as LocalSystem |
| H5 | Apply, then check the setting with the OS's own tool (`pmset -g`, `powercfg /a`, sysfs) | It changed; Activity shows it with Undo; Settings > Helper lists it as Done with the earlier value in its log file |
| H6 | Undo from Activity | The setting is back; the helper log has a second line |
| H7 | Run the data folder check: macOS `ls -l "/Library/Application Support/Dyno/Helper"`, Linux `ls -l /var/lib/dyno-helper`, Windows `icacls "%ProgramData%\Dyno\Helper"` | Owned by root / SYSTEM; only `token` belongs to your account (or your SID has read); others cannot read the token |
| H8 | As another local user (or another program as you), connect to the endpoint (for example `nc -U /var/run/net.decryptology.dyno.helper.sock`) and type anything | No answer and the connection closes; the helper log shows "A connection was refused" with the reason |
| H9 | Kill the helper (`sudo kill -9`, `taskkill /f` for the service) | The OS restarts it within seconds; Settings is Ready again |
| H10 | Windows: Network page with the helper installed | Per-program traffic appears; without the helper it says to install it |
| H11 | Windows: startup item in "Registry (all users)" or the all-users Startup folder; macOS: an all-users launch agent | The switch asks for the helper first; turning it off works and Task Manager's Startup tab / `ls /Library/LaunchAgents` shows it disabled; Undo restores it |
| H12 | Windows: a Defender exclusion item | The folder appears under Windows Security > Exclusions; Undo removes it; with Tamper Protection on the item reports Defender's refusal |
| H13 | Update Dyno to a newer build | Settings > Helper says "Needs an update"; Update installs it with one more prompt |
| H14 | Settings > Helper > Remove | Registration, program, data and socket/pipe are gone (`ls`, `sc query DynoHelper`); the settings it changed stay; Dyno still works |
| H15 | Linux in a container or over SSH with no desktop | The dialog says why the helper cannot be installed (no polkit agent / no systemd) |

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

## Extras, backup health and taming proof (WP-13, WP-14, WP-36)

| # | Steps | Expected |
| --- | --- | --- |
| 36 | Extras > Sleep while a video call or music plays (macOS, Linux) | The app appears under "Keeping this computer awake" with its reason; on Windows without administrator rights the tab says only an administrator can list blockers |
| 37 | Extras > Boot after a restart and after a second one | Two rows; the verdict appears from the fourth boot on. Linux: `systemd-analyze blame` entries; Windows: slow apps from the Diagnostics-Performance log |
| 38 | Extras > Files, choose a folder with two identical large files | Both listed as one duplicate group; the older is kept by default; removing the other goes to the Trash/quarantine and is restorable; selecting both is refused |
| 39 | Extras > Files, choose your home folder and select a file in Documents | Refused: "in a personal folder" |
| 40 | Extras > Inventory > Export report, open the file | Hardware and software lists; nothing else |
| 41 | Extras > Maintenance: a window that is closed now, wait/inspect | "Next window opens ..."; no log read or backup check starts outside it; Rescan on Issues still works |
| 42 | Backup health with Time Machine on (macOS) | Healthy with the last backup time; unplug the disk: no-backup/cannot-check sentence with a next step |
| 43 | Backup health on Windows with File History or Windows Backup enabled | Verify the event IDs used (4 and 201) against your PC; report mismatches |
| 44 | Install restic through Homebrew, export RESTIC_REPOSITORY and RESTIC_PASSWORD_FILE in the shell profile, restart the agent | Found through the login-shell PATH; healthy or stale |
| 45 | Set the stale limit to 1 hour with a day-old backup | One alarm "... backup is out of date"; a new backup clears it at the next check |
| 46 | Enable a temporary rule for 1 minute, burn CPU in a dummy | Steps happen; a "Measured" entry appears about 20 s after a step; at the end the rule switches itself off and the app is given back |
| 47 | With a rule acting on a dummy, bring the dummy to the front | Its CPU limit is released at once and it is not touched while in front |

