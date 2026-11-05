# Dyno feature candidates after 1.1.0

Research note, 2026-11-06. It ranks features Dyno could add after 1.1.0. It is input for a decision, not a plan: nothing here
is approved. Dyno stays read-only ([ADR 0021](../adr/0021-monitoring-only.md)), so every candidate only watches.

The code was read on `dyno/wp-int` at `ada69e396`. Backup health and File History leave in 1.1.0 (user decision 2026-11-06,
because the coverage was too partial to trust), so no candidate builds on them.

## Short answer

Make 1.2 about Windows and about answering "why was it slow then?". Windows still has no process details, signer, file
locks, startup list or facts (gap G8), so Dyno is weaker on Windows than on the other two systems. The other three picks
reuse history Dyno already keeps.

| Rank | Feature | Dyno today | Size | Release |
| --- | --- | --- | --- | --- |
| 1 | Windows parity: details, signer, locks, startup, facts (G8) | lacks on Windows | 8 to 12 days | 1.2 |
| 2 | What changed before it got slow | partly | 3 to 4 days | 1.2 |
| 3 | Compare two periods | partly | 3 to 5 days | 1.2 |
| 4 | Send a moment to Debrief | partly | 2 to 4 days | 1.2 |
| 5 | Per-app disk and network in history | partly | 4 to 6 days | 1.3 |
| 6 | Connectivity and latency watch | partly | 3 to 4 days | 1.3 |
| 7 | Benchmark trend over runs | mostly has | 2 days | 1.3 |
| 8 | Metrics export endpoint (WP-16) | lacks | 4 to 6 days | later |
| 9 | Session recording (WP-31) | lacks | 5 to 7 days | later, or fold into 4 |
| 10 | Several machines in one window, headless Dyno | lacks | 15 to 25 days | later, ADR first |

Sizes are rough days for one agent, before design. Firm estimates come after each design note has work packages.

## What other monitors offer

Read on 2026-11-06 from the official pages unless marked.

- iStat Menus has sensors, per-core CPU, memory pressure, S.M.A.R.T. status, an internet connectivity indicator, rules
  that notify (public IP change, internet outage, sustained high CPU, low disk), a combined menu-bar item and top apps by
  CPU, memory, network and disk. The page does not say how long history is kept. <https://bjango.com/mac/istatmenus/>
- Stats (exelban) has CPU, GPU, memory, disk, network, battery, sensors and Bluetooth modules. It says Apple Silicon
  sensors are thermal zones whose keys change with each chip, and that sensors and Bluetooth are its costliest modules.
  <https://github.com/exelban/stats>
- Stats' enhancement issues, sorted by reactions, include "View usage history in expanded window" (#1194, open), "For CPU
  warnings, include more than x seconds" (#2436, open), notifications on high CPU or low RAM (#913) and a combined
  menu-bar view (#1084). Dyno already has history, custom alerts with a duration (`for_secs`,
  `crates/dyno-core/src/alarms/custom.rs`) and per-metric tray items. <https://github.com/exelban/stats/issues?q=is%3Aissue+label%3Aenhancement+sort%3Areactions-%2B1-desc>
- HWiNFO: the official site answered 403. Third-party guides and an official forum post say it logs sensors to CSV,
  alerts on thresholds and shares readings through shared memory. Not verified on hwinfo.com.
  <https://www.hwinfo.com/forum/posts/52685/>, <https://help.xmg.gg/hc/en-gb/articles/17824800029725>
- HWMonitor lists voltages, temperatures, power, fans, clocks, S.M.A.R.T. and batteries, CSV logging (1.62) and real-time
  graphs (1.64). <https://www.cpuid.com/softwares/hwmonitor.html>
- Process Explorer shows the handles and DLLs each process holds, with a search for which process holds a handle.
  <https://learn.microsoft.com/en-us/sysinternals/downloads/process-explorer>
- Process Monitor records file, registry and process activity with filters, thread stacks and boot-time logging.
  <https://learn.microsoft.com/en-us/sysinternals/downloads/procmon>
- Autoruns lists every autostart location (logon, services, drivers, scheduled tasks, Explorer add-ons, WMI and more),
  hides signed Microsoft entries, verifies signatures, checks hashes on VirusTotal and has a CSV command line.
  <https://learn.microsoft.com/en-us/sysinternals/downloads/autoruns>
