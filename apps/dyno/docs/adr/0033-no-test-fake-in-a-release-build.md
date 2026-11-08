# 33. No test fake in a release build; real Windows readers for startup items, facts and file locks

- Status: Proposed (2026-11-08), for Dyno 1.1.1. Waits for an independent review and the user's decisions D1 to D6.
- Date: 2026-11-08
- Trigger: the investigation of 2026-11-08 (Windows tray work, evidence in the coordinator's scratch folder `win-tray/`:
  `vm/startup-page.png`, `vm/logs.txt` and the notes). In a Windows 11 VM running Dyno 1.1.0, Health showed **100, Good,
  "Nothing needs your attention"** and the startup card said **"No startup items. Nothing is set to start when you sign
  in."** That was on a machine where Dyno's own agent starts from the HKCU `Run` key (`dyno-agent/src/service/spec.rs`,
  `WINDOWS_RUN_KEY`). The user decided the same day that this is fixed in Dyno 1.1.1.
- Related:
  - ER-21 / rule 40 (a recurring bug means a missing design rule), ER-11 (failing test first), ER-14 / rule 7 (tests never
    touch the user's machine), ER-27 / rule 56 (logs explain every failure), ER-33 (a check counts only if its probe can
    see), ER-20 / rule 51 (more threads or a cheaper operation), rule 53 / family ADR 0033 (processes by identity), rule 55
    / family ADR 0034 (one implementation per mechanism), family ADR 0039 (release gates inspect the artifacts), family
    ADR 0040 (the Windows GUI test session) and ADR 0042 (one owner per harness).
  - Dyno ADR [0021](0021-monitoring-only.md) (monitoring only: Dyno does not enable or disable a startup entry) and
    ADR [0027](0027-helper-for-1-1-0.md) (the helper answers scoped reads).
  - The macOS fixes for the same symptom: `a935f3c00`, `f5497766c` and `79a31a415` (launch agents, `sfltool dumpbtm`
    through the helper, a partial list that says what it could not read). They never covered Windows.

## Context

### What is wired today (read at `origin/main` `9e18230c5`; paths under `apps/dyno/crates/dyno-platform/src/`)

`windows/mod.rs:23-38`:

```rust
pub fn platform(secrets_dir: &std::path::Path) -> Platform {
    let fake = crate::fake::Fake::new();
    let f = fake.platform();
    Platform {
        sensors: ..., processes: ..., activity: ..., issues: ..., links: ..., zone: ..., counters: ...,
        network: ..., hardware: ..., power: ..., wol: ...,
        ..f
    }
}
```

The struct-update spread `..f` fills the three fields the Windows wiring does not name, `locks`, `startup` and `facts`,
from `fake::Fake`. Every Dyno release since the first commit (`be400ed05`) has shipped these to Windows users:

| Port | What the fake answers (`fake.rs`) | What the user saw |
| --- | --- | --- |
| `StartupItems` | `Ok(vec![])`, `unavailable()` empty | "No startup items. Nothing is set to start when you sign in." |
| `FactsSource` | `SystemFacts::unknown("Fake OS 1.0")`: every fact "Not checked yet" | No disk-health, restart-pending or update reason can appear, so Health reads 100 Good; Fake OS 1.0 is the OS name in the facts |
| `FileLocks` | `Ok(vec![])` for every path | "What is locking this file?" says nothing holds it |

The gap was known: `docs/architecture.md:42` says "the Windows platform still uses the in-memory fakes for them, so those
screens are empty on Windows", and `features.md` says "not wired yet" in three places. The product did not say so. A fake
answers in the shape of real data: an empty list, unknown facts. So the page could not tell "nothing there" from "never
read". The parsers for the missing readers already exist and are tested: `parse/startup_approved.rs` (`approved_state`),
`parse/lnk.rs` (`parse_lnk`), `parse/win_facts.rs`, `parse/powercfg.rs`. No reader calls them.

### Why this is a pattern, not one bug (ER-21)

The same shape recurs at three levels. Each time, a test double was available in production code and a production path
used it:

1. **`windows::platform()`**: `..f`, above.
2. **`hardware::battery_capacity()`** on an OS that is not macOS, Linux or Windows (`hardware.rs:62`) returns
   `FakeBattery(Reading::unavailable(...))`. The answer is honest, but the type is a test fake chosen because it was at
   hand.
