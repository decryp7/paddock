# 77. Tests never reach the machine's system services: one reach decision, guarded adapters, a register check and a host probe

- Status: Accepted (2026-11-09). The user accepted it as recommended on 2026-11-09 (ER-21 redesigns).
- Trimmed 2026-11-11 (user, comparable-practice review): TRIM: keep the `paddock-secrets` and `paddock-service` guard (WP-3), a small `account_home` helper, the login-item guard in three apps, Dyno's helper centre and wakes; dropped the TOML register, its check and ratchet (WP-5), the host probe (WP-6) and most per-app sweeps.
- Date: 2026-11-09
- Related: ER-14 / rule 7 (tests never touch the user's machine), ER-21 / rule 40 (recurring bugs mean a missing design
  rule), ER-22 / rules 42, 43 and 55 (share first; one implementation per mechanism), ER-27 (logs explain every failure),
  ER-28 (a cause is confirmed by measurement), ER-31 (release impact), ER-33 / rule 71 (a check counts only with a
  positive control), rule 53 and ADR 0033 (identity before acting).
- Composes with: **ADR 0073** (no test double in a shipped build: a guarded adapter here *refuses*, it never swaps in a
  fake inside a shipped binary), **ADR 0074** (declared hosts and machine leases: the host probe of §5 runs on the hosts
  0074 declares; Part C there covers host *size*, this ADR covers host *services*), **ADR 0078** (one shared test scratch:
  the same run marker names the run's scratch root), **C14** (`conformance/src/hermetic.mjs`, HOME and TMPDIR in a
  scratch folder) and **C15** (no installed Chrome in test files).