- Activity Monitor has CPU, memory, energy, disk, network, GPU and cache views and system diagnostics. The page does not
  mention a 12-hour energy history. <https://support.apple.com/guide/activity-monitor/welcome/mac>
- Glances exports to CSV, InfluxDB, ElasticSearch and others, has a REST JSON API and a web UI, and watches Docker and
  Podman. <https://nicolargo.github.io/glances/>
- btop shows CPU, GPU (NVIDIA, AMD, Intel), disk I/O, network and a process tree on Linux, macOS and the BSDs.
  <https://github.com/aristocratos/btop>
- Netdata collects per-second metrics, runs anomaly detection on every metric, keeps tiered retention and streams
  several agents into a parent. The page does not describe its metric-correlation feature. <https://www.netdata.cloud/>
- Little Snitch has a world map of connections and a traffic history of up to 12 months, searchable by app, host and
  country. <https://www.obdev.at/products/littlesnitch/index.html>
- LuLu shows the process, its signer and a VirusTotal hash lookup for each new outgoing connection.
  <https://objective-see.org/products/lulu.html>
- CrystalDiskInfo: no official page read. Download sites describe a health status, S.M.A.R.T. graphs and mail or tray
  alerts. Not verified. <https://www.snapfiles.com/get/cdiskinfo.html>
- Windows Task Manager and Resource Monitor were not read in this pass.

What users ask for: forum threads ask which process slowed the PC earlier, and the answers say Windows keeps no such record
unless logging started before. Sources are forum posts, not surveys.
<https://techcommunity.microsoft.com/discussions/microsoftlearn/how-to-show-cpu-history-for-last-7-days/3609046/replies/3610064>,
<https://thwack.solarwinds.com/discussion/48235/cpu-spike-finding-the-responsible-process-es>

## Candidates

### 1. Windows parity (G8)

- Gives you on Windows what macOS and Linux already show: command line, modules, connections and signer per process,
  "what is locking this file", the startup list and facts.
- Dyno lacks it on Windows. `crates/dyno-platform/src/windows/mod.rs` wires `NoDetails` for details and takes locks,
  startup and facts from `crate::fake::Fake`. The "unsigned" flag of New and unusual also waits on it (roadmap WP-07).
- Read-only. Restart Manager (`RmGetList`) only asks who holds a file. The startup list reads the Run keys, the Startup
  folders and the scheduled tasks.
- Per OS it is Windows-only work. The user's own processes need no helper. Other accounts' processes and services come
  through the 1.1 helper.
- Risk: FFI in small `unsafe` modules, verified only on the Windows runner and the VM. Signature checks are slow with
  Defender on, so they run off the sampler and are capped, as on macOS.
- Value: high, because Windows is one of the three main systems and several pages are empty there today.

### 2. What changed before it got slow

- Shows installs, updates and new startup items on the history chart, and Why slow adds "changed 20 minutes before it
  got slow" as evidence.
- Partly there. Extras > What changed records installs, updates, removals and startup changes
  (`crates/dyno-core/src/extras/changes.rs`). History events have only `Alert`, `Boot` and `Network` kinds
  (`TimelineEventKind`, `crates/dyno-core/src/protocol.rs`), and `CauseKind` in `diagnosis.rs` has no change cause.
- Read-only. No helper. Change detection exists on all three systems, but the Windows and Linux readers are verified by
  fixtures only.
- Risk: a change near a slowdown is not its cause. The wording must say "before", never "caused".

### 3. Compare two periods

- Puts two windows side by side, such as "this Tuesday morning against last Tuesday": min, average and max per metric,
  the top apps in each, and the events in each.
- Partly there. `Request::History` takes a range and an end, and its reply carries `top_consumers` and events. Baselines
  compare now with the usual value, and the weekly report covers one week. No engine operation compares two windows.
- Read-only, no helper, the same on all three systems because it reads Dyno's own store.
- Risk: older data is coarser. Raw points last 24 hours, minute buckets 14 days, quarter-hours 90 days and hours 730 days
  (`Retention::default`, `crates/dyno-core/src/history.rs`). Both sides must use the same tier, and per-app rows
  (`app_minutes`) last only 14 days, so the page has to say so.

### 4. Send a moment to Debrief

