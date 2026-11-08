# 59. Installer glue as declarations: apps declare, the shared include generates

- Status: Proposed (2026-11-08), revision 2. The user approved the direction on 2026-11-08 and placed it **before Pitwall
  1.1 ships**: no app keeps hand-written NSIS install, service or uninstall steps; each per-machine app declares what it has
  in `windows/per-machine.json`, the shared include generates every step from that declaration, the shared harness proves
  the generated steps once on a Windows VM with a stub service, and each app gets a static check.
- Revision 2 (2026-11-08): answers the independent review of `8491ea588` ("changes needed"). P1-1: declared texts are an
  allowlist that needs no escaping, and no declared text reaches a command line (§1, I10, case C2r). P1-2: a missing program
  has a defined outcome per hook (§2.3) and harness cases S9, S10. P2: every app program runs through `paddock-setup run`
  with a job object, which re-checks the folder and logs to `setup.log` (§2.2; Q1 decided); `folderExtras` dropped; harness
  observability (the stub logs its own image, controls keyed by image and verb, two real stub services, a positive control for
  each negative case); C1 and C2 as a required CI job; sentence-case messages; Pitwall's explain rule kept; more validation
  and a defined hash; timeouts halfway through a verb and the verbs' idempotence contract (§5); a generated Rust constant for
  the service name (§6); an artifact-gate work package; what happens without `docs/harnesses.toml`; the duplicate harness
  case R renamed; the landing order. Q2 decided as the reviewer proposed (the `sc` fallback also for a trusted folder that
  `check-dir` refuses, marked for security re-review); Q3 accepted. Re-estimated.
- Date: 2026-11-08
- Related:
  - Family ADR [0053](0053-per-machine-windows-installs.md) (per-machine Windows installs; its amendment 1 on
    `docs/dyno-adr-0053-amend1`): this record changes how the app's own steps of §2 ("the app's own steps go between them")
    are written, adds `paddock-setup run`, and extends §8's check C91. Everything else in 0053 stands.
  - Pitwall ADR 0024 (per-machine Windows install, on `pitwall/1.1-b6c`): its "Pitwall's own step" row and the VM rows PW-3,
    PW-6 and PW-9 move into the shared harness.
  - ER-21 / rule 40 (a recurring kind of bug needs a design rule), rule 55 and ADR
    [0034](0034-one-implementation-per-mechanism.md) (one implementation per mechanism), rules 42 and 43 (share first; a
    shared change updates its users in the same change), rule 54 (a release-path change builds the installers before it
    lands), ER-14 (VMs only), ER-23 (independent review), ER-27 (logs explain every failure), ER-33 (a check counts only when
    its probe has shown it can see the thing), ADR [0033](0033-processes-by-identity-never-by-a-bare-pid.md) (processes by
    identity; a job object holds the tree), ADR [0039](0039-release-gates-inspect-the-artifacts.md) (artifact gates), ADR
    [0042](0042-one-owner-per-harness-and-small-batches.md) (one owner per harness), ADR
    [0031](0031-patience-not-wall-clocks.md) (bounded waits).
- Number: 0059. The highest family ADR on any branch is 0058 (`git ls-tree --name-only <ref> docs/adr/` over every
  `refs/remotes/origin/*` and `refs/heads/*`, 2026-11-08). No new check number: C91 is extended (§6); the highest check
  named anywhere is C99 (`origin/pitwall/1.1-b5`).
- Sources read (2026-11-08): the include on `origin/main` (`INCLUDE_VERSION "4"`) and on `pitwall/1.1-integrate`
  (`7d4ba0f72`, `INCLUDE_VERSION "8"`, the uninstaller's trust decision, level 45, `PADDOCK_DIR_ACCEPTED`); Dyno's and
  Pitwall's `installer-hooks.nsh` and `per-machine.json` on that branch; `crates/paddock-service/src/per_machine.rs` (the
  generator); `crates/paddock-setup/nsis/test/` (the harness: cases A to U, P, Q, two cases both named R, M1 to M5); C91 on
  `origin/main`; Pitcrew's `installer-hooks.nsh` and `apps/pitcrew/crates/pitcrew-service/src/platform/windows.rs` on
  `origin/main`.

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
| Pitwall | none | `Pitwall`, `pitwall-server.exe` (in the stop list), LocalSystem, restart on failure | the installer | `uninstall` before `stop-all` (abort level 43 while still registered), `install` in POSTINSTALL (failure: `check-install`'s words, or `install`'s own when the check passes, in a message; never fatal) | `uninstall` before `stop-all`; `sc` fallback with the tool missing; nothing when the folder is not accepted |
| Pitcrew | none | `Pitcrew`, `pitcrew-service.exe` (in the bundle), LocalSystem, restart on failure | the installer | `uninstall` in PREINSTALL with no folder check and no failure check, `install` in POSTINSTALL (failure: a fixed message) | `uninstall`, unchecked |