- Number: 0077 and 0078 were free on every `origin` branch on 2026-11-09 (`git ls-tree -r --name-only` over
  `git branch -r`: family ADRs end at 0076; Debrief's per-app ADRs use 0059 and 0061-0070 under `apps/debrief/docs/adr`).
  Rule and check numbers are assigned on acceptance.
- Evidence read at `origin/main` `b51c72509` unless a row names a branch. Survey commands are in "Instances"; the
  scratch copy of the scans is `/Volumes/d7sst7/dev-targets/scratch/adr-0077-0078/`.

## Pattern

**A test reaches a real system service of the machine it runs on.** Whenever product code that talks to a machine-wide
service (a privileged helper, launchd/systemd/the Windows SCM, login items, the keychain or Credential Manager, an
installed browser, scheduled OS wakes, a container engine) runs inside a test, which service it reaches depends on what
the test remembered to inject. If the test forgets, or starts a *shipped binary* that composes itself (an agent, a
sidecar, the app), the real adapter runs against the developer's or the runner's own machine.

**The invariant that should hold.** Product code reaches a system service only through a port whose real adapter first
asks one family-wide *reach decision*; the decision refuses (a typed reason, logged, shown as "not included") whenever the
process runs under a test run, or (for a machine-wide service) its data folder was moved by an override variable. A
test that needs a real service names an explicit *test resource* (a pinned test browser, a test keychain file, a label
prefix of its own), read only in test-support builds, never the user's default. A static check lists every client of a system
service and fails on one outside a guarded adapter; a host probe shows that a test job changed nothing in the machine's services.

## Context: the incidents

| # | Where | What happened | State |
| --- | --- | --- | --- |
| 1 | Dyno `dyno-agent` `term` tests, CI job 33005 (run 7810, macOS), fixed on `origin/fix/dyno-agent-term-race` `c03fb61be` (cause C3 in its message) | The tests' agents ran on `DYNO_DATA_DIR`, which counted as the user's own folder, so they used the computer's **installed privileged helper**: `HelperStatus` answered Ready ("Administrator readings are on"), helper calls took 1-4 s each under load, a stop waited on `dyno-wakes` past the TERM grace (2 of 40 stress iterations), and at start `WakeCentre::sync_all` **cancels every Dyno OS wake its folder does not own** through that helper, so a test run could cancel the user's scheduled wakes. The coordinator reports refusals from test runs in the helper's audit log on the dev Mac (not re-read for this draft). | Fixed for Dyno only: `Dirs::is_users_own` (`paths.rs`) compares with `DYNO_DATA_DIR`; failing test first `term an_agent_on_a_data_folder_of_its_own_never_asks_this_computers_helper`. |
| 2 | Browser tests (`docs/lessons.md:93`) | Tests launched the user's installed Google Chrome: every killed launch left a 0.7 GB code-sign clone; 620 clones filled 441 GB. | Fixed per driver: `CHROMIUM_TEST_PATH` / `SPOTTER_TEST_BROWSER` and `find_for_tests`; C15 bans installed-Chrome strings in test files. The production `find()` is still callable from a test. |
| 3 | Agents and Docker (`docs/lessons.md:59`) | A blanket `docker ps -q \| xargs docker stop` destroyed a user's long-running container. | Agent guidance only. Pitcrew's container hygiene (`pitcrew-service/src/hygiene.rs`) prunes containers and images through the engine CLI; its tests use a fake `docker` script by choice. |
| 4 | Keychain and service names (`docs/lessons.md:216`) | Tests used production names (pipes, sockets, data folders, keychain entries); the CI runner itself runs the installed app, so a test binding the real pipe failed with access denied. | Guidance: derive names from the data folder. Six shipped files still call `keyring::Entry` directly (below). |
| 5 | Host runners (`docs/lessons.md:825`) | A host runner's temp folder is the user's temp folder; checks saw other runs' files. | Fixed for TMPDIR by `conformance/ci/job-tmpdir.sh`; not for services. |
| 6 | Agent tests (`docs/lessons.md:442`) | Agent tests ran the real socket lister, counters and hardware readers. | Fixed for Dyno by platform ports with fakes. |

Every fix so far is per client and opt-in: the test must choose the fake or the test resource. Incident 1 shows the
failure mode that remains: an integration test starts a **shipped binary** (`dyno-agent`), which composes its real
adapters because nothing tells it that it runs under a test; `cfg(test)` does not reach a dependency or a child process.

## Instances: every client of a system service (`origin/main` `b51c72509`, 2026-11-09)

**Method.** `git grep` over shipped Rust (excluding `tests/`, `*_tests.rs`, `fake*.rs`, `testing.rs`, `vendor/`) for the
service APIs and programs: `keyring::Entry`, `security_framework`, `SMAppService`, `windows_service::service_manager` /
`OpenSCManager`, the program constants `LAUNCHCTL`, `SYSTEMCTL`, `OSASCRIPT`, `PMSET`, `Command::new("launchctl"|"sc"|...)`,
`powermetrics`, `"docker"`/`"podman"` as programs, Run-key and registry writes, installed-browser paths; then the ports
that wrap them. Comment lines are excluded. WP-0 confirms each row by reading it.

| Service | Shared port and real adapter | App clients and how they are guarded today | Class |
| --- | --- | --- | --- |
| Privileged helper (Dyno) | none shared; `dyno-agent` `HelperTransport` (`helper_link.rs:51`), real `PeerTransport` (`helper_peer.rs:158`), `HelperCentre` (`helper_centre.rs:32`), `helper_os.rs` | Dyno only; guarded on the branch by `Dirs::is_users_own` (env-based) | Broken on `main` (incident 1) |
| OS wakes | `dyno-agent` `WakeCentre` (`wakes.rs:53`, `sync_all` `:243`) through the helper (macOS, Linux), own timer (Windows); `dyno-platform/src/wakes.rs` (`PMSET`) | Dyno; same guard as the helper on the branch | Broken on `main` (cancels user wakes) |
| Power assertions | `paddock-power` `PowerOs`, `SleepWakeSource` (`port.rs`), `native.rs` | Dyno (4 files) | At risk (held only while the test runs) |
| Services, launchd jobs, login items, Run entries | `paddock-service` `System`, `AppService`, `Elevator` (`port.rs`), `Backend` (`backend/mod.rs:361`), `Autostart`, `Elevation`, `LoginService` (`login.rs`), `privileged/*` (`ProtectedFiles`, `TrustPort`, `KnownFolders`) | Dyno (23 files), shell-demo (3); **Pitcrew and Pitwall** use their own `app/src-tauri/src/service_install.rs` (`Command::new` on `launchctl`/`systemctl`/`sc`; registered copies in `docs/known-copies.toml`) and `service_repair.rs` (`launchctl print`, `print-disabled`, `bootout`; the same mechanism) | At risk: only a test's choice of fake |
| Login items (Tauri plugin) | `paddock-service` `Autostart`, `LoginService` (canonical of known-copies `launch-at-login`) | **Pitcrew, Pitwall, Spotter** register `tauri_plugin_autostart` with `MacosLauncher::LaunchAgent` (`app/src-tauri/src/lib.rs:32`, `:115`, `:29`) and toggle it through `ManagerExt` (`commands.rs:24`, `app_commands.rs:11`, `commands.rs:15`): a LaunchAgent plist, a Run value or an XDG autostart file of the real app | At risk: no port, no guard |
| Credentials | `paddock-secrets` `Secrets`, `Vault` (`vault.rs`), `os/native.rs`; **already guarded** by `real_blocked()` (`os/mod.rs:19-35`): `OsVault` panics when `cfg!(test)` in the crate's own tests or `PADDOCK_SECRETS_FORBID_REAL=1`, unless `PADDOCK_SECRETS_REAL=1` | Dyno (4), workbench-demo (1) through the port. **Bypass, direct `keyring::Entry`:** Debrief `app/src-tauri/src/ai/pitwall.rs:29`; Pitcrew `channels.rs:63,73,82` and `credentials.rs:16`; Pitwall `credentials.rs:44` and `pitwall-cli/src/discover.rs:163`; Spotter `credentials.rs:36` (6 files) | At risk; 6 outside any port |
| Installed browsers | `paddock-chromium` `discover.rs` (`TEST_BROWSER_ENV` = `CHROMIUM_TEST_PATH`, `find_for_tests`) | Copies: Debrief `chromium-driver/src/discover.rs`, Spotter `spotter-render/src/discover.rs` (`SPOTTER_TEST_BROWSER`), both registered; Debrief `dev_overrides.rs` | Guarded by the test's choice; C15 on test text |
| Container engine | none | Pitcrew `pitcrew-service/src/hygiene.rs` (prune stopped containers and dangling images with `docker`/`podman`) | At risk |
| Playwright | `@decryp7/paddock-e2e` `browserProblems`, `assertChromeForTesting` | every Playwright config (10) | Guarded (config check, C15) |

**Counts.** 7 kinds of service; 4 shared ports exist (`paddock-service`, `paddock-secrets`, `paddock-power`,
`paddock-chromium`) and one app port (Dyno's helper). One of them has a test guard of its own: `paddock-secrets`'
`real_blocked()`, opt-in per test suite through `PADDOCK_SECRETS_FORBID_REAL`, with `PADDOCK_SECRETS_REAL` to override it;
the others ask nothing. Outside any port: 6 credential files, 2 service-install copies with their 2 `service_repair.rs`
twins, 3 `tauri_plugin_autostart` login items, 2 browser-discovery copies, 1 container client. Data-folder override variables
differ per app (`DYNO_DATA_DIR`, `DEBRIEF_DATA_DIR`, `DEBRIEF_HOME`, `PITWALL_HOME`, `SPOTTER_DATA_DIR`; `paddock-env`'s
`<ID>_HOME` in debug builds, `Source::{Os, Override, Portable}`), so "is this the user's own folder" has one answer only
in Dyno, and only on a branch.

**A hidden gap in Dyno's fix.** `is_users_own` compares the folder with `Dirs::current()`, and `paths.rs` builds
`Dirs::current()` from `HOME`. No Dyno test uses `paddock_testkit::EnvLock::hermetic` today, and `term.rs` sets
`DYNO_DATA_DIR`, so the fix holds for the tests that exist. But a test that does use `hermetic` (`env.rs:53`, `HOME`
in the scratch folder) and no `DYNO_DATA_DIR` would make `Dirs::current()` the test's own folder, the check would say
"the user's own", and the helper (a machine-wide service) would be reached. The guard is also Dyno's alone.

## Decision

### 1. One reach decision, in `paddock-env`

```text
pub enum Reach { Allowed, Refused(Why) }
pub enum Why {
    TestRun { run: String },                 // PADDOCK_TEST_RUN (or NEXTEST_RUN_ID) is set
    MovedDataFolder { var: &'static str },   // the data folder was moved by an override variable
}
pub fn system_reach(vars: &Vars, dirs: &AppDirs) -> Reach   // pure; vars and the account home passed in
pub fn account_home() -> io::Result<PathBuf>  // from the account database (getpwuid_r; SHGetKnownFolderPath with the
                                              // process token), never from HOME/USERPROFILE
```

- **The marker** is `PADDOCK_TEST_RUN=<run id>`. Nextest's own `NEXTEST_RUN_ID` counts as the marker too (nextest sets
  it in every test process), so a local `cargo nextest run` is covered without any setup; a plain `cargo test` is covered
  by the testkit (§3) and by the folder condition.
- **The folder condition counts only an override.** A data folder moved by the app's override variable (`<ID>_HOME`,
  `DYNO_DATA_DIR`, `DEBRIEF_DATA_DIR`, `PITWALL_HOME`, `SPOTTER_DATA_DIR`; one list in `paddock-env`) refuses, and only
  for **machine-wide services** (privileged helper, OS wakes, services and launchd jobs, power assertions, container
  engine); per-account services (credentials, login items, the browser) follow the marker alone. Portable mode
  (`dirs.rs:81`) and a custom `XDG_DATA_HOME` are real users' setups and are never refused. A machine-wide service whose
  client compares folders uses `account_home()` (account database), never `HOME`, so a hermetic `HOME` cannot make a
  test folder look like the user's.
