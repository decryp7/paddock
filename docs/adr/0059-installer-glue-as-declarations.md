# 59. Installer glue as declarations: apps declare, the shared include generates

- Status: Proposed (2026-11-08). The user approved the direction on 2026-11-08 and placed it **before Pitwall 1.1 ships**:
  no app keeps hand-written NSIS install, service or uninstall steps; each per-machine app declares what it has in
  `windows/per-machine.json`, the shared include generates every step from that declaration, the shared harness proves the
  generated steps once on a Windows VM with a stub service, and each app gets a static check.
- Date: 2026-11-08
- Related:
  - Family ADR [0053](0053-per-machine-windows-installs.md) (per-machine Windows installs; its amendment 1 on
    `docs/dyno-adr-0053-amend1`): this record changes how the app's own steps of §2 ("the app's own steps go between them")
    are written, and extends §8's check C91. Everything else in 0053 stands.
  - Pitwall ADR 0024 (per-machine Windows install, on `pitwall/1.1-b6c`): its "Pitwall's own step" row and the VM rows PW-3
    and PW-6 move into the shared harness.
  - ER-21 / rule 40 (a recurring kind of bug needs a design rule), rule 55 and ADR
    [0034](0034-one-implementation-per-mechanism.md) (one implementation per mechanism), rules 42 and 43 (share first; a
    shared change updates its users in the same change), rule 54 (a release-path change builds the installers before it
    lands), ER-14 (VMs only), ER-23 (independent review), ER-27 (logs explain every failure), ER-33 (a check counts only when
    its probe has shown it can see the thing), ADR [0042](0042-one-owner-per-harness-and-small-batches.md) (one owner per
    harness), ADR [0031](0031-patience-not-wall-clocks.md) (bounded waits).
- Number: 0059. The highest family ADR on any branch is 0058 (`git ls-tree --name-only <ref> docs/adr/` over every
  `refs/remotes/origin/*` and `refs/heads/*`, 2026-11-08). No new check number: C91 is extended (§6); the highest check
  named anywhere is C99 (`origin/pitwall/1.1-b5`).
- Sources read (2026-11-08): the include on `origin/main` (`INCLUDE_VERSION "4"`) and on `pitwall/1.1-integrate`
  (`7d4ba0f72`, `INCLUDE_VERSION "8"`, the uninstaller's trust decision, level 45, `PADDOCK_DIR_ACCEPTED`); Dyno's and
  Pitwall's `installer-hooks.nsh` and `per-machine.json` on that branch; `crates/paddock-service/src/per_machine.rs` (the
  generator); `crates/paddock-setup/nsis/test/` (the harness, cases A to U, P, Q, R, M1 to M5); C91 on `origin/main`;
  Pitcrew's `installer-hooks.nsh` and `apps/pitcrew/crates/pitcrew-service/src/platform/windows.rs` on `origin/main`.

## Context

### The pattern

ADR 0053 made the shared parts of a per-machine install one include (`crates/paddock-setup/nsis/paddock-per-machine.nsh`)
and one program (`paddock-setup.exe`), proven once by the harness. It left "the app's own steps" to each app's hooks file.
Those steps are now the place where the installer's safety bugs come back, in each app separately:

| Where | Finding | Fix (branch `pitwall/1.1-b6c`) |
| --- | --- | --- |
| Pitwall 1.0.x hooks | `pitwall-server.exe uninstall` ran before any check of the folder: a folder the user could write put a user's program in front of an elevated installer (security review of B6, P2-4) | `PADDOCK_CHECK_DIR` first, then the verb |
| Pitwall and Dyno uninstallers | an `uninstall.exe` copied to a user folder next to a planted `paddock-setup.exe` ran both elevated (B6b review, P1-1) | include v5 to v7: `_PADDOCK_TRUST_FOLDER`, level 45, `PADDOCK_DIR_ACCEPTED` asked by each app's hook |
| Pitwall installer | a failed service removal either aborted every time or never; it must abort only while the service is still registered (`7f37adb40`) | `sc query` after a failed verb, 1060 means gone |
| Dyno uninstaller | the agent and the helper ran from a folder nobody had accepted (`1d55c2726`) | a hand-written `$DynoDirOk` from `PADDOCK_DIR_ACCEPTED` |
| Dyno and Pitwall uninstallers | with `paddock-setup.exe` missing from a trusted folder, the service stayed registered (`2442ca3c2`) | a hand-written `sc stop` and `sc delete` fallback, written twice |
| Dyno hooks | `dyno-agent stop` runs with no timeout; a refresh failure is written to `setup.log` with a hand-written `FileOpen` | not fixed |

Each fix was right and each was written per app, twice, with the same shape (gate a program run on the folder; remove a
service; tell the user). Pitcrew, the next adopter, still has the 1.0 shape (its verb runs from `$INSTDIR` before any check;
see "Pitcrew" below) and would be the third copy. The cost is not only the copies: because each app's hooks differ, every
app needs its own Windows VM round for installer behaviour that is really the family's.

