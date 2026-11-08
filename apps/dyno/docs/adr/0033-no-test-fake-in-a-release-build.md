# 33. No test fake in a release build; real Windows readers for startup items, facts and file locks

- Status: Accepted (2026-11-08), for Dyno 1.1.1. Approved: user, 2026-11-08: D1 to D9 as recommended, and D10 decided as
  "the non-test host check runs on every Dyno branch run; its cost is measured and reported, and the user is asked again
  only if it is large". The independent review approved revision 2 with no P1 (re-review of `2f911c8d3`); revision 3
  folds in its P2s.
- Date: 2026-11-08
- Revision 2 (2026-11-08): answers the independent review of `350e351d9` (6 P1, 15 P2).
  - P1 fixes:
    - the shipped list comes from `tauri.conf.json` and includes `dyno-helper`; the assertion lives in the test CI runs;
    - a non-test build on the Windows and macOS legs, and a corrected claim about pushes to main;
    - no WinTrust code exists, so publisher reading is deferred (D8);
    - D2 is split: `MemorySecrets` has a production user;
    - Health lists the facts it could not check (§4.5);
    - packaged-app startup tasks are a new source, and the sources that are not read are named (§4.1).
  - The P2 answers are inline, marked "(review P2-n)".
- Revision 3 (2026-11-08): the re-review of `2f911c8d3` approved revision 2 with no P1. This revision folds in its P2s,
  marked "(re-review P2-x)":
  - the declaration check's scope;
  - the packaged-app measurement;
  - the host-check cost, with moving the check to the nightly now a user decision (D10);
  - "not checked" kinds;
  - `signerRead` from the engine;
  - the estimate arithmetic;
  - the protocol bump;
  - the task-registration right;
  - the WP-1/WP-2 order.
- Trigger: the investigation of 2026-11-08 (Windows tray work, evidence in the coordinator's scratch folder `win-tray/`:
  `vm/startup-page.png`, `vm/logs.txt` and the notes). Dyno 1.1.0 ran in a Windows 11 VM where its own agent starts from the
  HKCU `Run` key (`dyno-agent/src/service/spec.rs`, `WINDOWS_RUN_KEY`). There:
  - Health showed **100, Good, "Nothing needs your attention"**;
  - the startup card said **"No startup items. Nothing is set to start when you sign in."**

  The user decided the same day that this is fixed in Dyno 1.1.1.
- Related:
  - Engineering rules: ER-21 / rule 40 (a recurring bug means a missing design rule), ER-11 (failing test first), ER-14 / rule 7
    (tests never touch the user's machine), ER-27 / rule 56 (logs explain every failure), ER-33 (a check counts only if its
    probe can see), ER-20 / rule 51 (more threads or a cheaper operation), ER-22 / rule 42 (share first).
  - Paddock rules: rule 53 / family ADR 0033 (processes by identity), rule 54 (an installer build before landing a
    release-path change), rule 55 / family ADR 0034 (one implementation per mechanism).
  - Family ADRs: 0039 (release gates inspect the artifacts), 0040 (the Windows GUI test session), 0042 (one owner per
    harness), 0051 (`paddock-tools`: a `WinVerifyTrust` Authenticode check, accepted 2026-11-04, on
    `origin/docs/paddock-tools-design-v2`) and 0054 (`wiring.toml` edges).
  - Dyno ADRs: [0021](0021-monitoring-only.md) (monitoring only: Dyno does not enable or disable a startup entry) and
    [0027](0027-helper-for-1-1-0.md) (the helper answers scoped reads).
  - The macOS fixes for the same symptom: `a935f3c00`, `f5497766c` and `79a31a415` (launch agents, `sfltool dumpbtm`
    through the helper, and a partial list that says what it could not read). They never covered Windows.

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

The struct-update spread `..f` fills three fields the Windows wiring does not name (`locks`, `startup` and `facts`) from
`fake::Fake`. Every Dyno release since the first commit (`be400ed05`, the same lines) has shipped them to Windows users:

| Port | What the fake answers (`fake.rs`) | What the user saw |
| --- | --- | --- |
| `StartupItems` | `Ok(vec![])`; `unavailable()` is empty | "No startup items. Nothing is set to start when you sign in." |
| `FactsSource` | `SystemFacts::unknown("Fake OS 1.0")`: every fact "Not checked yet" | Health cannot raise a disk-health, restart-pending or update reason, so it reads 100 Good; the facts name the OS "Fake OS 1.0" |
| `FileLocks` | `Ok(vec![])` for every path | "What is locking this file?" says nothing holds it |

The gap was known. `docs/architecture.md:42` says "the Windows platform still uses the in-memory fakes for them, so those
screens are empty on Windows", and `features.md` says "not wired yet" in three places. The product never said so to the
user. A fake answers in the shape of real data (an empty list, unknown facts), so neither the page nor the Health score
could tell "nothing there" from "never read". Four parsers already exist and are tested, but nothing calls them:

- `parse/startup_approved.rs` (`approved_state`);
- `parse/lnk.rs` (`parse_lnk`);
- `parse/win_facts.rs`;
- `parse/powercfg.rs`.

### Why this is a pattern, not one bug (ER-21)

The same shape appears at three levels: a test double was reachable from production code, and production code used it.

1. **`windows::platform()`** uses `..f`, shown above.
2. **`hardware::battery_capacity()`** (`hardware.rs:62`), on an OS that is not macOS, Linux or Windows, returns
   `FakeBattery(Reading::unavailable(...))`. The answer is honest, but the type is a test fake chosen because it was at
   hand.
3. **Test doubles are public in every build.** In Dyno:
   - `dyno_platform::fake`;
   - `counters::FakeCounters`;
   - `hardware::{FakeThrottle, FakeWear, FakeBattery}`;
   - `wol::{FakeWakeTimer, WolPorts::fake}`;
   - `dyno_core::wol::fake`.

   In the family crates: `paddock_power::{FakePower, FakeEvents}`. Only these are behind `#[cfg(test)]`:
   `linux/tools.rs` `fake`, `dyno-agent`'s `cache::testing`, `disk::testing` and `issues::testing`, and `dyno-bench`'s
   `fakes`.

Rule 7 makes every Dyno test use the fakes, so no test on any OS could notice that the shipped wiring used them too. The
macOS fix added real readers but left the mechanism in place: a new port or a new OS still gets a fake by default, and
nothing says so.

### The same check on macOS, Linux and other systems

- `macos/mod.rs` and `linux/mod.rs` name all 14 `Platform` fields; there is no spread and no fake.
- `other.rs` names all fields too. Its honest `Unsupported` adapters answer `PlatformError::Unsupported("Not supported on
  this OS yet.")`, and that is the pattern this record keeps.
- The other struct-update spreads in production code (`discovery.rs`, `geo_db.rs`, `nt.rs`, `readability.rs` and more)
  copy data values, not port bundles.
- The agent destructures `Platform` field by field (`agent.rs:663`), so a new port is a compile error there.
- `dyno-agent`'s `DiskPorts::none()` (`Running::start`) is an honest stub ("This agent has no system to ask."). The real
  binaries call `Running::start_os` (`cli.rs:263`), which wires `DiskPorts::os()`.
- Windows process details use `NoDetails`. It is an honest, documented stub, not a fake, and it is out of scope (D6).

## Decision

### 1. The mechanism: a fake does not exist in a build that ships

