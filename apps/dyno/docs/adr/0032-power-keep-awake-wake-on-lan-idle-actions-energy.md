# 32. Power: keep awake, Wake-on-LAN, idle and timed actions, scheduled wake and an energy report

- Status: Proposed (2026-11-03). Decisions D1 to D11 below wait for the user; claims marked [V] wait for WP-0's VM checks.
- Date: 2026-11-03
- Trigger: the user's decision of 2026-11-03 to add power features to Dyno 1.1.0, inspired by Enviprot's ASDM
  ([enviprot.com](https://www.enviprot.com/en/)) without its enterprise parts (console, policies, AD portals, WOL proxy
  fleets, CO₂ savings marketing). Dyno stays a personal, non-commercial app on macOS, Windows and Linux.
- Related:
  - **Amends ADR [0021](0021-monitoring-only.md)** (monitoring only): power state becomes the one kind of machine change
    Dyno makes (D1). Nothing else of 0021 point 3 returns.
  - ADR 0027 (the helper for 1.1.0, `docs/dyno-helper-1-1`, accepted 2026-11-03) and `0027-amendments.md`
    (`feat/dyno-helper-ipc-1b`): caller identity from the connection, owner/administrator scope per request, closed
    request list, "Not added: anything that changes a setting" (amended by D2), no network in the helper, D8 (Linux helper
    for the `.deb` only).
  - ADR 0028 (tray items keep their place, `feat/dyno-tray-per-metric`): a Keep-awake tray item is one more item kind.
  - ADR 0031 and `0031-amendments.md` (network features): wire-contract and test shape copied; it reads no neighbour table.
  - ADR 0008 (SQLite and retention), ADR 0014 (local API and CLI), ADR 0023 (app shell, dialogs as OS windows),
    ADR 0024 (local days through the zone port), ADR 0025 (supervisor).
  - Family: `crates/paddock-jobs` (jobs, `time` clock port, fake clock), `paddock-proc` (`ProcessId`, timeouts),
    `paddock-mcp` (actions need an explicit opt-in, off until the user turns the server on), `paddock-settings`,
    `paddock-secrets`, `paddock-health`, `paddock-dialogs`; family ADR 0022 §G.7 (no `crates/` change inside a release
    window), ADR 0034 / rule 55, ADR 0041 / rule 60 (tests control time); rules 7 (ER-14), 10 to 13, 17, 19, 21, 42, 51
    to 53; ER-27; the principle of 2026-11-03 (network features on by default, each with its own switch).
- Evidence: [S] read in source on `origin/main` `b74c133c1`; [D] official documentation read 2026-11-03 (Sources);
  [M] measured on the build Mac, read-only; [I] inference; [V] to verify in WP-0 on a VM.

## Context

What Dyno already has that these features build on:

- **Idle time per OS** behind `dyno_core::ports::UserActivity::idle_secs` [S]: Windows `GetLastInputInfo`
  (`dyno-platform/src/windows/activity.rs`), macOS `CGEventSourceSecondsSinceLastEventType`
  (`macos/activity.rs`), Linux `loginctl` (`linux/activity.rs`), which says "not idle" until the desktop's own idle
  threshold passes, and knows no focus on Wayland.
- **Power source and battery** (`dyno-platform/src/battery.rs`, `starship-battery`) [S]: AC or battery, charge, time to
  empty. No battery reads as AC.
- **Live CPU, disk and network rates** from the sampler, and processes with `paddock_proc::ProcessId` (pid plus start time).
- **Energy figures**: per-app "Energy impact" on macOS (`extras/usage.rs`, an index, not watts) [S]; CPU package energy
  through the helper's `Counters(CpuEnergy)` (0027 §5: `powermetrics`, RAPL). No whole-system watts anywhere.
- **Not present** (correcting the brief): Dyno does **not** read ARP/NDP neighbour tables on `origin/main` or on the ADR 0031
  branches (grep for `GetIpNetTable`, `RTM_GETNEIGH`, `/proc/net/arp`, `NET_RT_FLAGS`: no hits). Dyno has no keep-awake,
  no MCP server (it uses no `paddock-mcp`), and no shared crate offers keep-awake. `Request::Shutdown` already exists and
  stops the agent, so the new requests are named `Power*`.

What the OS allows, per feature (sources at the end):

| Need | macOS | Windows | Linux | Rights |
| --- | --- | --- | --- | --- |
| Keep awake | `IOPMAssertionCreateWithName` `PreventUserIdleSystemSleep` / `…DisplaySleep` [D QA1340]; forced sleep (lid, menu, thermal) can only be delayed | `PowerCreateRequest` + `PowerSetRequest` `SystemRequired` (+ `DisplayRequired`) [D]; ended at user-initiated sleep, and on Modern Standby on battery 5 min after the sleep timeout [D] | logind `Inhibit("sleep:idle", "Dyno", why, "block")` returns an fd [D]; display: `org.freedesktop.ScreenSaver.Inhibit` where a desktop offers it [I] | user |
| Released at exit/crash | powerd drops a dead client's assertions [V] | kernel object closed with the process [I, V] | kernel closes the fd [D] | – |
| Others' "stay awake" | `IOPMCopyAssertionsByProcess` [I, V] | `powercfg /requests` needs administrator: 0027's `Extras(WakeRequests)` | `ListInhibitors` (all users) [D] | user; Windows helper |
| Idle time | have | have | have (see above) | user |
| Full screen / presenting | window list bounds [I, V] | `SHQueryUserNotificationState` `QUNS_BUSY`, `…D3D_FULL_SCREEN`, `…PRESENTATION_MODE` [D] | can't tell | user |
| Audio playing | `kAudioDevicePropertyDeviceIsRunningSomewhere` [I, V] | `IAudioSessionManager2` active sessions [I, V] | can't tell, except as an inhibitor | user |
| Sleep | Apple event `kAESleep` to loginwindow [D QA1134] | `SetSuspendState(FALSE…)`, `SE_SHUTDOWN_NAME` [I] | logind `Suspend(false)`; `CanSuspend` = `yes`/`challenge`/`no`/`na` [D] | user (polkit on Linux) |
| Hibernate | not offered (`hibernatemode` is a root setting) | `SetSuspendState(TRUE…)` if hibernation is on | logind `Hibernate`, `CanHibernate` | user |
| Shut down, apps may object | `kAEShutDown` to loginwindow: apps are asked to quit and can cancel [D QA1134]; may need Automation consent [V] | `ExitWindowsEx(EWX_SHUTDOWN\|EWX_POWEROFF, planned reason)` without `EWX_FORCE`: `WM_QUERYENDSESSION`; an app with unsaved changes holds the shutdown until the user acts [D] | logind `PowerOff(false)` asks no app; GTK apps' logout inhibitors are visible through gnome-session `IsInhibited` [I, V] | user |
| Wake at a time | `IOPMSchedulePowerEvent(kIOPMAutoWake)` / `pmset schedule wake`; "pmset must be run as root in order to modify any settings" [M: `man pmset`, Darwin 27.2] | `SetWaitableTimer(…, fResume=TRUE)` in the running agent; `ERROR_NOT_SUPPORTED` when the system cannot resume [D]; honoured only with "Allow wake timers" and, on Modern Standby, unreliably [V] | systemd timer `WakeSystem=true` "requires privileges and is thus generally only available in the system service manager" [D]; `rtcwake` writes the root-owned `wakealarm` [I] | **helper** on macOS and Linux; user on Windows |
| Sleep/wake events (energy log) | `IORegisterForSystemPower` [D QA1340] | `PowerRegisterSuspendResumeNotification` [I] | logind `PrepareForSleep` [D] | user |
| WOL send | UDP broadcast is a Local Network operation; a launchd **agent** needs the Local Network privilege and `AssociatedBundleIdentifiers` [D TN3179] | plain UDP | plain UDP | user |
| Neighbours | `sysctl` `NET_RT_FLAGS`/`RTF_LLINFO`, NDP [I] | `GetIpNetTable2` [I] | netlink `RTM_GETNEIGH` [I] | user |
| This PC's WOL | `pmset -g` `womp` readable [M] | adapter power settings (`MSPower_DeviceWakeEnable`, `MSNdis_DeviceWakeOnMagicPacketOnly`) [I, V]; fast startup disarms WOL from shutdown, WOL is supported from sleep and from an explicit hibernate [D] | `ETHTOOL_MSG_WOL_GET` needs `CAP_NET_ADMIN` [D]: **helper**; `/sys/class/net/<if>/device/power/wakeup` readable [I] | user; Linux helper |

## Decision

### 1. Shape

All four features live in Dyno's core and agent; the agent (the per-user login service, ADR 0010) holds every OS power
object, so state survives closing the window and dies with the agent. Pure logic in `dyno_core::power` (`awake`,
`idle`, `schedule`, `wol`, `energy`), OS adapters behind ports in `dyno-platform::power`, the keep-awake guard and sleep/wake
events in a new shared crate (D3). The UI is a **Power** page plus tray entries; the CLI gets `dyno awake`, `dyno wake`.

### 2. Keep awake

`AwakeSpec { scope: System | SystemAndDisplay, until: Indefinite | For(Duration) | At(LocalTime) | WhileProcess(ProcessId)
| WhileBusy { conds: Vec<BusyCond>, grace } }`, `BusyCond = Cpu { above_pct } | Network { above_bytes_per_s } | Disk {
above_bytes_per_s } | Process { program }`. One active spec at a time; a new one replaces it.

- The reason string the OS shows names Dyno and the spec ("Dyno: keeping awake for 1 h, you asked"), so `pmset -g
  assertions`, `powercfg /requests` and `systemd-inhibit --list` show the same truth as Dyno.
- **WhileProcess** holds the process, never its pid (rule 53): a process handle with `SYNCHRONIZE` (Windows), a pidfd
  (Linux ≥ 5.3), kqueue `EVFILT_PROC NOTE_EXIT` (macOS), each followed by a `ProcessId` start-time check after
  registration; where none works, a `ProcessId` check every 2 s.
- **WhileBusy** uses the sampler's existing rates (no new polling); the hold ends after every condition has been false for
  `grace` (hysteresis, default 2 min). "A download" is offered as "Network busy", the honest name.
- **Crash and exit:** the OS releases the hold with the agent. After a supervisor restart the hold is **not** re-taken;
  the tray says "Keep awake ended: Dyno restarted" with "Turn on again" (D4).
- When the OS ends the hold (user sleep, lid, Windows Modern Standby on battery), the state shows why and the hold is
  re-taken on wake if the spec still applies.

### 3. Wake-on-LAN

- **Devices** `WolDevice { id, name, mac, last_ip, interface, port (9), secure_on: bool }` in `dyno.db` (ADR 0008); the
  SecureOn password (4 or 6 bytes) in `paddock-secrets`, never in the database or logs.
- **Discovery:** a new `Neighbours` port (table above) lists MAC, IP, interface and state; "Add from network" shows entries
  not yet saved, the user picks. Nothing is stored without a click; host names come from Dyno's existing name cache only.
- **Send** (`dyno_core::wol::packet`, pure): 6 × `0xFF`, 16 × MAC, optional SecureOn bytes (AMD 20213). For each up,
  non-loopback IPv4 interface that matches the device's interface (or all, if it is unknown): the limited broadcast
  `255.255.255.255` from a socket bound to that interface's address, plus the directed broadcast of its subnet; 3 rounds
  1 s apart. Each send's result per interface is reported.
- **Test wake** is a `paddock-jobs` job: phase "Sending" (interfaces done of total), phase "Waiting for an answer"
  (seconds elapsed, growing); success when the MAC appears `REACHABLE` in the neighbour table with a fresh probe or the
  last IP answers an unprivileged echo (Windows `IcmpSendEcho2`, macOS `SOCK_DGRAM` ICMP, Linux ping socket if
  `ping_group_range` allows, else neighbours only). Default 120 s; Cancel in both phases. Answers: "Awake after 23 s
  (answered ping)", "No answer in 120 s: it may be off at the wall, have WOL off, or block ping", with the new IP saved
  if DHCP moved it.
- **This PC's readiness** (`WolReadiness`, every field `Known(v) | CantTell { reason }`): macOS `womp`; Windows adapter
  wake settings, fast startup state, Modern Standby (`powercfg /a` text, user-readable [V]), the hint "WOL from shut down
  depends on the BIOS/UEFI: Dyno can't tell"; Linux per-interface `wakeup` and, through the helper, the ethtool WOL modes
  (never the password).
- **Relay** through another Dyno: not built (D8).

### 4. Idle and timed actions

`IdleRule { action: Sleep | Hibernate | ShutDown, after: Duration, exceptions: Exceptions, ac_only: bool, countdown:
Duration }` and `TimedAction { action, at: LocalTime, repeat: Once | Daily | Weekdays(set) }`.

- **True inactivity** = `idle_secs ≥ after` **and** no exception holds. Exceptions (each a switch): audio playing,
  full screen or presenting, another program asked to stay awake (inhibitors, assertions, power requests; default on),
  CPU above X %, disk or network above a rate, a chosen program running, on battery (when `ac_only`).
- **Unknown is busy.** Any input the OS cannot give (`CantTell`) blocks the action and the Power page says which one.
  On Linux without an idle source the rule is "Not available here".
- **Countdown** (default 60 s, minimum 30 s): a notification with Cancel, a countdown dialog (an OS window, ADR 0023)
  with "Cancel" focused and "Snooze 30 min", and the tray header. Any input, any exception appearing, or a switch to
  battery under `ac_only` cancels it. At zero the agent re-reads every input once and acts only if all still pass.
- **Acting uses the OS's normal path, never a forced one (D5):** macOS loginwindow events; Windows `ExitWindowsEx` without
  `EWX_FORCE` / `SetSuspendState`; Linux logind with every block inhibitor respected and, before a shutdown, gnome-session
  or KDE's own inhibitors checked. Copy says: "Dyno asks your computer to shut down the normal way. Apps with unsaved
  work may stop it and ask you. Dyno can't save other apps' work."
- **Outcome watch:** after asking, the state is "Waiting for Windows to shut down" (no timeout, rule 12); if the agent is
  still alive after 2 min it says "Shutdown didn't happen: an app may be asking you to save", and the rule re-arms only after
  new activity. One cancel or one failed attempt never retries in the same idle period.
- Timed actions use the zone port (ADR 0024); a nonexistent local time (DST gap) runs at the first minute after it, a
  repeated one once.

### 5. Scheduled wake

`WakeEntry { id, at: LocalTime, repeat, then: StayAwake(Duration) + wake_devices: Vec<DeviceId> + job: Option<DynoJob> }`;
`DynoJob` is a fixed list (benchmark, weekly report), never a command line (D9).

- **Windows:** the agent sets a waitable timer with `fResume`; needs "Allow wake timers" (read and shown); works while the
  agent runs, which the page says; on Modern Standby `CantTell` until WP-0 measures it.
- **macOS and Linux `.deb`:** helper requests (D2). AppImage and no helper: "Needs the administrator helper" with the button
  that turns it on (rule 19).
- Nothing wakes from shut down (S5); the page says so.

### 6. Energy report

- Dyno logs its own power events (`booted`, `slept`, `woke`, `agent_started`, `agent_stopped`) from the OS notifications into
  `dyno.db`; time not covered is **Unknown (Dyno wasn't running)**, never guessed.
- On-time and sleep time per day. Energy per interval: measured where a whole-system figure exists (battery discharge on
  battery; macOS system input power if WP-0 finds a user-readable one), else `user_watts_on` / `user_watts_sleep`
  labelled "Estimated from your numbers"; CPU package energy is shown apart as "CPU only", never as a total. Cost = kWh ×
  tariff (one rate per kWh, currency from the locale). CO₂ only when the user gives a factor (g/kWh). No savings claims.

### 7. Wire model

New `Request` variants (Rust DTOs in `dyno_core::power`, mirrored in `app/src/app/core/api.types.ts`, checked by
`crates/dyno-core/tests/power_wire.rs` → `power.wire.json` and `power.contract.spec.ts`, as ADR 0031):

| Request | Reply / effect |
| --- | --- |
| `PowerState` | `PowerView { awake: AwakeView, idle: IdleView, timed: Vec<TimedAction>, wakes: Vec<WakeEntry>, caps: PowerCaps }`; also pushed on every change through `Subscribe` |
| `PowerKeepAwake { spec }` / `PowerStopKeepAwake` | `AwakeView` |
| `PowerIdleCancel` / `PowerIdleSnooze { minutes }` | `IdleView` |
| `PowerSetTimed { action }` / `PowerCancelTimed { id }` | `Vec<TimedAction>` |
| `PowerSetWake { entry }` / `PowerCancelWake { id }` | `WakeEntry` with `route: Agent \| Helper`, or `NeedsHelper` |
| `WolDevices`, `WolCandidates`, `WolSave { device, secret? }`, `WolRemove { id }` | lists |
| `WolWake { id, test }` | job id; `WolOutcome { per_interface, answered: Option<Answer>, elapsed }` |
| `WolReadiness` | `ThisPcWol` |
| `EnergyReport { range }` | `EnergyReport { days, totals, sources, unknown_secs }` |

`AwakeView = Off | Holding { spec, since, ends: Option, os_ref } | Waiting { spec, why } | Ended { why, at }`;
`IdleView = Disabled | Watching { idle_secs, blockers } | Countdown { action, deadline, cause } | Acting { action, since }
| Stopped { why }`. `PowerCaps` holds `Known | CantTell { reason }` per capability, so the UI never guesses.

**Helper (`HELPER_API` 3, amends 0027 §5):** `ScheduleWake { tag, at_utc }`, `CancelWake { tag }`, `ListWakes`, owner
and administrators only; `tag` is `[a-z0-9-]{1,32}`, at most 8 pending, not in the past, at most 31 days ahead. macOS:
`IOPMSchedulePowerEvent(date, "net.decryptology.dyno.<tag>", kIOPMAutoWake)`, listing and cancelling only events whose id
starts with Dyno's prefix. Linux: `systemd-run --unit=dyno-wake-<tag> --on-calendar=<utc> --timer-property=WakeSystem=true
/bin/true` with fixed arguments through `paddock-proc` and a timeout. `WolState { ifname }` (Linux, read-only, modes only,
any caller). Audit-logged like every helper request.

**MCP (D7):** Dyno adopts `paddock-mcp`: read tools `power_state`, `wol_devices`, `energy_report`; actions `wake_device`
and `keep_awake` (bounded duration, ≤ 12 h) only behind its action opt-in. Sleep, shutdown, timed actions and scheduled
wakes are never MCP tools.

### 8. State machines (pure, clock injected)

Both are `fn step(state, event, now: Instant, wall: ZonedTime) -> (State, Vec<Effect>)` over `paddock_jobs::time`'s
clock port; effects (`Acquire`, `Release`, `Notify`, `ShowCountdown`, `Recheck`, `Act`, `Log`) are run by the agent's
power thread (live work: normal priority, rule 52).

```
Keep awake:  Off --Start--> Holding --OsEnded(why)--> Waiting --Woke & spec applies--> Holding
             Holding --Stop | deadline | process exited | busy false for grace--> Off (Release)
             Holding --Acquire failed--> Ended(why);  any --agent restart--> Ended("Dyno restarted")

Idle action: Disabled --rule on--> Watching --idle ≥ after & no blocker & all known--> Countdown
             Countdown --input | blocker | battery(ac_only) | Cancel--> Watching (re-arm after new activity)
             Countdown --Snooze--> Watching(until now+snooze);  Countdown --deadline--> Recheck
             Recheck --all pass--> Acting (Act);  Recheck --any fails--> Watching
             Acting --resumed | 2 min alive--> Stopped(outcome) --new activity--> Watching
```

Invariants checked by property tests: `Act` only after `Recheck` passed with every input `Known`; never two `Act` in one
idle period; `Cancel` from any state yields no `Act`; every `Acquire` is matched by one `Release` or an OS end.

### 9. Settings (`paddock-settings`, versioned, defaults on missing or corrupt)

| Key | Default | Note |
| --- | --- | --- |
| `power.keepAwake.enabled` | on | acts only when the user starts it |
| `power.keepAwake.display` / `.grace` / thresholds | off / 2 min / CPU 50 %, net 500 KB/s, disk 5 MB/s | |
| `power.keepAwake.trayItem` | off | ADR 0028 item kind `awake` |
| `power.wol.enabled` | **on** | network feature; sends only on a click |
| `power.wol.discovery` / `.directedBroadcast` / `.testTimeout` | on / on / 120 s | |
| `power.idle.enabled` | **off** | acts on the machine |
| `power.idle.action` / `.after` / `.countdown` / `.acOnly` | Sleep / 30 min / 60 s / on | |
| `power.idle.exceptions.*` | all on except "chosen program" | |
| `power.timed.enabled` / `power.wake.enabled` | **off** / **off** | |
| `power.energy.enabled` | on | reads and stores Dyno's own power events |
| `power.energy.wattsOn` / `.wattsSleep` / `.tariff` / `.co2Factor` | unset | unset means "not shown" |
| `mcp.enabled` / `mcp.actions` | off / off | `paddock-mcp`'s own switches |

### 10. Safety

- Shutdown-class features are off by default; turning one on shows what it does and what Dyno cannot do.
- Never forced: no `EWX_FORCE`, no `interactive=false` override of a block inhibitor, no `kAEReallyShutDown`-style path.
- Another program's "stay awake" counts as possible unsaved or running work and blocks by default; switching that
  exception off asks once ("Dyno will ignore programs that asked to keep the computer awake").
- `ac_only` re-checked at zero; a laptop on battery never shuts down by Dyno unless the user turned `ac_only` off.
- Cancel works in every state from the tray, the dialog, the notification, the Power page and the CLI.
- **Logs (ER-27):** every transition with its inputs (idle seconds, each exception's value or `CantTell` reason, power
  source), why it did not act, the OS call with its result code, the outcome; keep-awake acquire/release with the OS
  reference; WOL per interface (destination, bytes, error); test-wake timing; helper wake ids. MAC addresses appear as the
  last two bytes and the device id; device names and the SecureOn password never.

### 11. Tests (rule 7 / ER-14, rule 60)

- **Pure core:** both machines on a fake clock, exhaustive over short event sequences plus proptests for the invariants;
  DST gap and fold; schedule caps (one over each); magic packet bytes for an invented, locally administered MAC
  (`02:00:5e:10:00:01`) with and without SecureOn; energy totals with gaps, partial days and unknown spans.
- **Fakes only in unit and integration tests:** `FakePowerOs` records `Acquire`/`Release`/`Act` and never calls the OS;
  `FakeNeighbours`, `FakeUdp` (records datagrams per interface), `FakeProber`, a fake helper link. Parsers (`powercfg`,
  `pmset -g`, `ip -j neigh`-shaped netlink records) on invented same-shape bytes, with fuzz targets.
- **Never on the user's or any real machine:** no test sleeps, shuts down, hibernates, wakes or keeps awake the machine it
  runs on, and no packet leaves loopback. Real adapters run only in VMs (`vm-smoke.json`, local only) behind
  `DYNO_VM_POWER=1`: macOS, Windows 11 (S3 and Modern Standby images where available), Ubuntu; WOL between two VMs on
  an isolated host-only network.
- **UI:** every state below with the mock backend; contract spec; ADR 0037 declarations for the page and settings.

### 12. Parallelism and cost (rule 51 / ER-20)

Nothing here is bulk work. Conditions reuse the sampler's values (no new polling); the idle machine steps on existing
ticks and one deadline; keep-awake holds one OS object; process exits are waited on by handle. WOL sends at most 3 × 2 ×
interfaces datagrams; a test wake probes once every 2 s for at most 120 s. Neighbours are read on demand, cached 5 s.
The energy report reads hourly rollups (8,760 rows a year), one query. No thread-count setting: nothing to tune (said in
the design, as rule 51 asks).

### 13. UI sketch

- **Tray menu:** header line when active ("Keeping awake, 42 min left" / "Sleep in 0:48, Cancel"); **Keep awake ▸** Off,
  30 min, 1 h, 2 h, Until I stop it, While busy, Until a program quits…, ☐ Keep the display on; **Wake ▸** up to 10
  devices, "Manage devices…"; **Cancel sleep** while counting down. Optional `awake` tray item (ADR 0028).
- **Power page**, five tabs: *Keep awake* (Off, Holding with time left, Waiting with why, Ended with why and "Turn on again",
  Not available with reason); *Wake devices* (table: name, MAC, last IP, interface, last wake result; empty state with
  "Add from network"; candidates list with skeleton, empty, error with Retry; per-row Wake and Test wake with the job's
  phases; This PC's readiness panel with `CantTell` reasons); *Sleep and shut down* (off explainer, Watching with live
  blockers, Countdown, Acting/Waiting, Stopped with outcome); *Schedule* (entries, route Agent/Helper, "Needs the helper",
  "Allow wake timers is off" with the setting's path); *Energy* (days chart and table, sources labelled Measured /
  Estimated / CPU only / Unknown, cost and CO₂ only when set).

## Decisions (for the user)

| # | Question | Recommended |
| --- | --- | --- |
| D1 | Amend ADR 0021 so Dyno may change power state (hold awake, sleep, hibernate, shut down, wake timers), and nothing else? | **Yes**, this list only; every other 0021 removal stays |
| D2 | Allow the helper three narrow write requests (`ScheduleWake`, `CancelWake`, `ListWakes`; Dyno-tagged, owner/admin, capped) on macOS and the Linux `.deb`, plus read-only `WolState` on Linux? | **Yes.** Fallback: no scheduled wake on macOS/Linux and a copyable `sudo pmset schedule wake …` hint |
| D3 | Put the keep-awake guard and sleep/wake notifications in a new shared crate `paddock-power` (Debrief imports, Sighter training, Pitwall and Pitcrew downloads plausibly need it); keep WOL and idle actions in Dyno? | **Yes**, landed in the slot family ADR 0022 §G.7 allows; until then in `dyno-platform` as a registered copy (rule 55) |
| D4 | Keep awake: system-only by default, display opt-in, 2 min hysteresis, not re-taken after a crash restart? | **Yes** |
| D5 | Idle/timed actions: off by default; default action Sleep; never forced; unknown input blocks; other programs' stay-awake requests block by default; AC only by default? | **Yes** |
| D6 | WOL: add devices only from the user's pick of neighbour entries or by hand; limited + directed broadcast per interface, port 9, 3 rounds; SecureOn in `paddock-secrets`; test wake by neighbour state or ping? | **Yes** |
| D7 | MCP: Dyno adopts `paddock-mcp` with read tools and only `wake_device` and `keep_awake` as actions behind its opt-in; no sleep/shutdown/schedule tools? | **Yes** |
| D8 | Relay through another Dyno on the far subnet? | **Not in 1.1.0**: it needs an authenticated LAN listener Dyno does not have; directed broadcast covers routed LANs that allow it |
| D9 | Scheduled wake does: stay awake N min, wake chosen devices, run a fixed Dyno job; never an arbitrary command? | **Yes** |
| D10 | Energy: measured only from whole-system figures; CPU energy apart; else the user's watts, labelled; tariff cost; CO₂ only with a factor; unknown spans shown? | **Yes** |
| D11 | Landing order within 1.1.0: keep awake → WOL → energy → idle/timed → scheduled wake; if 1.1.0's date slips, the last two move to 1.1.x? | **Yes** |

## Work packages

Agent-days, one agent each; WP-0 first.

| WP | Package | Depends on | Days |
| --- | --- | --- | --- |
| P0 | VM measurements for every [V]: assertion and request release on kill, `IOPMSchedulePowerEvent` as non-root (expect refusal), Automation consent for loginwindow events, Local Network prompt for the agent, Modern Standby request end and wake timers, polkit answers on Ubuntu, gnome-session `IsInhibited`, macOS system power source | – | 2–3 |
| P1 | `paddock-power`: keep-awake guard (3 adapters, fake, reason strings), sleep/wake event port; ≥ 99 % coverage | P0 | 3–4 |
| P2 | `dyno_core::power`: both machines, conditions, schedule with zone and DST, energy accounting, wire types, proptests | – | 4–5 |
| P3 | Agent power thread: effects, sampler conditions, process-exit holds (rule 53), settings, logs, CLI `awake`/`wake` | P1, P2 | 3–4 |
| P4 | WOL: packet, `Neighbours` port × 3, per-interface sender, test-wake job, device store, secrets | P2 | 4–5 |
| P5 | This PC's readiness × 3, helper `WolState` (Linux) | P4, 0027 helper | 2–3 |
| P6 | Idle/timed adapters: sleep, hibernate, shut down by the normal path; exception readers (audio, full screen, others' requests, desktop inhibitors) × 3 | P0, P3 | 4–6 |
| P7 | Scheduled wake: Windows timer in the agent; helper `HELPER_API` 3 requests with caps, audit and tests | P3, 0027 helper | 3–4 |
| P8 | Energy: power-event log and rollups, report, user estimates, tariff | P1, P2 | 2–3 |
| P9 | Dyno MCP server on `paddock-mcp` (read tools, two actions, audit) | P3, P4 | 2–3 |
| P10 | UI: tray entries, Power page (five tabs, every state), countdown dialog, settings sections, mock backend, specs | P3–P8 | 6–8 |
| P11 | Wire golden and contract spec; user guide, privacy text, `features.md` | P10 | 1–1.5 |
| P12 | VM checks and fixes (local only) | all | 3–4 |

**Total: 39 to 53.5 agent-days**, critical path P0 → P1 → P3 → P6 → P10 → P12 (about 21 to 29 days); P2, P4 and P8 run
beside it. This comes on top of ADR 0027 (43 to 65) and ADR 0031 (20 to 28) for 1.1.0.

## Risks

- **R1 macOS Local Network privilege** is tracked by code signature; Dyno is ad-hoc signed (rule 23), so an update may
  re-prompt or silently block WOL (compare the 1.0 feedback "signing id breaks macOS network"). Mitigation:
  `AssociatedBundleIdentifiers` in the agent plist, a `CantTell`/"Allow Dyno on the local network" state with the
  Settings path, P0 measures it.
- **R2 Windows Modern Standby** ends holds on battery and may not honour wake timers; shown, not hidden.
- **R3 Linux shutdown without a session manager** closes apps without asking; mitigation: Sleep default, the desktop's
  inhibitors checked, the copy says it.
- **R4 A wrong action on a person's machine** is the worst failure here; mitigation: off by default, countdown, recheck,
  unknown-is-busy, AC only, no retries, full decision logs.
- **R5 The helper gains its first writes** (D2), widening ADR 0027's trust story; bounded to Dyno-tagged wake events with
  caps and owner/admin callers.
- **R6 Schedule load:** ~40 to 54 more agent-days in 1.1.0; D11's order keeps the riskiest parts last and separable.

## Consequences

- Dyno is no longer "watches only": Settings and the README say "Watches your PC and, when you ask, manages its power".
- One new shared crate (D3) and one new family adoption (`paddock-mcp` in Dyno).
- The helper's request list grows by four; `HELPER_API` becomes 3.

## Sources (read 2026-11-03)

- Apple QA1340, Registering and unregistering for sleep and wake notifications / preventing sleep:
  https://developer.apple.com/library/archive/qa/qa1340/_index.html
- Apple QA1134, Programmatically causing restart, shutdown and/or logout (archived, 2008):
  https://developer.apple.com/library/archive/qa/qa1134/_index.html
- Apple TN3179, Understanding local network privacy:
  https://developer.apple.com/documentation/technotes/tn3179-understanding-local-network-privacy
- `man pmset` on the build Mac (Darwin 27.2), and `pmset -g` read as a user [M]; IOKitUser `IOPMLib.h`
  (`IOPMSchedulePowerEvent`, `kIOPMAutoWake`): https://opensource.apple.com/source/IOKitUser/ (root requirement for the
  call itself not stated there: P0)
- Microsoft, PowerSetRequest: https://learn.microsoft.com/en-us/windows/win32/api/winbase/nf-winbase-powersetrequest
- Microsoft, SetWaitableTimer: https://learn.microsoft.com/en-us/windows/win32/api/synchapi/nf-synchapi-setwaitabletimer
- Microsoft, ExitWindowsEx: https://learn.microsoft.com/en-us/windows/win32/api/winuser/nf-winuser-exitwindowsex
- Microsoft, QUERY_USER_NOTIFICATION_STATE:
  https://learn.microsoft.com/en-us/windows/win32/api/shellapi/ne-shellapi-query_user_notification_state
- Microsoft, Wake on LAN behavior (updated 2026-09-21):
  https://learn.microsoft.com/en-us/troubleshoot/windows-client/setup-upgrade-and-drivers/wake-on-lan-feature
- Microsoft, Modern Standby wake sources (secondary, not read in full):
  https://learn.microsoft.com/en-us/windows-hardware/design/device-experiences/modern-standby-wake-sources
- systemd, Inhibitor locks: https://systemd.io/INHIBITOR_LOCKS/
- systemd, org.freedesktop.login1: https://www.freedesktop.org/software/systemd/man/latest/org.freedesktop.login1.html
- systemd.timer (`WakeSystem=`): https://www.freedesktop.org/software/systemd/man/latest/systemd.timer.html
- Linux kernel, ethtool netlink (`ETHTOOL_MSG_WOL_GET`): https://docs.kernel.org/networking/ethtool-netlink.html
- util-linux rtcwake(8): https://man7.org/linux/man-pages/man8/rtcwake.8.html
- gnome-session D-Bus API (3.14 reference, marked unstable; secondary):
  https://gnome.pages.gitlab.gnome.org/gnome-session/re05.html
- AMD, Magic Packet Technology, publication 20213 (1995):
  https://www.amd.com/content/dam/amd/en/documents/archived-tech-docs/white-papers/20213.pdf
- Enviprot ASDM feature overview: https://www.enviprot.com/en/