The pattern: **the app's installer steps are an undeclared program.** They hold the same five decisions (is the folder
trusted, is it accepted, is this an update or an uninstall, is the service still registered, is the tool there), each made
again in NSIS by hand. Patching instances does not end (ER-21).

### What the apps' steps are today

| App | Graceful stop | Service | Registered by | On update | On real uninstall |
| --- | --- | --- | --- | --- | --- |
| Dyno | `dyno-agent.exe stop` (PREINSTALL, PREUNINSTALL), no timeout | `DynoHelper`, `dyno-helper.exe`, runs from its own protected folder, excluded from the stop list | the app (the user turns it on in Settings) | kept; `dyno-helper.exe refresh` in POSTINSTALL when registered (120 s, failure noted, never fatal) | `dyno-helper.exe uninstall` when registered; `sc` fallback with the tool missing; nothing when the folder is not accepted |
| Pitwall | none | `Pitwall`, `pitwall-server.exe` (in the stop list), LocalSystem, restart on failure | the installer | `uninstall` before `stop-all` (abort level 43 while still registered), `install` in POSTINSTALL (failure: `check-install`'s words in a message, never fatal) | `uninstall` before `stop-all`; `sc` fallback with the tool missing; nothing when the folder is not accepted |
| Pitcrew | none | `Pitcrew`, `pitcrew-service.exe` (in the bundle), LocalSystem, restart on failure | the installer | `uninstall` in PREINSTALL with no folder check and no failure check, `install` in POSTINSTALL (failure: a fixed message) | `uninstall`, unchecked |

Two kinds of service are enough: one **the installer registers** (removed before files change, registered again after,
removed on uninstall: Pitwall, Pitcrew), and one **the app registers** at the user's request (kept across updates and
reinstalls, optionally refreshed, removed only on a real uninstall: Dyno's helper). No app keeps program-folder files of its
own beyond what the include already handles (`updates\`, `paddock-setup.exe`).

## Decision

### 1. The declaration: `windows/per-machine.json`, version 2

One file per app, beside the hooks file. Its **one definition is a Rust type**, `paddock_setup::declaration::Declaration`
(serde, `deny_unknown_fields`), parsed once at the edge (parse, don't validate) and validated by
`Declaration::validate(&Bundle)`, where `Bundle` is what the generator reads from `tauri.conf.json` and
`tauri.windows.conf.json` (`mainBinaryName`, `externalBin`, `productName`). A JSON Schema,
`crates/paddock-setup/schema/per-machine.schema.json`, is **generated** from the type (`schemars`) by the same sync test that
writes the generated include, and checked current; each app's file names it in `$schema` for editors. The Rust type and its
validation are the schema in force; the JSON Schema is its published form (one declared definition, a generator keeps them
equal).

```jsonc
{
  "$schema": "../../../../../crates/paddock-setup/schema/per-machine.schema.json",
  "version": 2,                                   // required; 1 (today's file) is refused with "regenerate: version 2"
  "stopExclusions":   [{ "image": "…exe", "reason": "…" }],          // unchanged from ADR 0053 §2.1
  "survivesForcedEnd": [{ "image": "…exe", "why": "…" }],            // unchanged from ADR 0053 amendment 1 §3
  "gracefulStop": [                               // at most 4; run before stop-all, in this order
    { "program": "dyno-agent.exe", "verb": ["stop"], "timeoutMs": 30000 }
  ],
  "services": [                                   // at most 4
    {
      "name": "Pitwall",                          // the SCM service name
      "label": "the Pitwall service",             // used in generated messages
      "program": "pitwall-server.exe",            // a program of the bundle, run from $INSTDIR
      "registeredBy": "installer",                // "installer" | "app"
      "register":   { "verb": ["install"], "timeoutMs": 120000,      // installer only
                      "explain": { "verb": ["check-install"], "timeoutMs": 30000 },   // optional: stdout is the message
                      "failedText": "Open Pitwall, Settings, Service to try again." }, // optional, used without explain
      "refresh":    { "verb": ["refresh"], "timeoutMs": 120000 },    // app only, optional
      "unregister": { "verb": ["uninstall"], "timeoutMs": 120000 },
      "fallback":   { "kind": "sc-delete",                           // "sc-delete" | "none"
                      "leftBehind": "Its folder … were not removed; …" }   // optional, appended to the message
    }
  ],
  "folderExtras": [                               // at most 8; files the app's programs create in $INSTDIR
    { "path": "cache", "kind": "dir", "onUpdate": "keep", "why": "…" }     // removed on a real uninstall only
  ]
}
```

Validation (each a typed error naming the field and the fix):

- `program` of every graceful stop and every service is a program of the bundle (`mainBinaryName` or `externalBin`, with
  Tauri's installed name), so nothing outside the app's own files can be declared.
- A graceful-stop program is in the stop list (not excluded): a stop that hangs past its timeout is then ended by `stop-all`.
- A service `registeredBy: "app"` has its program in `stopExclusions`: a service kept across an update must not run from the
  install folder, whose files the update replaces (Dyno's helper runs from its own protected copy). An installer-registered
  service may run from `$INSTDIR`; it is removed before anything is stopped (§2).
- `register` and `explain` only for `registeredBy: "installer"`; `refresh` only for `"app"`.
- Verbs are 1 to 4 words, each `^[a-z][a-z0-9-]{0,31}$`: no paths, quotes, `$`, or variables can reach a command line.
  Service names match `^[A-Za-z][A-Za-z0-9_.-]{0,79}$` and are unique; programs pass ADR 0053's `valid_name`.
- `timeoutMs` is required on every program run, from 1 000 to 300 000 (an installer must finish, ADR 0053 §3; ADR 0031's
  "no wall clocks" is about a job's progress, and these are bounded external calls whose expiry has a defined meaning, §2).
- Texts (`label`, `failedText`, `leftBehind`, `reason`, `why`) are 1 to 400 characters, one line, no control characters;
  the generator escapes them for NSIS (`$` to `$$`, `"` to `$\"`), so a text can never expand a variable or end a string.
- `folderExtras.path` is relative to `$INSTDIR`, one to three components of `[A-Za-z0-9._-]`, no `..`, no leading `.`,
  not a name the bundle or the include owns (`updates`, `paddock-setup.exe`, `uninstall.exe`, a bundle program). Elevated
  deletion stays inside the checked install folder: nothing under `%ProgramData%` or a profile is ever declared (ADR 0053
  §2.5). Data elsewhere is the service verb's job (it runs in Rust, with its own checks).

The schema is closed on purpose: anything an app needs that it cannot declare is a change to this record and to the include,
proven by the harness, not a line in the app's hooks.

### 2. What is generated, and where the logic lives

Two files change; the hooks file becomes two lines.

- **`paddock-per-machine.generated.nsh` (per app, generated, data only).** The generator (moved from
  `paddock_service::per_machine` to `paddock_setup::generate`, next to the include it feeds) writes `!define`s and nothing
  else: `PADDOCK_DECLARATION 2`, `PADDOCK_STOP_IMAGES`, per graceful stop `PADDOCK_GS_<n>_PROGRAM`, `_VERB`, `_TIMEOUT`, per
  service `PADDOCK_SVC_<n>_NAME`, `_LABEL`, `_PROGRAM`, `_BY` (`INSTALLER` or `APP`), the verbs and timeouts, `_FALLBACK`,
  `_LEFT_BEHIND`, `_EXPLAIN_*` or `_FAILED_TEXT`, per extra `PADDOCK_EXTRA_<n>_PATH`, `_KIND`, `_ON_UPDATE`, and
  `PADDOCK_DECLARATION_SHA256` (of the canonical JSON) for the artifact gate.
- **`paddock-per-machine.nsh` (the include, version 9).** It holds every line of NSIS logic, including the four
  `NSIS_HOOK_*` macros, defined when `PADDOCK_DECLARATION` is 2. The steps are unrolled at compile time over the fixed slots
  (`!ifdef PADDOCK_SVC_1_NAME` … `_4_`), so one hand-written, harness-tested implementation serves every app; the generated
  data only switches its branches on and off. `VIAddVersionKey "PaddockDeclaration"` carries the hash.
- **`installer-hooks.nsh` (per app).** Comments, then exactly:

  ```nsis
  !include "${__FILEDIR__}\paddock-per-machine.generated.nsh"
  !include "${__FILEDIR__}\paddock-per-machine.nsh"
  ```

  A hooks file that defines an `NSIS_HOOK_*` macro of its own fails `makensis` ("macro already defined") as well as C91.
  App-specific text lives in the declaration (`label`, `failedText`, `leftBehind`), so the hooks file needs no app text
  either (the direction allowed it): it holds no text at all.

Data-only generation was chosen over generating NSIS code per app: with code generation, each app's generated text is a
different program, and the harness would prove the templates, not the code that ships. Here the shipped NSIS is
byte-identical across apps (the sync test already enforces that), and the harness builds it with fixture declarations that
switch on every branch (§4).

**The generated steps, in order.** "Accepted" is `$PaddockDirAccepted` (below); "run" is `_PADDOCK_RUN_APP`, the one macro
that runs a program of `$INSTDIR`.

| Hook | Steps |
| --- | --- |
| `NSIS_HOOK_PREINSTALL` | `PADDOCK_INSTALLING`; `PADDOCK_CHECK_DIR` (sets accepted, or aborts 41); `PADDOCK_GUARD_PER_USER`; each graceful stop; each installer-registered service: run `unregister`, and on a non-zero or `timeout` result ask `_PADDOCK_SVC_REGISTERED`: still registered aborts with level 43 and the generated message, gone (1060) continues; `PADDOCK_STOP_ALL`; each `folderExtras` entry with `onUpdate: "remove"` (the new installer removes them, because on an upgrade the uninstaller that runs is the old version's, with the old list) |
| `NSIS_HOOK_POSTINSTALL` | `PADDOCK_INSTALL_SETUP`; `PADDOCK_CLEAN_RUN_VALUES`; each installer-registered service: run `register`, on failure the `explain` verb's first stdout line (or `failedText`) in a message (silent: details and `setup.log` only), never an abort; each app-registered service with `refresh`, when `_PADDOCK_SVC_REGISTERED`: run `refresh`, a failure is a note in the details and in `setup.log` through `paddock-setup note`, never an abort; `PADDOCK_FINISH_INSTALL` |
| `NSIS_HOOK_PREUNINSTALL` | `PADDOCK_INSTALLING` (the trust decision; untrusted ends here with level 45); `PADDOCK_REINSTALL_AS_UPDATE`; `_PADDOCK_ACCEPT_DIR` (asks `check-dir` once, only when trusted and the tool is there); each graceful stop, when accepted; each installer-registered service: `_PADDOCK_SVC_REMOVE`; `PADDOCK_STOP_ALL`; on a real uninstall only (`$UpdateMode <> 1`): each app-registered service `_PADDOCK_SVC_REMOVE`, then every `folderExtras` entry; `PADDOCK_CLEAN_RUN_VALUES`; `PADDOCK_REMOVE_PROGRAM_FILES` |
| `NSIS_HOOK_POSTUNINSTALL` | `PADDOCK_FINISH_UNINSTALL` |

`_PADDOCK_SVC_REMOVE` (uninstaller): when not `_PADDOCK_SVC_REGISTERED`, nothing; when accepted, run `unregister`, and on a
failure with the service still registered, a note and (not silent) "<label> could not be removed (code n). Remove it in
Windows Services." (an uninstaller never aborts: it must stay usable); when trusted and the tool is missing and `fallback` is
`sc-delete`, `"$SYSDIR\sc.exe" stop <name>` and `delete <name>` and the generated message with `leftBehind`; otherwise a
note "not removed: $INSTDIR is not an accepted install folder" and nothing runs.

**The shared macros the generated steps rest on:**

- `$PaddockDirAccepted` is set exactly once per run: in the installer by `PADDOCK_CHECK_DIR` (which aborts when refused, so
  it is 1 whenever an app step runs); in the uninstaller by `_PADDOCK_ACCEPT_DIR` (1 only when `$PaddockTrusted` is 1,
  `$PaddockHaveSetup` is 1 and `check-dir` exits 0). It replaces the hand-written `$DynoDirOk` and the per-hook
  `PADDOCK_DIR_ACCEPTED` calls; `PADDOCK_DIR_ACCEPTED` stays as the internal step.
- `_PADDOCK_RUN_APP _PROGRAM _VERB _TIMEOUT` refuses at run time unless `$PaddockDirAccepted` is 1 (a note, `$0` =
  `notaccepted`), checks the file exists, runs `nsExec::ExecToLog /TIMEOUT=<t> '"$INSTDIR\<program>" <verb>'`, and writes the
  line, the exit code (or `timeout`) and its meaning to the details. Only generated code calls it; the program name and verb
  come from the generated `!define`s, never from a register.
- `_PADDOCK_SVC_REGISTERED _NAME` is the one answer to "is the service registered": `"$SYSDIR\sc.exe" query <name>`; exit
  1060 is "not registered", any other code (0, or an error such as 5) counts as registered, so a doubtful answer never lets
  an install continue over a live service and never skips a removal. It replaces Dyno's `ReadRegStr ... ImagePath` and
  Pitwall's inline `sc query`.

### 3. The safety invariants

Each is a property of the include for every valid declaration. The harness case that proves it is named in §4.

| # | Invariant | Proven by |
| --- | --- | --- |
| I1 | **Trust before anything.** An uninstaller whose `$INSTDIR` is not the folder HKLM registers, directly under `$PROGRAMFILES64` in the accepted spelling, ends in `PADDOCK_INSTALLING` with level 45: no program of that folder runs (not `paddock-setup.exe`, no graceful stop, no service verb) and nothing is deleted. | P, P2, P3 |
| I2 | **`PADDOCK_DIR_ACCEPTED` gates every app program.** No graceful stop, `register`, `refresh` or `unregister` runs unless `$PaddockDirAccepted` is 1. The installer aborts at 41 before any of them; the uninstaller with a refused folder runs none of them and goes on. | B, Q2 (per fixture) |
| I3 | **Level 44 (TOOL_MISSING).** An installer whose `paddock-setup.exe` is gone never reports success. An uninstaller in a trusted folder without the tool runs no program of the folder and removes each declared service with `sc` when its `fallback` is `sc-delete`, saying so; with `none` it leaves it and says so. | M1 to M5, S3, S4 |
| I4 | **Level 45 (UNTRUSTED)** as I1, also when the copied uninstaller sits next to planted copies of every declared program. | P2 |
| I5 | **Update versus uninstall.** Installer-registered services are removed before files change on install, update, reinstall and uninstall, and registered again in POSTINSTALL. App-registered services are kept on update and on a reinstall (an in-place run, ADR 0053 §2.4), refreshed when declared and registered, and removed only on a real uninstall. `folderExtras` go only on a real uninstall unless `onUpdate: "remove"`. | E2, G2, F2, M5 |
| I6 | **A failed removal aborts the install only while the service is still registered.** An `unregister` that fails (non-zero or `timeout`) aborts with level 43 before `stop-all` and before any file is copied when `sc query` does not answer 1060; it continues when it does. | S1, S2, S5 |
| I7 | **Order.** Graceful stops, then installer-registered removals, then `stop-all`; app-registered removals after `stop-all`; `register` and `refresh` only after the files and `paddock-setup.exe` are installed. | call order in every case's stub log |
| I8 | **Every program run is bounded** by its declared timeout, and `timeout` has the meaning of a failure. | S5, GS1 |
| I9 | **A failed registration or refresh never fails the install,** and its reason reaches the user (message) or the log (silent). | S6, S7 |
| I10 | **Declared text cannot become code.** A text with `$`, quotes and backticks compiles and is shown literally. | generator unit tests; compile test C2 |
| I11 | **No hand-written hooks.** A hooks file that defines an `NSIS_HOOK_*` macro, or calls anything but the two includes, does not build (`makensis`) and fails C91. | compile test C1; C91 tests |

### 4. The harness proves each invariant once

`crates/paddock-setup/nsis/test/` keeps its shape (a minimal installer with Tauri's hook points, the real include, a stub
`paddock-setup.exe`), and gains:

- **Fixture declarations** in `test/declarations/`: `plain` (no services, no graceful stop: Spotter's and Debrief's future
  shape), `installer-service` (Pitwall's and Pitcrew's shape), `app-service` (Dyno's helper shape, with `refresh` and a
  `leftBehind`), `full` (two graceful stops, one service of each kind, `fallback: none` on one, a `folderExtras` entry of each
  `onUpdate`). `build.sh` runs the **real generator** (`cargo run -p paddock-setup --example generate`) over each fixture and
  builds `harness-<fixture>.exe`; no generated file is typed by hand any more.
- **A stub service program**, `stub-server.nsi` extended (it stands for every declared program): it logs `SERVER <args>`,
  and per verb `PADDOCK_STUB_SERVER_EXIT_<VERB>` sets its exit code, `..._SLEEP_<VERB>` makes it outlast a timeout, and
  `..._DELETE_<VERB>=1` makes it delete the stub service with `sc delete` before exiting (a verb that fails after removing).
  `..._OUT_<VERB>` sets stdout (the `explain` words).
- **A real stub service.** Before the service cases, the runner registers `PaddockHarnessSvc` (`sc create ... start=
  demand`, binary = the installed stub, never started), so `sc query` answers 0 and the `sc` fallback really deletes; after
  each case it removes it. Only a service the harness created, in the test VM (ER-14). A probe first shows the runner can see
  it (`sc query` 0 after create, 1060 after delete), or the run reports `check broken` (ER-33).
- **Compile tests** that need no VM and run on macOS and Linux in CI wherever `makensis` is present (`build.sh --compile-only`):
  C1, a hooks file with an extra `!macro NSIS_HOOK_PREINSTALL` must fail to build; C2, the `full` fixture with `$`, quotes
  and backticks in every text builds and the strings in the compiled script are escaped (read back with `makensis
  -PPO`, the preprocessed output).

Cases (new or widened; A to U, P, Q, R and M1 to M5 stay):

| Case | Fixture | What it does | Passes when |
| --- | --- | --- | --- |
| E2 | each | update over an install (`/UPDATE`) | installer-registered: `SERVER uninstall` before `stop-all`, `SERVER install` after `clean-run`; app-registered: no `uninstall`, `refresh` only while the stub service is registered |
| G2 | each | in-place reinstall (`_?=`, same folder) | as E2 for the uninstaller half: app-registered service untouched, extras with `onUpdate: keep` still there |
| F2 | each | real uninstall | every declared service removed in the declared order, extras gone, the stub service gone (`sc query` 1060) |
| B | each | installer with `check-dir` refused | level 41, no `SERVER` line |
| Q2 | each | uninstaller with `check-dir` refused | no `SERVER` line, no `sc` call, the note, the uninstall goes on |
| P2 | `full` | copied `uninstall.exe` next to planted copies of the tool and every declared program | level 45, no tool or `SERVER` line, HKLM key, files and the stub service unchanged |
| P3 | `installer-service` | HKLM uninstall key deleted by hand, then the real folder's `uninstall.exe` | level 45 with "run the installer again, then uninstall"; no `SERVER` line; the installer run again restores the key and the uninstall then completes |
| S1 | `installer-service` | update, `unregister` exits 1, stub service still registered | level 43, the generated message, no `stop-all`, no file replaced |
| S2 | `installer-service` | update, `unregister` exits 1 after deleting the stub service | `sc query` 1060 in the details, install completes |
| S3 | `installer-service`, `app-service` | real uninstall, trusted folder, tool removed | no `SERVER` line, `sc stop` and `sc delete` logged, stub service gone, the `leftBehind` text in the message line |
| S4 | `full` | as S3 for the service with `fallback: none` | no `sc` call for it, the "not removed" note, stub service still registered |
| S5 | `installer-service` | update, `unregister` sleeps past its timeout with the service registered | `timeout` in the details, level 43; the measured time is within the timeout plus 5 s |
| S6 | `installer-service` | `register` exits 1, `explain` prints a line | install exit 0, the line in the details (silent run) |
| S7 | `app-service` | update with the stub service registered, `refresh` exits 1 | install exit 0, `note` with the refresh failure in `setup.log` |
| GS1 | `full` | a graceful stop that sleeps past its timeout | `timeout` logged, `stop-all` still runs, install completes |
| M5 | `app-service` | in-place reinstall with the tool dropped | update mode by the folder compare, the service untouched |

**One end-to-end run per include version** inside Tauri's real template: `run-dyno-e2e.ps1` becomes `run-app-e2e.ps1
-Installer <setup.exe> -Product <name>`, run on the release candidate of the first app that ships the new include (Pitwall
1.1), not once per app: the template is the same for every app, and the include is byte-identical.

The harness is registered in `docs/harnesses.toml` (ADR 0042) with its home on the line that lands this record, so later
changes to it come from one place.

### 5. What still needs per-app testing

The harness proves the include runs the declared programs at the right time, under the right gates, and reacts correctly to
their exit codes. It cannot prove what an app's own programs do. Each app still tests, as runtime behaviour:

- **Its verbs do what the declaration claims**: `install`, `uninstall`, `refresh`, `stop` (exit 0 only when done; non-zero
  when the service is still registered; idempotent removal of a missing service). These are Rust and are unit-tested over
  the app's SCM port; the real-SCM rows run in the VM by the command line, without an installer (Pitwall PW-4, PW-7).
- **Service behaviour around the installer**: whether recovery restarts the program between `unregister` and `stop-all`
  (Pitwall PW-1), sessions after an upgrade (PW-2), Pitcrew's busy runner (below).
- **The `survivesForcedEnd` statements** (ADR 0053 amendment 1): a review of the app's code, as today.
- **The app's runtime**: UI, readings, start at login, the updater panel.
- **One installer smoke per release** (install, update over itself, uninstall, in the VM) as the rule-54 evidence that the
  built artifact is the one tested; it does not repeat the include's cases.

What no longer needs a per-app VM round: the trust decision, the folder gate, levels 41 to 45, the update and uninstall
distinction, the 1060 abort rule, the `sc` fallback and the step order. Pitwall's PW-3, PW-6 and PW-9 become harness cases
(P2, S3 and P3) and leave its VM table.

### 6. The static per-app check: C91, extended

C91 keeps what it checks (ADR 0053 §8) and adds, for every app with `installMode: perMachine`:

1. **The hooks file is two includes.** After removing blank lines and `;` comments, `installer-hooks.nsh` is exactly the two
   `!include` lines, in that order. Anything else (`nsExec`, `Exec`, `ExecWait`, `ExecShell`, `System::Call`, `!macro`,
   `File`, `Delete`, `RMDir`, `WriteReg*`, `nsis_tauri_utils`, a `!define`) is a finding naming the line, with the fix "declare
   it in windows/per-machine.json; a need the schema does not cover is a change to ADR 0059 and the include". This replaces
   C91's current list of required macro calls, which the include now makes itself.
2. **The declaration is version 2 and valid.** C91 reads `version` and fails on 1 or a missing file; the full validation
   (§1, the bundle cross-checks) is `paddock_setup::generate::check_app`, the test that already runs with C91
   (`scripts/affected.mjs` `PER_MACHINE_INPUT`, which gains `crates/paddock-setup/schema/` and the generator's sources). A
   JSON Schema validator in C91 itself is not added: conformance has no dependencies, and the typed parser is the stricter
   check (ADR 0022 moves such checks to Rust).
3. **The generated file and the schema are current**, and the include copy equals the canonical one (as today).
4. **A declared service matches the app's own service code**: each adopting app keeps one unit test in the crate that owns
   the service's Windows name (Pitwall's server or service crate, `pitcrew-core`, Dyno's helper crate) that reads
   `per-machine.json` and compares `name` and `program` with its `ServiceSpec` or service-name constant. C91 requires that the test exists (a file under the app that
   names `per-machine.json` in a `#[test]`); the comparison itself is the test's.

The ADR 0039 artifact gate adds: the built installer's `PaddockInclude` is 9 and its `PaddockDeclaration` equals the hash of
the app's declaration at the tagged commit.

### 7. Migration

The include's copy in every adopting app must equal the canonical one (the sync test), so the include change and the
adoption by Dyno and Pitwall land **in one change** (rules 42 and 43); there is no window in which one app runs v9 and the
other v8. The work branches from the Pitwall 1.1 line (`pitwall/1.1-integrate`), which holds include v8, Dyno's trust
changes and Pitwall's adoption, and lands with it. Family ADR 0022 §G.7 (no `crates/` change inside another app's release
window) does not block it: the change is part of Pitwall 1.1's own window, and Dyno's next release (1.1.1) carries it.

**Dyno** (declaration; hooks file to two lines; behaviour changes listed for its release notes):

```jsonc
"gracefulStop": [{ "program": "dyno-agent.exe", "verb": ["stop"], "timeoutMs": 30000 }],
"services": [{
  "name": "DynoHelper", "label": "the Dyno helper service", "program": "dyno-helper.exe", "registeredBy": "app",
  "refresh":    { "verb": ["refresh"],   "timeoutMs": 120000 },
  "unregister": { "verb": ["uninstall"], "timeoutMs": 120000 },
  "fallback":   { "kind": "sc-delete", "leftBehind": "Its folder, Dyno Helper in Program Files, and its data were not removed; delete them by hand if you do not need them." }
}]
```

Changes: `dyno-agent stop` gets a 30 s bound (the hooks' accepted "no timeout" limit goes; a hung agent is then ended by
`stop-all`); "registered" is `sc query` instead of the `ImagePath` read; the refresh note goes to `setup.log` through
`paddock-setup note` instead of a hand-written `FileOpen`; a helper `uninstall` that fails with the service still registered
now shows a message instead of only a detail line. Its harness e2e (`run-app-e2e.ps1`) is rerun on the 1.1.1 candidate.

**Pitwall** (declaration; hooks file to two lines):

```jsonc
"services": [{
  "name": "Pitwall", "label": "the Pitwall service", "program": "pitwall-server.exe", "registeredBy": "installer",
  "register":   { "verb": ["install"], "timeoutMs": 120000, "explain": { "verb": ["check-install"], "timeoutMs": 30000 } },
  "unregister": { "verb": ["uninstall"], "timeoutMs": 120000 },
  "fallback":   { "kind": "sc-delete" }
}]
```

Changes: the verbs get bounds (today unbounded); an uninstall whose service removal fails with the service still
registered shows a message; ADR 0024's "Pitwall's own step" row points here, and PW-3, PW-6 and PW-9 move to the harness.

**Pitcrew** (later, in its 1.1 adoption of ADR 0053, which already budgets 2 to 3 days): the same declaration as Pitwall
with `"name": "Pitcrew"`, `"program": "pitcrew-service.exe"`, no `explain` (it has no `check-install` verb) and
`"failedText": "Open Pitcrew, Settings, Service to try again."`. Its `uninstall` waits up to a minute for runners to finish a
job, so its `unregister` bound is 120 s. Open for its adoption: ADR 0003's "a busy runner keeps running and is adopted by
the new service" must hold under `stop-all`; the runner programs are not programs of the bundle, so they
are not in the stop list, but `pitcrew-service ctrl-break` runs from `$INSTDIR` and is; the adoption states why ending it
is harmless or excludes it with a reason. Pitcrew's 1.0.x uninstallers run before any new hook (as Pitwall's 1.0.x, ADR 0024
PW-5); its release notes say to install 1.1 once by hand.

**New apps**: `scripts/new-app.mjs` writes a version-2 declaration with no services, and the two-line hooks file.

## Consequences

- An app's installer behaviour is data: adding a service, a helper or a graceful stop is a declaration the schema checks,
  not NSIS that a reviewer has to read for the five decisions.
- A fix to the trust decision, the folder gate, the abort rule or the fallback lands once, in the include, and the harness
  proves it once; no app can miss it (rule 55), because no app has steps of its own.
- Per-app Windows VM work shrinks to the app's verbs, its service behaviour and one installer smoke per release.
- The schema is closed: a new kind of step (for example a second fallback kind, an `onUpdate` for services) costs an
  amendment, an include change and harness cases. That is the intended price.
- Every program run gets a bound; a hang in a verb now ends with a defined outcome instead of an installer that has to be
  closed by hand.
- Two behaviour changes reach users: messages where a failed service removal in an uninstaller was only a detail line, and
  Dyno's bounded agent stop. Both go into the release notes.

## Open questions for the reviewer and the user

| # | Question | Proposal |
| --- | --- | --- |
| Q1 | Does `nsExec /TIMEOUT` end the child at the timeout, or leave it running? (Its source suggests it ends it; not measured.) | Case S5 measures it in W3. If it leaves the child, `_PADDOCK_RUN_APP` runs through `paddock-setup run --timeout` (a held job object ends the tree) and the bound holds either way; the harness case stays. |
| Q2 | An uninstaller whose folder is trusted (the registered one, under Program Files) but refused by `check-dir` (owner or reparse point changed) runs nothing today and leaves the service. Should the `sc` fallback apply there too? `sc.exe` from System32 runs nothing of the folder. | Keep today's behaviour (leave it, say so) in v9; decide separately, since it changes a reviewed security decision. |
| Q3 | Pitwall's in-place reinstall removes and re-registers its service; a cancel between the halves leaves it unregistered until the installer runs again (ADR 0053 §2.4's half-removed app). | Accepted as in 0053; S-cases do not change it. |

