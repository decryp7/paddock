# Dyno 1.1.0: wire every reading the helper advertises (plan, 2026-11-04)

Status: proposed. The user delayed Dyno 1.1.0 until every reading the admin helper advertises is wired end to end. This plan
splits the work into packages that up to four agents can build at the same time, with no file owned by two agents at once.

Read with: Dyno ADR 0027 (`docs/adr/0027-helper-for-1-1-0.md`) and its amendments; family ADR 0052 on
`origin/docs/adr-wiring-outcomes-ui` (the wired and outcome contracts, C83 to C87); `fix/dyno-windows-smart` (4377d9362, Smart
helper-first, `HelperFirstWear`); `docs/helper-verification.md` (what each reader returned on the three VMs).

Base: `origin/steward/dyno-1.1.0` at 1589c46bb. Every `file:line` below is at that commit unless it names another branch.

## 1. What is wrong, in one paragraph

The helper answers 19 request kinds (`dyno-core/src/helper_wire.rs:43-118`) and the Settings card promises a per-OS list of readings
(`helper_wire.rs:380-401`, `readings(host)`; repeated in `docs/user-guide.md:204-206`, `CHANGELOG.md` lines 33, 37 and 59, `app/src/app/features/power/energy-tab.ts:84` and `dyno-agent/src/firewall.rs:18`). The agent sends only `Info`, `Audit`,
`ScheduleWake`, `CancelWake`, `ListWakes`, `InstallTool` and `Uninstall` from production code (`helper_link.rs:98-216`; every
other `HelperRequest::` construction outside the helper crate is in a `#[cfg(test)]` module or `tests/`). Most helper-side readers
exist and were run on the three VMs (`helper-verification.md:97-108`), but nothing on the agent side asks them, and the helper itself
still refuses `Firewall` (`dyno-helper/src/dispatch.rs:278-283`). This is ADR 0052's pattern 1 ("built but never wired").

## 2. Per reading: helper side, agent side, wire, UI, tests, security, size

Legend for "helper side": **exists** = production reader wired into `Parts` in `dyno-helper/src/serve.rs:99-119`; tests named.
"Same domain type" = the helper reply and the local reader produce the same Rust type, so the contract test is an equality over
one fixture; "convert" = a pure conversion function is needed and is what the contract test pins.

Every agent-side reading is built on the seam of §3 (`HelperFirst<R>`): **helper Ready → helper answer; else local fallback;
else an honest needs-admin reason** with the next step ("Turn on Administrator readings in Settings"). Each reading's reply on the
agent wire carries `via: ReadVia` (§3.3), which the UI shows as described in §3.4.

### 2.1 Smart (all OS where the helper reads it: Windows, Linux) - being fixed

- Helper: exists (`SmartctlReader`, `dyno-helper/src/tools.rs` `smart_reader`); fixed further on `fix/dyno-windows-smart`.
- Agent: `HelperFirstWear` on that branch (`dyno-agent/src/helper_wear.rs`, composed at `agent.rs:504` there).
- This plan: merge that branch first (WP-00), then WP-0 re-expresses `HelperFirstWear` as the first `HelperFirst<R>` instance
  (behaviour unchanged, its tests kept). No new size beyond WP-0.

### 2.2 Firewall: pf (macOS), nft (Linux) - WP-FW

