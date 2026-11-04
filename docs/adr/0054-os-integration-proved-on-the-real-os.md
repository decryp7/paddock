# 54. OS integration proved on the real OS: read-back registrations, a lifecycle suite, no discarded errors at the edges, fakes under contract, recorded red runs and layout matrices

- Status: Proposed (2026-11-04). Written on the user's go-ahead of 2026-11-04 (ER-21 / rule 40 redesign after the Dyno
  1.1.0 work of that day). Docs only: nothing here is built yet. Patterns 1 and 2 are meant for Dyno 1.1.0 (the user's
  direction); patterns 3 to 5 apply family-wide after that release.
- Date: 2026-11-04
- Related: ER-11 / rule 5 (a bug fix starts with a failing test), ER-14 / rule 7 (tests never touch the user's machine;
  VMs only), ER-21 / rule 40, ER-22 / rules 42 and 43 (share first; shared behaviour tested once), ER-27 (logs explain every
  failure; rule 56 on its branch), rule 24 (services survive updates), rule 54 (the release path builds installers; (c)
  proposed by ADR 0040), rule 55 / ADR 0034 (one implementation per mechanism). ADR 0021 (portable tests, OS-aware paths in
  `paddock-testkit`), ADR 0022 (checks in `./dev check`, Rust), ADR 0024 (no polling), ADR 0031 (patience, not wall
  clocks), ADR 0037 (the app composition contract, its real-app smoke), ADR 0039 (release gates inspect the artifacts),
  ADR 0040 (the Windows GUI test session in `vm-smoke`), ADR 0042 (one owner per harness: `scripts/vm-smoke` lands from its
  home), ADR 0049 (`wire.toml`, the push check). **ADR 0052** on `origin/docs/adr-wiring-outcomes-ui` (accepted): its
  `wiring.toml`, the `// silent: <why>` hatch and caps, C87 (silent drops, family-wide, report-only first), `ReadBack`,
  `features.toml` with VM steps, and the shared layout checks `expectLayoutClean` (WP-C2). This ADR extends those and does
  not repeat them. **ADR 0053** on `origin/docs/dyno-adr-0027-per-machine` (proposed): its Windows per-machine VM suite
  (§9, PM-1 to PM-17, two accounts, UAC automation) is the base the lifecycle suite of §1.3 is built on, and its
  `UserRunEntry` is the first adapter of §1.1. The commit-lint trailer on `origin/ci/failing-first-trailer`
  (`263d49d0b`, `5c4358e64`, not on main): §4 extends it.
- Numbers (scanned with `git ls-tree -r --name-only origin/<branch> docs/adr` and `git grep` over every remote branch after
  `git fetch`, 2026-11-04):
  - **ADR 0054.** `origin/main` has up to 0049; 0052 is on `origin/docs/adr-wiring-outcomes-ui`, 0053 on
    `origin/docs/dyno-adr-0027-per-machine`; no branch has 0054.
  - **Checks C92 to C96.** The highest check named on any branch is C91 (ADR 0053).
  - **Rules 68 to 71.** Rules 56 to 59 are on branches, 60, 61 and 65 are proposed by ADRs 0041, 0042 and 0049, 64 by the
    0045 draft, 66 is on `origin/docs/rules-2026-11-04`, 67 is ADR 0052's. ADR 0052 notes that the drafts 0046 and 0048 still
    name rules 65 and 66 provisionally and must take new numbers when written out: **they take 72 and up**, since this ADR
    takes 68 to 71.
  - **ER-34 to ER-37.** ER-31 and ER-32 are on `engineering-rules` main; ER-33 is proposed by ADR 0052.

## Evidence

Read in git on 2026-11-04 at the commits named, and in the VM result files of that day. "Verified" means read in the code or
result file; "measured" means a `git grep` count whose command is given; "reported" means the coordinator's account, not
found in git.

### Pattern 1: OS integration verified only against fakes, never across the real OS lifecycle

| Instance | Source | How checked |
|---|---|---|
| The Windows helper service was installed at 08:48:53 (event 7045) and "terminated unexpectedly" at 08:49:14 (event 7034), 21 s later. After a real restart the service did not exist (`sc` error 1060), before and after Dyno started; the helper card said "Off" and Processes had no helper chip. The cause is not in git yet; the helper's own error went nowhere (pattern 2), which is why the run could not name it | `/Volumes/d7sst7/dev-targets/scratch/dyno-vm-checks/out/2026-11-04T08-59-25-186Z-dyno-win.md` steps 5 and 8 (installer from CI run 7521, `vm-smoke` at `f2698d9cc`) | verified (result file) |
| "Start at login" on Windows: the UI said on ("Launch at login after switching it on: true"); after the restart nothing started Dyno, the `Run` key had no Dyno value, and the tray had 0 icons. The UI showed the intended state, not the OS's | same file, step 9 | verified |
| "Start at login" on Linux (GNOME): switched on, after the reboot Dyno did not start | `.../2026-11-04T09-22-13-585Z-dyno-linux.md` step 6 (CI run 7528, `vm-smoke` at `6463a4843`) | verified |
| Linux "Turn off administrator readings" reported done; 75 s later `dyno-helper.service` was still `active` and `enabled`, through the packaged unit `/usr/lib/systemd/system/dyno-helper.service` and its `multi-user.target.wants` link (the turn-off removed only `/etc/systemd/system/dyno-helper.service`, which did not exist), and `/var/lib/dyno-helper` stayed | same file, step 15 | verified |
| The Windows tray tooltip's `NIM_MODIFY` lacked `NIF_GUID`, so the shell refused it and every icon kept its first tip; the unit tests passed because nothing asked the shell | fix `ae396138c`; `apps/dyno/vendor/PATCHES.md` (the `set_tooltip` patch); its Windows VM run recorded in `18106f8e7` | verified (commits) |
| Both login fixes landed together in `91d703d81` (`feat/dyno-launch-at-login-default`): `tauri-plugin-autostart` replaced by `paddock_service::login` writers that read their write back. The read-back is tested against a fake registry (`login.rs`, `fn registry(sys, drop_writes)`), not against Windows | `crates/paddock-service/src/login.rs` at `91d703d81` | verified |
| Earlier: macOS TCC denied self-responsible processes on the external work volume; only a real run on the runner showed it | lessons.md "TCC blocked self-responsible processes on the external work volume" (2026-11-04) | read |

ADR 0052 §1.4 requires a VM step per platform feature; it does not say what such a step must cover. Every instance above
passed its unit tests and most passed a first-run VM step; they failed at a restart, a turn-off, a modify after the add, or
21 s later.

### Pattern 2: errors swallowed at OS edges