## Work packages

Agent-days for one agent. All shared code is in `crates/paddock-setup` (and the move out of `paddock-service`); the
integrating agent changes `crates/Cargo.toml` (one `schemars` line) and `docs/harnesses.toml`.

| WP | Package | Owned paths | Depends on | Days |
| --- | --- | --- | --- | --- |
| W1 | `paddock_setup::declaration` (types, validation, typed errors) and `paddock_setup::generate` (moved from `paddock_service::per_machine`: data-only generated file, schema output, `check_app`, `write_app`, `adopting_apps`); unit tests for every validation rule and the escaping; the `generate` example | `crates/paddock-setup/src/{declaration,generate}.rs`, `crates/paddock-setup/schema/`, `crates/paddock-setup/examples/generate.rs`, `crates/paddock-service/src/per_machine*` (removed) | – | 1–1.5 |
| W2 | Include v9: the four hooks defined by the include, the slots, `_PADDOCK_RUN_APP`, `_PADDOCK_SVC_REGISTERED`, `_PADDOCK_SVC_REMOVE`, `_PADDOCK_ACCEPT_DIR`, the generated messages, the version keys; compile tests C1 and C2 in `build.sh --compile-only` | `crates/paddock-setup/nsis/paddock-per-machine.nsh`, `crates/paddock-setup/nsis/test/build.sh` | W1 | 1–1.5 |
| W3 | Harness: the four fixtures, the stub server's verb controls, the real stub service with its probe, cases E2 to GS1; run on the Windows VM, results recorded in `docs/research/` with the run's log; `run-app-e2e.ps1` | `crates/paddock-setup/nsis/test/` | W2 | 1.5–2 |
| W4 | C91: the hooks-file grammar, the version, the service-test presence; its tests; `PER_MACHINE_INPUT` widened | `conformance/src/checks/C91-per-machine.mjs`, `conformance/test/c91-per-machine.test.mjs`, `scripts/affected.mjs` (one pattern) | W1 | 0.5 |
| W5 | Dyno: declaration, hooks file, the service-name test in Dyno's helper crate, release notes; the rule-54 dispatch of `dyno-release.yml`; the e2e rerun | `apps/dyno/app/src-tauri/windows/`, `apps/dyno/crates/dyno-helper*` (one test), `apps/dyno/CHANGELOG.md` | W2, W4 | 0.5–0.75 |
| W6 | Pitwall: declaration, hooks file, the service-name test in Pitwall's server or service crate, ADR 0024's row and VM table, release notes; the rule-54 dispatch; the e2e on the 1.1 candidate | `apps/pitwall/app/src-tauri/windows/`, `apps/pitwall/crates/pitwall-{server,service}` (one test), `apps/pitwall/docs/adr/0024-*`, `apps/pitwall/CHANGELOG.md` | W2, W4 | 0.5–0.75 |
| W7 | Docs: ADR 0053 pointer (§2 and §8 refer here), the testing manual's installer page, `new-app.mjs` and the template, a lessons entry, the harness register line | `docs/adr/0053-*` (a pointer line), `docs/lessons.md`, `scripts/new-app.mjs`, `templates/` | W2 | 0.25–0.5 |
| later | Pitcrew: the declaration and its test inside its ADR 0053 adoption | `apps/pitcrew/...` | W1–W4 | 0.25 (of its 2–3) |

**In all 5.25 to 7.5 agent-days** (Dyno and Pitwall included), against the 3 to 5 noted on 2026-11-08: the difference is
the closed schema with its validation and generated JSON Schema (W1), four harness fixtures instead of one (W3), and the
two behaviour changes that need release notes. Order: W1, then W2 and W4 beside each other, then W3, then W5 and W6 together
with W7. With two agents the critical path W1, W2, W3, W6 is about **4 to 5.5 working days**, plus one VM session for W3
and the two rule-54 dispatches. Pitwall 1.1 cannot tag before W6 lands.