- **Helper side.** Readers exist and are tested: `PfReader` and `NftReader` (`dyno-platform/src/firewall_readers.rs:82-131`, unit
  tests in the same file over a fake `ToolRunner`; parsers `parse/firewall.rs`). They are **not** in `Parts`, and the handler answers
  `UNSUPPORTED` (`dispatch.rs:280-283`). To write: a `firewall: Box<dyn FirewallReader>` part, chosen per host in `serve.rs`
  (macOS `PfReader` over `/sbin/pfctl`; Linux `NftReader` over a fixed `/usr/sbin/nft`, an absent binary answering `Unsupported`
  "nftables is not installed"; Windows `Unsupported`, the agent reads Windows Defender Firewall itself), and the handler arm
  `reader.read()` then `FirewallSource::for_access(ctx.access)` (`dyno-core/src/firewall.rs:175`, which already reduces to counts for
  `Access::CountsOnly`, the restriction table's value for non-privileged callers, `helper_policy.rs:141`).
- **Agent side.** Merge into `FirewallCentre` (`dyno-agent/src/firewall.rs`): today `helper_sources` become placeholder sources with
  `NEEDS_HELPER` (`firewall.rs:62-67`). Replace each with a `HelperFirst` source over `HelperLink`'s existing
  `impl FirewallReader` (`helper_link.rs:223`): Ready → the helper's `FirewallSource`; not Ready → the same placeholder with the
  needs-admin reason; no local fallback (`pfctl -s` and `nft list` need root, ADR 0027 WP-0 results). Composition hook:
  `FirewallCentre::for_this_os` takes the helper reading as a parameter (one declared line in `agent.rs`).
- **Wire.** `HELPER_API`: none (request and reply exist). App-agent: `FirewallSource` gains `via` (WP-0 adds the field).
- **UI.** Firewall page, per source card: via helper → rules table (owner/admin) or counts ("12 rules; only the owner or an
  administrator sees them"); unavailable → the card's empty state with the reason and a "Turn on administrator readings" button
  (opens Settings > Helper). Text at `firewall.rs:18` becomes the seam's shared needs-admin sentence.
- **Tests.** Failing first: `dispatch::tests::firewall_answers_the_readers_view_and_counts_only_for_other_callers` (today returns
  `UNSUPPORTED`). Contract: `FirewallSource` from `PfReader` over a recorded fixture equals what the agent shows after a round trip
  through `HelperReply::Firewall` (postcard) and `FirewallCentre`. Real-engine e2e (§4.2 rig): `Request::Firewall` over the agent's
  IPC, helper rig with a fake `pfctl` runner, asserts `via = helper` and the rules. VM: macOS (Tart) and Linux (§5).
- **Security.** Fixed programs and arguments only; no caller input reaches the reader (the request has no fields); reply capped at
  `MAX_FIREWALL_RULES` 4 096 and `MAX_RULE_TEXT` 256 (`firewall.rs:10-12`); `Slow` weight (one per account at a time,
  `helper_policy.rs:79-90`); rule addresses never logged (`firewall_readers.rs:59`).
- **Size:** 2 agent-days (helper 1, agent and UI 1). Depends on WP-0 for the agent half; the helper half can start at once.

### 2.3 LoginItems / BTM (macOS) - WP-BTM

- **Helper side.** Exists and is tested: `SfltoolReader` (`dyno-platform/src/btm_reader.rs`, 2 tests), wired in
  `serve.rs:155-161`; the handler scopes by the connection's uid (`dispatch.rs:201-206`). Verified on macOS 26.6 (ADR 0027 WP-0).
- **Agent side.** `LaunchAgents::for_current_user` always builds `NeedsHelperBtm` (`dyno-platform/src/macos/startup.rs:178`;
  composed by `macos/mod.rs:30`). `HelperBtm<L: LoginItemsLink>` exists (`startup.rs:42-66`) but is never built. The platform is built
  before the agent's `Shared` exists, so the link must be late-bound: a `SharedLoginItems(Weak<Shared>)` that implements
  `LoginItemsLink` through the seam (Ready → `HelperLink::login_items`; else `AccessDenied` with the needs-admin reason), passed to
  `LaunchAgents::with_btm(..)` from the composition (one declared line in `agent.rs`). Local fallback: none (`sfltool dumpbtm` asks for
  a password as a user; `startup.rs:30-35`).
- **Wire.** None on `HELPER_API`. App-agent: the startup list reply gains `via` for the BTM part (WP-0's field on the "not shown"
  note structure).
- **UI.** Startup page: via helper → login items and app-registered agents listed with the launch agents, a source chip "Read by the
  helper"; unavailable → the existing "Not shown: Login items ..." line (CHANGELOG line 59) with the reason and the Turn on button.
- **Tests.** Failing first: `startup::tests::a_ready_helper_lists_login_items_beside_launch_agents` (today `NeedsHelperBtm` is
  hard-wired). Contract: `LoginItemsReply` from `SfltoolReader` over the recorded dump equals the one that crosses the wire.
  E2E: agent rig, `Request::Startup` (or its current name) lists a BTM item with `via = helper`. VM: macOS (Tart).
- **Security.** The uid comes from the connection (`ctx.uid`), never the request; other accounts' items only counted
  (`btm_reader.rs:1-3`); `Slow` weight; 15 s timeout.
- **Size:** 1.5 agent-days. Depends on WP-0.

### 2.4 Processes of every account (Windows, macOS, Linux) - WP-PROC

- **Helper side.** Exists and is tested: `SystemSweeper` (`dyno-platform/src/helper_sweep.rs`, 5 tests), `build_page`
  (`dyno-core/src/helper_processes.rs:301`, scope tests from line 393); run on all three VMs (`helper-verification.md:103-107`).
- **Agent side.** Merge point: the process table the sampler fills from `ProcessSource` (`dyno-core/src/ports.rs:81-87`), and the
  "Other users" fold (`dyno-agent/src/sampler.rs:19, 220-234`; UI `processes-page.ts:55`). Build `HelperFirstProcesses`, a
  `ProcessSource` decorator: the local source stays the source of the caller's own processes (it has rates); the helper's pages fill
  the processes the local source cannot see (other accounts, protected system processes). Join key `(pid, started)` (rule 53: never
  the pid alone). Rates for helper rows come from deltas of the cumulative `cpu_time_ms` and disk bytes between two sweeps (a pure
  function with its test). The helper sweep runs on its own worker every 5 s (a setting, "Auto"), never on the sampling thread
  (ER-17); the sampler takes the latest sweep. Paging: `MAX_PROCESS_PAGE` 1 024 (`helper_requests.rs:9`), the worker follows `next`
  up to `MAX_SWEEP` 16 384. When the helper answers `others: Some(OthersTotal)` (a non-owner caller under the default policy), the
  "Other users" row shows that total instead of what the agent could add up.
- **Convert:** `ProcessEntry` (helper) → `ProcessSample` (agent): a pure `sample_of(entry, previous)`; `OwnerKind::OtherUser` without
  an account name stays "Another account".
- **Wire.** `HELPER_API`: none. App-agent: the process snapshot gains `via` per row source (`helper`/`local`) and one
  `others_via` for the total. Protocol 4 is unreleased: append fields with serde defaults.
- **UI.** Processes page: via helper → every account's processes, an "Account" column filled; non-owner callers see the "Other
  users" total row that now says "(counted by the helper)"; local only → today's view plus one line "Other accounts' processes need
  the helper. Turn on administrator readings." Never colour alone.
- **Tests.** Failing first: `helper_processes_agent::tests::another_accounts_process_appears_when_the_helper_is_ready` (fake link
  page with an `OtherUser` entry; today the table never has it). Contract: one `Sweep` fixture → helper `build_page` → `sample_of`
  equals the `ProcessSample` the local source builds for the same fake process (name, pid, started, owner kind, memory). E2E: agent
  rig, `Request::Processes` lists the other account's process with `via = helper`; a second sweep gives a CPU rate. VM: all three OS
  (Windows as a standard user plus the owner, to see `Own(Total)`).
- **Security.** Scope from the connection's account and the policy file (`dispatch.rs:207-218`), never a request field; allow-listed
  fields only, no command line (test `the_wire_roundtrips_and_has_no_field_for_a_command_line`, `helper_processes.rs:573`); other
  accounts' profile paths cut to a file name; pages and sweep capped.
- **Size:** 3.5 agent-days (largest; the merge and rate logic). Depends on WP-0.

### 2.5 Traffic per program (Windows) - WP-TRF

- **Helper side.** Exists: `WinTraffic` (`dyno-platform/src/windows/traffic.rs`, 1 test), wired in `serve.rs:26-39`.
  **Security finding (P1):** the handler returns every program's counters to any caller (`dispatch.rs:273-277`; `access()` gives
  `Traffic` `Full` to everyone, `helper_policy.rs:137-143`), although `Visibility` is documented as "what a caller sees of processes
  and traffic" (`helper_policy.rs:159`). A standard user would see other accounts' programs by pid and name. To write:
  `traffic_answer(counters, sweep_owners, caller, scope)` in `dyno-core/src/network_traffic.rs` (owners from the same
  `ProcessSweeper`), mirroring `build_page`'s scoping, with other accounts as one total under `OthersPolicy::Total`.
- **Agent side.** Merge point: `NetCentre`'s traffic port (`dyno-platform/src/network.rs:143-167`; Windows calls `WinTraffic` locally,
  which says it needs administrator rights). `HelperFirst` over `HelperLink::traffic()`: Ready → helper counters; else local (which on
  Windows answers `Unsupported` "administrator"), so the reason is the seam's needs-admin sentence. Same domain type
  (`Vec<TrafficCounter>`).