1. **Every test double is compiled only for tests.** The doubles of the Context list sit behind
   `#[cfg(any(test, feature = "test-support"))]`: `dyno_platform::fake`, `counters::FakeCounters`,
   `hardware::{FakeThrottle, FakeWear, FakeBattery}`, `wol::FakeWakeTimer` with `WolPorts::fake()`, and
   `dyno_core::wol::fake`.
   - `dyno-platform` and `dyno-core` each get a `test-support` feature. `dyno-platform`'s also enables
     `dyno-core/test-support` and, with D2, `paddock-power/test-support`.
   - Consumers enable these features **only in `[dev-dependencies]`**. Today that is `dyno-agent`, `dyno-cli` (whose normal
     dependencies do not include `dyno-platform`: "the CLI never links the platform adapters"), `dyno-helper` and
     `app/src-tauri`.
   - With Cargo's resolver 2 (`apps/dyno/Cargo.toml`), dev-dependency features are not unified into a build that compiles
     no test target. So `tauri build` and `sidecar.mjs`'s single `cargo build -p dyno-agent -p dyno-cli -p dyno-bench
     -p dyno-bench-gpu -p dyno-helper --profile dist` compile `dyno-platform` without the fakes.
2. **So a build without test targets cannot name a fake.** The compiler resolves every path, alias and re-export.
   `..crate::fake::Fake::new().platform()`, `use crate::fake as f` and a new `FakeX` used outside a test are all compile
   errors in such a build for the OS that code compiles on. §3 says which builds those are on a branch.
3. **Every platform names every port.** Each `platform()` (Windows, macOS, Linux and other) lists every field of
   `Platform` and of its sub-structs (`NetworkPorts`, `HardwareReaders`, `PowerPorts`, `WolPorts`), with no `..`.
   - A port an OS cannot read yet gets an honest adapter that names its gap. `other::Unsupported` becomes the shared
     `unsupported::NotReadHere { what, why }`:
     - `list()` answers `PlatformError::Unsupported(why)`;
     - `facts()` answers the real OS name, with every other fact `Reading::unavailable(why)`.
   - No `Default` is derived for `Platform` or its sub-structs. The only value a spread could take is another whole
     platform, and each of those is either per-OS (compiled out on the other OSes) or the fake (absent without test
     targets).
4. **`FakeBattery` in `battery_capacity()`** becomes `NotReadHere`: the same answer under an honest name.
5. **The family doubles `paddock_power::{FakePower, FakeEvents}`** get the same gate in the same change (D2). Their users
   (`git grep` at `origin/main`, 2026-11-08) are:
   - Dyno's `fake.rs` and its tests `dyno-agent/tests/{power,wakes}.rs`;
   - `paddock-power`'s own integration tests `tests/it/{events,keep_awake}.rs`. These compile the library without
     `cfg(test)`, so they need a self dev-dependency: `paddock-power = { path = ".", features = ["test-support"] }`.

   The crate doc's "no test in the family may call the real adapters" stays, with one sentence added: "building them is
   inert; calling them is not" (review P2-9). It is a shared change (rules 39 and 43), so the integrating agent lands it
   with a CHANGELOG entry.
6. **`paddock_secrets::MemorySecrets` stays public** (D2b). `apps/workbench-demo/src-tauri/src/lib.rs:97` uses it in
   production, as a demo that must not touch the user's credential store. That makes it a legitimate ephemeral store, not
   only a test double. Its doc line changes from "In-memory `Secrets` for tests" to "an in-memory store that forgets on
   exit (tests, demos)". Dyno's only production path to it, `WolPorts::fake()`, is gated by item 1.
7. **The fake's doc example** (`fake.rs:4-12`) moves into a unit test, because a doctest compiles against the library
   without its own `test-support`.
8. **`approved_value`** (`parse/startup_approved.rs:17`) writes what Task Manager writes to turn an entry off, and has no
   use under ADR 0021. It moves under `#[cfg(test)]`, where it builds fixtures for §5 (review P2-12).

### 2. The test that fails today, and wiring visible in the logs

- **`adapter()`.** A provided method is added to every trait that a `Platform` field holds (review P2-3):
  - `dyno_core::ports`: `SystemSensors`, `ProcessSource`, `FileLocks`, `StartupItems`, `FactsSource`, `UserActivity`,
    `NetworkLinks`, `CounterSource`, `NetworkSockets`, `ReverseDns`, `ProcessTraffic`, `ThrottleSignals`,
    `DiskWearSource`, `BatteryCapacitySource`, `PowerActions`, `IdleSignals`;
  - `dyno_core::issues::IssueSource`;
  - the `dyno_core::wol` traits that `WolPorts` holds, including the wake scheduler.

  ```rust
  /// The concrete adapter behind this port (for the wiring test and the agent's start log).
  fn adapter(&self) -> &'static str { std::any::type_name::<Self>() }
  ```

  - A provided method is monomorphised per implementation, so through a `Box<dyn Port>` it returns the concrete type, for
    example `dyno_platform::fake::Fake`. Nobody overrides it.
  - A wrapper that holds another `Box<dyn Port>` reports its own name and hides the inner adapter, so the test sees only
    the top-level wiring. That is enough for `Platform`: today no `platform()` wraps a port in another.
  - The zone is a value (`Arc<SystemZone>`), not a trait object, and is listed by its type.
  - `Platform::adapters()` returns a `(port, adapter)` pair for every field. It uses a destructuring
    `let Platform { .. }` that names each field, so a new port it does not list is a compile error.
- **The no-fake test** lives in each OS module: `windows::tests::the_windows_platform_wires_no_fake`, and the same for
  `macos` and `linux`.
  - It builds the real `platform()` with a temporary secrets folder and calls **no port method**.
  - It asserts that no adapter path contains `::fake::` or `::testing::`, and that no last segment starts with `Fake`.
  - On `origin/main` the Windows test fails and names `locks`, `startup` and `facts` → `dyno_platform::fake::Fake`. The
    macOS and Linux tests pass and guard against a regression.
  - ER-14: the WP checks that every constructor `platform()` calls is inert, and makes any that is not lazy. Inert means
    no OS state changed and no file written outside the temporary folder. `PowerPorts::os()` builds unit structs
    (`paddock-power` `native.rs`) and `WolPorts::os` builds its store under the given folder. The Linux end-to-end test
    already builds the real platform.
- **A declaration check (review P2-2).** A `syn` scan in the style of `dyno-agent/tests/helper_wiring.rs` reads the
  non-test sources of every shipped crate (re-review P2-A). The crate list is not hand-kept. It is §3.1's derived shipped
  set (`dyno-app`, `dyno-agent`, `dyno-cli`, `dyno-bench`, `dyno-bench-gpu`, `dyno-helper`) plus their path dependencies
  under `apps/dyno/crates`, read from `cargo metadata` (today `dyno-core`, `dyno-platform`, `dyno-helper-service` and
  `dyno-plugin-host` and `dyno-plugin-api`, and any added later). The scan fails on:
  - a public item named `Fake*`;
  - a `mod fake` or `mod testing`;
  - an `impl` of a port trait for a type named `Fake*`, `Stub*` or `Scripted*`;

  unless the item sits under `cfg(test)` or `cfg(any(test, feature = "test-support"))`. Its fixture tests (sources that
  must fail and sources that must pass) run every time. This is a declaration check, not the use-site grep §3 rejects. It
  catches a new ungated double on any OS, before anyone wires it.
- **ER-27.** The agent logs one `Platform wired` line at start with every `port=adapter` pair. A Dyno diagnostic bundle
  would have shown `startup=dyno_platform::fake::Fake` in the first Windows report. Type names are not user content.
- The shared traits (`paddock_power::PowerOs` and `SleepWakeSource`, `paddock_secrets::Secrets`) get no `adapter()`: it
  would change the family API just for a test. For `PowerOs` and `SleepWakeSource`, §1.5's gate (D2a) is the guarantee.
  The declaration check scans only Dyno's crates; family crates are out of its scope (re-review P2-A). The test lists
  them as `(unnamed)`.

### 3. The checks that keep it true (D1)

The guarantee is **§1's `cfg` gate**: no build without test targets can name a fake. It is kept honest by three checks,
each of which covers what the others cannot see:

1. **The feature never reaches a shipped binary (all targets, Linux leg)** (review P1-1).
   - `SHIPPED` in `scripts/check-shipped.mjs` is derived instead of hand-kept: `dyno-app` plus every
     `bundle.externalBin` of `app/src-tauri/tauri.conf.json`, mapped to its package (`dyno` → `dyno-cli`). The result is
     `dyno-app`, `dyno-agent`, `dyno-cli`, `dyno-bench`, `dyno-bench-gpu` and `dyno-helper`. The hand list misses the
     helper, which is shipped since 1.1.
   - The existing "no shipped binary depends on `dyno-helper`" assertion becomes "no shipped binary other than the helper
     depends on it".
   - A new assertion: no shipped package's `cargo tree -p <pkg> -e normal --target all -f '{p} {f}'` enables
     `test-support` on `dyno-platform`, `dyno-core` or `paddock-power`.
   - A positive control (ER-33, review P2-1): the same probe with dev edges, `cargo tree -p dyno-agent -e normal,dev -f
     '{p} {f}'`, must **show** `dyno-platform` with `test-support`. If it does not, the probe or the feature name has
     changed, and the check reports `check broken` instead of passing.
   - Both live in `scripts/check-shipped.test.mjs`, the file `paddock.ci.json` `checks` runs on the Linux leg. The script's
     own `main` runs them too, for a local run.