3. **Test doubles are public in every build:** `dyno_platform::fake` (the whole `Fake` platform, `FakePowerActions`,
   `FakeIdleSignals`), `counters::FakeCounters`, `hardware::{FakeThrottle, FakeWear, FakeBattery}`,
   `wol::{FakeWakeTimer, WolPorts::fake}`, `dyno_core::wol::fake::*`, and in the family crates
   `paddock_power::{FakePower, FakeEvents}` and `paddock_secrets::MemorySecrets` ("In-memory `Secrets` for tests"). Only
   `linux/tools.rs` `fake`, `dyno-agent`'s `cache::testing` and `disk::testing`, and `dyno-bench`'s `fakes` are behind
   `#[cfg(test)]`.

Rule 7 makes every Dyno test use the fakes, and the fakes are what every test sees. So no test, on any OS, could notice that
the shipped wiring also used them. The macOS fix added real readers but left the mechanism in place: a new port, or a new
OS, gets a fake by default, silently.

### The same check on macOS, Linux and other systems

- `macos/mod.rs` and `linux/mod.rs` name all 14 `Platform` fields explicitly; no spread, no fake.
- `other.rs` (an OS that is not Windows, macOS or Linux) names all fields and uses honest `Unsupported` adapters that
  answer `PlatformError::Unsupported("Not supported on this OS yet.")`. That is the pattern this record keeps.
- Other struct-update spreads in Dyno's production code (`discovery.rs`, `geo_db.rs`, `nt.rs`, `readability.rs` and
  more) copy data values, not port bundles. The agent destructures `Platform` field by field (`agent.rs:663`), so a new
  port is a compile error there.
- `dyno-agent`'s `DiskPorts::none()` (`Running::start`) is an honest stub ("This agent has no system to ask."). The real
  binaries call `Running::start_os` (`cli.rs:263`), which wires `DiskPorts::os()`. That is not a fake, but the same
  temptation; §2 covers it.
- Windows process details are `NoDetails`. This honest stub adds nothing, and the lists say why. It is a known, documented
  gap (`features.md`), not a fake. It is out of scope here (D6).

## Decision

### 1. The mechanism: a fake does not exist in a build that ships