- **Wire.** `HELPER_API`: `HelperReply::Counters(Vec<TrafficCounter>)` becomes `Traffic(TrafficAnswer { counters, others })` (free to
  change; keep a `wire_roundtrip` sample per variant, `dyno-core/tests/wire_roundtrip.rs:966` enforces it). App-agent: `via` on the
  traffic part of the network snapshot.
- **UI.** Network page, "By program": via helper → per-program bytes; others as one "Other users" row for non-owners; unavailable →
  the reason and the Turn on button (today the column says Unsupported).
- **Tests.** Failing first: `dispatch::tests::traffic_is_scoped_to_the_caller` (today a non-owner gets the other account's row).
  Contract: helper and local paths both give `Vec<TrafficCounter>` from one fake `GetPerTcpConnectionEStats` fixture. E2E: agent rig,
  `Request::Network` shows per-program bytes with `via = helper`. VM: Windows (owner and a second standard account).
- **Size:** 2 agent-days (scoping 1, wiring 1). Depends on WP-0 and on WP-FW's helper half having landed in `dispatch.rs` (sequenced,
  not concurrent).

### 2.6 SocketOwners and FileHolders (Linux) - WP-LNX

- **Helper side.** Exists and is tested: `ProcHolders` (`dyno-platform/src/helper_holders.rs`, 3 tests over a temporary `/proc`);
  `socket_answer`/`file_answer` (`dyno-core/src/helper_holders.rs:145-190`, 3 tests); run on Ubuntu (137 sockets, the test's own file
  holder found, `helper-verification.md:107`).
- **Agent side, sockets.** Merge point: `NetworkSockets` (`ports.rs:39-44`) → `NetCentre::look` (`dyno-agent/src/network.rs:92`).
  On Linux, sockets whose owner the user cannot see have `pid: None`. `SocketRow` has no inode (`dyno-core/src/network.rs:74-87`), so
  the Linux reader must keep the inode it already parses (`parse/linux_proc` `socket_inode`) in a Linux-only field, and the seam fills
  `pid` and name from the helper's `SocketOwner { inode, holder }`. Fallback: local rows as today with "owner hidden" and the reason.
- **Agent side, files.** Merge point: `FileLocks` (`ports.rs:89-93`; Linux `LinuxLocks`, own processes only). The agent `stat`s the
  path the person chose itself and sends only `(dev, ino)` (`helper_wire.rs:53-58`); the helper never opens the file. `Holder` →
  `Locker` is a pure conversion.
- **Wire.** `HELPER_API`: none. App-agent: `via` on the socket list and on the "What holds this file?" reply.
- **UI.** Network page: via helper → every socket has its program; local → "Owner hidden (another account)" with the reason.
  Files "What is holding this?": via helper → holders of every account, scoped; local → own processes and the reason.
- **Tests.** Failing first: `network::tests::a_hidden_socket_owner_is_filled_from_the_helper`; `files::tests::holders_come_from_the_helper_by_device_and_inode`.
  Contract: one fake `/proc` tree → `ProcHolders` + `file_answer` → `Locker` equals what `LinuxLocks` gives for the caller's own
  process. E2E: agent rig on Linux CI. VM: Linux.
- **Security.** No path crosses the wire (dev and ino only); scoping by connection account (`dispatch.rs:219-232`); caps
  `MAX_SOCKETS` 65 536, `MAX_HOLDERS` 256 (`helper_holders.rs:13-17`); 20 s scan budget (`tools.rs` `holder_source`).
- **Size:** 2.5 agent-days. Depends on WP-0.

### 2.7 Counters: temperature and energy (macOS) - WP-CNT

- **Helper side.** Exists and is tested: `SystemCounters` (`dyno-platform/src/helper_counters.rs`, 7 tests). On macOS it answers CPU
  energy and fans from `powermetrics` (`helper_counters.rs:236-243`) and **refuses temperatures by design**: "macOS reports a thermal
  pressure level, not temperatures, to software" (`helper_counters.rs:244-246`). On Apple silicon there is no `smc` sampler, so no
  fans; in the VM no CPU power was listed, and the parse of real `powermetrics` output on hardware is not verified
  (`helper-verification.md:106`). So the macOS claim "Temperature and energy readings" (`helper_wire.rs:393`) cannot be met as worded
  (decision D2).
- **Agent side.** Two merge points: (1) temperatures/fans into `SystemSample` (`dyno-core/src/metrics.rs:259` `temperatures:
  Reading<Vec<Temperature>>`), filled by `SysinfoSensors` (`dyno-platform/src/sensors.rs:52` "reports no temperature sensors"); the
  seam adds the helper's `Thermal`/`Fans` values, read every 30 s on a worker (powermetrics takes about a second), cached;
  (2) energy into `EnergyReport` (`dyno-core/src/power/energy.rs:205-220`), whose `EnergySource` today has only `Estimated` and
  `NotSet` (`energy.rs:175-180`): add `Measured { what: CpuPackage }` and keep the person's estimate beside it (CPU-only energy is
  not the machine's). Convert: `ThermalZone` → `Temperature`, `EnergyDomain` deltas → joules per day (pure, tested).