2. **A non-test build of the shipped packages on every OS leg, on branches** (review P1-2).
   - `cargo check --locked --lib --bins -p dyno-platform -p dyno-agent -p dyno-app -p dyno-helper` runs on all three legs.
     These are the packages that link `dyno-platform` in their normal dependencies. `dyno-cli` does not.
   - On Linux this is Dyno's `rustChecks`.
   - On Windows and macOS it needs a new contract field, `hostRustChecks`, which the shared `app-gates` action runs on the
     non-Linux legs after clippy and with the same preparation (WP-1b; `.gitea/` is a shared integration point, so the
     integrating agent changes it). Today `rustChecks` runs on Linux only (`.gitea/actions/README.md:55`), and the Windows
     and macOS legs run only `clippy --all-targets` and `nextest`, which unify `test-support` in.
   - **Cost (re-review P2-C).** Revision 2 said this recompiles only `dyno-platform` and its dependents, which is wrong.
     `app/src-tauri`'s dev-dependencies change the features of third-party and shared crates:
     - `tauri` with `["test"]`;
     - `paddock-shell` with `["tray", "mock-runtime"]`;
     - `paddock-menu` and `paddock-files` with `["tauri"]`.

     So the non-test graph of `dyno-app` also rebuilds `tauri`, its plugins and the shell crates. Expect minutes on the
     Windows leg. WP-1 and WP-1b measure the cost per leg before anything else is decided, and record it (rule 48).
   - **Where it runs (D10, decided 2026-11-08):** on every Dyno branch run, Windows and macOS legs included. Nothing moves to the nightly automatically; a large measured cost goes back to the user.
3. **The installers are built before WP-1 and WP-2 land** (rule 54).
   - The gate changes exactly what the dist build compiles, so the batch dispatches `dyno-release.yml` with gates on and
     bundles built for all three OSes.
   - Correction of revision 1: `dyno-release.yml` does not run on a push to main. Its triggers are tags and
     `workflow_dispatch` (`dyno-release.yml:5`, "push to main — nothing: the nightly builds the installers"). The nightly is
     detection after a merge, not a branch gate.

The build brief adds a local `cargo xwin check` of the same packages for `x86_64-pc-windows-msvc` before every push
(ER-9).

Rejected:

- **Clippy `disallowed-types` / `disallowed-methods`.** The cost of the allows is small: one
  `#![cfg_attr(test, allow(clippy::disallowed_types))]` per crate root, the pattern `lib.rs:8` already uses for `unwrap`,
  plus one per integration-test crate (review P2-14). But Clippy lints `--all-targets`, which unifies `test-support` in,
  so it never sees the build that ships. And a crate-wide allow under `cfg(test)` does not stop a production reference
  inside a `test-support`-gated module.
- **A source-text conformance check at use sites** (a grep for `fake::` or `..`). It misses aliases and re-exports, and it
  cannot tell a data spread (`..raw()`, common in production) from a port spread. §2's declaration check is a different
  thing: it inspects where doubles are declared, not where they are used.
- **The no-fake test alone.** It sees the platform's ports, not a fake used elsewhere in production code (the agent, the
  app), and it relies on a naming convention. It stays as the readable failure for the wiring, not as the guarantee.

### 4. Real Windows readers