1. **Every test double is compiled only for tests.** `dyno_platform::fake`, `counters::FakeCounters`,
   `hardware::{FakeThrottle, FakeWear, FakeBattery}`, `wol::FakeWakeTimer` with `WolPorts::fake()` and
   `dyno_core::wol::fake` are behind `#[cfg(any(test, feature = "test-support"))]`. `dyno-platform` and `dyno-core` get
   a `test-support` feature (empty, or enabling the other crate's). Consumers enable it **only in `[dev-dependencies]`**:
   `dyno-agent`, `dyno-cli` and `app/src-tauri` (which already lists `dyno-platform` there, "Tests start an in-process
   agent over the fake platform"). With Cargo's resolver 2, dev-dependency features are not unified into a build that
   compiles no test target, so `tauri build` and `cargo build --profile dist` compile `dyno-platform` without the fakes.
2. **A release build therefore cannot name a fake.** The compiler resolves every path, alias and re-export, so
   `..crate::fake::Fake::new().platform()`, `use crate::fake as f` or a new `FakeX` used outside a test is an error in
   the shipped build of that OS. This is the build-time assertion the brief asks for, and it is the check D1 chooses (§3).
3. **Every platform names every port.** Each `windows/`, `macos/`, `linux/` and `other` `platform()` lists all fields
   of `Platform` and its sub-structs (`NetworkPorts`, `HardwareReaders`, `PowerPorts`, `WolPorts`) with no `..`. A port
   that an OS cannot read yet gets an honest adapter that names its gap. The existing `other::Unsupported` becomes the
   shared `unsupported::NotReadHere { what, why }`: `list()` answers `PlatformError::Unsupported(why)`, and `facts()` answers
   the real OS name with every other fact `Reading::unavailable(why)`. A gap is then a sentence on the page, never an
   empty list. `Platform`'s doc comment states this. No `Default` is derived for `Platform` or its sub-structs, so the only
   value a spread could take is another full platform, and every full platform is either per-OS (`cfg`'d away on the
   others) or the fake (absent in a release build).
4. **`FakeBattery` in `battery_capacity()`** becomes `NotReadHere` (the same answer under an honest name).
5. **The family crates' test doubles** (`paddock_power::{FakePower, FakeEvents}`, `paddock_secrets::MemorySecrets`) get
   the same gate in the same change (D2). Today only Dyno's tests and `paddock-pitwall`'s own `#[cfg(test)]` modules use
   them (`git grep`, 2026-11-08), so the adoption is three `[dev-dependencies]` lines. This is a shared change (rules 39
   and 43): the integrating agent lands it, with the CHANGELOG entry.
6. **The fake's doc example** (`fake.rs:4-12`) moves into a unit test: a doctest compiles against the library without
   its own `test-support`.

### 2. The test that fails today, and wiring visible in the logs

- **`adapter()` on Dyno's port traits.** Every port trait in `dyno_core::ports` and `dyno_core::wol` gets a provided method:

  ```rust
  /// The concrete adapter behind this port (for the wiring test and the agent's start log).
  fn adapter(&self) -> &'static str { std::any::type_name::<Self>() }
  ```

  A provided method is monomorphised for each implementation, so through a `Box<dyn Port>` it returns the concrete type,
  for example `dyno_platform::fake::Fake`. Nobody overrides it. `Platform::adapters()` returns `(port, adapter)` for
  every field, including the sub-structs and the vector of issue sources, with a destructuring `let Platform { .. }` that
  names every field: a new port that is not listed is a compile error (no `..` there either).
- **The no-fake test** in each OS module: `windows::tests::the_windows_platform_wires_no_fake`, and the same for
  `macos` and `linux`. It builds the real `platform()` with a temporary secrets folder and calls **no port method**. It
  asserts that no adapter path contains `::fake::` or `::testing::` and that no last segment starts with `Fake`. On
  `origin/main` the Windows test fails and names `locks`, `startup` and `facts` → `dyno_platform::fake::Fake`. The macOS
  and Linux tests pass and guard against regressions. ER-14: the WP checks that every constructor `platform()` calls is
  inert (no OS state changed, no file outside the temporary folder). `PowerPorts::os()` and `NetworkPorts::os()` build
  unit structs. `WolPorts::os` builds its store under the given folder. A constructor that is not inert becomes lazy.
  The Linux end-to-end test (`linux/mod.rs`) already builds the real platform the same way.
- **ER-27:** the agent logs one `Platform wired` line at start with every `port=adapter` pair. Dyno's diagnostic
  bundle would have shown `startup=dyno_platform::fake::Fake` on the first Windows report. Type names are not user
  content.
- The shared traits (`paddock_power::PowerOs`, `SleepWakeSource`, `paddock_secrets::Secrets`) get no `adapter()` (that
  would be a family API change for a test). For them, §1.5's gate is the guarantee, and the test lists them as
  `(unnamed)`.

### 3. The check that keeps it true (D1): the build-time assertion, kept honest by the feature graph

Chosen: **§1's `cfg` gate**, so the shipped build of each OS does not compile when production code names a fake. Two
small checks keep the gate meaningful:

- **The feature never reaches a shipped binary.** `scripts/check-shipped.mjs` already runs `cargo tree -e normal
  --target all` for every shipped package (`SHIPPED`) to keep `dyno-helper` out. It gains a second assertion over
  `cargo tree -e normal --target all -f '{p} {f}'`: no shipped package's normal (non-dev) graph enables
  `dyno-platform/test-support`, `dyno-core/test-support`, `paddock-power/test-support` or `paddock-secrets/test-support`
  on any target. Resolution is static, so the Linux `checks` leg covers the Windows and macOS graphs too. Its unit test
  feeds a fixture tree with the feature on and expects the failure.
- **A non-test build on branches.** CI's Rust gates on every OS compile test targets (`clippy --all-targets`,
  `nextest`), and those unify the dev features in. So Dyno's `rustChecks` (Linux) gains `cargo check --locked -p
  dyno-platform -p dyno-agent -p dyno-cli --lib --bins`, which catches any non-OS-specific production reference on the
  branch. A Windows-only or macOS-only reference is caught by the no-fake test on that OS's runner (for the platform
  wiring), by the build brief's local `cargo xwin check -p dyno-platform --target x86_64-pc-windows-msvc` (ER-9), and at
  the latest by `dyno-release.yml`'s installer build, which runs on every push to main. The cost of the extra check is
  measured in WP-1 and recorded (rule 48). If it is over two minutes on the Linux leg, it moves to the nightly run.

Rejected:

- **Clippy `disallowed-types` / `disallowed-methods`.** Clippy cannot exempt test code from these lints. About 30 test
  files and every new one would carry an `#[allow]`, and an allow-list that grows with every test hides a production
  `#[allow]` among them. Clippy also lints Windows code only on the Windows leg, and it is a lint over `--all-targets`, not
  over the shipped build.
- **A source-text conformance check** (`grep` for `fake::` or `..`). It misses aliases and re-exports, and it cannot tell
  a data spread (`..raw()`, many in production) from a port spread.
- **Only the no-fake test.** It sees the platform's ports, not a fake used elsewhere in production code (for example in the
  agent), and it relies on a naming convention. It stays as the readable failure for the wiring, not as the guarantee.