- **Either condition refuses.** The decision is read once at composition (parse at the edge) and passed to every guarded
  adapter as a value; one `info` line at start names it (`SystemReach=refused Why=test-run Run=<id>`, no paths).
- **In release builds the marker may only refuse (D3, rule 73).** Integration tests start shipped binaries, so a release
  build reads the marker, but there it can only turn services off (logged at start); it never selects anything. Under
  rule 73(c) `PADDOCK_TEST_RUN` and the honoured `NEXTEST_RUN_ID` are registered in ADR 0073's inventory
  (`docs/test-doubles.toml`) as kind `guard` (class E there, like `PADDOCK_FAKE_BROWSER`), with a review date.
- **Folded in.** `paddock-secrets`' `real_blocked()` becomes this reach check (the marker refuses; its crate-internal
  `cfg!(test)` stays), and both `PADDOCK_SECRETS_FORBID_REAL` and `PADDOCK_SECRETS_REAL` are removed in the same change,
  with every suite that sets them.

### 2. Every real adapter of a system service is guarded

Each real adapter in the table takes `Reach` at construction. Refused, it does not reach the service: queries answer a
typed "not included: test run" (Dyno's helper status "not included", as `c03fb61be` already does), commands fail with a
typed error naming the reason. It never substitutes a fake (ADR 0073). The fakes stay where they are, in test builds.

- **Test resources exist only in gated builds.** A port may accept an explicit test resource (the pinned browser named by
  `CHROMIUM_TEST_PATH`, a keychain file of the test's own, a launchd label with a per-run prefix), but the code that reads
  it sits behind `cfg(any(test, feature = "test-support"))` (rule 73(a) and (c)); a release build has no such path and,
  under the marker, only refuses. The production default (the installed browser, the login keychain, the app's real
  label, the installed helper) is always refused under the marker. `CHROMIUM_TEST_PATH` and `SPOTTER_TEST_BROWSER` move
  behind that gate where they are not already (WP-3).
- **Clients outside a port move into one:** the 6 credential files to `paddock-secrets`; the 3 `tauri_plugin_autostart`
  login items to `paddock-service`'s `Autostart` (the registered `launch-at-login` copies), or a guard around the plugin's
  `enable`/`disable` until they move; Pitcrew's container hygiene
  behind a small `ContainerEngine` port in `pitcrew-service`; the service-install and browser-discovery copies are guarded
  in the same change as their canonical (rule 55) or waived until their registered `delete_by`.

### 3. Who sets the marker (one owner per place)

| Place | Sets | Through |
| --- | --- | --- |
| Rust test processes under nextest | `NEXTEST_RUN_ID` (nextest's own) | nothing to add |
| Rust tests under `cargo test`, and children a test starts | `PADDOCK_TEST_RUN` | `paddock_testkit::EnvLock::hermetic` sets it; `paddock_testkit::command(..)` (the kit's child-process builder) sets it on every child |
| CI jobs | `PADDOCK_TEST_RUN=<job id>` | `conformance/ci/job-tmpdir.sh setup` (already the one writer of `TMPDIR`), so job-isolation's acquire and the `job-tmpdir` action both export it |
| Playwright and Vitest | `PADDOCK_TEST_RUN` | `@decryp7/paddock-e2e` `paddockConfig` (webServer and launched apps' env), the Vitest preset |
| VM tests of an installed app | nothing | the installed app on a test VM is that VM's user's app (ADR 0074 leases the VM) |

### 4. The static check (number on acceptance): a register that enumerates

`docs/system-services.toml` lists each service kind with its API and program patterns and the files allowed to use them
(the real adapters). Patterns include `keyring::Entry`, `security_framework`, `tauri_plugin_autostart` (`init`, `ManagerExt`), `SMAppService`, `windows_service::service_manager`, the programs `launchctl`, `systemctl`, `sc`, `pmset`, `osascript`, `powermetrics`, `docker`, `podman`, Run-key writes, installed-browser paths, and the retired variables `PADDOCK_SECRETS_FORBID_REAL` and `PADDOCK_SECRETS_REAL`. The check scans shipped Rust and TypeScript (test code is out: a test calling a guarded adapter is
refused at run time) and fails on a match outside the listed adapters, and on a listed adapter whose constructor does not
take `Reach` (a syn pass for the constructor's parameters, as ADR 0076's C42 prototype parses Rust). Today's clients
outside a port start in a shrink-only ratchet, each with its app's next tag. **Positive controls:** a fixture with a bare
`keyring::Entry::new`, a `Command::new("launchctl")` and an adapter constructor without `Reach` must fail; the check prints
how many patterns and adapters it scanned and reports `check broken` when either is zero (rule 71). C15 stays for test text.

### 5. The host probe: a test job leaves the machine's services as it found them

Every test job on a host runner (macOS and Windows; the Linux containers have no user services) snapshots the service
state at acquire and compares at release, through job-isolation (one owner):

- macOS: `launchctl print gui/<uid>` labels with the family prefixes, `~/Library/LaunchAgents` names, `pmset -g sched`,
  the family's generic-password item names in the login keychain (`security dump-keychain` without `-d`; the probe keeps
  and logs only items whose service is one of the family's names, never other items, attributes or secrets),
  the Dyno helper's audit log lines written during the job (count and the refusing caller's run id);
- Windows: family services (`sc query`), HKCU and HKLM `Run` values, Task Scheduler entries with family names,
  `cmdkey /list` target names;
- both: containers and images with family labels (`docker ps -a --filter label=...`) where an engine exists.

A difference fails the job with one `::error::` naming the item and the next step (ER-27). **Positive control:** the
probe's own test adds and removes a labelled launchd job (Windows: a Run value) in a scratch prefix and must report it;
when a probe cannot read its source (no keychain access, no `sc`) it reports `not measured`, which fails a release check
(ER-33). Agents' local runs get the same probe as `./dev check services --since <snapshot>`.

### 6. Logs (ER-27)

The start line of §1; each refused call logs once per adapter and reason (`Service=helper Why=test-run`), never per call;
the probe prints what it compared and how many items per source.

### 7. Rule text (proposed; number on acceptance)

"Product code reaches a machine-wide service (privileged helper, service manager, login items, credential store,
installed browser, OS wakes, container engine) only through a guarded adapter that refuses under a test run or, for a machine-wide service, on a
data folder moved by an override variable; a test that needs a real service names a test resource of its own, read only
in test-support builds. Checked by the
service register check and the host probe." ER-14 gains a pointer to the mechanism (ER candidate).

## Alternatives rejected

- **Fix each client when it bites** (today): six incidents, each fixed in one client; the next client (a new keychain use,
  a new helper call) starts unguarded.
- **Only the folder condition** (Dyno's fix): not every service has a data folder, and a folder compared through `HOME`
  is blind under a hermetic `HOME`.
- **Any non-default folder refuses:** would refuse real users in portable mode or with a custom `XDG_DATA_HOME`.
- **Only `cfg(test)` / a `testing` feature:** does not reach a shipped binary that a test starts (incident 1).
- **A fake chosen by the marker inside the shipped binary:** a test double in a shipped build (ADR 0073).
- **Only the host probe:** finds the damage after it happened (a cancelled wake, a removed container).

## Consequences

- A forgotten fake no longer reaches the user's machine; it gets "not included: test run" and the test fails readably.
- Every app reads its folders and reach through `paddock-env` (Debrief, Pitcrew, Pitwall and Spotter adopt it for this).
- Machine services are visibly off in any process with the marker; a developer who wants them in a dev run starts the app
  without the test harness.
- One more probe step per host job (about 1 s per snapshot on macOS, to be measured in WP-6 and recorded, rule 48).

## Work packages (raw agent-days, one agent)

| WP | Content | Days |
| --- | --- | --- |
| WP-0 | Read every row of the instance table; write `docs/system-services.toml` and the ratchet | 1 |
| WP-1 | `paddock_env::system_reach`, `account_home` on macOS, Windows and Linux, the log line; tests on injected vars | 2 |
| WP-2 | Marker sources: testkit `hermetic` and `command`, `job-tmpdir.sh`, `paddockConfig`, the Vitest preset; tests | 1.5 |
| WP-3 | Guard the shared adapters: `paddock-secrets` (fold `real_blocked()` in, remove its two variables and their users), `paddock-service` (all real backends), `paddock-power`, `paddock-chromium` `find()`; test-resource variables behind `test-support`; the 0073 register entries | 2 |
| WP-4 | Apps: Dyno (helper centre, wakes, replace `is_users_own`) 1.5; Pitcrew (keyring ×2 files, `service_install` and `service_repair`, container port) 1.5; Pitwall (credentials, `pitwall-cli`, `service_install` and `service_repair`) 1; Spotter (credentials, discovery copy) 0.5; Debrief (keyring in `ai/pitwall.rs` unless the AI removal deletes it, discovery copy) 0.5; the `tauri_plugin_autostart` login items in Pitcrew, Pitwall and Spotter (guard or move to `Autostart`) 1 | 6 |
| WP-5 | The register check, its fixtures and positive controls | 2 |
| WP-6 | The host probe on macOS and Windows, its positive control, `./dev check services` | 2.5 |
| WP-7 | Rule text, ER candidate, lessons entry, ADR index, CHANGELOGs | 0.5 |
| | **Total** | **17.5** |

First cut (closes incident 1's class for the app that has the dangerous services, and stops new clients): WP-0, 1, 2, 3,
Dyno's part of WP-4, WP-5 = **10 days**. The rest lands in each app's next release phase.

## Release impact (ER-31)

Shared crates (`paddock-env`, `paddock-secrets`, `paddock-service`, `paddock-power`, `paddock-chromium`, the testkit) change,
so every app that ships them is in the affected plan; the change is additive (a new constructor parameter) and lands with
its adoption (rule 43). Without the marker and with no override variable set, nothing changes for a user's installed app: portable mode and a
custom `XDG_DATA_HOME` are not refused (§1). Under D7 (c) Dyno's next tag waits only for the instance fixes
on fix branches (`fix/dyno-agent-term-race` `3d33217af`, `fix/dyno-wakes-stop-window` `70d92f27a`), to land before
the tag; the ratchet entries block other apps from their release after the one in flight.

## Decisions for the user

| # | Question | Options | Recommendation |
| --- | --- | --- | --- |
| D1 | What refuses | (a) The marker for every service, and a data folder moved by an override variable for machine-wide services only; portable mode and a custom `XDG_DATA_HOME` never refuse. (b) Any folder that is not the default (refuses real portable users). (c) The marker only. | (a): (b) breaks real users; (c) misses a dev run on a moved folder reaching the installed helper. |
| D2 | What a refused adapter does | (a) Typed refusal, "not included: test run", logged once. (b) An in-memory stand-in in the shipped binary. | (a) (ADR 0073). |
| D3 | The marker in release builds | (a) Read in release builds, where it can only refuse (logged at start); test resources only in `test-support` builds; the marker registered as a `guard` (class E) in ADR 0073's register. (b) Debug builds only (integration tests that start a release binary would reach the real services). | (a): it complies with rule 73(c) and covers tests that start shipped binaries. |
| D4 | Real services in tests | (a) Only explicit test resources (pinned browser, own keychain file, per-run label prefix); no blanket opt-in. (b) A `PADDOCK_TEST_REAL=<kinds>` opt-in on hosts ADR 0074 declares as test machines. | (a); add (b) only when a test needs the installed helper itself, on a leased VM. |
| D5 | Checks | (a) Register check plus host probe. (b) Register check only. (c) Probe only. | (a): the check stops new clients, the probe catches what the check cannot see (a shell script, a tool). |
| D6 | Where the decision lives | (a) `paddock-env` (folders and the override already live there). (b) A new small crate. | (a). |
| D7 | Release gating | (a) Every ratchet entry blocks its app's next tag (adds all 17.5 days to the next tags). (b) Dyno's next tag waits for the first cut (adds 10 days before it). (c) Dyno's next tag waits only for the instance fixes on fix branches (`fix/dyno-agent-term-race` `3d33217af`, `fix/dyno-wakes-stop-window` `70d92f27a`), to land before the tag (0 extra days); the first cut (10 days) runs in parallel and new clients fail from its landing; other apps' entries block their release after the one in flight (Spotter 1.0 not blocked). (d) Report only. | (c): the dangerous Dyno paths are already closed by the instance fixes; the mechanism follows without holding the tag. |
| D8 | Binding rule | Rule text of §7 and an ER-14 amendment candidate. | Yes. |
| D9 | Scope now | (a) All WPs (17.5 days). (b) The first cut (10 days), the rest per app. | (b). |