All readers run with the user's rights. Dyno changes no startup entry (ADR 0021), so nothing here needs the helper. The
`StartupLocation` doc comments that say "disabling needs the privileged helper" predate ADR 0021 and are reworded ("written
for every user").

Each reader is a thin adapter over small OS traits (`RegistryView`, `FolderList`, `LogonTasks`, `PackagedStartup`,
`RestartManager`) and a pure `assemble` function. The tests run `assemble` on every OS (§5). The new FFI modules
(`windows/startup.rs`, `windows/facts.rs`, `windows/locks.rs` and the task and package adapters) are added to
`apps/dyno/wiring.toml` `[edges] modules` in the same change (family ADR 0054, review P2-11).

#### 4.1 Startup items (`windows/startup.rs`, `parse/win_startup.rs`)

| # | Source | Where | `StartupLocation` | State from |
| --- | --- | --- | --- | --- |
| 1 | HKCU Run | `HKCU\Software\Microsoft\Windows\CurrentVersion\Run`, 64-bit and 32-bit views | `UserRunKey` | HKCU `…\Explorer\StartupApproved\Run` |
| 2 | HKCU RunOnce | `…\RunOnce`, both views | `UserRunOnceKey` (new, D3) | always Enabled (runs at the next sign-in, then Windows removes it) |
| 3 | HKLM Run, 64-bit | `HKLM\SOFTWARE\Microsoft\Windows\CurrentVersion\Run` (`KEY_WOW64_64KEY`) | `MachineRunKey` | HKLM `…\StartupApproved\Run` |
| 4 | HKLM Run, 32-bit | the same key with `KEY_WOW64_32KEY` (`WOW6432Node`) | `MachineRunKey` | HKLM `…\StartupApproved\Run32` |
| 5 | HKLM RunOnce | both views | `MachineRunOnceKey` (new, D3) | always Enabled |
| 6 | User Startup folder | `FOLDERID_Startup` (`SHGetKnownFolderPath`) | `UserStartupFolder` | HKCU `…\StartupApproved\StartupFolder` |
| 7 | Common Startup folder | `FOLDERID_CommonStartup` | `CommonStartupFolder` | HKLM `…\StartupApproved\StartupFolder` |
| 8 | Logon tasks | Task Scheduler 2.0 COM: `ITaskService::Connect`, a recursive walk from `\` with `GetTasks(TASK_ENUM_HIDDEN)`, tasks with a `TASK_TRIGGER_LOGON` trigger | `ScheduledTask` | `IRegisteredTask::Enabled` and the trigger's `Enabled` |
| 9 | Packaged-app startup tasks (MSIX/AppX `StartupTask`: Teams, Windows Terminal, Store apps) | per-user state under `HKCU\Software\Classes\Local Settings\Software\Microsoft\Windows\CurrentVersion\AppModel\SystemAppData\<PackageFamilyName>\<TaskId>` (`State`), the name from the package's display name where the package API gives it | `PackagedApp` (new, D3) | the `State` value, mapped as WP-0 measures it |

- **Packaged apps (review P1-6, D7).** Task Manager lists them under Startup apps, and a machine whose only startup items
  are packaged apps would otherwise show "No startup items" again.
  - WP-0 measures before it toggles anything (re-review P2-B):
    1. On a fresh image, record the `SystemAppData` keys and compare them with Task Manager's Startup apps list.
    2. Only then toggle each task (enabled, turned off by the user, turned off by policy) and diff the keys.
  - Toggling writes the key, so a task that was never toggled may have no key or no `State` value. If any task that Task
    Manager lists has no key:
    - the reader enumerates the current user's packages (`Windows.Management.Deployment.PackageManager`) and the
      `uap5:StartupTask` entries of their manifests;
    - each task's default comes from its `Enabled` attribute;
    - the registry `State` is only an override.

    That needs the `windows` crate features `Management_Deployment` and `ApplicationModel` (WP-5b).
  - The executable comes from the package's manifest when the install location can be read; otherwise `exe` is `None`
    and the command is `<PackageFamilyName>!<TaskId>`.
  - If WP-0 finds the state cannot be read reliably, the fallback is the permanent sentence below, not silence.
- **What is deliberately not read** (review P1-6, D5). The startup card states its coverage once, under the list, on
  Windows: "Lists registry Run and RunOnce entries, Startup folders, sign-in tasks and Store apps. Not listed: services,
  tasks that start with Windows, policy Run keys and Winlogon entries."
  - The things not read: services and drivers; boot-trigger tasks (D5); `…\Policies\Explorer\Run` (HKCU and HKLM);
    Winlogon `Userinit` and `Shell`; Active Setup; shell extensions; WMI event subscriptions.
  - The sentence is coverage, not a failure, so it is not part of `unavailable()`. The user guide carries the same list.
- **Values.** `REG_SZ` and `REG_EXPAND_SZ`; the latter is expanded with `ExpandEnvironmentStringsW` into a bounded buffer.
  - Bounds (review P2-6):
    - at most `MAX_RUN_VALUES` (1,000) values per key;
    - each value at most `MAX_VALUE_BYTES` (32 KiB), checked against `RegQueryInfoKey`'s longest value before any read;
    - longer values, other types and values past the cap are skipped and counted (`Skipped` in the log).
  - Ids (review P2-5):
    - registry: `reg:<HKCU|HKLM>:<run|runonce>:<64|32>:<value name>`;
    - folders: `folder:<user|common>:<file name>`;
    - tasks: `task:<task path>` (for example `task:\Microsoft\Windows\…`);
    - packaged apps: `appx:<PackageFamilyName>!<TaskId>`.

    A test checks that `is_os_owned` classes a `\Microsoft\Windows\` task as the OS's, and an HKLM Run id or an
    `…\Microsoft\Windows\CurrentVersion\Run` path as not the OS's.
  - `HKCU\Software` is not redirected under WOW64, so its 32-bit view is the same key. Entries with the same hive, key,
    name and command are merged.
- **StartupApproved.** The value with the entry's name, read with the existing `approved_state`. A missing or unreadable
  value means Enabled, which is what Windows does.
  - The table's mapping (which approved key governs which source) is what Task Manager writes on Windows 10 and 11.
  - WP-0 confirms it in the VM: it turns off one entry of each kind in Task Manager and diffs the keys (ER-28: measured,
    not assumed). The parsing tests use the measured bytes, built with the `cfg(test)` `approved_value`.
- **Startup folders.**
  - At most `MAX_FOLDER_FILES` (1,000) files per folder; `desktop.ini` is skipped.
  - A `.lnk` is read (at most `MAX_LNK_BYTES`) and resolved to its target and arguments with the existing `parse_lnk`. A
    shortcut with no path, such as a Store app's, shows as the `.lnk` itself.
  - Any other file is listed as its path.
- **Tasks** (review P2-7).
  - Bounds: at most `MAX_TASKS` (10,000) tasks visited, at a folder depth of at most 16.
  - A folder whose `GetFolders` or `GetTasks` fails with `E_ACCESSDENIED` is counted. `unavailable()` then says "N task
    folders could not be read without administrator rights."
  - A logon trigger counts only when its `UserId` is empty (any user) or names the current user. A trigger for another
    account does not run for this user.
  - The command is the `IExecAction` `Path` and `Arguments`. A task with only COM-handler actions shows its handler's
    CLSID.
  - COM is initialised with the helper that `windows/firewall_com.rs` uses (rule 55: no second copy).
  - `GetTasks` documents only `TASK_ENUM_HIDDEN` and an `HRESULT` (Microsoft Learn, fetched 2026-11-08). It says nothing
    about which tasks a standard user cannot see. WP-0 measures that: with no error, there is no count, and the user
    guide says so.
- **Partial lists.** Each source is read on its own. A failure goes into `unavailable()` as one sentence ("The all-users
  Run key (32-bit) could not be read: access denied."), and the other sources still list their entries. When every source
  fails, `list()` returns the error, as the macOS reader does. The list is kept between `list()` calls until `refresh()`;
  tasks are the slow part.
- **Publisher (review P1-3, D8).** Revision 1 claimed an existing WinTrust signer reader. There is none: Windows process
  details are `NoDetails` (`SignatureInfo::Unknown`), and `git grep` finds only the `windows` crate's feature names.
  - In 1.1.1 `publisher` stays `None` on Windows.
  - The engine says so (re-review P2-E): `StartupItems` gains a provided `signer_read(&self) -> bool` (default `true`;
    `false` for the Windows reader), and the agent puts it on the startup reply as `signerRead`. It is mirrored in
    `api.types.ts` and the mock as part of D3's wire change.
  - The UI disables "Hide signed apps" and shows "Signer not read on this system yet" when `signerRead` is false. The UI
    never decides this from the OS name.
  - "Hide system entries" still works: `is_os_owned` uses the path and the task name.
  - A signer reader (embedded Authenticode plus catalog signatures, bounded by file size and time, rule 51) is a later item.
    It should be shared with family ADR 0051's `WinVerifyTrust` check (ER-22 / rule 42), which also gives Windows process
    details a signer.

#### 4.2 Facts (`windows/facts.rs`)

| Fact | Source | Notes |
| --- | --- | --- |
| `os` (required) | `HKLM\SOFTWARE\Microsoft\Windows NT\CurrentVersion`: `ProductName`, `EditionID`, `DisplayVersion` (or `ReleaseId`), `CurrentBuildNumber`, `UBR`, `InstallationType`; the build cross-checked with `RtlGetVersion` (not shimmed by the manifest) | `ProductName` still says "Windows 10" on Windows 11, so a client build ≥ 22000 is named "Windows 11" by a pure `win_os_name()` in `parse/win_facts.rs`. Example: "Windows 11 Pro 24H2 (build 26100.2314)". A server keeps its `ProductName`. |
| `restart_pending` | the keys `…\WindowsUpdate\Auto Update\RebootRequired` and `…\Component Based Servicing\RebootPending` exist | `PendingFileRenameOperations` is left out because many installers set it, which would give false "restart needed" reasons. A key that cannot be read is unavailable with its reason. |
| `power_plan` | `PowerGetActiveScheme` and `PowerReadFriendlyName`, plus the Windows 11 power mode from `PowerGetEffectiveOverlayScheme` | The fact is the **effective** mode (review P2-10). Power saver is true for the Power saver scheme or the "Best power efficiency" overlay. WP-0 confirms the overlay GUIDs. The GUID mapping (`POWER_SAVER_GUID`) is reused from `parse/powercfg.rs`; no `powercfg` process and no locale parsing. |
| `visual_effects_full` | HKCU `VisualEffects\VisualFXSetting` | the existing parser |
| `background_apps` | HKCU `BackgroundAccessApplications` | WP-0 checks that the value still means something on Windows 11 23H2 and later. If not, the fact is unavailable ("Windows no longer keeps one switch for this") (review P2-10). |
| `dev_folders` | `dev_folders::scan(%USERPROFILE%)` | as on macOS |
| `disks` | WMI `root\Microsoft\Windows\Storage`, `MSFT_PhysicalDisk` (`FriendlyName`, `HealthStatus` 0/1/2/5) through the `wmi` crate already linked | WP-0 confirms a standard user can read it. If not, the fact is unavailable ("needs the helper"), and Health shows it as not checked (§4.5). |
| `pending_updates` | not read: "Dyno does not ask Windows Update (the search is slow and contacts Microsoft). Settings > Windows Update shows them." | the macOS stance (`softwareupdate` is not run) |
| `last_boot_secs` | not read in 1.1.1: "Windows keeps boot times in a log Dyno does not read yet." | Decided now (review P2-10): WP-0 does not measure it; a later helper read can fill it. |

Each fact is timed and read on its own. A failure is that fact's `Reading::unavailable`, never an error for the whole
(`FactsSource` contract).

#### 4.3 File locks (`windows/locks.rs`)

The reader uses Restart Manager:

1. `RmStartSession`.
2. `RmRegisterResources` with the one path.
3. `RmGetList`. On `ERROR_MORE_DATA` it retries with the size `pnProcInfoNeeded` reports, at most 3 times, because the
   list can grow between calls (review P2-7). The total is capped at `MAX_HOLDERS` = 4,096, with `truncated` set.
4. `RmEndSession`, in a guard that runs on every path.

- **Identity.** Each `RM_PROCESS_INFO` gives `Process.dwProcessId` and `Process.ProcessStartTime`. These map to
  `ProcessRef { pid, started }` through `parse/time.rs` (`started_from_filetime`, `same_start`), the identity rule 53 asks
  for.
- **Name and kind.** `strAppName` gives the name, falling back to the process table's. `ApplicationType` gives the kind:

  | `ApplicationType` | Kind |
  | --- | --- |
  | `RmMainWindow`, `RmOtherWindow` | Application |
  | `RmService` | Service |
  | `RmExplorer` | Explorer |
  | `RmConsole` | Console |
  | `RmCritical` | System |

  The mapping is a pure function and is tested.
- **Folders.** `RmGetList` returns `ERROR_ACCESS_DENIED` when "a path registered to the Restart Manager session is a
  directory" (Microsoft Learn, fetched 2026-11-08). A folder path therefore answers `PlatformError::Unsupported("Pick a
  file: Windows reports which programs hold a file, not a folder.")` before any session starts.
- **Other cases.** A missing path answers `NotFound`. Dyno's own process is filtered out, as on macOS.
- **Rights.** The page does not say what a standard user sees of other accounts' processes. WP-0 measures it. Rows without
  a name keep the pid and say "another account".

#### 4.4 Logging (ER-27)

- **Startup.**
  - One line per source, `Startup source read`, with `Source=hklm-run-32`, `Count`, `Skipped`, `DurationMs` and `Outcome`.
  - A failure is logged at `warn` with the raw Win32 error or HRESULT and the sentence shown, and with the denied task-folder
    count.
  - A summary line gives the total, the number of unavailable sources and the duration.
  - Entry names and commands are not logged by default: they can hold user paths.
- **Facts.** One line per fact, with `Fact`, `Outcome` and `DurationMs`. The OS name is logged; the agent's start line
  already logs `OsVersion`.
- **Locks.** `Lockers read`, with `Count`, `Truncated`, `DurationMs`, `Retries` and each Restart Manager call's error code.
  The path is user content and is not logged.
- **Wiring.** The `Platform wired` line of §2.

#### 4.5 Health says what it could not check (review P1-5)

Today `health::assess` adds reasons only for facts that have a value: `disks`, `restart_pending` and `pending_updates`
(`health.rs:140,261,271`). The hero prints "Nothing needs your attention." whenever there are no reasons
(`health-hero.ts:32-33`). An unread fact is therefore an all-clear: the same false all-clear this record exists to remove,
on the most visible card, and on macOS too, where pending updates are never read.

- **Engine.** `HealthReport` gains `not_checked: Vec<NotChecked { what, why, kind }>`. `assess` fills it for every fact it
  scores whose reading has no value, with the reading's own sentence.
  - `kind` keeps two different cases apart (re-review P2-D). The engine decides the kind, never the UI:
    - `CouldNotRead`: the read failed or was denied this time ("disk health: needs administrator rights");
    - `NotReadHere`: Dyno does not read this fact on this OS by design ("pending updates" on macOS and Windows,
      "boot time" on Windows).
  - `assess` learns which facts are not read by design from the facts themselves. `SystemFacts` gains
    `not_read_here: Vec<FactKind>`, appended, which each `FactsSource` sets: `MacFacts` declares pending updates,
    `WinFacts` declares pending updates and boot time, and `NotReadHere` declares everything except the OS name.
- **Wire (re-review P2-G).** WP-H plans a `PROTOCOL_VERSION` bump.
  - Postcard is not self-describing. A newer reader of an older `HealthReport` fails at the end of input, and an older
    reader can ignore trailing bytes only when the struct is last in its frame. So appending fields to `HealthReport` and
    `SystemFacts` is a protocol change, not an append the protocol rule allows.
  - The bump goes with the golden update, and the app's version-mismatch path (it already asks for an agent restart) is
    tested for it.
  - `HealthReport` and `SystemFacts` are not persisted: `HealthReport` exists only in `Reply::Health` (`protocol.rs:836`),
    built in `handlers.rs:631`. So there is no stored data to migrate. The app, agent and CLI ship in one installer.
- **UI.**
  - `CouldNotRead` entries are listed under the hero as "Not checked: …".
  - `NotReadHere` entries are shown once as coverage, in smaller text under the reasons ("Dyno does not check pending
    updates on macOS"), not as a problem.
  - When `reasons` is empty, the hero says "Nothing found in what Dyno could check" if any entry is `CouldNotRead`.
    Otherwise it says "Nothing needs your attention", with the coverage line under it.
  - The score is unchanged, so a gap is said, not guessed.
- **Tests (failing first).**
  - `assess` over `SystemFacts::unknown(..)` returns the three scored facts in `not_checked` as `CouldNotRead`. Over facts
    that declare pending updates in `not_read_here`, it returns that fact as `NotReadHere`. Both fail today: the fields do
    not exist.
  - A spec of the hero with an empty `reasons` and a non-empty `not_checked`.
  - The mock backend's Health answer gains a `notChecked` row.
- **WP-1.** After WP-1 and WP-H alone, Windows shows "Dyno does not check disk health, restart needed or pending updates
  on this system yet" as coverage, from `NotReadHere`, until WP-3 lands.
  - **Amendment 2026-11-09 (approved: coordinator), after the independent review of WP-H.** That coverage-only reading kept
    "100, Good, Nothing needs your attention" on Windows, the symptom this section removes. A reader that is missing is not
    a design choice, so a third kind `NotReadYet` is added: the interim adapter (`facts_not_read`) declares every fact in
    `SystemFacts::not_read_yet`, and `assess` reports it as `NotReadYet`. The hero treats `NotReadYet` like `CouldNotRead`:
    the qualified headline and the "Not checked" list. Entries with the same kind and reason are merged into one (`what`
    is a list). Startup items that cannot be listed are a `not_checked` entry too (`NotReadYet` for an unsupported
    read, otherwise `CouldNotRead`), not a silent absence. `NotReadHere` stays for what Dyno never reads by design.

### 5. Tests (failing first, ER-11)

| Test | Where | Fails today because |
| --- | --- | --- |
| The no-fake test (§2), per OS | `windows/mod.rs`, `macos/mod.rs`, `linux/mod.rs` | Windows: `locks`, `startup` and `facts` are `dyno_platform::fake::Fake` |
| The declaration check (§2) and its fixtures | `dyno-platform/tests/no_ungated_doubles.rs` | `fake`, `FakeCounters`, `FakeThrottle`, `FakeWear`, `FakeBattery`, `FakeWakeTimer` and `dyno_core::wol::fake` are ungated |
| The shipped-feature check, its positive control and the derived `SHIPPED` (§3.1) | `scripts/check-shipped.test.mjs` | `dyno-helper` is missing from `SHIPPED`; the feature does not exist, so the positive control reports `check broken` |
| `assess` fills `not_checked`; the hero spec (§4.5) | `dyno-core/src/health.rs`, `health-hero.spec.ts` | the field and the line do not exist |
| `assemble` over fake registry, folder, task and package shapes, on every OS (case list below) | `parse/win_startup.rs` | the module does not exist |
| `is_os_owned` over the new ids (§4.1) | `dyno-core/src/startup.rs` | the id formats do not exist |
| `win_os_name` cases: Windows 10; Windows 11 with "Windows 10" in `ProductName`; a server; no `DisplayVersion`; no `UBR` | `parse/win_facts.rs` | the function does not exist |
| Restart Manager mapping from `RM_PROCESS_INFO`-shaped values to `Locker`; the retry on `ERROR_MORE_DATA`; the folder and missing-path answers | `windows/locks.rs` (the pure part on every OS) | the module does not exist |
| On the Windows runner only, the readers against temporary state the test creates (hygiene below) | `windows/locks.rs`, `windows/startup.rs` | the modules do not exist |
| `StartupLocation` postcard: the new variants decode as appended | `dyno-core` protocol golden | the variants do not exist |

The `assemble` cases:

- **Run keys:** an enabled HKCU Run value; a value turned off by StartupApproved (the bytes WP-0 measured); an HKLM
  32-bit (`WOW6432Node`) entry governed by `Run32`, not by `Run`; the same name in the 64-bit view with a different state;
  HKCU 64-bit and 32-bit duplicates merged; RunOnce always enabled.
- **Values:** `REG_EXPAND_SZ` expanded through a fake environment; a non-string value skipped and counted; a value over
  `MAX_VALUE_BYTES` skipped and counted.
- **Folders:** a `.lnk` in the user folder resolved, and one turned off by `StartupFolder`; a `.lnk` with no path;
  `desktop.ini` skipped.
- **Tasks:** a logon task enabled, one turned off, a boot-only task left out, a logon trigger for another user left out,
  and a denied folder counted into one sentence.
- **Packaged apps:** a packaged task enabled, one turned off by the user, one turned off by policy.
- **Failures and bounds:** one failing source gives one sentence while the others still list; every source failing gives
  an error; every cap holds.

**Windows-runner test hygiene (ER-14, review P2-8).** Nothing the tests create may run, persist or touch the real user's
entries:

- Every object the tests create carries the prefix `DynoTest-<uuid>`, and each test first sweeps leftovers with that
  prefix. Cleanup is an RAII guard that also runs on panic.
- The startup reader runs over a temporary key `HKCU\Software\DynoTest-<uuid>\…` and a temporary folder, through the
  `RegistryView` and `FolderList` traits pointed at them. The real `Run` key is never written.
- The task test registers its task **disabled**, with an action pointing at a path that does not exist in the test's
  temporary folder, in the root folder under the prefix. WP-0 checks whether a standard user may register a task with a
  logon trigger at all, and whether it may create a task folder; if so, the task goes in its own folder (re-review
  P2-H). If a standard user may not register a logon task, the test runs only where the runner account is an
  administrator, and it says so when skipped. The mapping tests over fake task shapes still run everywhere.
- The locks test's child process holds a temporary file. It is started and ended through `paddock_proc` (rule 53) and
  cannot outlive the test.

**The VM row (local only; family ADR 0040).** A `startup-facts-locks` row is added to `scripts/vm-smoke/win-rows.mjs` on
the harness's home branch `test/dyno-vm-harness-1.1-win`, by its owner or through it (family ADR 0042). It runs on the
merged harness:

1. Seed over the harness's admin SSH session, then verify every seed with `reg query`, `schtasks /query`, `dir` and
   `Get-AppxPackage` before any assertion (ER-33: the precondition is checked). The seeds:
   - an HKCU `Run` value;
   - an HKCU `Run` value marked off in `StartupApproved\Run`;
   - an HKLM `Run` value in the 32-bit view;
   - a `.lnk` in the user Startup folder;
   - a logon task (`schtasks /create /sc onlogon`);
   - a packaged app with a startup task, if the image has one (Windows Terminal declares one).
2. **Positive control.** Dyno's own agent entry (HKCU `Run`, `dyno-agent.exe`) must be listed. If it is not, the row
   reports `check broken`, not "fail" (ER-33).
3. Assert through the app (`startupEntries` over CDP, as `startup.mjs` does in the investigation) and on the Health page's
   text. Each seed is listed with its location label and its state (the turned-off one as off). `unavailable()` is empty
   or names only denied task folders. The coverage sentence is shown.
4. Facts: the OS name in the agent's facts equals `Get-CimInstance Win32_OperatingSystem` `Caption` and `BuildNumber`
   (mapped), and is never "Fake OS". The Health card shows "Not checked: pending updates".
5. Locks: a PowerShell job in the guest holds a file open, and Dyno's "What is locking this file?" lists `powershell`.
6. The VM snapshot is reverted afterwards. The seeds never reach the host (ER-14).

## Work packages

The days are agent-days for one agent. WP-1 and WP-H can land on their own: together they remove the fakes and make the
Windows gaps and the Health card say so honestly, ahead of the readers. WP-2 runs beside WP-0, so it does not lengthen the critical path.

| WP | Package | Depends on | Days |
| --- | --- | --- | --- |
| WP-0 | VM measurements (local) and source reading: the StartupApproved keys and bytes per source kind (Task Manager toggles, key diff); the packaged-app `SystemAppData` keys on a fresh image before any toggle, compared with Task Manager's list, then the `State` values after toggles; `MSFT_PhysicalDisk` as a standard user; Restart Manager's view of other accounts as a standard user; hidden and denied task folders as a standard user; whether a standard user may register a task with a logon trigger at all, and create a task folder; the power overlay GUIDs; `BackgroundAccessApplications` on 23H2+; the Task Scheduler walk's time and count on a fresh Windows 11. The remaining Microsoft Learn pages are fetched and dated in Sources. | – | 1 |
| WP-1 | The mechanism (depends on WP-2). Tests first: the no-fake tests, the declaration check, and the `check-shipped` changes (derived `SHIPPED` with `dyno-helper`, the feature assertion, the positive control). Then: `adapter()` and `Platform::adapters()`; the `test-support` gates in `dyno-platform` and `dyno-core`; dev-dependency features in `dyno-agent`, `dyno-cli`, `dyno-helper` and the app (after WP-2); `NotReadHere` in place of `Unsupported` and `FakeBattery`; Windows `platform()` naming every port, with `NotReadHere` for the three gaps; the `Platform wired` log line; the doctest moved; `approved_value` under `cfg(test)`; the Linux `rustChecks` line with its cost measured; `cargo xwin check` clean; the rule 54 installer dispatch. | – | 2–2.5 |
| WP-1b | Shared CI contract: the `hostRustChecks` field in `app-gates` and its README, Dyno's entry, a run on the Windows and macOS legs with the cost recorded (integrating agent: `.gitea/`) | WP-1 | 0.5–1 |
| WP-2 | Family: the `test-support` gate in `paddock-power` with its self dev-dependency for `tests/it`, the "inert to build" doc sentence, the `MemorySecrets` doc line and the CHANGELOG entries (integrating agent; D2). It lands **before or with WP-1**, because `dyno-platform/test-support` enables `paddock-power/test-support` (re-review P2-H). | – | 0.5–1 |
| WP-H | Health `not_checked` with its two kinds: the engine fields (`HealthReport.not_checked`, `SystemFacts.not_read_here`) and the failing tests, the `PROTOCOL_VERSION` bump with its golden and the mismatch-path test, the hero lines and spec, the mock, `api.types.ts` | WP-1 | 1.5–2 |
| WP-3 | Windows facts: `win_os_name` and its tests first, then the registry, power (overlay included), WMI and folder reads, with per-fact logging | WP-0, WP-1 | 1.5–2 |
| WP-4 | Startup from the registry and folders: `assemble` and its tests first; the `RegistryView` and `FolderList` adapters with bounds; StartupApproved; the new location variants appended on the wire (D3: `dyno-core`, `api.types.ts`, `labels.ts`, the mock, the filter, `is_os_owned` tests); the coverage sentence; `signerRead` and the disabled "signed apps" filter (D8); per-source logging; the temporary-key Windows test | WP-0, WP-1 | 2.5–3 |
| WP-5 | Startup from logon tasks: the `LogonTasks` trait, the COM walk with caps, the user filter and the denied-folder count, the pure mapping tests, the Windows-runner test with the hygiene of §5 | WP-4 | 1.5–2 |
| WP-5b | Startup from packaged apps: the `PackagedStartup` trait, the `SystemAppData` read, the state mapping WP-0 measured, the package-manifest enumeration as the base when WP-0 finds untoggled tasks without a key (`windows` features `Management_Deployment`, `ApplicationModel`), its `assemble` cases (D7) | WP-0, WP-4 | 1.5–2 |
| WP-6 | File locks through Restart Manager: the mapping and retry tests first, the session guard, the Windows-runner test on its own temporary file | WP-0, WP-1 | 1–1.5 |
| WP-7 | The VM row on the harness branch, run on the merged harness; screenshots of the Health page and the startup card, independently reviewed (ER-23) | WP-3 to WP-6, WP-H | 1–1.5 |
| WP-8 | Docs: `architecture.md`, `features.md` (Windows startup, facts and locks built; process details and signer not yet), the `windows/mod.rs` module doc, the user guide (coverage list, hidden tasks, what is not read), CHANGELOG 1.1.1, a lessons entry (the pattern and the checks), `wiring.toml` edges, and `docs/known-copies.toml` if the COM helper is shared | all | 0.5–1 |
| CI | Windows-leg round trips: there is no local Windows host apart from the VM, and each Windows CI iteration is long | – | 1 |

**Total: 16 to 21.5 agent-days**, the sum of the table's rows (revision 1 had 10 to 12.5).

Revision 2's rows summed to 15 to 20.5, not the 16 to 21.5 it printed (re-review P2-F). Revision 3 adds two half-days:
WP-H (two kinds and the planned protocol bump) and WP-5b (the manifest base). The sum is now:

- minimum: 1 + 2 + 0.5 + 0.5 + 1.5 + 1.5 + 2.5 + 1.5 + 1.5 + 1 + 1 + 0.5 + 1 = 16;
- maximum: 1 + 2.5 + 1 + 1 + 2 + 2 + 3 + 2 + 2 + 1.5 + 1.5 + 1 + 1 = 21.5.

The increase over revision 1 comes from:

- the packaged-app source;
- Health `not_checked`;
- the CI contract field;
- the real D2 adoption;
- the declaration check;
- the broader WP-0;
- an explicit allowance for Windows CI round trips.

The critical path is WP-0 (with WP-2 beside it) → WP-1 → WP-4 → WP-5 → WP-7: 1 + 2 + 2.5 + 1.5 + 1 = 8 to
1 + 2.5 + 3 + 2 + 1.5 = 10 days. The riskiest rows are WP-5b (WinRT package enumeration) and WP-1b (a shared action change
measured on two host legs). WP-1b, WP-2, WP-H, WP-3, WP-5b and WP-6 run
beside it. Deferring the signer reader (D8) avoids about 1 to 1.5 days of FFI work in this release. If the user wants it
in 1.1.1, it is a shared package with family ADR 0051 and its own estimate.

### Parallelism and cost (ER-20)

- **Startup.**
  - The registry reads (about ten) and two folder listings take milliseconds. The Task Scheduler walk and the
    packaged-app lookup are the slow parts; WP-0 measures them.
  - The sources run one after another on the agent's startup job thread, which is never the UI thread. A parallel COM walk
    would need one apartment per thread for a gain of a few hundred milliseconds at most, so it is not done.
  - The list is cached until `refresh()`.
- **Facts.** They run every few minutes, off the sampling path. The WMI disk query is the slowest read. If WP-0 measures it
  over 250 ms, disk health is kept for 30 minutes, the way `smart_reader` caches.
- **Locks.** One Restart Manager session per question, bounded by `MAX_HOLDERS` and three retries.
- **CI.** The non-test `cargo check` adds a rebuild of the Tauri and shell crates without their test features on each leg
  (re-review P2-C). It runs on every branch run (D10), and WP-1 and WP-1b measure and report its cost.

### WP-0 results (measured 2026-11-09, partial)

Local VM only (ER-14): `paddock-win11-test` (VMware Fusion, Windows 11 Pro Arm64, `DisplayVersion` 26H2, build 26300.9711, stable channel),
reverted to its `gui-session-tools` snapshot before and after, started headless. Callers: `paddock-test` (standard user) and `paddock-admin` over
SSH, so every reading below is from a **network logon in session 0**; no interactive token was used. Probes: `wp0-collect.ps1` (one PowerShell
script, a positive control per step; a control that said `BROKEN` is reported as such). Raw outputs are in the coordinator's scratch folder
(`wp0/out/`), not in the repository. Two steps had a broken control on the first run and were redone (the Restart Manager holder died with its SSH
session; the first quoting mangled the path); only the corrected runs are reported.

| # | Question | Result |
| --- | --- | --- |
| 1 | StartupApproved bytes per source kind | **Not measured**: the toggles need Task Manager (a GUI session), which this run did not have. Read only: HKLM `StartupApproved\Run` has `SecurityHealth` = `04 00 00 00 00 00 00 00 00 00 00 00` (first byte even = enabled by §4.1's rule, here `0x04`, not `0x02`/`0x06`); HKCU `StartupApproved\{Run,Run32,StartupFolder}` and HKLM `Run32`, `StartupFolder` do not exist on this image. So a missing key or value must be read as Enabled, and `0x04` must parse as enabled (a test input for `approved_state`). |
| 2 | Packaged-app `SystemAppData` keys on a fresh image | The task keys exist **without any toggle**: `…\SystemAppData\<PackageFamilyName>\<TaskId>` for `MSTeams_8wekyb3d8bbwe\TeamsTfwStartupTask` (`State=2`), `Microsoft.WindowsTerminal_8wekyb3d8bbwe\StartTerminalOnLoginTask` (`State=0`) and `Microsoft.MicrosoftOfficeHub_8wekyb3d8bbwe\WebViewHostStartupId` (`State=2`). The manifests of the 122 installed packages (read from `AppxManifest.xml`) declare exactly these three `StartupTask` entries, with `Enabled` = true, false, true, which matches `State` 2, 0, 2 (the `StartupTaskState` values: 0 disabled, 1 disabled by user, 2 enabled, 3 disabled by policy, 4 enabled by policy). Other subkeys (`PSR`, `Schemas`, `SplashScreen`, `HAM`) carry no `State` and are not tasks. **Not compared with Task Manager's Startup apps list** (no GUI). Consequence: on this image the registry alone lists every task; the manifest enumeration (WP-5b base) is still the safer source for a machine whose tasks have never been touched, and is not proven necessary. |
| 3 | `State` after toggles (user, policy) | **Not measured** (no Task Manager; no policy set). Only the three states seen above (0, 2) are measured. |
| 4 | `MSFT_PhysicalDisk` as a standard user | **Access denied** (`Get-CimInstance`, network logon). As `paddock-admin`: one row, `HealthStatus=0` (healthy), `OperationalStatus=2`. So the disks fact **needs the helper** for a standard user (at least in a non-interactive logon); with no helper it is `unavailable("needs the helper")`, as §4.2 plans. |
| 5 | Restart Manager as a standard user | **`RmGetList` returns `ERROR_ACCESS_DENIED` (5) for a standard user on every path tried, including a file that user's own process holds open** (network logon). As `paddock-admin` the same call works: `ERROR_MORE_DATA` (234) with the needed count, then the holder (`Windows PowerShell`, `ApplicationType` 5 = `RmConsole`, session 0), for a file held by either account. So WP-6 cannot assume a standard user can list holders; **measure again in an interactive session (the `PaddockRunner` task) before WP-6 relies on it**, and plan the answer "needs administrator rights" (helper) for the denied case. A folder path also answered 5 for the standard user (the folder case could not be told apart). |
| 6 | Hidden and denied task folders as a standard user | The walk (`GetFolders(0)`, `GetTasks(TASK_ENUM_HIDDEN)`, recursive) raised **no error** for the standard user and still saw fewer tasks: **176 tasks in 141 folders (33 hidden) as `paddock-test` against 252 in 142 folders (56 hidden) as `paddock-admin`**. So there is no denied count to report, as the ADR expected: the 76 missing tasks are filtered silently, and the user guide must say standard users do not see all tasks. |
| 7 | May a standard user register a logon task or create a task folder | Create a task folder in the root: **allowed**. Register a task with a logon trigger: **denied** (`0x80070005`). So the Windows-runner test registers its task only where the runner account is an administrator, and skips with a message otherwise (§5 hygiene). The cleanup check afterwards (`schtasks /query`, as admin) found no `DynoTest-` task or folder. (The step's own control used `Get-ScheduledTask`, which also answers access denied for a standard user, so it is not a valid control; use `schtasks /query` as admin.) |
| 8 | Power overlay GUIDs, effective mode | `PowerGetEffectiveOverlayScheme` after `powercfg /overlaysetactive`: `OVERLAY_SCHEME_MIN` (Best power efficiency) `961cc777-2547-4f9d-8174-7d86181b8a7a`; `OVERLAY_SCHEME_HIGH` (Better performance) `3af9b8d9-7c97-431d-ad78-34a8bfea439f`; `OVERLAY_SCHEME_MAX` (Best performance) `ded574b5-45a0-4f42-8737-46345c09c238`; `OVERLAY_SCHEME_NONE` (Balanced) all zeros. The call needs no rights (standard user, `rc=0`); actual and effective were equal in every case. "Power saver" for the fact is the Power saver scheme **or** `961cc777-…`. |
| 9 | `BackgroundAccessApplications` on 23H2+ | The key exists with a value `Migrated=4` and one subkey per package (44 here), but **no subkey has `Disabled` or `DisabledByUser`** on this image. So the per-app flags are absent when nothing was set; whether Settings writes them on 26H2 was not measured. The fact stays unavailable ("Windows no longer keeps one switch for this") unless a toggle in Settings is shown to write them. |
| 10 | Task Scheduler walk time and count | Standard user: **34.5 s** for the first walk (176 tasks, cold, emulated Arm); administrator: 4.8 s (252 tasks) a minute later. A single cold sample each, so an order of magnitude only. A standard-user first read of tens of seconds is why the startup list is cached until `refresh()` and the tasks source reads last and reports on its own. |
| 11 | WMI disk query time | Administrator, 10 runs: first 2,395 ms (cold), then 20 to 33 ms, median 28 ms. Over the 250 ms rule only on the first call, so the 30-minute cache is not needed; the first call must not run on the sampling path. (Standard user: denied, see 4.) |

Also seen: the registry `ProductName` says `Windows 10 Pro` on this build-26300 Windows 11, and `Win32_OperatingSystem.Caption` says `Microsoft Windows 11
Pro`, as §4.2 expects of `win_os_name()`. What `sysinfo::long_os_version()` (the interim `NotReadHere` facts) prints here was **not measured**.

**Not measured, and why:** the Task Manager toggles of items 1 and 3 and the Startup apps comparison of item 2 (no interactive session driven in
this run); Restart Manager and the disk query from an interactive token (item 5); the sysinfo OS name. The remaining Microsoft Learn pages were
not fetched in this run.

### Windows red and green run of the no-fake test (2026-11-09)

On the same VM, the same snapshot, a standard user over SSH; both binaries were cross-built with `cargo xwin test --no-run` (x86-64, run under
emulation). Red: the tip with only `windows::platform()` put back to the old `..fake.platform()` spread (the new test and wiring kept): `windows::tests::the_windows_platform_wires_no_fake` fails with
`the Windows platform wires test doubles: ["locks=dyno_platform::fake::Fake", "startup=dyno_platform::fake::Fake", "facts=dyno_platform::fake::Fake"]`.
Green: the tip (`008969752`): the same test passes. Two other tests of `windows::` fail in **both** runs and are environmental, not this change:
`windows::counters::…::the_real_catalog_lists_memory_and_a_real_counter_reads_back_after_two_collections` (PDH status `0xc0000bc8`) and
`windows::pdh::tests::reads_counters_on_this_machine` ("This OS reports no disk busy time"); a network logon has no performance counter objects.

## Decisions (accepted by the user, 2026-11-08)

- **D1. The checks.** Recommended:
  - the `cfg` gate as the guarantee;
  - the `check-shipped` feature-graph check over the derived shipped list, with a positive control;
  - a non-test `cargo check` of `dyno-platform`, `dyno-agent`, `dyno-app` and `dyno-helper` on all three legs. Windows and
    macOS need the new shared `hostRustChecks` field.
  - an installer build before WP-1 and WP-2 land (rule 54);
  - the no-fake test and the declaration check as the readable failures.

  Alternatives:
  - (b) `cargo xwin check` on the Linux leg, if the CI image can carry xwin;
  - (c) no host-leg check, with nightly detection accepted as a residual risk.
- **D2a. Gate `paddock_power::{FakePower, FakeEvents}` in the same change.** Recommended: yes. They are pure doubles, and
  adopting the gate takes the self dev-dependency plus Dyno's lines.
- **D2b. `MemorySecrets`.** Recommended: it stays public, documented as an ephemeral store, because the workbench demo uses
  it in production. The alternative is a new `EphemeralSecrets` for the demo and gating `MemorySecrets`: more change, for
  no gain to Dyno.
- **D3. New `StartupLocation` variants.** Recommended: `UserRunOnceKey` ("Registry, next sign-in only (this user)"),
  `MachineRunOnceKey` and `PackagedApp` ("Store app").
  - They are **appended after `LoginItem`** (`protocol.rs:1-5`: append only), mirrored in `api.types.ts`, `labels.ts`,
    the mock and the filter.
  - An older app or CLI that receives a new variant cannot decode the whole startup reply. The app, agent and CLI ship in
    one installer, so this happens only if an old CLI talks to a new agent. The CLI then shows its usual "could not read
    that reply" with the version hint. No fallback is built.
- **D4. The facts in 1.1.1.** Recommended: all rows of §4.2, with pending updates and boot time honestly unavailable and
  shown as not checked (§4.5).
- **D5. Boot-trigger tasks and services.** Recommended: not listed in 1.1.1, and named in the coverage sentence.
- **D6. Windows process details (`NoDetails`).** Recommended: out of scope. It is an honest, documented gap.
- **D7. Packaged-app startup tasks.** Recommended: read as a ninth source (WP-5b). The alternative is a permanent
  coverage sentence, "Store apps that start at sign-in are not listed yet".
- **D8. The signer of startup entries.** Recommended: not read in 1.1.1. The engine's `signerRead: false` disables the
  "Hide signed apps" filter on Windows, with a sentence. A later shared reader goes with family ADR 0051's `WinVerifyTrust` check, and also serves
  Windows process details.
- **D9. Health's "not checked" line (§4.5).** Recommended: in 1.1.1, on every OS, because it is the symptom that triggered
  this record. Deferring it would leave "100, Good, Nothing needs your attention" on any machine where a scored fact cannot
  be read.
- **D10. Where the non-test host check runs (re-review P2-C).** Recommended: keep it on the branch legs (Windows and
  macOS) and measure first (WP-1, WP-1b). Then decide with the numbers, which are recorded in `docs/performance.md`.
  The alternatives, to choose only after the measurement:
  - (b) check only `dyno-platform`, `dyno-agent` and `dyno-helper` on the host legs. These do not pull the Tauri dev
    features. The residual risk is that a Windows-only or macOS-only fake reference in `dyno-app` is found only by the
    rule 54 dispatch and the nightly.
  - (c) move the whole check to the nightly. That is D1(c), the residual risk accepted.

  **Decided (user, 2026-11-08):** the check runs on every Dyno branch run. WP-1 and WP-1b measure its cost per leg and
  report it (rule 48), and the user is asked again only if the cost is large.

## Consequences

- A Windows user sees:
  - their real startup entries, Store apps included, with a stated coverage;
  - a Health card that names what it could not check;
  - real answers to "what is locking this file".

  A source Dyno cannot read is a sentence, not an empty list or a silent all-clear.
- A new port, double or OS without a real reader cannot ship as a fake:
  - the gate stops a build without test targets from naming it;
  - the declaration check stops it from being declared ungated;
  - the non-test check on every leg finds a reference on the branch.
- Test-only code moves behind a feature. Every test crate's `[dev-dependencies]` names `test-support` once. A test that
  forgets it fails to compile with "could not find `fake` in `dyno_platform`", which is quick to fix.
- The agent's log names every adapter at start, so a support bundle shows what is wired.
- **Follow-up (review P2-4, not in 1.1.1):** family doubles outside `paddock-power` are still public in Dyno's graph
  (`paddock_jobs::FakeClock`, `paddock_service::fake_systemd::FakeSystemd`, `FakePort`, `paddock_net::FakeWire`, `paddock_health`'s
  `FakeToaster`). Dyno uses them only under `cfg(test)`; gate them the D2 way, or extend the declaration scan to flag a non-test `use` of
  a family `Fake*` path.
- The macOS and Linux wiring is unchanged apart from the `NotReadHere` rename and the new test. macOS gains the Health "not
  checked" line for pending updates.
- Cost: 16 to 21.5 agent-days in Dyno 1.1.1, one shared CI contract field, and one extra `cargo check` per leg (measured).

## Sources

- **Fetched 2026-11-08 (Microsoft Learn):**
  - "RmGetList function": `ERROR_MORE_DATA` with `pnProcInfoNeeded`; `ERROR_ACCESS_DENIED` when "a path registered to
    the Restart Manager session is a directory". The page says nothing about other accounts' processes, so WP-0 measures
    that.
  - "ITaskFolder::GetTasks": `TASK_ENUM_HIDDEN` returns hidden tasks too; the method returns an `HRESULT`. The page says
    nothing about rights or about what is filtered, so WP-0 measures that.
- **Not yet fetched (WP-0 fetches and dates each one before the code relies on it):**
  - "Run and RunOnce Registry Keys";
  - "Registry Keys Affected by WOW64" and "Accessing an Alternate Registry View";
  - "KNOWNFOLDERID";
  - `ITaskService`, `TASK_TRIGGER_TYPE2`, `ILogonTrigger::UserId`, `IExecAction`;
  - `RM_PROCESS_INFO`, `RM_APP_TYPE`;
  - `RtlGetVersion`;
  - `MSFT_PhysicalDisk`;
  - `PowerGetActiveScheme`, `PowerGetEffectiveOverlayScheme`;
  - the MSIX `uap5:StartupTask` extension.
- **Measured, not sourced:**
  - StartupApproved values: the format is in `parse/startup_approved.rs` (Dyno's own, tested); the per-source key mapping
    is measured in WP-0.
  - The packaged-app `SystemAppData` `State` layout is not documented by Microsoft; WP-0 measures it.