Two kinds of service are enough: one **the installer registers** (removed before files change, registered again after,
removed on uninstall: Pitwall, Pitcrew), and one **the app registers** at the user's request (kept across updates and
reinstalls, optionally refreshed, removed only on a real uninstall: Dyno's helper). No app keeps program-folder files of its
own beyond what the include already handles (`updates\`, `paddock-setup.exe`), so the declaration has no entry for them
(YAGNI; revision 1's `folderExtras` is dropped, and a need for one is an amendment).

## Decision

### 1. The declaration: `windows/per-machine.json`, version 2

One file per app, beside the hooks file. Its **one definition is a Rust type**, `paddock_setup::declaration::Declaration`
(serde, `deny_unknown_fields`), parsed once at the edge (parse, don't validate) and validated by
`Declaration::validate(&Bundle)`, where `Bundle` is what the generator reads from `tauri.conf.json` and
`tauri.windows.conf.json` (`mainBinaryName`, `externalBin`, `productName`) together with the stop list it derives. A JSON
Schema, `crates/paddock-setup/schema/per-machine.schema.json`, is **generated** from the type (`schemars`) by the same sync
test that writes the generated include, and checked current; each app's file names it in `$schema` for editors. The Rust type
and its validation are the schema in force; the JSON Schema is its published form (one declared definition, a generator keeps
them equal).

```jsonc
{
  "$schema": "../../../../../crates/paddock-setup/schema/per-machine.schema.json",   // optional, accepted
  "$comment": "…",                                // optional, accepted, never generated into anything
  "version": 2,                                   // required
  "stopExclusions":   [{ "image": "…exe", "reason": "…" }],          // unchanged from ADR 0053 §2.1
  "survivesForcedEnd": [{ "image": "…exe", "why": "…" }],            // unchanged from ADR 0053 amendment 1 §3
  "gracefulStop": [                               // at most 4; run before stop-all, in this order
    { "program": "dyno-agent.exe", "verb": ["stop"], "timeoutMs": 30000 }
  ],
  "services": [                                   // at most 4
    {
      "name": "Pitwall",                          // the SCM service name
      "label": "the Pitwall service",             // used inside generated sentences
      "program": "pitwall-server.exe",            // a program of the bundle, run from $INSTDIR
      "registeredBy": "installer",                // "installer" | "app"
      "register":   { "verb": ["install"], "timeoutMs": 120000,      // installer only
                      "explain": { "verb": ["check-install"], "timeoutMs": 30000 },   // optional (§2.3)
                      "failedText": "Open Pitwall, Settings, Service to try again." }, // optional, used without explain
      "refresh":    { "verb": ["refresh"], "timeoutMs": 120000 },    // app only, optional
      "unregister": { "verb": ["uninstall"], "timeoutMs": 120000 },
      "fallback":   { "kind": "sc-delete",                           // "sc-delete" | "none"
                      "leftBehind": "Its folder and its data were not removed; delete them by hand if you do not need them." }
    }
  ]
}
```

Validation (each a typed error naming the field and the fix):

- `version` must be 2. A file without it or with 1 (today's) is refused with "windows/per-machine.json is version 1: convert
  it to version 2 (ADR 0059)". `$schema` and `$comment` are accepted and ignored; any other unknown key is refused.
- `program` of every graceful stop and every service is a program of the bundle (`mainBinaryName` or `externalBin`, with
  Tauri's installed name), so nothing outside the app's own files can be declared; `paddock-setup.exe` and `uninstall.exe`
  are refused as programs (they belong to the include and the template).
- A graceful-stop program is in the stop list (not excluded): a stop that outlives its bound is ended by `run`'s job object,
  and anything it left is ended by `stop-all`.
- A service `registeredBy: "installer"` has its program **in the stop list**: the installer removes the service first, then
  `stop-all` ends whatever is left of its program in any session before files are replaced.
- A service `registeredBy: "app"` has its program in `stopExclusions`: a service kept across an update must not run from the
  install folder, whose files the update replaces (Dyno's helper runs from its own protected copy).
- `register` and `explain` only for `registeredBy: "installer"`; `refresh` only for `"app"`.
- Verbs are 1 to 4 words, each `^[a-z][a-z0-9-]{0,31}$`. Service names match `^[A-Za-z][A-Za-z0-9_.-]{0,79}$` and are
  unique; programs pass ADR 0053's `valid_name`. `timeoutMs` is an integer from 1 000 to 300 000, required on every program
  run (an installer must finish, ADR 0053 §3; these are bounded external calls whose expiry has a defined meaning, §2.3).
- **Texts that reach the installer** (`label`, `failedText`, `leftBehind`) are 1 to 200 characters from an allowlist: Unicode
  letters and digits, space, and `. , ; : ( ) ? ! / + = _ - ›`. No quotes of either kind, no backslash, no `$ % < > | & ^`,
  no backtick, no control characters. Such a text means the same in an NSIS string, in a `MessageBox` and on a Windows
  command line, so the generator copies it unchanged and **no escaping exists that could be wrong** (I10 by construction).
  "Settings › Apps" is written with `›`, as ADR 0053's messages already are.
- `reason` and `why` are free text (1 to 2 000 characters, no control characters); they are documentation, read by C91 and
  reviewers, and the generator never copies them into any generated file.

**The hash.** `PADDOCK_DECLARATION_SHA256` is the SHA-256 of the file's JSON value serialised by RFC 8785 (JSON
Canonicalization Scheme: keys sorted by UTF-16 code units, no whitespace, the declaration's integers as plain decimals),
with `$schema` and `$comment` removed first so editorial changes leave it alone. The generator and the artifact gate use
the same function (`paddock_setup::declaration::canonical_hash`; rule 55).

The schema is closed on purpose: anything an app needs that it cannot declare is a change to this record and to the include,
proven by the harness, not a line in the app's hooks.

### 2. What is generated, and where the logic lives

#### 2.1 Three files

- **`paddock-per-machine.generated.nsh` (per app, generated, data only).** The generator (moved from
  `paddock_service::per_machine` to `paddock_setup::generate`, next to the include it feeds) writes `!define`s and nothing
  else: `PADDOCK_DECLARATION 2`, `PADDOCK_STOP_IMAGES`, per graceful stop `PADDOCK_GS_<n>_PROGRAM`, `_VERB`, `_TIMEOUT`, per
  service `PADDOCK_SVC_<n>_NAME`, `_LABEL`, `_PROGRAM`, `_BY` (`INSTALLER` or `APP`), the verbs and timeouts, `_FALLBACK`,
  `_LEFT_BEHIND`, `_EXPLAIN_*` or `_FAILED_TEXT`, and `PADDOCK_DECLARATION_SHA256`. Every value is a program name, a verb, an
  integer, an enum word, a service name, a hash or an allowlisted text (§1).
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
  App-specific text lives in the declaration, so the hooks file holds no text at all.

Data-only generation was chosen over generating NSIS code per app: with code generation, each app's generated text is a
different program, and the harness would prove the templates, not the code that ships. Here the shipped NSIS is
byte-identical across apps (the sync test already enforces that), and the harness builds it with fixture declarations that
switch on every branch (§4).

#### 2.2 Every app program runs through `paddock-setup run` (Q1 decided)

No generated step starts a program of `$INSTDIR` with `nsExec` directly. The one macro that runs an app program,
`_PADDOCK_RUN_APP _PROGRAM _VERB _TIMEOUT`, first refuses unless `$PaddockDirAccepted` is 1 (`$0` = `notaccepted`, no call),
then calls (through `nsExec::ExecToStack`, from `$PLUGINSDIR` like every other `paddock-setup` call):

```text
paddock-setup run --dir "<$INSTDIR>" --program <program> --verb <word> [--verb <word> …] --timeout <ms>
```

Every argument is a checked folder, a `valid_name`, a verb word or an integer: no declared text and no register content is
on the command line. `run` (a new command of `paddock-setup`, `crates/paddock-setup/src/run.rs`):

1. **Re-checks the folder** with `check-dir`'s rule immediately before the start (the same function; ADR 0053 §2.2) and that
   `<dir>\<program>` is a regular file directly in it, not a reparse point. Refused: outcome `refused`; missing: `nofile`.
2. **Starts the program in a new job object** with `JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE`, created suspended and assigned before
   it runs, so every process it starts is in the job. Its standard output and error are read on two threads, each bounded
   (stdout 4 KiB kept, stderr's last 2 KiB kept).
3. **Waits on the process handle with the bound.** On expiry it terminates the job (the whole tree), waits for the job to be
   empty, and the outcome is `timeout`. The job handle is closed on every path, so even a crash of `run` ends the tree.
4. **Logs** to `$INSTDIR\updates\setup.log` through the checked log (ADR 0053 §2.5): the command (program and verbs; no user
   content exists in it), the start time, duration, exit code or outcome, and the stderr tail (ER-27).
5. **Answers** on its own stdout: the child's stdout (at most 512 bytes, one line per line, control characters dropped), then
   a last line `PADDOCK-RUN exit=<n>` or `PADDOCK-RUN timeout` / `nofile` / `refused`. It exits 0 when it could report;
   any other exit code is a failure of `run` itself (outcome `toolfailed`). The include reads the marker, never `run`'s exit
   code, so a program's own exit code can never be mistaken for one of `run`'s outcomes.

`_PADDOCK_RUN_APP` leaves the outcome in `$0` (`0`, another number, `timeout`, `nofile`, `refused`, `toolfailed`,
`notaccepted`) and the child's output in `$1`, and writes one line to the details. `run` replaces revision 1's direct
`nsExec /TIMEOUT` (whose handling of the child at expiry was not measured: Q1).

`_PADDOCK_SVC_REGISTERED _NAME` is the one answer to "is the service registered": `"$SYSDIR\sc.exe" query <name>` (a
System32 program; the name is regex-checked); exit 1060 is "not registered", any other code (0, or an error such as 5)
counts as registered, so a doubtful answer never lets an install continue over a live service and never skips a removal. It
replaces Dyno's `ReadRegStr ... ImagePath` and Pitwall's inline `sc query`.

`_PADDOCK_SC_REMOVE _N` is the one fallback: `"$SYSDIR\sc.exe" stop <name>`, then `delete <name>`, each exit code to the
details, and to `setup.log` through `paddock-setup note --dir … --event sc-delete --service <name> --code <n>` (fixed
wording chosen by the tool; only the regex-checked name and an integer on the command line) whenever the tool is there. With
the tool missing there is no safe writer, and the details say so ("not in setup.log: paddock-setup.exe is missing").

`$PaddockDirAccepted` is set exactly once per run: in the installer by `PADDOCK_CHECK_DIR` (which aborts at 41 when refused,
so it is 1 whenever an app step runs); in the uninstaller by `_PADDOCK_ACCEPT_DIR` (1 only when `$PaddockTrusted` is 1,
`$PaddockHaveSetup` is 1 and `check-dir` exits 0). It replaces the hand-written `$DynoDirOk` and the per-hook
`PADDOCK_DIR_ACCEPTED` calls. `run`'s own re-check (step 1) is the second gate, at the moment of the start.

#### 2.3 The generated steps and every outcome

| Hook | Steps |
| --- | --- |
| `NSIS_HOOK_PREINSTALL` | `PADDOCK_INSTALLING`; `PADDOCK_CHECK_DIR`; `PADDOCK_GUARD_PER_USER`; each graceful stop; each installer-registered service: `_PADDOCK_SVC_UNREGISTER_FOR_INSTALL`; `PADDOCK_STOP_ALL` |
| `NSIS_HOOK_POSTINSTALL` | `PADDOCK_INSTALL_SETUP`; `PADDOCK_CLEAN_RUN_VALUES`; each installer-registered service: `_PADDOCK_SVC_REGISTER`; each app-registered service with `refresh`, when registered: `_PADDOCK_SVC_REFRESH`; `PADDOCK_FINISH_INSTALL` |
| `NSIS_HOOK_PREUNINSTALL` | `PADDOCK_INSTALLING` (the trust decision; untrusted ends here with level 45); `PADDOCK_REINSTALL_AS_UPDATE`; `_PADDOCK_ACCEPT_DIR`; each graceful stop, when accepted; each installer-registered service: `_PADDOCK_SVC_REMOVE`; `PADDOCK_STOP_ALL`; on a real uninstall only (`$UpdateMode <> 1`): each app-registered service: `_PADDOCK_SVC_REMOVE`; `PADDOCK_CLEAN_RUN_VALUES`; `PADDOCK_REMOVE_PROGRAM_FILES` |
| `NSIS_HOOK_POSTUNINSTALL` | `PADDOCK_FINISH_UNINSTALL` |

Outcomes of each step (`n` is a non-zero exit code; "registered" is `_PADDOCK_SVC_REGISTERED` asked after the run):

| Step | `0` | `n`, `timeout`, `refused` or `toolfailed` | `nofile` (the program is missing) |
| --- | --- | --- | --- |
| Graceful stop | done | a detail line; go on (`stop-all` ends what is left) | a detail line; go on |
| `_PADDOCK_SVC_UNREGISTER_FOR_INSTALL` (installer) | done | still registered: abort, level 43, "Windows could not remove `<label>` (code n), so `<product>` cannot be updated safely. Remove the service in Windows Services, restart the PC, or uninstall `<product>` first, then run this installer again."; not registered (1060): go on | **not registered: go on. Still registered: go on** to `stop-all`, the file copy and `_PADDOCK_SVC_REGISTER`, whose verb replaces a registration that names this folder's program and refuses one that names another program (§5); the details say "the program to remove `<label>` is missing; it is registered again after the files are installed" |
| `_PADDOCK_SVC_REGISTER` | done | runs `explain` when declared: when `explain` exits 0 the message is `register`'s own output, otherwise `explain`'s output (today's Pitwall rule); without `explain`, `failedText`; the message is "Windows could not register `<label>`." plus that text (silent: details and `setup.log` only); never an abort | as `n`, with "the program is missing; run the installer again" |
| `_PADDOCK_SVC_REFRESH` | done | a note in the details and in `setup.log` (written by `run`); never an abort | as `n` |
| `_PADDOCK_SVC_REMOVE` (uninstaller), accepted folder | done | still registered: a detail line, `setup.log`, and (not silent) "Windows could not remove `<label>` (code n). Remove it in Windows Services."; an uninstaller never aborts (it must stay usable) | still registered: the `sc` fallback when `fallback` is `sc-delete` (the folder is accepted; `sc.exe` from System32 runs nothing of it), otherwise the message above |

`_PADDOCK_SVC_REMOVE` when the service is not registered does nothing. When the folder is **not accepted** it never runs a
program of the folder, and:

- trusted, the tool missing: the `sc` fallback when `fallback` is `sc-delete`, with the message "Windows removed `<label>`
  without the installer's checking tool, because the tool was missing." plus `leftBehind`; with `none` the message "Windows did not
  remove `<label>`. Remove it in Windows Services.";
- **trusted, the tool present, `check-dir` refused** (an owner or reparse point changed in the registered folder; Q2,
  decided): the same `sc` fallback and messages. A LocalSystem service whose program failed the folder check is a standing
  risk, and `sc.exe` from System32 runs nothing of the folder. **This branch is marked for explicit security re-review**
  before W2 lands;
- untrusted: unreachable, `PADDOCK_INSTALLING` has ended the run with level 45.

**A timeout halfway through a verb.** `run` ends the verb's whole tree at the bound, so the service can be left half
registered or half removed. The outcome is decided by the service's state, never by the verb's: after `unregister`,
`_PADDOCK_SVC_REGISTERED` decides as in the table (installer: abort while registered; uninstaller: message or fallback);
after `register`, the failure message is shown and the details record whether the service is registered. The verbs' contract
(§5) makes the next run of the same verb finish the job.

### 3. The safety invariants

Each is a property of the include for every valid declaration. The harness case that proves it is named in §4.

| # | Invariant | Proven by |
| --- | --- | --- |
| I1 | **Trust before anything.** An uninstaller whose `$INSTDIR` is not the folder HKLM registers, directly under `$PROGRAMFILES64` in the accepted spelling, ends in `PADDOCK_INSTALLING` with level 45: no program of that folder runs (not `paddock-setup.exe`, no graceful stop, no service verb) and nothing is deleted. | P, P2, P3 |
| I2 | **The folder gates every app program.** No graceful stop, `register`, `explain`, `refresh` or `unregister` runs unless `$PaddockDirAccepted` is 1, and `run` refuses at the moment of the start when the folder or the program fails the check. The installer aborts at 41 before any of them; the uninstaller with a refused folder runs none of them and goes on. | B, Q2, S11 (each with its positive control) |
| I3 | **Level 44 (TOOL_MISSING).** An installer whose `paddock-setup.exe` is gone never reports success. An uninstaller in a trusted folder without the tool runs no program of the folder and removes each registered service with `sc` when its `fallback` is `sc-delete`, saying so; with `none` it leaves it and says so. | M1 to M5, S3, S4 |
| I4 | **Level 45 (UNTRUSTED)** as I1, also when the copied uninstaller sits next to planted copies of every declared program. | P2 |
| I5 | **Update versus uninstall.** Installer-registered services are removed before files change on install, update, reinstall and uninstall, and registered again in POSTINSTALL. App-registered services are kept on update and on a reinstall (an in-place run, ADR 0053 §2.4), refreshed when declared and registered, and removed only on a real uninstall. | E2, G2, F2, M5 |
| I6 | **A failed removal aborts the install only while the service is still registered.** An `unregister` that fails (non-zero, `timeout`, `refused`, `toolfailed`) aborts with level 43 before `stop-all` and before any file is copied when `sc query` does not answer 1060; it continues when it does. A missing program continues and registers again (§2.3). | S1, S2, S5, S9 |
| I7 | **Order.** Graceful stops, then installer-registered removals, then `stop-all`; app-registered removals after `stop-all`; `register` and `refresh` only after the files and `paddock-setup.exe` are installed. | call order in every case's stub log |
| I8 | **Every program run is bounded** by its declared timeout; at the bound the program's whole process tree is gone, and `timeout` has the meaning of a failure. | S5, GS1 |
| I9 | **A failed registration or refresh never fails the install,** and its reason reaches the user (message) or the log (silent). | S6, S6b, S7 |
| I10 | **Declared text cannot become code.** Only allowlisted texts reach NSIS (§1), they need no escaping, and none reaches a command line. | validation unit tests; C2 (compile) and C2r (run time) |
| I11 | **No hand-written hooks.** A hooks file that defines an `NSIS_HOOK_*` macro, or contains anything but the two includes, does not build (`makensis`) and fails C91. | C1; C91 tests |
| I12 | **Every app-program run and every `sc` fallback is logged** to the details and, where a checked log exists, to `setup.log` (command, duration, outcome, stderr tail). | every S case reads `setup.log` |

### 4. The harness proves each invariant once

`crates/paddock-setup/nsis/test/` keeps its shape (a minimal installer with Tauri's hook points, the real include, a stub
`paddock-setup.exe`), and gains:

- **Fixture declarations** in `test/declarations/`: `plain` (no services, no graceful stop: Spotter's and Debrief's future
  shape), `installer-service` (Pitwall's and Pitcrew's shape), `app-service` (Dyno's helper shape, with `refresh` and a
  `leftBehind`), `full` (two graceful stops, one service of each kind, `fallback: none` on one). `build.sh` runs the **real
  generator** (`cargo run -p paddock-setup --example generate`) over each fixture and builds `harness-<fixture>.exe`; no
  generated file is typed by hand any more.
- **The real `run`.** The stub `paddock-setup.exe` handles the include's own commands as today, but `run` is the real one:
  the harness installer embeds a stub that forwards `run` to the real `paddock-setup.exe` built for the VM (or, simpler,
  the real tool with its other commands' answers injected through `PADDOCK_STUB_*`, decided in W3 by whichever keeps the
  existing cases unchanged). The job object, the re-check and the log are therefore what ships.
- **One stub program for every declared program**, `stub-server.nsi` extended and copied under each declared program name
  (`harness-server.exe`, `harness-agent.exe`, `harness-helper.exe`): it logs `SERVER <$EXEFILE> <args>`, so the log says which
  image ran. Its controls are keyed by image and verb: `PADDOCK_STUB_<IMAGE>_<VERB>_EXIT`, `_SLEEP` (outlast a bound), `_CHILD`
  (start a grandchild that sleeps, to show the tree is ended), `_DELETE=<service>` (delete that stub service with `sc delete`
  before exiting: a verb that fails after removing) and `_OUT` (stdout). Image and verb are upper-cased with `.` and `-` as `_`.
- **Real stub services.** Before the service cases the runner registers `PaddockHarnessSvc` and, for `full`,
  `PaddockHarnessSvc2` (`sc create … start= demand`, binary the installed stub, never started), so `sc query` answers 0 and the
  fallback really deletes; after each case it removes them. Only services the harness created, in the test VM (ER-14). A
  probe first shows the runner can see them (`sc query` 0 after create, 1060 after delete), or the run reports `check
  broken` (ER-33).
- **A positive control for every negative case.** Each case that passes on the absence of a line (no `SERVER`, no `sc`) runs
  its control first in the same fixture and run: the same step with the gate open, which must show the line. A negative case
  whose control fails reports `check broken`, never a pass.
- **Compile tests** C1 (a hooks file with an extra `!macro NSIS_HOOK_PREINSTALL`, and one with an `nsExec` line, must fail to
  build) and C2 (the `full` fixture with every allowlisted character in every text builds, and `makensis -PPO` shows the text
  unchanged in the preprocessed script). They need no VM and run in a **required CI job** (`installer-compile`, Linux, with the
  Debian `nsis` package in the job's image). Without `makensis` the job reports `not measured` and fails; it never skips
  silently (ER-33).
- The existing case with the duplicate name R (the real GUI tool) is renamed **RG**; R stays the spelling case.

Cases (new or widened; A to U, P, Q, R, RG and M1 to M5 stay):

| Case | Fixture | What it does | Passes when |
| --- | --- | --- | --- |
| E2 | each | update over an install (`/UPDATE`) | installer-registered: `SERVER harness-server.exe uninstall` before `stop-all`, `install` after `clean-run`; app-registered: no `uninstall`, `refresh` only while its stub service is registered |
| G2 | each | in-place reinstall (`_?=`, same folder) | as E2 for the uninstaller half: app-registered service untouched |
| F2 | each | real uninstall | every declared service removed in the declared order, both stub services gone (`sc query` 1060) |
| B | each | installer with `check-dir` refused (control: accepted) | level 41, no `SERVER` line |
| Q2 | each | uninstaller with `check-dir` refused and the services not registered (control: accepted) | no `SERVER` line, no `sc` call, the note, the uninstall goes on |
| P2 | `full` | copied `uninstall.exe` next to planted copies of the tool and every declared program (control: the real folder's uninstaller) | level 45, no tool or `SERVER` line, HKLM key, files and both stub services unchanged |
| P3 | `installer-service` | HKLM uninstall key deleted by hand, then the real folder's `uninstall.exe` | level 45 with "run the installer again, then uninstall"; no `SERVER` line; the installer run again restores the key and the uninstall then completes |
| S1 | `installer-service` | update, `unregister` exits 1, stub service still registered | level 43, the message, no `stop-all`, no file replaced |
| S2 | `installer-service` | update, `unregister` exits 1 after deleting the stub service | 1060 in the details, install completes |
| S3 | `installer-service`, `app-service` | real uninstall, trusted folder, tool removed, service registered (control: tool present) | no `SERVER` line, `sc stop` and `sc delete` in the details, stub service gone, `leftBehind` in the message line |
| S4 | `full` | as S3 for the service with `fallback: none` | no `sc` call for it, the "did not remove" message, its stub service still registered |
| S5 | `installer-service` | update, `unregister` sleeps past its bound and starts a grandchild, service registered | `timeout`, level 43; the measured time is within the bound plus 5 s; neither the stub nor its grandchild is running afterwards |
| S6 | `installer-service` | `register` exits 1 with output, `explain` exits 1 with output | install exit 0, `explain`'s line in the details |
| S6b | `installer-service` | `register` exits 1 with output, `explain` exits 0 | install exit 0, `register`'s own line in the details |
| S7 | `app-service` | update with the stub service registered, `refresh` exits 1 | install exit 0, the refresh failure in `setup.log` |
| S8 | `installer-service`, `app-service` | real uninstall, trusted folder, tool present, `check-dir` refused, service registered (control: accepted) | no `SERVER` line; `sc stop` and `sc delete`; stub service gone; the event in `setup.log` only if the tool's checked log accepted the folder, otherwise the details say why. **Security re-review** |
| S9 | `installer-service` | update, service registered, its program deleted from the install folder (control: present) | no abort; `stop-all`, the files and `SERVER harness-server.exe install` follow; the "missing" detail line |
| S10 | `installer-service`, `app-service` | real uninstall, service registered, its program deleted (control: present) | no `SERVER` line; the `sc` fallback; stub service gone |
| S11 | `installer-service` | update; between `PADDOCK_CHECK_DIR` and the verb the test hook replaces `harness-server.exe` by a junction (control: no swap) | `run` answers `refused`; no `SERVER` line; the folder rule in `setup.log` |
| GS1 | `full` | a graceful stop that sleeps past its bound and starts a grandchild | `timeout` logged; neither process remains; `stop-all` still runs; install completes |
| C2r | `full` | an install whose `register` fails, so the message with `failedText` and an uninstall whose fallback shows `leftBehind` | the details carry both texts byte for byte as declared |
| M5 | `app-service` | in-place reinstall with the tool dropped | update mode by the folder compare, the service untouched |

**One end-to-end run per include version** inside Tauri's real template: `run-dyno-e2e.ps1` becomes `run-app-e2e.ps1
-Installer <setup.exe> -Product <name>`, run on the release candidates of Dyno 1.1.1 and Pitwall 1.1 (the first two that
ship it), not once per later app: the template is the same for every app, and the include is byte-identical.

**Ownership of the harness.** ADR 0042's register `docs/harnesses.toml` does not exist yet on any branch (2026-11-08). Until
it does, the harness's home is stated in `crates/paddock-setup/nsis/test/README.md` (home: the line that lands this record;
changes come from there) and in this record; when the register lands, its first entry for this harness is added by the
register's own work package, not by this one.

### 5. What still needs per-app testing, and the verbs' contract

The harness proves the include runs the declared programs at the right time, under the right gates, within their bounds,
and reacts correctly to every outcome. It cannot prove what an app's own programs do. Each app's verbs **must** meet this
contract, and each app tests it as runtime behaviour:

- **Exit codes.** 0 only when done. `unregister` exits non-zero when the service is still registered.
- **Idempotence.** `unregister` of a missing service exits 0; `refresh` of a current or missing helper exits 0. Every verb
  may be killed at any point (the bound), and the next run of the same verb finishes or repairs what the killed run left.
- **`register`** replaces a registration whose program is this folder's program (the S9 path and a reinstall over a
  half-removed app) and refuses one that names another program, with words for the user on its output.
- **No interaction.** No verb opens a window or waits for input.

These are Rust and are unit-tested over the app's SCM port; the real-SCM rows run in the VM by the command line, without an
installer (Pitwall PW-4, PW-7). Also per app:

- **Service behaviour around the installer**: whether recovery restarts the program between `unregister` and `stop-all`
  (Pitwall PW-1), sessions after an upgrade (PW-2), Pitcrew's busy runner (below).
- **The `survivesForcedEnd` statements** (ADR 0053 amendment 1): a review of the app's code, as today.
- **The app's runtime**: UI, readings, start at login, the updater panel.
- **One installer smoke per release** (install, update over itself, uninstall, in the VM) as the rule-54 evidence that the
  built artifact is the one tested; it does not repeat the include's cases.

What no longer needs a per-app VM round: the trust decision, the folder gate, levels 41 to 45, the update and uninstall
distinction, the 1060 abort rule, the missing-program rule, the `sc` fallback, the bounds and the step order. Pitwall's PW-3,
PW-6 and PW-9 become harness cases (P2, S3, P3) and leave its VM table.

### 6. The static per-app check: C91, extended

C91 keeps what it checks (ADR 0053 §8) and adds, for every app with `installMode: perMachine`:

1. **The hooks file is two includes.** After removing blank lines and `;` comments, `installer-hooks.nsh` is exactly the two
   `!include` lines, in that order. Anything else (`nsExec`, `Exec`, `ExecWait`, `ExecShell`, `System::Call`, `!macro`,
   `File`, `Delete`, `RMDir`, `WriteReg*`, `nsis_tauri_utils`, a `!define`) is a finding naming the line, with the fix "declare
   it in windows/per-machine.json; a need the schema does not cover is a change to ADR 0059 and the include". This replaces
   C91's list of required macro calls, which the include now makes itself.
2. **The declaration is version 2 and valid.** C91 reads `version` and fails on 1 or a missing file; the full validation
   (§1, the bundle cross-checks) is `paddock_setup::generate::check_app`, the test that already runs with C91
   (`scripts/affected.mjs` `PER_MACHINE_INPUT`, which gains `crates/paddock-setup/schema/` and the generator's sources). C91
   adds no JSON Schema validator: conformance has no dependencies, and the typed parser is the stricter check (ADR 0022 moves
   such checks to Rust).
3. **The generated files and the schema are current**, and the include copy equals the canonical one (as today).
4. **The service name has one source.** The generator also writes `windows/per-machine.generated.rs` with one constant per
   service (`pub const SERVICE_<N>_NAME: &str = "Pitwall";` and its program file name). The crate that owns the service's
   Windows registration includes it (`include!(concat!(env!("CARGO_MANIFEST_DIR"), "/<path>/per-machine.generated.rs"))`)
   and uses the constant instead of its own literal (Pitcrew's `WINDOWS_SERVICE_NAME` in `pitcrew-core` becomes that
   constant). C91 requires that some Rust source under the app includes the file, and `check_app` that the file is current;
   a literal service name elsewhere is then a second source that review and the app's own tests catch, not C91.

### 7. The artifact gate

`release.toml` (ADR 0039) for each per-machine app gains two probes of the built installer's version resource:
`PaddockInclude` = 9 and `PaddockDeclaration` = `canonical_hash` of the app's declaration at the tagged commit, computed by
`./dev release verify` with the same Rust function. A missing key is a failure, not a skip (ER-33). Work package W8.

### 8. Migration and landing

The include's copy in every adopting app must equal the canonical one (the sync test), so the include change and the
adoption by Dyno and Pitwall land **in one change** (rules 42 and 43); there is no window in which one app runs v9 and the
other v8. The work branches from the Pitwall 1.1 line (`pitwall/1.1-integrate`), which holds include v8, Dyno's trust
changes and Pitwall's adoption. **It lands on main as soon as it is green** (gates, the harness on the VM, both rule-54
dispatches), **ahead of the Pitwall tag**: Dyno 1.1.1 needs the trust fix that comes with include v8, and waiting for
Pitwall's tag would hold it. Family ADR 0022 §G.7 (no `crates/` change inside another app's release window) does not block
it: the change is part of Pitwall 1.1's own window, and Dyno's next release (1.1.1) carries it.

**Dyno** (declaration; hooks file to two lines; behaviour changes for its release notes):

```jsonc
"gracefulStop": [{ "program": "dyno-agent.exe", "verb": ["stop"], "timeoutMs": 30000 }],
"services": [{
  "name": "DynoHelper", "label": "the Dyno helper service", "program": "dyno-helper.exe", "registeredBy": "app",
  "refresh":    { "verb": ["refresh"],   "timeoutMs": 120000 },
  "unregister": { "verb": ["uninstall"], "timeoutMs": 120000 },
  "fallback":   { "kind": "sc-delete", "leftBehind": "Its folder, Dyno Helper in Program Files, and its data were not removed; delete them by hand if you do not need them." }
}]
```

Changes: `dyno-agent stop` gets a 30 s bound and runs in a job object (the hooks' accepted "no timeout" limit goes); the
helper's verbs run through `run`; "registered" is `sc query` instead of the `ImagePath` read; the refresh note is written by
`run` instead of a hand-written `FileOpen`; a helper `uninstall` that fails with the service still registered shows a
message instead of only a detail line; with the helper's program missing, a registered helper is removed with `sc`; the
helper is also removed with `sc` when the registered folder fails `check-dir` (Q2). The helper crate's service name comes
from the generated constant.

**Pitwall** (declaration; hooks file to two lines):

```jsonc
"services": [{
  "name": "Pitwall", "label": "the Pitwall service", "program": "pitwall-server.exe", "registeredBy": "installer",
  "register":   { "verb": ["install"], "timeoutMs": 120000, "explain": { "verb": ["check-install"], "timeoutMs": 30000 } },
  "unregister": { "verb": ["uninstall"], "timeoutMs": 120000 },
  "fallback":   { "kind": "sc-delete" }
}]
```

Changes: the verbs get bounds and run in a job object (today unbounded); the explain rule is kept (`check-install`'s words,
or `install`'s own when the check passes); an uninstall whose service removal fails with the service still registered shows
a message; an update with the server program missing goes on and registers again instead of aborting; Q2's fallback. ADR
0024's "Pitwall's own step" row points here, and PW-3, PW-6 and PW-9 move to the harness. Its release notes also mention
the cancel window of a manual upgrade (Q3): a cancel between the old uninstaller's in-place run and the new install leaves
the service unregistered until the installer runs again.

**Pitcrew** (later, in its 1.1 adoption of ADR 0053, which already budgets 2 to 3 days): the same declaration as Pitwall
with `"name": "Pitcrew"`, `"program": "pitcrew-service.exe"`, no `explain` (it has no `check-install` verb) and
`"failedText": "Open Pitcrew, Settings, Service to try again."`. Its `uninstall` waits up to a minute for runners to finish a
job, so its `unregister` bound is 120 s. Its `install` refuses an existing registration today ("it is installed already;
uninstall it first"), so its adoption changes it to the §5 contract (replace a registration of the same program). Open for
its adoption: ADR 0003's "a busy runner keeps running and is adopted by the new service" must hold under `stop-all` and
under `run`'s job object (a runner started by the service is not in the verb's job, but its process must not be in the
stop list either); the runner programs are not programs of the bundle, but `pitcrew-service ctrl-break` runs from `$INSTDIR`
and is in the stop list; the adoption states why ending it is harmless or excludes it with a reason. Pitcrew's 1.0.x
uninstallers run before any new hook (as Pitwall's 1.0.x, ADR 0024 PW-5); its release notes say to install 1.1 once by
hand.

**New apps**: `scripts/new-app.mjs` writes a version-2 declaration with no services, and the two-line hooks file.

## Consequences

- An app's installer behaviour is data: adding a service, a helper or a graceful stop is a declaration the schema checks,
  not NSIS that a reviewer has to read for the five decisions.
- A fix to the trust decision, the folder gate, the abort rule, the fallback or the bounds lands once, in the include or in
  `run`, and the harness proves it once; no app can miss it (rule 55), because no app has steps of its own.
- Per-app Windows VM work shrinks to the app's verbs, its service behaviour and one installer smoke per release.
- The schema is closed: a new kind of step costs an amendment, an include change and harness cases. That is the intended
  price.
- Every app-program run is bounded, ends its whole tree at the bound, and leaves a line in `setup.log`; a hang in a verb ends
  with a defined outcome instead of an installer that has to be closed by hand.
- Behaviour changes reach users (each app's list in §8): messages where a failed service removal in an uninstaller was only a
  detail line, bounded verbs, the missing-program rule and Q2's fallback. They go into the release notes.

## Decisions on the open questions (2026-11-08, coordinator, after the review)

| # | Question | Decision |
| --- | --- | --- |
| Q1 | Does `nsExec /TIMEOUT` end the child at the bound? | Not relied on: every app-program run goes through `paddock-setup run` with a job object (§2.2); S5 and GS1 assert the tree is gone. |
| Q2 | A trusted folder that `check-dir` refuses: leave the service or remove it with `sc`? | **Remove it with `sc stop` and `sc delete`** (§2.3); harness case S8; **marked for explicit security re-review**. |
| Q3 | The cancel window of a manual upgrade (ADR 0053 §2.4) | Accepted as in 0053; Pitwall's release notes mention it. |

## Work packages

Agent-days for one agent. Shared code is in `crates/paddock-setup` (and the move out of `paddock-service`); the
integrating agent changes `crates/Cargo.toml` (one `schemars` line) and `.gitea/` (the `installer-compile` job).

| WP | Package | Owned paths | Depends on | Days |
| --- | --- | --- | --- | --- |
| W1 | `paddock_setup::declaration` (types, validation, the text allowlist, typed errors, `canonical_hash`) and `paddock_setup::generate` (moved from `paddock_service::per_machine` with its tests and every caller: data-only generated `.nsh`, the generated `.rs` constants, schema output, `check_app`, `write_app`, `adopting_apps`); unit tests for every validation rule; the `generate` example | `crates/paddock-setup/src/{declaration,generate}.rs`, `crates/paddock-setup/schema/`, `crates/paddock-setup/examples/generate.rs`, `crates/paddock-service/src/per_machine*` (removed) | – | 1.5–2 |
| W2a | `paddock-setup run`: the re-check, the job object, bounded output, the marker line, the log; `note --event`; unit tests over a fake process port and Windows tests with real `paddock_proc::testchild` children (ER-14) | `crates/paddock-setup/src/run.rs`, `crates/paddock-setup/src/native/`, `args.rs`, `log.rs` | W1 | 1–1.5 |
| W2b | Include v9: the four hooks defined by the include, the slots, `_PADDOCK_RUN_APP`, `_PADDOCK_SVC_*`, `_PADDOCK_SC_REMOVE`, `_PADDOCK_ACCEPT_DIR`, the messages, the version keys; compile tests C1 and C2 | `crates/paddock-setup/nsis/paddock-per-machine.nsh`, `crates/paddock-setup/nsis/test/build.sh` | W1, W2a | 1–1.5 |
| W3 | Harness: the four fixtures, the image-keyed stub, two stub services with their probe, the real `run`, positive controls, cases E2 to C2r, RG renamed, the README; two VM sessions (first run and the fix-up run), results with logs in `docs/research/`; `run-app-e2e.ps1` | `crates/paddock-setup/nsis/test/` | W2b | 2–2.5 |
| W4 | C91: the hooks-file grammar, the version, the generated `.rs` include; its tests; `PER_MACHINE_INPUT` widened; the `installer-compile` CI job (with the integrator) | `conformance/src/checks/C91-per-machine.mjs`, `conformance/test/c91-per-machine.test.mjs`, `scripts/affected.mjs` (one pattern) | W1 | 0.5–0.75 |
| W5 | Dyno: declaration, hooks file, the helper crate on the generated constant, release notes; the rule-54 dispatch of `dyno-release.yml`; the e2e on the 1.1.1 candidate | `apps/dyno/app/src-tauri/windows/`, `apps/dyno/crates/dyno-helper*`, `apps/dyno/CHANGELOG.md` | W2b, W4 | 0.5–0.75 |
| W6 | Pitwall: declaration, hooks file, the service crate on the generated constant, ADR 0024's row and VM table, release notes (with Q3's window); the rule-54 dispatch; the e2e on the 1.1 candidate | `apps/pitwall/app/src-tauri/windows/`, `apps/pitwall/crates/pitwall-{server,service}`, `apps/pitwall/docs/adr/0024-*`, `apps/pitwall/CHANGELOG.md` | W2b, W4 | 0.5–0.75 |
| W7 | Docs: ADR 0053 pointer (§2 and §8 refer here), the testing manual's installer page, `new-app.mjs` and the template, a lessons entry | `docs/adr/0053-*` (a pointer line), `docs/lessons.md`, `scripts/new-app.mjs`, `templates/` | W2b | 0.25–0.5 |
| W8 | Artifact gate: the two version-resource probes in each per-machine app's `release.toml` and `./dev release verify`'s use of `canonical_hash`; tests | `tools/paddock-dev` (release verify), `apps/{dyno,pitwall}/release.toml` | W1 | 0.5–1 |
| R | Review rounds: the independent review of the built change, the security re-review of S8's branch and of `run`, and their fixes | – | W3 | 1–1.5 |
| later | Pitcrew: the declaration, the constant and its `install` verb on the §5 contract, inside its ADR 0053 adoption | `apps/pitcrew/...` | W1–W4 | 0.5 (of its 2–3) |

**In all 8.75 to 13 agent-days**, against the reviewer's 7.5 to 10: the top is higher because W3 counts two VM sessions and
R counts two review subjects (the change and the security re-review). Revision 1 said 5.25 to 7.5; the difference is `run` (W2a), the
artifact gate (W8), the review rounds (R), the generator move with its callers (W1), the second VM session and positive
controls (W3), and the CI job. Order: W1; then W2a, W4 and W8 beside each other; W2b; W3; W5 and W6 together with W7; R
across W3 to W6. With two agents the critical path W1, W2a, W2b, W3, W6 and R is about **6.5 to 9 working days**, plus CI
waits for the two rule-54 dispatches. The change lands on main once green, ahead of Pitwall's tag (§8).