| Instance | Source | How checked |
|---|---|---|
| `fn service_main(..) { let _ = serve_until_stopped(); }`: every error of the SCM registration, the status reports and the serve loop is dropped; `serve_until_stopped` keeps only `result.is_err()` of `serve()` as exit code 1 (`ServiceExitCode::Win32(1)`), so the event log says "terminated unexpectedly" with no cause | `apps/dyno/crates/dyno-helper/src/winsvc.rs:22` and `:53-54` at `afc0106b2` (`origin/dyno/wp-int`) | verified |
| `let _ = icon.set_tooltip(..)` in Dyno's tray: the refused modify above was invisible in the log ("app log 'tooltip sent' lines: 0" in the VM result) | `apps/dyno/app/src-tauri/src/tray.rs` before `ae396138c` (now `if let Err(e) = self.icon.set_tooltip(..)`, line 379 at `afc0106b2`) | verified |
| `unsafe { Shell_NotifyIconW(NIM_MODIFY, &nid) };` in `set_tray_visible`: the `BOOL` is still dropped | `apps/dyno/vendor/tray-icon/src/platform_impl/windows/mod.rs:84` at `afc0106b2` | verified |
| The autostart failure was silent: the plugin's `enable()` succeeded without anything readable at sign-in, and nothing was logged | `91d703d81`'s message; VM step 9 | verified |
| The helper log is deferred until its folders are verified (`fc140fc6f`, P1.2: "standard error until the data folder, the program, the configuration ... are verified"). A service's standard error goes nowhere on Windows, so a refusal at start was invisible | `fc140fc6f` | verified |
| Footprint in Dyno, non-test Rust at `afc0106b2`: `let _ = ` 79 in `dyno-platform/src`, 17 in `dyno-helper/src`, 44 in `app/src-tauri/src`, 128 in `dyno-agent/src`; `.ok();` 3, 0, 2 and 5 | `git grep -cE 'let _ = ' origin/dyno/wp-int -- <dir> ':!*tests*' ':!*test.rs'` | measured; not every site is an OS edge or a defect |
| Footprint family-wide, non-test Rust on `origin/main` (`4b6d99f57`): 1,816 `let _ = ` and 139 `.ok();` (most are `writeln!` into a `String` and other harmless drops) | `git grep -cE 'let _ = ' origin/main -- 'crates/*/src/*.rs' 'apps/*/src/*.rs' ':!*tests*' ':!*test.rs' ':!*/testing*'` | measured |

ADR 0052's C87 covers `let Ok(..) else`, `if let Ok` without `else` and `.ok()` feeding `?` or `unwrap_or*` everywhere,
report-only until its baseline is cut, and it adds Clippy `let_underscore_must_use` to the family preset. That preset lint
alone would fire on the 1,816 sites, most harmless, so it lands late and with a baseline. None of the instances above is a
`let Ok(..) else`; three are `let _ =` or a dropped FFI return, and two are a fatal error with nowhere to go. The family-wide
footprint says a blanket ban is mostly waivers; the instances say the OS edge is where a drop hides a user-visible failure.

### Pattern 3: test fakes drift from the real system

| Instance | Source | How checked |
|---|---|---|
| A fake trust port rooted at `/` was used by two staging tests that ran on Windows; the fix gated them on Unix instead of making the fake OS-native | `74803654f` ("gate two staging tests on unix; their fake trust port is rooted at /") | verified (commit) |
| The helper test rig re-decided the restriction table itself, a second implementation of auth (rule 55); fixed to call `dyno_helper::auth::authorize` and `auth::scope` | `deb7db7d6` | verified (commit) |
| Dyno's mock duplicated the engine's number-support strings; fixed by a golden the engine's own test writes (`number_support.rs:166-185`, `the_generated_golden_values_are_current`) and a mock spec that compares against it (`number-support-golden.spec.ts`) | at `afc0106b2` | verified |
| The login writers' read-back is proven against a fake registry whose `reg add` keeps what it is given (`login.rs`, `registry(sys, drop_writes)`), and the real Windows behaviour was not | `91d703d81`; pattern 1's step 9 | verified |

### Pattern 4: failing-first claims not backed by a run

| Instance | Source | How checked |
|---|---|---|
| `74e57f519` (WP-LNX) claimed red runs for two UI specs and `b8628bc21` for a Rust test; neither had been run. Corrected by `77bf0b5ae`, which ran them red at `b8628bc21` with the lines removed | `77bf0b5ae` | verified (commit) |
| `13bf96710` ("the climate reading is Bulk") claimed red against `Live`; not run. Corrected by `357b6704c`, red at `c19654a75` with `ReadClass::Live` set temporarily | `357b6704c` | verified (commit) |
| The trailer of `origin/ci/failing-first-trailer` (`263d49d0b`) requires `Failing-first: <test name> (<where the red run is recorded>)` on `fix:` commits only. Both claims above would have passed it: one was on a `feat:` commit, and the parenthesis is free text | `conformance/src/commit-lint.mjs` on that branch | verified |

### Pattern 5: shared UI layout fixed one platform at a time

| Instance | Source | How checked |
|---|---|---|
| The tray "label over value" row of the shared widget chooser took four rounds in one afternoon: `5670221aa` (13:36, "label-over-value follows what the tray draws"), `4d56051c3` (15:22, "menu bar rows stay on one line"), `c896717b9` (15:31) and `c927c9add` (15:37, "controls wrap as one group"), then `78b230db2` on `fix/dyno-tray-label-layout` (the container query measured the whole chooser, so with a preview column the rows used the wide layout). Each fixed the platform it was looked at on; the coordinator reports a Windows fix regressing the Mac row | commits; `ui/projects/paddock-ui/widgets/src/chooser.ts`, `apps/dyno/app/src/app/features/settings/menu-bar-items.ts` | verified (commits); regression order reported |
| The row's layout depends on a platform capability (what the tray can draw: the number-support golden's keys `mac` 18 px, `win16`, `win20`, `win24`), a container width and a variant (fixed or editable layout), and no test ran the combinations | `number-support-golden.spec.ts` at `afc0106b2` | verified |

## Context: the five patterns and their invariants

1. **Real OS lifecycle.** An OS registration (a service, a login entry, a tray icon, a scheduled task, an ACL, a pipe) is
   written once and lives through restarts, sign-ins of other users, updates and removal, which only the real OS performs.
   Each adapter is tested against a fake that keeps what it is given. *Invariant:* every registration is read back from the
   OS when it is made and whenever it is shown, the UI shows what the OS reports, and every registration has passed install,
   first run, restart, second user, update, turn-off and uninstall on a VM of each OS it supports before a release.
2. **Errors at the edges.** Code that calls the OS is where failures are most likely and least visible: a dropped `Result`,
   a dropped `BOOL`, a service with no console. *Invariant:* at an OS edge no error is dropped, and a service's fatal error
   reaches the OS's own log from its first line.