### 4. Real Windows readers

All three run with the user's rights. Dyno changes no startup entry (ADR 0021), so nothing here needs the helper. The
`StartupLocation` doc comments that say "disabling needs the privileged helper" date from before ADR 0021 and are reworded
("written for every user"). Each reader is a thin adapter over small OS traits (`RegistryView`, `FolderList`,
`LogonTasks`, `RestartManager`) and a pure `assemble` function. The tests run `assemble` on every OS (§5).

#### 4.1 Startup items (`windows/startup.rs`, `parse/win_startup.rs`)

| Source | Where | `StartupLocation` | State from |
| --- | --- | --- | --- |
| HKCU Run | `HKCU\Software\Microsoft\Windows\CurrentVersion\Run`, 64-bit and 32-bit views | `UserRunKey` | HKCU `…\Explorer\StartupApproved\Run` |
| HKCU RunOnce | `…\RunOnce`, both views | `UserRunOnceKey` (new, D3) | always Enabled (runs at the next sign-in, then Windows removes it) |
| HKLM Run, 64-bit | `HKLM\SOFTWARE\Microsoft\Windows\CurrentVersion\Run` (`KEY_WOW64_64KEY`) | `MachineRunKey` | HKLM `…\StartupApproved\Run` |
| HKLM Run, 32-bit | the same key with `KEY_WOW64_32KEY` (`WOW6432Node`) | `MachineRunKey` | HKLM `…\StartupApproved\Run32` |
| HKLM RunOnce | both views | `MachineRunOnceKey` (new, D3) | always Enabled |
| User Startup folder | `FOLDERID_Startup` (`SHGetKnownFolderPath`) | `UserStartupFolder` | HKCU `…\StartupApproved\StartupFolder` |
| Common Startup folder | `FOLDERID_CommonStartup` | `CommonStartupFolder` | HKLM `…\StartupApproved\StartupFolder` |
| Logon tasks | Task Scheduler 2.0 COM: `ITaskService::Connect`, a recursive walk from `\` with `GetTasks(TASK_ENUM_HIDDEN)`, tasks with a `TASK_TRIGGER_LOGON` trigger | `ScheduledTask` | `IRegisteredTask::Enabled` and the trigger's `Enabled` |

- **Values:** `REG_SZ` and `REG_EXPAND_SZ` (expanded with `ExpandEnvironmentStringsW` into a bounded buffer). Other
  types are skipped and counted. The id is `<hive>:<key>:<view>:<value name>`. `HKCU\Software` is not redirected under
  WOW64, so its 32-bit view is the same key: entries with the same hive, key, name and command are merged, and a test
  covers it.
- **StartupApproved:** the value of the same name, read with the existing `approved_state`. A missing value or one it
  cannot read means Enabled (that is what Windows does). The table's mapping (which approved key governs which source,
  HKCU or HKLM) is what Task Manager writes on Windows 10 and 11. WP-0 confirms it in the VM by turning off one entry of
  each kind in Task Manager and diffing the keys (ER-28: measured, not assumed). The parsing tests then use the measured
  bytes.
- **Startup folders:** at most `MAX_FOLDER_FILES` (1,000) per folder. `desktop.ini` is skipped. A `.lnk` is read
  (at most `MAX_LNK_BYTES`) and resolved with the existing `parse_lnk` to its target and arguments. A shortcut with no path
  (for example a Store app) is shown as the `.lnk` itself, as `parse/lnk.rs` documents. Other files are listed as their
  path.
- **Tasks:** at most `MAX_TASKS` (10,000) tasks visited and a folder depth of 16. Each action is an `IExecAction`
  (`Path` and `Arguments`), and a task with only COM-handler actions shows its handler's CLSID as the command. The name
  is the task path (`\Microsoft\Windows\…`), which `is_os_owned` already treats as the OS's. COM is initialised for the
  calling thread with the helper that `windows/firewall_com.rs` uses (rule 55: no second copy). A standard user does
  not see tasks whose security descriptor hides them from that user. The API returns no error and no count for them, so
  the user guide says so, rather than a sentence on every list.
- **Partial lists:** each of the nine sources is read on its own. A failure goes into `unavailable()` as one sentence
  ("The all-users Run key (32-bit) could not be read: access denied.") and the other sources still list their entries.
  When all of them fail, `list()` returns the error (the macOS reader's rule). The list is kept between `list()` calls
  until `refresh()`, as on macOS: tasks are the slow part.
- **Publisher:** the signer, through the existing WinTrust code that the process signer uses, when the executable is a
  local file. Otherwise `None`.

#### 4.2 Facts (`windows/facts.rs`)

| Fact | Source | Notes |
| --- | --- | --- |
| `os` (required) | `HKLM\SOFTWARE\Microsoft\Windows NT\CurrentVersion`: `ProductName`, `EditionID`, `DisplayVersion` (or `ReleaseId`), `CurrentBuildNumber`, `UBR`, `InstallationType`; the build cross-checked with `RtlGetVersion` (not shimmed by the manifest) | `ProductName` still says "Windows 10" on Windows 11. A client build ≥ 22000 is named "Windows 11" (a pure `win_os_name()` in `parse/win_facts.rs`). Example: "Windows 11 Pro 24H2 (build 26100.2314)". A server keeps its `ProductName`. |
| `restart_pending` | the keys `…\WindowsUpdate\Auto Update\RebootRequired` and `…\Component Based Servicing\RebootPending` exist | `PendingFileRenameOperations` is left out: many installers set it, and it would raise false "restart needed" reasons. A key that cannot be read is unavailable with its reason. |
| `power_plan` | `PowerGetActiveScheme` and `PowerReadFriendlyName` | the GUID mapping (`POWER_SAVER_GUID`) is reused from `parse/powercfg.rs`. No `powercfg` process, no locale parsing. |
| `visual_effects_full`, `background_apps` | HKCU `VisualEffects\VisualFXSetting`, `BackgroundAccessApplications` | the existing parsers in `parse/win_facts.rs` |
| `dev_folders` | `dev_folders::scan(%USERPROFILE%)` | as on macOS |
| `disks` | WMI `root\Microsoft\Windows\Storage`, `MSFT_PhysicalDisk` (`FriendlyName`, `HealthStatus` 0/1/2/5) through the `wmi` crate already linked | WP-0 confirms a standard user can read it. If not, it is unavailable with "needs the helper" until a later helper read. |
| `pending_updates` | not read: "Dyno does not ask Windows Update (the search is slow and contacts Microsoft). Settings > Windows Update shows them." | the macOS stance (`softwareupdate` is not run) |
| `last_boot_secs` | not read in 1.1.1: "Windows keeps boot times in a log only administrators can read." | WP-0 confirms the channel's access. A later helper read can fill it. |

Each fact is timed and read on its own. A failure is that fact's `Reading::unavailable`, never an error for the whole
(`FactsSource` contract).

#### 4.3 File locks (`windows/locks.rs`)

Restart Manager: `RmStartSession`, then `RmRegisterResources` with the one path, then `RmGetList` (grown once on
`ERROR_MORE_DATA`, capped at `MAX_HOLDERS` = 4,096 with `truncated`), then `RmEndSession` in a guard on every path.

- Each `RM_PROCESS_INFO` gives `Process.dwProcessId` and `Process.ProcessStartTime`. These map to `ProcessRef { pid,
  started }` through `parse/time.rs` (the identity rule 53 asks for). `strAppName` gives the name, falling back to the
  process table's name, and `ApplicationType` gives the kind: `RmMainWindow` and `RmOtherWindow` are "Application",
  `RmService` is "Service", `RmExplorer` is "Explorer", `RmConsole` is "Console", `RmCritical` is "System". This mapping
  is a pure function and tested.
- Folders: Restart Manager reports holders of files, not folders. A folder path answers
  `PlatformError::Unsupported("Pick a file: Windows reports which programs hold a file, not a folder.")`. A missing path
  answers `NotFound`. Dyno's own process is filtered out as on macOS.
- Rights: a standard user's Restart Manager sees processes of every account, but other accounts' names may be empty.
  Those rows keep the pid and say "another account".

#### 4.4 Logging (ER-27)

- Startup: one line per source, `Startup source read` with `Source=hklm-run-32`, `Count`, `Skipped`, `DurationMs`,
  `Outcome`. A failure logs at `warn` with the raw Win32 error or HRESULT and the sentence shown. A summary line logs the
  total, the number of unavailable sources and the duration. Entry names and commands are not logged by default (they can
  hold user paths).
- Facts: one line per fact, with `Fact`, `Outcome` and `DurationMs`. The OS name is logged (the agent's start line
  already logs `OsVersion`).
- Locks: `Lockers read` with `Count`, `Truncated`, `DurationMs` and each Restart Manager call's error code. The path is
  user content and is not logged.
- The `Platform wired` line of §2.

### 5. Tests (failing first, ER-11)

| Test | Where | Fails today because |
| --- | --- | --- |
| The no-fake test (§2), per OS | `windows/mod.rs`, `macos/mod.rs`, `linux/mod.rs` | Windows: `locks`, `startup`, `facts` are `dyno_platform::fake::Fake` |
| The shipped-feature check (§3) and its fixture test | `scripts/check-shipped.mjs`, `.test.mjs` | the feature does not exist yet, so the fixture test fails |
| `assemble` over fake registry, folder and task shapes, on every OS: an enabled HKCU Run value; a value turned off by StartupApproved (the bytes WP-0 measured); an HKLM 32-bit (`WOW6432Node`) entry governed by `Run32` and not by `Run`; the same name in the 64-bit view with a different state; HKCU 64/32 duplicates merged; RunOnce always enabled; `REG_EXPAND_SZ` expanded through a fake environment; a non-string value skipped and counted; a `.lnk` in the user folder resolved, and one turned off by `StartupFolder`; a `.lnk` with no path; `desktop.ini` skipped; a logon task enabled, one turned off, and a boot-only task left out; one failing source giving one sentence while the others list; all failing giving an error; every cap | `parse/win_startup.rs` | the module does not exist |
| `win_os_name` cases: Windows 10, Windows 11 with "Windows 10" in `ProductName`, a server, no `DisplayVersion`, no `UBR` | `parse/win_facts.rs` | the function does not exist |
| Restart Manager mapping from `RM_PROCESS_INFO`-shaped values to `Locker`, and the folder and missing-path answers | `windows/locks.rs` (pure part on every OS) | the module does not exist |
| Windows runner only: the reader lists the holder of a temporary file the test itself holds open (its own child process), and the startup reader lists entries the test writes to a **temporary** registry key and folder through the `RegistryView` and `FolderList` traits pointed at them. The real HKCU `Run` key is never written (ER-14). | `windows/locks.rs`, `windows/startup.rs` | the modules do not exist |

**The VM row (local only; family ADR 0040).** A `startup-facts-locks` row in `scripts/vm-smoke/win-rows.mjs` is added on
the harness's home branch `test/dyno-vm-harness-1.1-win`, by its owner or through it (family ADR 0042). It runs on the
merged harness:

1. Seed over the harness's admin SSH session, then verify every seed with `reg query`, `schtasks /query` and `dir`
   before any assertion (ER-33: the precondition is checked). The seeds are: an HKCU `Run` value, an HKCU `Run` value
   marked off in `StartupApproved\Run` with the bytes WP-0 measured, an HKLM `Run` value in the 32-bit view, a `.lnk`
   in the user Startup folder, and a logon task (`schtasks /create /sc onlogon`).
2. **Positive control:** Dyno's own agent entry (HKCU `Run`, `dyno-agent.exe`) must be listed. If it is missing, the
   row reports `check broken`, not "fail" (ER-33).
3. Assert through the app (`startupEntries` over CDP, as `startup.mjs` does in the investigation) and on the Health
   page's text: each seed is listed with its location label and its state (the turned-off one as off), and
   `unavailable()` is empty.
4. Facts: the OS name in the agent's facts equals `Get-CimInstance Win32_OperatingSystem` `Caption` and `BuildNumber`
   (mapped), and is never "Fake OS".
5. Locks: a PowerShell job in the guest holds a file open, and Dyno's "What is locking this file?" lists `powershell`.
6. The VM snapshot is reverted afterwards. Seeds never reach the host (ER-14).

## Work packages

Agent-days for one agent. WP-1 can land on its own: it removes the fakes and makes the three Windows gaps say so
honestly, ahead of the readers.

| WP | Package | Depends on | Days |
| --- | --- | --- | --- |
| WP-0 | VM measurements (local): the StartupApproved key and bytes per source kind (Task Manager toggles, key diff); `MSFT_PhysicalDisk` and the Diagnostics-Performance log as a standard user; the Task Scheduler walk's time and task count on a fresh Windows 11 | – | 0.5 |
| WP-1 | The mechanism: the failing no-fake tests and the `check-shipped` feature check first; `adapter()` and `Platform::adapters()`; `test-support` gates in `dyno-platform` and `dyno-core`; dev-dependency features in `dyno-agent`, `dyno-cli` and the app; `NotReadHere` replacing `Unsupported` and `FakeBattery`; Windows `platform()` names every port, with `NotReadHere` for the three gaps until WP-3 to WP-6; the `Platform wired` log line; the doctest moved; the `rustChecks` line with its cost measured; `cargo xwin check` clean | – | 1.5–2 |
| WP-2 | Family: the `test-support` gate in `paddock-power` and `paddock-secrets`, their CHANGELOG entries, the dev-dependency lines in Dyno and `paddock-pitwall` (integrating agent; D2) | WP-1 | 0.5 |
| WP-3 | Windows facts: `win_os_name` and its tests first, then the registry, power-plan, WMI and folder reads, per-fact logging | WP-0, WP-1 | 1.5–2 |
| WP-4 | Startup from the registry and folders: `assemble` and its tests first, `RegistryView` and `FolderList` adapters, StartupApproved, RunOnce variants on the wire (D3: `dyno-core`, `api.types.ts`, labels, the mock), per-source logging, the temporary-key Windows test | WP-0, WP-1 | 2–2.5 |
| WP-5 | Startup from logon tasks: the `LogonTasks` trait, the COM walk with caps, its pure mapping tests, its Windows-runner test against a task the test registers under its own temporary folder and removes | WP-4 | 1.5–2 |
| WP-6 | File locks through Restart Manager: the mapping tests first, the session guard, the Windows-runner test on its own temporary file | WP-1 | 1–1.5 |
| WP-7 | The VM row on the harness branch, run on the merged harness with screenshots of the Health page and the startup card, and an independent review of the screenshots (ER-23) | WP-3 to WP-6 | 1–1.5 |
| WP-8 | Docs: `architecture.md` and `features.md` (the platform table: Windows startup, facts and locks built, process details still not), the user guide (hidden tasks, what is not read), CHANGELOG 1.1.1, a lessons entry (the pattern and the check), `docs/known-copies.toml` if the COM init helper is shared | all | 0.5 |

**Total: 10 to 12.5 agent-days.** The critical path is WP-0 → WP-1 → WP-4 → WP-5 → WP-7, about 6.5 to 8.5 days. WP-3 and
WP-6 run beside WP-4.

### Parallelism and cost (ER-20)

- Startup: about ten registry reads and two folder listings take milliseconds. The Task Scheduler walk is the slow part
  (WP-0 measures it). The sources run one after another on the agent's startup job thread, which is never the UI
  thread. A parallel COM walk would need one apartment per thread for a gain of a few hundred milliseconds at most, so it
  is not done. The list is cached until `refresh()`, so the work is done once per refresh, not per page view.
- Facts run every few minutes off the sampling path. The WMI disk query is the slowest read. If WP-0 measures it over
  250 ms, disk health is kept for 30 minutes (disk health changes slowly), the same caching `smart_reader` does.
- Locks: one Restart Manager session per question, bounded by `MAX_HOLDERS`. Nothing to parallelise.

## Decisions for the user

- **D1. The check.** Recommended: the build-time assertion (§1's `cfg` gate), kept honest by the `check-shipped`
  feature-graph check and the Linux non-test `cargo check`, plus the no-fake test for a readable failure (§3). The
  alternatives are a Clippy entry (allow-lists in about 30 test files) and a source grep (misses aliases).
- **D2. The family test doubles** (`paddock-power`, `paddock-secrets`) are gated in the same change. Recommended: yes.
  It is three lines of adoption, and otherwise a Dyno port can still be wired to `FakePower` in a release.
- **D3. RunOnce.** Recommended: two new `StartupLocation` variants, `UserRunOnceKey` ("Registry, next sign-in only
  (this user)") and `MachineRunOnceKey`. This is a wire change mirrored in `api.types.ts` in the same change. The
  alternative folds RunOnce into `UserRunKey` and `MachineRunKey` and loses the "once" fact.
- **D4. The facts in 1.1.1.** Recommended: all rows of §4.2 (the OS name, restart pending, power plan, visual effects,
  background apps, developer folders, disk health), with pending updates and boot time honestly unavailable. The minimum
  alternative is the OS name only, with every other fact `NotReadHere` (WP-3 at about 0.5 days).
- **D5. Boot-trigger tasks** (`TASK_TRIGGER_BOOT`). Recommended: not listed in 1.1.1. They run before anyone signs in,
  like services, and the page is "what slows your sign-in". This can be revisited with a services view.
- **D6. Windows process details** (`NoDetails`). Recommended: out of scope. It is an honest, documented gap, not a fake.
  It is a separate item for a later release.

## Consequences

- A Windows user sees their real startup entries, a Health score that can drop for a failing disk or a pending restart,
  and real answers to "what is locking this file". A source Dyno cannot read is a sentence, not an empty list.
- A new port, or a new OS, without a real reader does not compile into a release unless it is given an honest
  `NotReadHere`. Once the fakes are gated, the "every platform names every port" rule needs no lint of its own.
- Test-only code moves behind a feature. Every test crate's `[dev-dependencies]` names `test-support` once. A test that
  forgets it fails to compile with "could not find `fake` in `dyno_platform`", which is easy to fix.
- The agent's log names every adapter at start, so a support bundle shows what is wired.
- The macOS and Linux wiring is unchanged apart from the `NotReadHere` rename and the new test.
- Cost: 10 to 12.5 agent-days in Dyno 1.1.1, and one extra `cargo check` on the Linux leg (measured in WP-1).

## Sources

Microsoft Learn, as remembered at the time of writing. They were not fetched in this session. WP-0 re-reads each one and
records the date before the code relies on it.

- "Run and RunOnce Registry Keys" (the four keys, RunOnce deletion, the 260-character value limit).
- "Registry Keys Affected by WOW64" and "Accessing an Alternate Registry View" (`KEY_WOW64_32KEY`, `KEY_WOW64_64KEY`;
  `HKCU\Software` not redirected).
- "KNOWNFOLDERID" (`FOLDERID_Startup`, `FOLDERID_CommonStartup`) and `SHGetKnownFolderPath`.
- Task Scheduler 2.0: `ITaskService`, `ITaskFolder::GetTasks` (`TASK_ENUM_HIDDEN`), `ITriggerCollection`,
  `TASK_TRIGGER_TYPE2` (`TASK_TRIGGER_LOGON` = 9, `TASK_TRIGGER_BOOT` = 8), `IExecAction`.
- Restart Manager: `RmStartSession`, `RmRegisterResources`, `RmGetList`, `RM_PROCESS_INFO`, `RM_APP_TYPE`,
  `RmEndSession` (at most 64 sessions per user at once).
- `RtlGetVersion`; `MSFT_PhysicalDisk` (`HealthStatus`); `PowerGetActiveScheme`, `PowerReadFriendlyName`.
- StartupApproved values: the format in `parse/startup_approved.rs` (Dyno's own, tested). The per-source key mapping is
  measured in WP-0, not taken from a source.
