# Dyno 1.1.0: release-note claims only the VM round can confirm

The release notes in [CHANGELOG.md](../CHANGELOG.md) (section 1.1.0) say these things. Unit tests and the model checks
cover the logic, but only a run on a real or virtual machine shows them true. Each row names the [testing manual](testing-manual.md)
row that confirms it. Until a row is run on the build the tag would ship, the claim is "not measured" (ER-33).

| Claim in the notes | Testing manual row |
| --- | --- |
| On Windows, 1.0.0 offers 1.1.0 but cannot install it; uninstall 1.0.0 first keeping data, then install 1.1.0; history stays | 24l, 24o (and 24n for macOS, which says it updates as usual) |
| Windows installs once for every user, in Program Files; a second account gets its own agent and data | 24a, 24m |
| Windows update shows one permission prompt; declining or a stopped installer starts Dyno again with Install again | 24d |
| After an update the helper is refreshed, or Settings offers Update the helper | 24d, 24f, 24j |
| Administrator helper: turn on, repair, update, turn off, each with one system prompt; without it everything works | 24j, 24k, P1, P5 |
| Helper readings (every account's processes, temperatures, fans, energy, memory modules, connection owners) | P2, P3, 24j |
| Repair of the login service does not queue behind a start; Quit waits for or cancels a repair; no second agent while registering | 21, 22, 23, 4 |
| Start at login on and off; a Linux unit you disabled stays disabled | 4, 5, 21 |
| Jobs view Cancel for the Start at login change | 4, 5 (cancel while the change runs; no row names Cancel yet, add one before the round) |
| macOS: no Keychain password prompt after an update; Wake-on-LAN passwords and AI access keys in owner-only files | 24e, 34, W7, W8 |
| Power rules (sleep, hibernate, shut down) show a countdown with Cancel | W3, W4, W5 |
| Snooze delays a timed power action; Cancel ends it for today | W5, W3 |
| Keep awake holds in every mode and ends when it should | W1, W2 |
| Wake-on-LAN: save, wake, test, and the readiness check of this computer | W7 |
| Scheduled wake: wakes the machine; a missed wake is shown as missed and never run late | W6 |
| A scheduled wake saved during a sync is not cancelled; the list of computers refreshes after a wake or a save | W6, W7 |
| AI access over MCP: off by default, keys, per-tool switches, every call listed, actions need permission by name | W8 |
| Dyno lowers its background threads on Linux under nice | 11 (Dyno's own CPU), run on a Linux VM started under nice |
| The update package is checked again before install; a damaged signature is refused | 24g |
| Windows: tray icon and window start at logon with no command prompt; the helper starts with the computer | 4, 21, 24j (restart the VM) |
| macOS: memory in use matches Activity Monitor; menu-bar icons do not flash; Lifespan reads the internal SSD | 7, 29 (compare with Activity Monitor on a Mac; no row for Lifespan, add one) |
| Quit and stop measuring stops the agent until the next open or login | 2, 3, 23 |
| Report a problem and "--export-diagnostics" write a report without a window | 35 (no file paths or command lines in the report), plus a run with a window that does not start |
| Backup health is gone and saved settings naming it still load | 31, 6 (start with a 1.0.0 settings file) |
| Network lists, world map and firewall view; Dyno never changes the firewall | no row yet; add one before the round |
| Benchmarks: write at most 14.5 GiB per drive and delete it; on battery it asks first | 8, plus a disk benchmark run watching free space |
| Saved views and the 1.0 dashboard upgrade after a restart | 10, plus starting from a 1.0.0 data folder |

Rows named "add one" have no check in the manual today; the round should not pass those claims until a row exists and was run.
