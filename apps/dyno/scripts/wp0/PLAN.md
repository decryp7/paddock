# WP-0 measurement plan (Dyno ADR 0033)

Run once on 2026-11-09 (results: ADR 0033, "WP-0 results"); these scripts repeat it. `held.ps1` and `launch-held.ps1` hold a file open for the
Restart Manager step (run `held.ps1` in a background SSH session of its own: a holder started inside a session dies with it).

Runs by hand or by an agent briefed for it, on `/Volumes/d7sst7/vms/paddock-win11-test` only (VMware Fusion, Windows 11 Arm64).
Never any other VM, never `/Volumes/d8sst7/vm-data`, never the CI PC (ER-14). The guest is reverted to its `gui-session-tools`
snapshot first and last; no snapshot is taken. Callers as in ADR 0027 WP-0: `paddock-test` (standard user, SSH = network logon, and
console session 1 through the `PaddockRunner` scheduled task when a measurement needs an interactive token) and `paddock-admin`.

Preconditions to verify before any measurement (ER-33): the revert worked (marker file absent), `whoami /groups` of `paddock-test`
has no Administrators, the build number is recorded, the channel is stable (no `BranchName`, no `EnablePreviewBuilds`).

Every probe is shown to see the thing before its result counts: each step in `wp0-collect.ps1` ends with a positive control (a key
the step created itself, or a value that must exist) and prints `CONTROL ok` or `CONTROL BROKEN`; a `BROKEN` control makes the step
`check broken`, not a result.

| # | Question (ADR section) | Step in `wp0-collect.ps1` | Output file | Decides |
| --- | --- | --- | --- | --- |
| 1 | StartupApproved bytes and key per source kind (4.1) | `approved-dump`, then toggle ONE entry per kind in Task Manager (Run, Run32 via an HKLM 32-bit value, StartupFolder user, StartupFolder common) and `approved-dump` again; `diff` | `approved-before.txt`, `approved-after.txt` | the table's "State from" column, the bytes the parsing tests use |
| 2 | Packaged-app `SystemAppData` keys on a fresh image, before any toggle, against Task Manager's Startup apps list (4.1) | `appx-keys` (also lists `uap5:StartupTask` of each package manifest with its `Enabled` attribute); the Task Manager list is read by eye and typed into `taskmgr-list.txt` | `appx-keys.txt` | whether the manifest enumeration (WP-5b base) is needed |
| 3 | `State` values after toggling each packaged task: enabled, off by the user, off by policy (4.1) | toggle in Task Manager and `Set-ItemProperty` the policy case, then `appx-keys` | `appx-keys-after.txt` | the state mapping |
| 4 | `MSFT_PhysicalDisk` as a standard user (4.2) | `physdisk` as `paddock-test` | `physdisk-std.txt` | disks fact needs the helper or not |
| 5 | Restart Manager's view of other accounts' processes as a standard user (4.3) | `rm-probe` as `paddock-test`, on a file held by a process of `paddock-admin` and one held by `paddock-test` | `rm-std.txt` | "another account" rows |
| 6 | Hidden and denied task folders as a standard user; whether a count is available (4.1) | `tasks-walk` as `paddock-test` and as `paddock-admin` | `tasks-std.txt`, `tasks-admin.txt` | the denied-folder sentence |
| 7 | May a standard user register a task with a logon trigger, and create a task folder (5) | `tasks-register` as `paddock-test` (registers disabled, prefix `DynoTest-<uuid>`, removes it) | `tasks-register.txt` | hygiene of the Windows-runner test |
| 8 | Power overlay GUIDs, effective mode (4.2) | `power-overlay` after setting each Settings > Power mode | `power-overlay.txt` | `power_plan` fact |
| 9 | `BackgroundAccessApplications` on 23H2+ (4.2) | `background-apps` | `background-apps.txt` | the fact or "no single switch" |
| 10 | Task Scheduler walk time and count on a fresh Windows 11 (4.1, Parallelism) | `tasks-walk` prints `COUNT`, `FOLDERS`, `MS` | in 6 | whether the walk needs a cache beyond `refresh()` |
| 11 | WMI disk query time (4.2, Parallelism) | `physdisk` prints `MS` (10 runs, median) | in 4 | the 250 ms / 30 min cache rule |

Sources to fetch and date before the code relies on them (ADR "Not yet fetched"): Run and RunOnce keys, WOW64 registry views,
KNOWNFOLDERID, ITaskService / TASK_TRIGGER_TYPE2 / ILogonTrigger::UserId / IExecAction, RM_PROCESS_INFO / RM_APP_TYPE,
RtlGetVersion, MSFT_PhysicalDisk, PowerGetActiveScheme / PowerGetEffectiveOverlayScheme, MSIX uap5:StartupTask.

Run: `./wp0-run.sh --step <name>` per step (it refuses any VM but the pinned one, reverts, copies the script, runs
it over SSH with the right user, fetches the output, and reverts again unless `--keep`). Results go to
`$WP0_OUT/<step>/` (default `$TMPDIR/dyno-wp0-out`; the script refuses a folder inside the repository) and are then summarised in ADR 0033 "WP-0 results" with the
build number and date; benchmarks (items 10, 11) also as a results block in `apps/dyno/docs/performance.md` (rule 48).