- **Wire.** `HELPER_API`: none. App-agent: `EnergySource::Measured`, `via` on temperatures.
- **UI.** Energy tab: via helper → "CPU energy measured by the helper: x kWh" beside the estimate; the foot text at
  `energy-tab.ts:84` ("CPU-only energy and measured readings arrive with the helper") goes; unavailable → the helper's own reason
  ("powermetrics listed no CPU power, as in a virtual machine"). Temperatures widget: via helper → values; macOS → "macOS reports a
  pressure level, not temperatures" (the honest reason), never an empty chart.
- **Tests.** Failing first: `power::tests::measured_cpu_energy_is_reported_beside_the_estimate`. Contract: one `powermetrics` fixture
  → helper `CounterValue::CpuEnergy` → `EnergyDay` joules equal a hand-computed value. E2E: agent rig, `Request::Energy` with
  `source = measured`. VM: macOS (Tart: expects the "no CPU power listed" reason; a real Mac is needed to verify values, decision D3).
- **Security.** Counter ids are a closed enum, at most `MAX_COUNTER_IDS` 8 per request (`helper_requests.rs:13`); fixed program and
  arguments; values bounded (`helper_counters::bounded`, `dyno-core/src/helper_counters.rs:104`).
- **Size:** 3 agent-days (energy domain 1.5, sensors 1, UI 0.5). If D2 widens it to Linux RAPL and Windows thermal (readers exist:
  `helper_counters.rs:233-260`), add 0.5 day each OS for the VM rows only; the seam makes the code the same.

### 2.8 MemoryModules (Linux) - WP-MEM

- **Helper side.** Exists: `SystemMemory` (`helper_counters.rs:269-277`) calls the same `read_memory_layout` the agent uses
  (`dyno-platform/src/inventory/mod.rs:17`; Linux reads `/sys/firmware/dmi/tables/DMI`, root only, `mod.rs:34-48`). Same domain type
  (`MemoryLayout`). Tests: the parser's tests in `inventory/linux.rs`; no dispatch test of the reply (add one).
- **Agent side.** Merge point: `dyno-agent/src/inventory.rs:63`. Seam: Linux and Ready → helper; else local, whose
  `TotalOnly { reason: linux_dmi_root_only }` becomes the needs-admin reason with the next step.
- **Wire.** None on `HELPER_API`; `via` on the inventory's memory part.
- **UI.** Inventory > Memory: via helper → slots table; local → total only and the reason.
- **Tests.** Failing first: `inventory::tests::linux_slots_come_from_the_helper_when_it_is_ready`. Contract: same type, one DMI
  fixture through both paths gives equal `MemoryLayout`. E2E: agent rig. VM: Linux (a VM publishes no slot table, so the check is the
  reason text, `helper-verification.md:107`; real hardware is decision D3).
- **Security.** No input; fixed path; reply size bounded by the slot count.
- **Size:** 1 agent-day. Depends on WP-0.

### 2.9 Extras: wake requests and Windows Backup versions (Windows) - WP-EXT

- **Helper side.** Exists: `WindowsExtras` (`dyno-platform/src/helper_extras.rs`, 1 test) runs `powercfg /requests` and
  `wbadmin get versions`; parsers `parse_requests`/`parse_versions` (`dyno-core/src/helper_extras.rs:105-175`, 3 tests); counts only
  for non-privileged callers (`helper_policy.rs:141`). Run on Windows: 0 and 0, no error (`helper-verification.md:105`).