3. **Fakes under contract.** A fake is written to make a test pass and drifts as the real thing changes; a test double that
   decides (auth, formatting) is a copy. *Invariant:* every double passes the same contract suite as the real adapter, on
   each OS, and a double never decides what the real code decides.
4. **Recorded red runs.** A failing-first claim is a statement about a run. *Invariant:* every claim names a state that can
   be checked out again and the log of its red run.
5. **Layout matrices.** A shared component whose layout depends on a platform capability, a width and a variant is fixed on
   the platform someone looked at. *Invariant:* every combination runs the shared layout checks on every change.

## Decision

### 1. Write, then verify, on the real OS (pattern 1)

**1.1 The registration contract.** `crates/paddock-service` gets `os_reg`:

```rust
/// One thing an app registers with the OS. `apply` writes and reads back; the result is what the OS reports.
pub trait Registration: Send {
    type Desired;
    fn id(&self) -> &RegistrationId;               // the `wiring.toml` entry it implements (§1.2)
    fn observe(&mut self) -> Observed;              // reads the OS; never writes
    fn apply(&mut self, want: &Self::Desired) -> Result<Observed, RegError>;  // write, then observe
    fn remove(&mut self) -> Result<Observed, RegError>;                       // remove, then observe
}

#[must_use]
pub enum Observed {
    Present { detail: String },                      // as the OS reports it (start type, command, SDDL, ...)
    Absent,
    PendingApproval { place: String },              // macOS Login Items, Windows StartupApproved
    DisabledByUser { place: String },
    Failed { since: UtcInstant, code: Option<i64>, why: String },   // a service that stopped with an error
    Unknown { why: String },                         // the probe itself failed
}
```