- One click saves a chosen window (or a flight-recorder capture) with samples, events, issues, alarms and top apps as
  JSON lines that Debrief opens.
- Partly there. Issues already export for Debrief (`Request::SaveIssues`, `crates/dyno-agent/src/handlers.rs`), and
  Debrief reads JSON lines (`apps/debrief/docs/features.md`). Starting Debrief itself waits for WP-30.
- Read-only, no helper, the same on all three systems.
- Risk: privacy. The export has to reuse the diagnostics bundle's redaction and preview, because it carries process
  names.

### 5. Per-app disk and network in history

- Answers "which app was writing to the disk at 3 pm" after the fact.
- Partly there. `app_minutes` stores only CPU and memory (`crates/dyno-agent/src/store.rs`). Per-app network is shown
  live on macOS (`nettop`) and Linux (`sock_diag`), and the roadmap notes it is not stored (WP-13).
- Per OS this is uneven. Windows network per process needs administrator rights, so it needs the helper. Windows I/O
  counters count all I/O, not only disk. Linux `/proc/<pid>/io` covers only your own processes without the helper.
- Risk: the numbers mean different things per system, and the store grows. Label each source plainly.

### 6. Connectivity and latency watch

- Shows whether the internet is up, the time to the gateway and to a host you pick, and alarms on outages, as iStat Menus
  does.
- Partly there. Network change events record Wi-Fi joins, VPN and IP changes (WP-35). Nothing measures reachability.
- It sends small probes, so it is a network feature with its own switch (network features are on by default, decided
  2026-11-03). Use TCP connect timing, because unprivileged ICMP is not available on every Linux setup.

### 7. Benchmark trend

- Charts one benchmark metric across runs and marks a run outside the noise band.
- Mostly there. History is bounded at `MAX_RUNS = 200` and `compare` judges pairs against the noise band
  (`crates/dyno-core/src/bench.rs`). Only the trend view is missing.
- Value is modest, because few people benchmark often.

### 8. Metrics export endpoint (WP-16)

- A loopback-only HTTP endpoint with read-only metrics, for Grafana or scripts. Glances and Netdata offer this.
- Dyno lacks it. The 1.1 MCP tools are read-only but local to AI clients (`app/src-tauri/src/mcp.rs`).
- Risk: a new listening socket. Keep it off by default, loopback only, with a key in the OS credential store, as WP-16
  says. For one personal PC the value is low until several machines are watched.

### 9. Session recording (WP-31)

- Records chosen metrics to CSV or Parquet. Candidate 4 covers most of the need with less code, so fold it in unless
  Parquet is needed.

### 10. Several machines in one window, headless Dyno

- One dashboard for several of your PCs, possibly fed by a headless Dyno in Docker.
- Dyno lacks it. The roadmap asks for an ADR first (discovery, TLS, keys), and the headless design note is still pending.
- Risk: Docker on macOS and Windows runs Linux in a VM, so a containerised Dyno there sees the VM, not the host. A
  headless Dyno is accurate only on a Linux host, with host PID and network access. Size is large.

## Avoid

These cannot be accurate on all three systems, so they would break the "never a zero, always a reason" promise.

- Per-process energy outside macOS. Only macOS reports an energy impact per app.
- Per-sensor temperatures and fans on Apple Silicon beyond thermal zones, because the keys change with each chip (Stats).
- Bluetooth device batteries. They are reliable on macOS only.
- Boot-time tracing like Process Monitor's, which needs a kernel driver on Windows.
- Per-process GPU on Linux without NVIDIA's tool, already marked unavailable today.
- Fan control and anything else that acts (ADR 0021).

## Recommended order

1. 1.2: Windows parity (1), then What changed (2), Compare two periods (3) and Send a moment to Debrief (4). Items 2 to 4
   share the history store and can run beside 1 because they touch different files.
2. 1.3: per-app disk and network history (5), after the helper's Windows network reader is checked.
3. Later: 6 to 10, with WP-16 and the multi-PC ADR together, because the endpoint mostly matters once several machines
   are watched.

## Not verified

- HWiNFO's and CrystalDiskInfo's features come from third-party pages.
- How long iStat Menus keeps history, and Netdata's metric-correlation feature, are not stated on the pages read.
- Task Manager and Resource Monitor were not read.
- Sizes are judgement, not measured.