- **Agent side.** (1) What keeps the computer awake: `sleep_report` (`dyno-platform/src/extras.rs:58-67`) runs `powercfg /requests`
  locally and says "Windows only lets an administrator list what keeps it awake" when refused. (2) Backup check: `windows_backup`
  (`extras.rs:394-420`) tries the event log first, then `wbadmin get versions` locally (administrator only).
  **Rule 55 finding:** two parsers of the same `powercfg /requests` output (`dyno-core/src/extras/sleep.rs:140` `parse_powercfg_requests` and
  `helper_extras::parse_requests`) and two of `wbadmin get versions` (`backup.rs:279` `parse_wbadmin_latest` and
  `helper_extras::parse_versions`). Keep one each (the helper's types become the domain types, or the converse), delete the other
  in the same change, failing test first; no new `known-copies.toml` entry.
- **Convert:** `WakeRequest` → the sleep report's blocker; latest `BackupVersion` → `backup::judged`.
- **Wire.** None on `HELPER_API`; `via` on `SleepReport.blockers` and on the Windows Backup status.
- **UI.** Extras > Sleep: via helper → the list of requesters with reasons; local refused → today's sentence plus the Turn on
  button. Backup health: via helper → "Last Windows Backup version: <date>" judged as today; counts-only callers → "n versions".
- **Tests.** Failing first: `extras::tests::wake_requests_come_from_the_helper_when_powercfg_is_refused`. Contract: one `powercfg`
  fixture through both parsers gives the same blockers (until one parser is deleted; then the test pins the survivor on both paths).
  E2E: agent rig, `Request::SleepReport` and `JobSpec::RefreshBackup` with `via = helper`. VM: Windows.
- **Security.** Fixed programs and arguments, 15 s, `Slow` weight; detail only for owner/admin.
- **Size:** 2 agent-days. Depends on WP-0.

## 3. The common seam: `HelperFirst<R>` (WP-0, rule 55)

### 3.1 Shape

In `dyno-agent/src/helper_first.rs` (new):

```rust
/// One reading the helper can give better than the agent.
pub trait HelperReading: Send {
    type Value;
    /// The request through the link (a typed `HelperLink` method).
    fn via_helper(&self, link: &dyn HelperCalls) -> Result<Self::Value, PlatformError>;
    /// What the agent can read itself; `Reading::Unavailable(reason)` when nothing.
    fn local(&mut self) -> Reading<Self::Value>;
    /// Whether a helper answer that holds nothing should still fall back (Smart: yes).
    fn fall_back_on_empty(&self, _v: &Self::Value) -> bool { false }
}

pub struct HelperFirst<R: HelperReading> { reading: R, ready: ReadyGate, fallback: bool }
impl<R: HelperReading> HelperFirst<R> {
    pub fn read(&mut self) -> Sourced<R::Value>; // Sourced { value: Reading<V>, via: ReadVia }
}
```

`HelperCalls` is the object-safe slice of `HelperLink` the readings use (interface segregation; fakes in tests). `ReadyGate` is a
**cached** readiness: `HelperCentre::status()` makes two IPC calls per use (`helper_centre.rs:76-78` → `helper_link.rs:98-110`,
`Info` then `Audit`), which a per-tick reader must not pay. The gate keeps the last `Ready`/not-Ready for 5 s and drops it at once
on a transport error (`Absent`, `Silent`, `Foreign`) or a job that changes the helper.

### 3.2 What WP-0 also owns (so later packages never touch these files)

- `HelperLink` typed methods for every kind: `processes(q)`, `traffic()`, `socket_owners()`, `file_holders(dev, ino)`,
  `counters(q)`, `memory()`, `extras(kind)` (`helper_link.rs`; `smart`, `firewall`, `login_items` exist).
- `Shared.helper` becomes `HelperCentre<Box<dyn HelperTransport>>` (today it is fixed to `PeerTransport`, `agent.rs:298`), so the
  real-engine tests can serve a real `Dispatcher` (§4.2).
- `ReadVia` in `dyno-core/src/protocol.rs` and the `via` fields on every reply type named in §2, with serde defaults (`Local`), in
  one change, plus the TS mirror (`app/src/app/core/api.types.ts` or `generated/`) and the `*.wire.json` contract fixtures. Later
  packages only set the value.
- The UI piece of §3.4 in `app/src/app/core/` (one component, used by every page).
- `HelperFirstWear` (from `fix/dyno-windows-smart`) re-expressed as `HelperFirst<SmartWear>`, its tests unchanged.
- The test rig of §4.2.

### 3.3 `ReadVia`

```rust
pub enum ReadVia {
    Helper,                                   // read by the helper
    Local,                                    // read by Dyno with the person's rights
    NeedsHelper { reason: String, next: NextStep }, // not readable without the helper; next = OpenSettings("helper")
    Unavailable { reason: String },           // neither path can read it here (a VM without sensors, a missing tool)
}
```

`NextStep` is ADR 0052 §2.1's type; until WP-B1 lands it is a Dyno enum with the same variants, replaced then.

### 3.4 UI (ADR 0052 outcome contract)

One component `dy-reading-source` beside each reading: `Helper` → a neutral chip "Read by the administrator helper";
`Local` → nothing (the normal case for readings that need no helper) or, where the helper would add more, a one-line hint;
`NeedsHelper` → the reason in words and the button "Turn on administrator readings" (verb plus object, opens Settings > Helper);
`Unavailable` → the reason, no button. Never colour alone; never an empty table without a reason. A reading's value is live data,
not a job, so `pd-outcome`/`Stamped<T>` do not apply; the helper's install job does (ADR 0052 §2.1: the install re-probes `Info`
and ends `Unconfirmed` when the helper does not list the tool).

Size of WP-0: 2.5 agent-days.

## 4. Tests shared by every package

### 4.1 Per reading (named in §2)

1. A failing-first unit test at the merge point (ER-11).
2. A contract test that the helper and local paths produce the same domain type from one synthetic fixture (never real logs).
3. A real-engine end-to-end test (§4.2).
4. A VM row per OS the reading lists (§5).

### 4.2 The real-engine rig (WP-0)

`dyno-agent/tests/common/helper_rig.rs`: the agent's existing `Harness` (`tests/agent.rs`) with `Shared.helper` over a
`DispatcherTransport` that postcard-encodes each request, hands it to a real `dyno_helper::dispatch::Dispatcher` built from fake
readers (fake `ToolRunner`s and temporary `/proc` or `/sys` trees), and decodes the reply. `dyno-helper` becomes a
`[dev-dependencies]` entry of `dyno-agent` (no cycle: `dyno-helper` does not depend on `dyno-agent`). The test client speaks the
agent's real IPC. It covers wire encoding, authorisation (`Ctx` with owner, administrator and other), scoping and the agent merge;
it does not cover the OS reader, which the VM rows do. `no_real_machine.rs` gains the helper readers' production types, so no test
builds them.

## 5. VM checks (sequential: one Windows VM)

Harness: `scripts/vm-smoke` on `origin/test/dyno-release-vm-checks` (8128c1447), whose macOS power suite already turns the helper on
through the GUI (`README.md` "Power checks", row "Admin helper on"). Add a `--suite helper` with one step per row below; each step
reads the agent's answer through `dyno-agent status --json` (or the CLI's request command) and the visible page through the GUI
driver, and judges `via`, counts and the reason text. The helper's served-kinds line (§7.2) is read at the end of each OS run.