- `apply` returns `Err(RegError::Mismatch { want, observed })` when the read-back disagrees, never `Ok`. `RegError` carries
  the OS error and the next step (ADR 0052's `NextStep`). A job that registers ends `Confirmed` only through ADR 0052's
  `read_back`, with `observe` as the port it reads.
- A **service's** `observe` reads the start type and the image path from the service manager (`QueryServiceConfigW`,
  `systemctl show -p UnitFileState,ActiveState,FragmentPath`, `launchctl print`), its state and last exit code, and whether
  the service answered its own status request (for Dyno's helper, `Info`). On Linux it names the unit file that is in
  force (`FragmentPath`), so a turn-off that removed `/etc/systemd/system/<unit>` while the packaged
  `/usr/lib/systemd/system/<unit>` stays enabled reads back `Present` and fails (the instance above). Waiting for a state
  change uses `NotifyServiceStatusChangeW` and the unit's D-Bus `PropertiesChanged`, bounded by ADR 0031's patience, never
  a poll (ADR 0024).
- A **login entry** reads back from the place the OS starts it from: HKCU `Run` plus `StartupApproved\Run` (Windows), the
  autostart `.desktop` file and its `Hidden`/`X-GNOME-Autostart-enabled` keys (Linux), `SMAppService.status` (macOS).
  `91d703d81`'s `UserRunEntry`, `XdgAutostart` and `AppServiceAutostart` move onto the trait; `Autostart::set` goes.
- A **tray icon** on Windows checks the `BOOL` of every `Shell_NotifyIconW` call and reads back with
  `Shell_NotifyIconGetRect` after `NIM_ADD` (an icon the shell does not know has no rectangle); on macOS
  `NSStatusItem.isVisible`; on Linux the StatusNotifierWatcher's `RegisteredStatusNotifierItems`. A desktop without a tray
  host (GNOME without the AppIndicator extension, the Linux VM's step 7) reads back `Absent` with that reason, which the
  app shows.
- An **ACL** reads back the owner and DACL by handle (`GetSecurityInfo`, compared as SDDL with
  `paddock_service::windows_acl::PROTECTED_FOLDER_SDDL`); on Unix the owner and mode by descriptor. A **pipe** or socket
  reads back its security descriptor or mode by handle after creation.
- **The UI shows `Observed`, never the switch's own value.** A Settings switch for a registration is bound to the last
  `Observed`; when it differs from the stored choice the row says so in words with the next step ("Windows has no start
  entry for Dyno. Turn on again, or see the log."). The helper card shows the service manager's state and last exit code.
  The diagnostics bundle lists every registration's `Observed` (no user content, ER-27).

**1.2 Every registration is declared.** `apps/<app>/wiring.toml` (ADR 0052's register) gains:

```toml
[[os_registration]]
id = "helper-service"
kind = "service"                 # service | login-entry | tray-icon | scheduled-task | acl | pipe | firewall-rule
os = ["windows", "linux", "macos"]
scope = "machine"                # machine | user
adapter = "dyno_helper::setup::HelperService"   # the type that implements Registration
lifecycle = "helper-service"     # the vm-smoke lifecycle row (§1.3)
leftovers = []                   # what may stay after uninstall, each with why
```

**1.3 The lifecycle suite in `scripts/vm-smoke`.** One reusable suite per OS, built on ADR 0053 §9's Windows set-up (the
GUI session of ADR 0040, two accounts `paddock-admin` and `paddock-test`, a signed-out account, UAC automation), the Linux
Tart GNOME image and the macOS Tart image. For each `[[os_registration]]` of the app and each OS it lists, the suite runs
these steps and judges three things at each: the OS state (a probe per kind: `sc qc`/`sc query`, `reg query`, `schtasks`,
`icacls`, `systemctl show`, the autostart folder, `launchctl print`, `sfltool dumpbtm` read-only in the VM), the app's
reported `Observed` (from the diagnostics command), and the declaration.

| Step | Passes when |
|---|---|
| L1 Install | the installer's registrations are present as declared; the app's `Observed` agrees with the probe |
| L2 First run | first-run defaults (Dyno: start at login on) are present by the probe and shown as such; turning each switch off and on changes the probe within the patience bound and the UI follows the probe |
| L3 Settle | a service is still running and answering after the settle point: one served request after its start, then the service manager's state unchanged at the next app status read (the Windows helper's 21 s crash) |
| L4 Restart | after a real reboot and sign-in: services running, login entries started the app once, tray icons present, nothing repaired by hand |
| L5 Second user | a second account signs in (Fast User Switching on Windows, a second GNOME session on Linux, a second macOS account): its per-user registrations are its own, machine ones are shared, neither user's change alters the other's |
| L6 Update | install version N+1 over N (the update path the app ships: the updater's hand-off, or the package manager): every registration survives or is re-registered once (rule 24) |
| L7 Turn off | every switch that removes a registration (the helper's Turn off, start at login off): the probe shows it gone, including packaged copies and their links; leftovers only as declared |
| L8 Uninstall | every registration and every per-user one of signed-in accounts gone; `leftovers` only; no process left |

ADR 0053's PM-1 to PM-17 become the Windows per-machine rows of the same suite (PM-10 is L4, PM-6 is L5 with an update,
PM-9 is L8), so there is one harness. Each step writes the probe's raw output and the app's `Observed` into the result
file, as today's results do. VMs stay local (ER-14, the 2026-10-31 decision); CI never starts one.

**1.4 Checks.**
- **C92 (OS registrations declared and covered)**, a Rust check in `./dev check` (ADR 0022) over each app's non-test Rust:
  a call that makes an OS registration (a list in the check: `windows_service::service_manager::ServiceManager::create_service`,
  `CreateServiceW`, `ChangeServiceConfig*`, `Shell_NotifyIconW`, `SetSecurityInfo`, `SetNamedSecurityInfoW`,
  `CreateNamedPipeW`, `RegSetValueExW`, `reg add`/`schtasks`/`systemctl enable`/`launchctl bootstrap`/`icacls` run through
  `paddock_proc`, `SMAppService`, writes under an `autostart` folder) must be inside an impl of `Registration` whose type
  is the `adapter` of a declared `[[os_registration]]`; every declared entry must name an existing type and a `lifecycle`
  row that `scripts/vm-smoke` defines for each OS it lists. It is a name-level check, as C83 is; the lifecycle run is the
  backstop.
- **Rule 54 (d) (proposed):** a tag of an app with any `[[os_registration]]` needs a lifecycle result with no FAIL for the
  tagged head on each OS the registrations list; `./dev release tag` (ADR 0039's tag gate) reads the result files. A row
  that cannot run in a VM (macOS Login Items approval, which needs a click in System Settings) is a MANUAL row with a
  dated waiver (§6).

### 2. Errors at OS edges are never discarded (pattern 2)

**2.1 OS-edge modules.** An OS-edge module is a Rust module that holds a `Registration` adapter (§1.1), calls an OS API
through FFI (`windows`, `windows-sys`, `libc`, `nix`, `objc2`, `core-foundation`), or runs an OS tool through `paddock_proc`.
Each app lists its edge modules in `wiring.toml` (`[edges] modules = [..]`), and C93 also finds unlisted ones by those
imports. Each edge module starts with
`#![deny(clippy::let_underscore_must_use, clippy::let_underscore_future, clippy::let_underscore_untyped)]`, so the lint ADR
0052 adds to the preset late applies to the edges at once.

**2.2 C93 (edges never discard)**, in `./dev check`, over every edge module's non-test code. It fails on:
- `let _ = <expr>` and `_ = <expr>` (any type: the untyped form hides a `Result` or a `BOOL` alike);
- `.ok();` as a statement, and `.ok()` whose value is not used;
- an expression statement whose value is an FFI call's return (`unsafe { Shell_NotifyIconW(..) };`), unless the function
  is on the check's list of calls that return nothing;
- `if let Ok(..)` or `if let Some(..)` on a fallible OS call without an `else` (C87's shape, enforced here without a
  baseline);
- a `define_windows_service!`, a `main` of a binary declared as a service, or a launchd or systemd entry point that does
  not go through `paddock_service::service_main` (§2.3).

The escape hatch is ADR 0052's `// silent: <why>` (counted against the app's `silent` cap with the trailer ADR 0052
defines), not a new one. A drop that is deliberate (closing a handle during clean-up after an error was already reported)
says why.

**2.3 A service's fatal error reaches the OS log.** `paddock_service::service_main(spec, |stop| -> Result<(), ServiceError>)`
is the one way to run a service or daemon body:
- it opens an **OS-log sink before anything else**: the Windows Application event log under the service's own event source
  (registered by the install plan of `paddock_service::render`, with the source in the `[[os_registration]]` entry), the
  systemd journal (stderr, with `StandardError=journal` in the rendered unit), the macOS unified log (`os_log` under the
  service's identifier). A file log that is deferred for trust reasons (`fc140fc6f`) joins later; every line before it,
  and every refusal, goes to the OS log;
- it reports start-up phases with durations, and on `Err` writes the error with its cause chain and next step to the OS log
  and the file log (when open), then stops with `ServiceExitCode::ServiceSpecific(code)` on Windows (so the service
  manager's event 7024 carries the code) and a distinct exit status on Unix, from one table of codes per service;
- it never panics through the service manager: a panic is caught at the boundary, logged with its location and turned into
  an exit code.

Dyno's `winsvc.rs` is the first user: `service_main` replaces `service_main`/`serve_until_stopped`, and the helper's Linux
and macOS entry points move with it.

**2.4 TypeScript edges.** The backend adapters of each app (the `invoke` wrappers behind the DI token) and `paddock-ui`'s
shell adapters get `@typescript-eslint/no-floating-promises`, `@typescript-eslint/no-misused-promises` and a family rule
`paddock/no-silent-catch` (a `catch` or `.catch` whose body neither rethrows, logs through the app's logger, nor returns a
typed error value fails, unless it carries `// silent: <why>`). The family preset lists the adapter globs.

### 3. Fakes under contract (pattern 3)

**3.1 Contract suites.** `paddock-testkit` gets `contract`: a port's crate writes its contract once, as generic test cases
over a factory, and instantiates it for every implementation:

```rust
// in the port's crate, under its `testing` feature
pub fn trust_port_contract() -> Contract<dyn TrustPort> { /* cases: rooted paths, reparse points, owners, missing paths ... */ }

// in each implementation's tests
paddock_testkit::contract_tests!(trust_port_contract(), fake = |root| FakeTrust::new(root));
paddock_testkit::contract_tests!(trust_port_contract(), real = |root| RealTrust::new(root), os = ["windows", "linux", "macos"]);
```

- Each case gets a `TestRoot`, an OS-native temporary root from `paddock-testkit` (ADR 0021): `C:\...` on Windows, a
  temporary directory elsewhere. A fake that hard-codes `/` fails its own contract on the Windows runner (the `74803654f`
  instance), instead of being gated off.
- The real adapter's instantiation runs on each OS runner it lists, for ports over files, processes and sockets in that
  root. For ports that change OS registration state (§1), the real instantiation runs inside the lifecycle suite's VM, never
  on a CI runner or a person's machine (ER-14), and the CI run uses only the fakes.

**3.2 A double records, replays or delegates; it never decides.** A fake may store what it is given and answer from that
store, or from fixtures; a decision the real code makes (authorisation, limits, formatting, number support, parsing) is
either called from the real code (as `deb7db7d6` made the rig call `dyno_helper::auth::authorize`) or comes from a golden
the real engine writes in its own test (as `number_support.rs` writes `number-support-golden.ts`). `paddock_commands::testing`
gets the golden writer used there (`write_golden_ts(name, value)` with the "golden is current" test), so every golden has
one shape.

**3.3 Registers and checks.**
- `apps/<app>/fakes.toml` and `crates/fakes.toml` list every double: its port, its contract (or `golden = "<file>"` for a TS
  mock that imitates an engine function), and its owner.
- **C94 (doubles under contract)** enumerates Rust impls of port traits (ADR 0052's `paddock_shell::Port` marker, plus the
  traits named in `fakes.toml`) whose type is under `#[cfg(test)]`, in a `testing` module, or named `Fake*`, `Mock*`,
  `Stub*`, `Null*`, and TypeScript classes in `mock/` folders that implement an app's backend token interface; each must be
  in the register with a `contract_tests!` instantiation (or a golden spec) that exists, and each contract must also be
  instantiated for the real adapter.
- **C95 (mock imitations have engine goldens)**: a TypeScript mock function marked `/** @imitates <crate>::<fn> */` (and
  every `fakes.toml` entry with `imitates`) has a spec that compares it with a generated golden, and the Rust side has the
  "golden is current" test. A mock that computes what the engine computes without the tag is found by a name heuristic
  (`mock<Name>` where the engine has `<name>`) and reported.

### 4. A failing-first claim names a run that can be found again (pattern 4)

This extends `origin/ci/failing-first-trailer`; it does not add a second trailer.

**4.1 The trailer.**

```
Failing-first: <test id> red at <sha>[+patch] (log: notes:<sha> | ci:<run>/<job>)
Failing-first: none (<reason: docs-only, build-only, revert, test-only>)
```

- `<test id>` is what the runner takes (`cargo nextest` filter, the spec file and title).
- `<sha>` is a commit at which the test exists and fails: usually the `test:` commit before the fix. `+patch` means "that
  commit with a patch applied" (the fix's lines removed, as `77bf0b5ae` and `357b6704c` did), and the patch is in the note.
- `log:` names where the red run is kept: a git note under `refs/notes/failing-first` on `<sha>` (the log's last 200
  lines, masked, no user content, and the patch for `+patch`), or a CI run and job.
- A claim found wrong later is corrected by a `Failing-first correction: <sha of the claiming commit>: <new trailer>` line
  in a later commit, which commit-lint checks with the same format.

**4.2 `./dev red <test id> [--patch <file>]`** runs the test at the current tree (with the patch applied in a scratch
worktree when given), requires it to fail, writes the note, and prints the trailer to paste. `./dev` pushes
`refs/notes/failing-first` with the branch; CI fetches it.

**4.3 Checks.**
- commit-lint (the branch's `lint()`): the format above on **every** commit that carries a `Failing-first:` trailer, and
  the trailer is still required on `fix:` commits; `feat:` commits that add tests may carry it and are then checked.
- `./dev check --pre-push` (ADR 0038): each `notes:<sha>` reference has its note, and `<sha>` is an ancestor of the
  commit, or reachable from the branch.
- nightly: `./dev red --verify-sample 5` checks out five claims of the last seven days at their `<sha>` (with the patch),
  runs the test and expects red, then runs it at the claiming commit and expects green; a mismatch is a job-summary FAIL
  naming the commit (ER-27). The sample is drawn with a printed seed (ER-29).

### 5. Shared layout runs every platform, width and variant (pattern 5)

**5.1 A platform difference is a capability value.** A shared component never detects the OS. What differs per platform
reaches it as a typed input (for the chooser: the tray's `NumberSupport` per OS and size, already generated from the
engine's golden), so every platform's layout can run in one browser.

**5.2 `defineLayoutMatrix`.** Each `paddock-ui` component with layout variants has a `<name>.layout.ts` beside it:

```ts
export default defineLayoutMatrix(WidgetChooser, {
  capabilities: { tray: [GOLDEN.mac, GOLDEN.win16, GOLDEN.win20, GOLDEN.win24] },
  widths: [320, 520, 900, 1280],            // container widths, including the chooser with its preview column
  variants: { layout: ['fixed', 'editable'], items: ['one', 'limit'] },
  data: ['default', 'long-names'],
});
```

The gallery generates one route per matrix row. `@decryp7/paddock-e2e` gets `runLayoutMatrix(page, matrix)`, which visits
every row and runs the shared layout checks: today `expectNoHorizontalScroll` and `expectNoSilentClipping` of
`ui/projects/paddock-e2e/src/layout.ts`, and `expectLayoutClean` once ADR 0052's WP-C2 lands. A change to a component runs
its whole matrix (rule 50 scope: the affected plan maps the component's files to its matrix), so a fix for one platform is
judged on all of them before review.

**5.3 C96 (layout matrices)** enumerates `paddock-ui` components whose styles have `@container` or `@media` rules, or whose
inputs take a string-literal union or a `*Capability`/`*Support` type, and fails when one has no `.layout.ts`, or when the
matrix omits a value of a union input or a key of the capability's golden.

### 6. Rules and engineering rules (proposed)

> 68. **OS registrations are proved on the real OS, across their lifecycle, and the UI shows what the OS reports.** Every
> service, login entry, tray icon, scheduled task, ACL, pipe or firewall rule an app registers goes through a
> `paddock_service::os_reg::Registration` adapter that reads back what it wrote and returns the observed state; Settings,
> status and logs show the observed state, never the intended one. Each registration is declared in
> `apps/<app>/wiring.toml` and has a row in the `vm-smoke` lifecycle suite (install, first run, settle, restart, second user,
> update, turn off, uninstall) for each OS it supports; a tag needs that suite without FAIL for the tagged head (rule 54
> (d)). Checked by C92 and the lifecycle suite. *Why:* on 2026-11-04 Dyno 1.1.0's helper service crashed 21 s after install
> and was gone after a restart, start at login registered nothing on Windows or Linux while the switch said on, a Linux
> turn-off left the service running, and a tray modify was refused, all with every test green against fakes.

> 69. **Errors at OS edges are never discarded.** In an OS-edge module (a registration adapter, a module that calls an OS
> API through FFI or runs an OS tool), no `let _ =`, `_ =`, `.ok();`, dropped FFI return or `if let Ok` without `else`: an
> error is returned, carried into the observed state, or logged with its cause and next step, or the line says
> `// silent: <why>` (counted and capped, ADR 0052). A service or daemon runs through `paddock_service::service_main`, which
> writes its fatal error with the cause chain to the OS log (Windows event log, the journal, the unified log) from its first
> line and exits with a code that names it. Checked by C93, Clippy's `let_underscore_*` lints in edge modules, and ESLint
> `no-floating-promises` and `paddock/no-silent-catch` in backend adapters. *Why:* the Windows helper dropped its serve
> error and the event log said only "terminated unexpectedly"; a refused tray modify and a silent autostart left no line in
> the log.

> 70. **A test double passes the same contract as the real adapter, and never decides.** Every fake or mock of a port is
> registered in `fakes.toml` and passes the port's contract suite (`paddock_testkit::contract_tests!`), which also runs
> against the real adapter on each OS it supports (CI for file, process and socket ports in an OS-native temporary root;
> the VM for registration ports, ER-14). A double stores, replays or delegates: authorisation, limits, formatting and other
> decisions come from the real code or from a golden the real engine writes. Checked by C94 and C95. *Why:* a fake rooted
> at `/` ran on Windows, a test rig re-implemented the helper's auth, and the mock copied the engine's strings.

Pattern 4 is an **amendment to rule 5**, not a new rule (D4): a second rule on the same subject would split it, and the
failing-first branch already amends rule 5. The sentence it adds, replacing the branch's:

> 5. ... Every `fix:` commit, and every commit that claims a failing-first run, carries `Failing-first: <test id> red at
> <sha>[+patch] (log: notes:<sha> | ci:<run>/<job>)` (made with `./dev red`) or `Failing-first: none (<reason>)`; a wrong
> claim is corrected by a later `Failing-first correction:` line. commit-lint checks the format on new commits, the pre-push
> check that the note exists, and a nightly sample re-runs claims. *Why:* two claims of 2026-11-04 had not been run.

> 71. **A shared component with layout variants ships a layout matrix.** A `paddock-ui` component whose layout depends on a
> platform capability, a container width or a variant declares its matrix in `<name>.layout.ts` (every capability value,
> width, variant and data profile); platform differences reach it as a capability value, never by detecting the OS; the
> gallery renders every row and `paddock-e2e` runs the shared layout checks on each, so a fix for one platform is judged on
> all. Checked by C96. *Why:* the tray "label over value" row took four rounds in one afternoon, each fixing the platform
> being looked at.

For the engineering rules (coordinator-owned sync, as for ER-33):

> ER-34. **OS integration is proved on the real OS across its lifecycle.** Every registration with the OS (services, login
> entries, tray icons, scheduled tasks, permissions, pipes) reads back what it wrote, and the UI shows what the OS reports,
> never the intended state. Before a release each registration passes install, first run, restart, a second user, update,
> turn-off and uninstall on a VM of each OS it supports. *Why:* fakes keep whatever they are given; the OS does not.

> ER-35. **No error is discarded where code meets the OS.** At OS boundaries an error is returned, carried into the state
> the user sees, or logged with its cause; a deliberate drop says why. A service writes its fatal error to the OS's own log
> from its first line. *Why:* a service with no console and a dropped error leave nothing for anyone to read. (Amends ER-27
> in spirit; kept separate because it is checked by a lint, not by reading logs.)

> ER-36. **A test double passes the same contract tests as the real thing, on each OS, and never decides.** *Why:* fakes
> drift, and a fake that decides is a second implementation.

> ER-11 (amended). ... The claim names the red state (a commit, or a commit plus a patch) and where the red run's log is
> kept, and a tool can re-run it.

> ER-37. **Shared UI is tested over every platform capability, width and variant it supports, in one run.** *Why:* a fix
> on the platform in front of you regresses the one that is not.

## Checks that make the classes impossible or detected

| Check | Kind | Would have failed on |
|---|---|---|
| `Registration::apply` returns `Observed` or `Mismatch`; the UI binds `Observed` | type plus shared adapter | the switch that said on with no `Run` value; the Linux turn-off that reported done |
| C92 registrations declared, each with an adapter and a lifecycle row | source check | every Dyno registration today (none is declared) |
| Lifecycle suite L1 to L8, rule 54 (d) | real-OS release gate, local VM | the 21 s crash (L3), the service gone after a restart (L4), login on Windows and Linux (L4), the Linux turn-off (L7) |
| C93 edges never discard; `service_main`; Clippy `let_underscore_*` in edge modules | source check without baseline; lint; one mechanism | `winsvc.rs:22`, `tray-icon/.../windows/mod.rs:84`, the earlier `let _ = icon.set_tooltip` |
| ESLint `no-floating-promises`, `paddock/no-silent-catch` in adapters | lint | an adapter that drops an `invoke` failure |
| `contract_tests!` with `TestRoot`; C94 | shared kit plus enumerating check | the fake rooted at `/` (it fails its contract on Windows) |
| Doubles never decide; C95 goldens | register plus check | the rig's copy of auth; a mock's copied strings without a golden |
| commit-lint format, `./dev red` notes, pre-push note check, nightly sample | lint, tool, re-run | `74e57f519`, `b8628bc21`, `13bf96710` (no note, no red state) |
| `defineLayoutMatrix`, `runLayoutMatrix`, C96 | shared kit plus enumerating check | the chooser at 900 px with its preview column on each tray capability |

Each new check ships with a fixture it fails on (the 0046 draft's demand): `winsvc.rs` at `afc0106b2` and the tray-icon
line reduced to synthetic files for C93; an undeclared `Shell_NotifyIconW` call for C92; a fake rooted at `/` for C94; a
`fix:` commit with a free-text parenthesis for commit-lint; the chooser without a matrix for C96.

## Work packages and order

Sizes are agent-days (estimates, not measurements). Owned paths are the files a package changes; shared integration points
(`crates/*/src/lib.rs`, `crates/Cargo.toml`, `.gitea/`) take one declared line each from the integrator. `scripts/vm-smoke` is
a harness (ADR 0042): L-4 lands from its home, coordinated with ADR 0053's S9, which uses the same files.

### Into Dyno 1.1.0 (patterns 1 and 2)

| WP | What | Owned paths | Depends on | Days |
|---|---|---|---|---|
| L-1 | `os_reg`: `Registration`, `Observed`, `RegError`; service adapters for SCM, systemd (`FragmentPath`, packaged and `/etc` units) and launchd with event-driven waits; the login writers of `91d703d81` moved onto the trait; ACL and pipe read-back by handle | `crates/paddock-service/src/os_reg/{mod,service,login,acl,pipe}.rs`, `crates/paddock-service/src/login.rs` (+ one `lib.rs` line) | `91d703d81` landed | 3–4 |
| L-2 | Tray read-back: every `Shell_NotifyIconW` result checked and logged, `Shell_NotifyIconGetRect` after add, macOS and Linux visibility read-back, the tray's `Observed` in diagnostics | `apps/dyno/app/src-tauri/src/tray.rs`, `apps/dyno/vendor/tray-icon/src/platform_impl/windows/mod.rs`, `apps/dyno/vendor/PATCHES.md` | L-1 | 1–1.5 |
| L-3 | Dyno adopts: the helper service, the login entries, the helper folders' ACLs and the helper pipe as `Registration`s; Settings' start-at-login row and the helper card bound to `Observed` with the mismatch sentence; the diagnostics list; `[[os_registration]]` and `[edges]` in `apps/dyno/wiring.toml` | `apps/dyno/crates/dyno-helper/src/{setup.rs,winsvc.rs}`, `apps/dyno/app/src-tauri/src/` (login and helper commands), `apps/dyno/app/src/app/features/settings/`, `apps/dyno/wiring.toml` | L-1, ADR 0052 WP-A1 for `wiring.toml` (or its section created here, coordinated with that WP's owner) | 2.5–3.5 |
| L-4 | The lifecycle suite: steps L1 to L8 driven by `[[os_registration]]`, probes per kind and OS, the judge of OS state against `Observed` and the declaration; Windows on ADR 0053's accounts and UAC automation, Linux with a second GNOME account in the Tart image, macOS headless; PM-1 to PM-17 as Windows per-machine rows; README and testing-manual page | `scripts/vm-smoke/{lifecycle.mjs,lifecycle.test.mjs,probes/}`, `scripts/vm-smoke/README.md`, the Linux image script | ADR 0053 S9's accounts and UAC automation (shared; whichever lands first builds them) | 4–5 |
| L-5 | C92 in `./dev check`, with its fixture | `tools/paddock-dev/src/checks/c92_os_registrations.rs` (+ the check list line), fixtures | L-1 | 1.5 |
| L-6 | Rule 54 (d): `./dev release tag` reads the lifecycle results; tag checklist text | `tools/paddock-dev/src/release/`, `docs/rules.md` (rule 54 (d) once accepted) | L-4 | 0.5 |
| E-1 | `service_main` and the OS-log sinks (event source in the install plan, `StandardError=journal` in rendered units, `os_log`), the exit-code table, panic boundary | `crates/paddock-service/src/{service_main.rs,os_log/}`, `crates/paddock-service/src/render.rs` | – | 2–2.5 |
| E-2 | Dyno's helper adopts `service_main` on all three OS; the deferred file log sends its pre-trust lines to the OS log | `apps/dyno/crates/dyno-helper/src/{winsvc.rs,main.rs,log.rs}` | E-1 | 1 |
| E-3 | C93 in `./dev check` (the five shapes, `// silent:` counted with ADR 0052's caps), the inner `deny` attribute in each edge module | `tools/paddock-dev/src/checks/c93_edges.rs`, fixtures | – | 1.5 |
| E-4 | Fix every C93 finding in Dyno's edge modules (the upper bound is the `let _ =` counts above; most of `dyno-agent`'s are expected outside the edges) | Dyno's edge modules as C93 lists them | E-3 | 1.5–3 |
| E-5 | ESLint `no-floating-promises`, `no-misused-promises`, `paddock/no-silent-catch` for Dyno's backend adapters | `eslint/src/rules/no-silent-catch.mjs`, the preset's adapter globs, `apps/dyno/app/src/app/core/` findings | – | 0.5–1 |
| **Dyno 1.1.0 total** | | | | **19–25** |

**Order for 1.1.0:** E-1 and L-1 first (both in `paddock-service`, one agent or two with disjoint files), then E-2, L-2,
L-3 and E-3 in parallel, E-4 after E-3, L-4 beside them from the start, L-5 and L-6 last. The critical path is L-1 → L-3 →
the lifecycle runs on three OS, about **7 to 10 working days with four agents**. This is on top of ADR 0053's Dyno part (its
own 3 to 4 weeks), and the two share the VM work. A smaller cut is decision D5.

These `crates/` changes are additive (a new module, a new function, moved writers in one crate whose only adopter in 1.1.0
is Dyno), so other apps need not change with them (rule 43); they are Dyno's release path, so rule 54 (a) applies.

### After the Dyno 1.1.0 tag (family-wide)

| WP | What | Owned paths | Depends on | Days |
|---|---|---|---|---|
| A-1 | Patterns 1 and 2 in Spotter, Pitwall, Pitcrew (registrations, edges, lifecycle rows, `service_main` for Pitcrew's and Pitwall's services) | each app's `wiring.toml`, edge modules, service entry points | L-*, E-* | 2–3 per app |
| F-1 | `paddock_testkit::contract` (`Contract`, `contract_tests!`, `TestRoot`) | `crates/paddock-testkit/src/contract.rs` | – | 1.5 |
| F-2 | `fakes.toml` registers and C94 (Rust and TS enumeration) | `tools/paddock-dev/src/checks/c94_doubles.rs`, `apps/*/fakes.toml`, `crates/fakes.toml` | F-1 | 2 |
| F-3 | `write_golden_ts` in `paddock_commands::testing`; `@imitates`; C95 | `crates/paddock-commands/src/testing/golden.rs`, `tools/paddock-dev/src/checks/c95_goldens.rs` | – | 1.5 |
| F-4 | Contracts for `paddock-service`'s `System` (real in the lifecycle VM), Dyno's trust ports (real on each CI OS in a `TestRoot`), the helper rig, Dyno's mock; then the other apps | per crate and app | F-1 to F-3 | Dyno 2–3; others 1 each (4) |
| R-1 | commit-lint: the trailer format, every commit that carries it, `Failing-first correction:` | `conformance/src/commit-lint.mjs`, `conformance/test/c23-commit-lint.test.mjs` | `origin/ci/failing-first-trailer` landed | 0.5 |
| R-2 | `./dev red` (run, require red, note, trailer), notes pushed and fetched | `tools/paddock-dev/src/red.rs` | R-1 | 1.5 |
| R-3 | Pre-push note check; nightly `--verify-sample 5` with a printed seed | `tools/paddock-dev/src/check/pre_push.rs`, a `.gitea/workflows/nightly.yml` step (integrator) | R-2 | 1 |
| M-1 | `defineLayoutMatrix`, the gallery routes, `runLayoutMatrix` | `ui/projects/paddock-ui/testing/layout-matrix.ts`, `ui/projects/paddock-e2e/src/layout-matrix.ts`, the gallery's route generator | – | 2 |
| M-2 | C96, with the affected-plan mapping from a component to its matrix | `tools/paddock-dev/src/checks/c96_layout_matrix.rs`, `scripts/affected.mjs` (and its Rust port) | M-1 | 1.5 |
| M-3 | Matrices: the widget chooser first, then each component C96 lists | beside each component | M-1, M-2 | 2–4 |
| **Family total** | | | | **25.5–31.5** (with three apps under A-1) |

R-1 to R-3 touch no product and no release path, so the coordinator may land them before the Dyno tag if an agent is free
(rule 1 allows it within load); they are listed here because the user scoped 1.1.0 to patterns 1 and 2. M-1 sits in a
harness home in `ui/` (ADR 0042 §1), so it waits for the release window to close.

**Rollback.** Each check is its own module and can be reverted alone. `os_reg` replaces `Autostart` in one crate with one
adopter in 1.1.0, so reverting L-1 reverts L-3 with it. `service_main` is additive.

## Waivers (bounded, ADR 0052's rules: at most the next release-line tag or 60 days, a dated trailer to move)

| Waiver | Until (delete by) | Why |
|---|---|---|
| macOS lifecycle L2's Login Items approval and L5 (second user) are MANUAL rows for Dyno | `dyno-v1.2.0` or **2027-01-03**, whichever is first | the Tart macOS VM runs `--no-graphics`; approval needs a click in System Settings, and a second macOS GUI session is not automated yet |
| C92, C93 and the lifecycle gate do not block apps other than Dyno | each app's next release-line tag after `dyno-v1.1.0` (Spotter's is `spotter-v1.0.0`), at latest **2027-01-03** | A-1 adopts them per app (D6) |
| ESLint `paddock/no-silent-catch` in apps other than Dyno | the same | the same |
| C94, C95, C96 are report-only | **2027-01-03** | F-* and M-* land after the tag; each becomes enforcing when its adoption WP lands |

## Parallelism and cost (rule 51)

- The checks (C92 to C96, commit-lint) are source checks with no build; they run in ADR 0049's no-build push check and
  `./dev check --pre-push`, each only when its inputs change (the Rust sources of edge modules, `wiring.toml`, `fakes.toml`,
  `paddock-ui` components). C92 and C93 share one `syn` parse per app with ADR 0052's C83; their cost is measured in L-5
  and recorded in `docs/benchmarks.md` before anyone quotes it (rule 48).
- The lifecycle suite is the expensive part: it reboots each VM and signs in a second user. It runs once per release per
  OS, locally, not in CI. The three OS's VMs can run at once within the VM slot limits of `./dev ci slot` and the load cap
  (rule 41); L4 and L5 are serial within one VM. Its run time is not measured yet; L-4 records it. Cheaper: the steps
  share one install per OS (L1 to L8 in order, not eight installs), and L3's settle uses an event wait, not a timer.
- `./dev red` runs one named test, not a suite; the nightly sample is five tests.
- The layout matrices run in Chromium in the affected plan only for the components a change reaches; the full set nightly.
  The matrix size is capabilities × widths × variants × data (the chooser: 4 × 4 × 4 × 2 = 128 rows); C96's report gives the
  total row count before M-3 lands, and a matrix over 200 rows is split pairwise (every pair of values covered) instead of
  full product.

## Consequences

- Every registration has one adapter that says what the OS did, and a user sees "Windows has no start entry for Dyno"
  instead of a switch that is on and does nothing.
- A release of an app with a service or login entry needs a VM pass with reboots and a second user on each OS: hours of a
  person's or an agent's time per release, which is the cost of finding these before the user does.
- Edge modules lose `let _ =` entirely; deliberate drops say why and count against a cap.
- A service's failure is readable in Event Viewer, `journalctl -u` or Console without the app's own log.
- Fakes become a small, enumerated set with contracts, and some will go when their contract shows the real adapter can run
  in a temporary root.
- Writing a failing-first claim costs one `./dev red` run; a false claim becomes detectable.
- Shared components with variants carry a matrix file each; the gallery grows by their rows.
- **Breaking before v1 (no shims):** `paddock_service::login::Autostart` goes; Dyno's `winsvc.rs` entry changes; the
  `Failing-first:` format of the branch changes before it lands.

## Alternatives considered

- **Unit tests against richer fakes only.** The fakes were the problem; a fake that models SCM crashes and systemd's unit
  search path is a second implementation of the OS. Rejected.
- **Run the lifecycle on CI runners.** It changes services and login entries of a shared machine (ER-14) and cannot reboot
  a runner. Rejected; VMs stay local.
- **A time-based settle ("check the service again after 30 s").** A wall clock, which ADR 0031 rejects; a slow VM would
  pass a crash at 31 s. L3 uses the service's own answer and the service manager's state change.
- **Clippy `let_underscore_must_use` family-wide now.** 1,816 `let _ =` sites on main, most harmless; it would be mostly
  waivers and hide the edge sites among them. ADR 0052 already adds it with a baseline; this ADR enforces it at the edges.
- **A new `// discard: <why>` hatch** (the brief's first idea). Rejected in favour of ADR 0052's `// silent: <why>`: one
  hatch, one count, one cap (D2).
- **Keep the failing-first evidence in the commit message only.** It cannot be re-run, which is what failed twice. **CI
  artifacts only:** they expire and a local red run has none. Git notes keep it with the history (D3).
- **Pixel diffs of the layout matrix.** Brittle across fonts and OS builds (ADR 0052 rejected it too); the fact checks say
  what is wrong.
- **Fix the instances only.** Each is fixed or being fixed on the Dyno 1.1.0 line; the evidence shows each shape returning
  within one day. Rejected as the whole answer (ER-21).

## Decisions for the user

| # | Question | Recommendation |
|---|---|---|
| D1 | Pattern 2 as its own rule 69, or a clause of rule 67 (b) ("an error is logged or carried, never dropped") | **Own rule 69**: it is enforced at once in edge modules with no baseline, while 67 (b)'s C87 is family-wide and report-only first |
| D2 | The escape hatch for a deliberate drop | **ADR 0052's `// silent: <why>`**, counted against its cap, not a new `// discard:` |
| D3 | Where red-run logs live | **Git notes** under `refs/notes/failing-first`, with CI run references also allowed |
| D4 | Pattern 4 as a new rule or an amendment | **Amend rule 5** (and ER-11), extending `origin/ci/failing-first-trailer` |
| D5 | Dyno 1.1.0 scope: the full set (19 to 25 days) or a cut | **Full set**, as the user asked; if the date matters more, the cut is L-1, L-3 (service and login only), L-4 for Windows and Linux (L1, L3, L4, L6 to L8), E-1, E-2, E-3 and E-4: about 14 to 19 days, with tray read-back (L-2), C92 (L-5), E-5 and macOS lifecycle rows waived to `dyno-v1.2.0` / 2027-01-03 |
| D6 | Spotter 1.0 under the lifecycle gate | **Yes**: Spotter goes per machine after Dyno's VM pass (ADR 0053 O7), and the suite is generic; its waiver ends at `spotter-v1.0.0` |
| D7 | Where registrations are declared | **`wiring.toml`** (ADR 0052's per-app register) rather than `paddock.ci.json`, which stays about CI; `vm-smoke` reads both |
| D8 | Real-adapter contract runs for registration ports | **In the lifecycle VM only** (ER-14); CI runs the fakes and the real adapters of file, process and socket ports |