| OS, machine | Steps |
| --- | --- |
| Windows: `paddock-win11-test` (snapshot `dyno-gui-pending`, ADR 0040 GUI session, `win-gui-probe.ps1`) | Install the build; as the owner turn on Administrator readings (UAC Yes). Processes: an "Account" column with SYSTEM and service rows, `via helper`. Sign in a second standard account (exists in the snapshot or created by the step) and repeat: other accounts as one "Other users" total. Network > By program: bytes per program, `via helper`; as the second account only its own programs (WP-TRF scoping). Extras > Sleep: requesters listed (start a `powercfg /requestsoverride`-free requester: play a muted video in Edge, or the test's own `SetThreadExecutionState` helper exe). Backup: "no versions" reason (the VM has none). Lifespan: Smart as on the Smart branch. Security package: `icacls C:\ProgramData\Dyno\Helper\tools` shows the three entries, `/inheritance:r`; as the standard user `echo x > ...\tools\x.exe` is denied. Turn off: card Off, folder gone. |
| macOS: fresh Tart clone of `macos-tahoe-base` | Turn on (SecurityAgent sheet). Startup: login items listed beside launch agents, `via helper`. Firewall: pf card with rule count and rules (enable pf with a one-rule anchor in the step, restore after). Processes: root and `_` service accounts listed. Energy: the honest reason (the VM lists no CPU power). Temperatures: "pressure level" reason. Security: `ls -ld` of the tools and staging folders root:wheel 0755. Turn off. |
| Linux: Ubuntu 24.04 VM with a desktop (the Orb machine has no polkit agent, `helper-verification.md:132-136`; a Tart Linux guest or UTM) | Install the `.deb`, turn on (polkit). Firewall: nft ruleset (one table added by the step, removed after). Network: every socket has a program. Files: hold a file open as a second user, "What is holding this?" names it (scoped). Processes: root rows. Memory: the reason (no DMI in a VM). Security: tools and staging root:root 0755. Turn off. |

Order: Windows (1.5 days with the second-account rows), macOS (1 day), Linux (1 day, including setting up a VM with a desktop if
none exists: decision D5). Each step's id goes into ADR 0052's `features.toml` when that lands.

## 6. Security package (WP-SEC)

**Finding (from the brief; to be confirmed by the package's first test, not re-measured here):** `C:\ProgramData\Dyno\Helper\tools`
and `...\staging` are created by `create_dir_all` (`dyno-helper/src/tools.rs`, `FsPorts::create_staged`) and inherit ProgramData's
ACL, which lets Users create files and folders. A standard user could plant `smartmontools-7.5\bin\smartctl.exe` before the helper
runs it as SYSTEM (`tools.rs` `smart_reader`), or a file in staging the installer runs. `startup::verdicts` checks only the program,
the data folder and the configuration (`dyno-helper/src/startup.rs:21-40`).

Work:

1. **Lock on create.** One function `protect_dir(path)` in `dyno-helper` that creates the folder if absent and applies a protected
   DACL with no inheritance from above: SYSTEM and Administrators full, Users read and execute. Windows reuses `paddock-service`'s
   `acl_command` and `acl_shape_ok` (`crates/paddock-service/src/privileged/windows_acl.rs:12-26, 64-...`; used by `apply_acl`,
   `install.rs:407-424`). macOS and Linux: `root:wheel`/`root:root`, mode 0755, created with `DirBuilder::mode` under a umask of
   0o022, and the parent chain checked by the existing `verify_protected` (`paddock_service::privileged`).
2. **Verify before every install and every run.** `install` (`dyno-platform/src/tool_install.rs:129`) and each `SmartctlReader`
   run first check the folder and the program's facts with `verify_protected` (owner, no write for others, on Windows the ACL shape),
   and refuse with a reason and an audit line when not protected; `startup::verdicts` adds the tools and staging folders when they
   exist. The verification uses the open handle's facts, not the path a second time (rule 53 spirit: no check-then-use race).
3. **Repair.** The Settings card's Repair re-applies the ACL (install path), so an existing 1.1 pre-release install is fixed by
   Repair, not by a migration (no backward compatibility before v1).
4. **Least privilege elsewhere.** Confirm no other helper-written folder (logs, audit) inherits create rights (`paths.rs`), with
   the same test.

Tests (failing first): `tools::tests::a_tools_folder_that_others_can_write_is_refused_before_smartctl_runs` (a fake `ProtectedFiles`
reporting Users:W); `setup::tests::install_locks_tools_and_staging` (the icacls arguments, the shape check); on Windows CI
(`cfg(windows)`), a real temporary folder under a test root gets the DACL and a second `CreateFile` as a restricted token is denied
(reuse paddock-ipc's restricted-token test helper if one exists; else the VM row). VM: the Windows row of §5.

Size: 2.5 agent-days. Files: `dyno-helper/src/{tools.rs, startup.rs, setup.rs}` (new `protect.rs`), `dyno-platform/src/tool_install.rs`,
`dyno-platform/src/smart_reader.rs`. A change to `paddock-service` (exporting the two functions if they are not public) is a shared
crate change: the integrating agent, one declared line. Independent security review before merge (ER-25 reviewer).

## 7. Dead handlers and the wiring check

### 7.1 LogTail and WolState (not advertised) - WP-DEAD

- `LogTail`: `HelperLink::log_tail` (`helper_link.rs:198`) has no production caller. **Recommended: wire it** into the diagnostic
  bundle (`dyno-agent/src/diagnostics.rs`): the helper's last 200 lines, owner/admin only (`access()` denies others,
  `helper_policy.rs:140`), through the bundle's redactor. ER-27 says the bundle must explain helper failures; today it cannot.
  0.5 day. Alternative: remove the request and the method.
- `WolState`: the Linux Wake-on-LAN readiness says "the card's own `ethtool` mode needs the helper and is not read here"
  (`dyno-platform/src/wol/readiness.rs:6`). **Recommended: wire it** into that readiness check through the seam (Linux; the
  interface name is an `IfName`, parsed at the wire). 1 day. Alternative: declare it unwired until 1.2 (`wiring.toml`, §7.2) and keep
  the sentence.

Either way, nothing stays sendable without a caller or a dated declaration.

### 7.2 The check: every advertised reading has a production sender (C83 for Dyno, now) - WP-C83

1. **Readings become data.** `readings(host)` returns `&[Advertised { text, kinds: &[RequestKind] }]` instead of strings
   (`helper_wire.rs:380-401`), so each line names the request kinds that deliver it. The card and the user guide render `text`.
2. **Static check** (`dyno-agent/tests/helper_wiring.rs`, no build step beyond `cargo test`): for each `RequestKind` the
   `readings()` of any host names, parse `crates/dyno-agent/src/**/*.rs` and `crates/dyno-platform/src/**/*.rs` with `syn` (already in
   the workspace through the wire tests, else a dev-dependency), skip `#[cfg(test)]` items, and require (a) a `HelperRequest::<Kind>`
   construction in a non-test function and (b) a call to that function from non-test code outside its own impl, or the impl of a
   trait the composition builds. Kinds not advertised must be in `apps/dyno/wiring.toml` `[[unwired]]` with `why` and `until`
   (ADR 0052 §1.1's format, so `./dev check` C83 takes the file over unchanged when WP-A1 lands). Fails today on 9 kinds; passes when
   §2 is done. Name-level, not a call graph: the fix text says so.
3. **Doc claims follow the data.** A test that the user guide's helper paragraph and the CHANGELOG's helper lines mention every
   `Advertised.text` (and nothing advertised that `readings()` lacks): the claims at `user-guide.md:204-206` already differ from
   `readings()` (it lists "temperatures, fans and CPU energy" for every OS and disk health without an OS, while `readings()` lists
   temperature only for macOS and disk health only for Windows).
4. **Dynamic half.** The helper counts served requests per kind (an array indexed by `RequestKind`, no content) and logs
   `served: kind=count ...` at stop; the VM suite fails when an advertised kind of that OS was served 0 times (ADR 0052 §1.5).

Size: 1.5 agent-days. Lands early, red, as the burn-down list (marked `#[ignore]` with a reason until WP-INT; then on).

## 8. Packages, owners and order

### 8.1 Package table

| WP | What | Owns (files) | Depends on | Agent-days |
| --- | --- | --- | --- | --- |
| WP-00 | Merge `fix/dyno-windows-smart` into `steward/dyno-1.1.0`, affected tests (integrator) | merge only | - | 0.5 |
| WP-0 | Seam `HelperFirst<R>`, `ReadyGate`, `HelperLink` typed methods, `Shared.helper` boxed, `ReadVia` + `via` fields + TS mirror + wire fixtures, `dy-reading-source`, `HelperFirstWear` port, the rig | `dyno-agent/src/{helper_first.rs, helper_link.rs, helper_centre.rs, helper_wear.rs}`, `dyno-core/src/protocol.rs`, `app/src/app/core/{api.types.ts, generated/, *.wire.json}`, new UI component, `dyno-agent/tests/common/helper_rig.rs` | WP-00 | 2.5 |
| WP-SEC | Locked tools and staging folders, verify before install and run, Repair re-applies | `dyno-helper/src/{tools.rs, startup.rs, setup.rs, protect.rs}`, `dyno-platform/src/{tool_install.rs, smart_reader.rs}` | WP-00 | 2.5 |
| WP-C83 | `readings()` as data, static check, doc-claims test, served counter, `wiring.toml` | `dyno-core/src/helper_wire.rs` (`readings` only), `dyno-agent/tests/helper_wiring.rs`, `dyno-helper/src/server.rs`, `apps/dyno/wiring.toml`, Settings card text binding | WP-00 | 1.5 |
| WP-FW-h | Firewall helper half: `Parts.firewall`, readers per host, handler with `for_access` | `dyno-helper/src/{dispatch.rs, serve.rs}` | WP-00 | 1 |
| WP-FW-a | Firewall agent half and UI | `dyno-agent/src/firewall.rs`, `app/src/app/features/firewall/` | WP-0, WP-FW-h | 1 |
| WP-BTM | macOS login items through the helper | `dyno-platform/src/macos/{startup.rs, mod.rs}`, new `dyno-agent/src/helper_readers/login_items.rs`, startup page | WP-0 | 1.5 |
| WP-PROC | Every account's processes, 3 OS | new `dyno-agent/src/helper_readers/processes.rs`, `dyno-agent/src/sampler.rs`, `dyno-core/src/others.rs`, `app/src/app/features/processes/` | WP-0 | 3.5 |
| WP-TRF | Traffic scoping (helper) and per-program traffic (Windows) | `dyno-core/src/network_traffic.rs`, `dyno-helper/src/dispatch.rs` (Traffic arm), `serve.rs` (traffic part), `dyno-platform/src/network.rs` (traffic), new `helper_readers/traffic.rs`, network page "By program" | WP-0, WP-FW-h landed | 2 |
| WP-LNX | Socket owners and file holders (Linux) | `dyno-platform/src/linux/` socket reader (inode), `dyno-core/src/network.rs` (Linux field), `dyno-agent/src/{network.rs, files.rs}`, new `helper_readers/holders.rs`, network and files pages | WP-0 | 2.5 |
| WP-CNT | Energy and temperatures (macOS; D2 may widen) | `dyno-core/src/power/energy.rs`, `dyno-core/src/metrics.rs` (temperatures only), `dyno-platform/src/sensors.rs`, `dyno-agent/src/power.rs`, new `helper_readers/counters.rs`, `app/src/app/features/power/energy-tab.ts`, temperature widget | WP-0 | 3 |
| WP-MEM | Memory modules (Linux) | `dyno-agent/src/inventory.rs`, new `helper_readers/memory.rs`, inventory page | WP-0 | 1 |
| WP-EXT | Wake requests and Backup versions (Windows), one parser each | `dyno-platform/src/extras.rs`, `dyno-core/src/{extras/sleep.rs, backup.rs, helper_extras.rs}`, `dyno-agent/src/extras/`, new `helper_readers/extras.rs`, extras and backup pages | WP-0 | 2 |
| WP-DEAD | LogTail into the bundle; WolState into Linux WoL readiness (or declared unwired) | `dyno-agent/src/diagnostics.rs`, `dyno-platform/src/wol/readiness.rs`, new `helper_readers/wol.rs` | WP-0 | 1.5 |
| WP-VMH | `--suite helper` steps for the three OS (harness code, no VM run yet) | `scripts/vm-smoke/helper-*.mjs` and their tests, on a branch off `test/dyno-release-vm-checks` | - | 1.5 |
| WP-INT | Integrate, `agent.rs` composition lines, full suite once (ER-8), C83 test un-ignored, claims reconciled (user guide, CHANGELOG, card) | `agent.rs`, `CHANGELOG.md`, `docs/user-guide.md`, `docs/helper-verification.md` | all code WPs | 1.5 |
| WP-REV | Independent reviews: security (WP-SEC, WP-TRF scoping), UI screenshots (ER-23), rules review | read-only | WP-INT | 1 |
| WP-VM | VM runs, sequential: Windows 1.5, macOS 1, Linux 1, one fix round 1 | results in `docs/helper-verification.md` | WP-INT, WP-VMH | 4.5 |

Total: **36 agent-days** (code 27, harness 1.5, integration and review 2.5, VM 4.5, merge 0.5).

`agent.rs` (composition) is a shared integration point: each package declares its one composition line in its hand-off, and
WP-INT adds them. `protocol.rs` and the TS mirror are touched only by WP-0. `dispatch.rs`/`serve.rs` are touched by WP-FW-h in
wave 1 and by WP-TRF in wave 2, never at the same time.

### 8.2 Waves with four agents

| Wave | Lane A | Lane B | Lane C | Lane D |
| --- | --- | --- | --- | --- |
| 0 (day 0 to 0.5) | WP-00 (integrator) | - | - | - |
| 1 (days 0.5 to 3) | WP-0 (2.5) | WP-SEC (2.5) | WP-C83 (1.5), then WP-VMH starts (1 of 1.5) | WP-FW-h (1), then WP-VMH rest or idle |
| 2 (days 3 to 8) | WP-PROC (3.5), WP-FW-a (1) | WP-CNT (3), WP-DEAD (1.5) | WP-LNX (2.5), WP-MEM (1), WP-BTM (1.5) | WP-TRF (2), WP-EXT (2) |
| 3 (days 8 to 10.5) | WP-INT (1.5), WP-REV (1), with fixes from review in the owning lanes | | | |
| 4 (days 10.5 to 15) | WP-VM: Windows, then macOS, then Linux, then a fix round (sequential; the other lanes fix what each run finds) | | | |

**Critical path:** WP-00 (0.5) → WP-0 (2.5) → lane C of wave 2 (5) → WP-INT (1.5) → WP-REV (1) → WP-VM (4.5) = **15 working
days**. Lane A (4.5) and lane B (4.5) have half a day of slack each; moving WP-BTM from lane C to whichever lane finishes first
shortens wave 2 to about 4.5 days and the path to about 14.5 days.

**Calendar:** about **3 weeks** (14 to 15 working days) with four agents, if the VMs are free when wave 4 starts and no wave-1
finding changes the seam. A Linux desktop VM that does not exist yet (D5) adds about a day to wave 4 unless it is set up during
wave 2 by an idle lane.

### 8.3 Risks

- **The seam is on the critical path.** A seam that changes after wave 2 starts touches every package. Mitigation: WP-0 lands with
  the Smart port and one second instance (memory, the smallest) before wave 2 starts, reviewed.
- **Processes merge cost.** Paging up to 16 sweeps of 1 024 every 5 s on a busy machine; measure the sweep and page time on the
  Windows VM in WP-PROC (rule 48: results block in `docs/performance.md`) and make the interval a setting with "Auto".
- **Values not verified on real hardware** (macOS `powermetrics` CPU power, Linux RAPL and DMI, Windows MSAcpi): every VM lacks
  them (`helper-verification.md:103-107`). The VM rows can only check the honest reason. Decision D3.
- **Policy file interplay.** `OthersPolicy::Detail`/`Hidden` changes what non-owners see for processes and (after WP-TRF) traffic;
  the VM's second-account row tests only the default.
- **Wire churn.** `HELPER_API` changes (WP-TRF's reply) need a matching helper and agent; the version stays 3 because 1.1.0 is
  unreleased, but every dev machine with a 1.1 pre-release helper must Repair. Say so in the hand-off.

## 9. Decisions for the user

- **D1. Fallback policy.** Keep "helper first, then local" for every reading (the Smart decision of 2026-11-04 generalised;
  recommended), or helper only for the readings where the local result is misleadingly partial (processes, sockets).
- **D2. The macOS "Temperature and energy" claim.** The helper cannot read temperatures on macOS (by design of macOS). Options:
  (a) reword the card to "CPU energy (and fan speeds on Intel Macs)" and show the thermal pressure level locally (recommended);
  (b) drop the line on macOS. Also: advertise the readers that exist but are not claimed (Linux RAPL energy and fans, Windows
  thermal zones) at 0.5 day each for the VM rows, or leave them unclaimed and declared in `wiring.toml`.
- **D3. Real-hardware verification.** The VMs cannot show measured energy, DMI slots or ACPI temperatures. Release with the honest
  reason verified in the VMs and values verified on the user's own Mac and PC by hand (recommended, a manual row), or require
  real-hardware rows before the tag.
- **D4. Dead handlers.** Wire `LogTail` into the diagnostic bundle and `WolState` into Linux WoL readiness (recommended, 1.5 days),
  or remove them from `HELPER_API` / declare them unwired until 1.2.
- **D5. Linux VM.** The Orb machine has no polkit agent; use or set up a desktop Linux VM for the helper suite (recommended), or
  accept a CLI-only Linux row with the polkit prompt as a manual step.
- **D6. Traffic scoping (security, P1).** Fix in 1.1.0 as part of WP-TRF (recommended; otherwise a standard user sees other
  accounts' programs), or make `Traffic` owner/admin only for now (0.5 day, simpler, and non-owners get nothing).
