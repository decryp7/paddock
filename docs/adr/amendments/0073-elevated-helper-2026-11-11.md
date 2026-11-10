# Amendment to ADR 0073 (2026-11-11): Debrief's elevated work moves into one narrow helper, and the elevated relaunch goes

- Status: **Proposed 2026-11-11, revision 2.**
  - The direction was decided by the user on 2026-11-11: Debrief 1.2 replaces "Restart as administrator" (the whole app
    relaunched elevated through `runas`) with a **narrow elevated helper**. The helper does only the work that needs
    administrator rights and hands the results back. The user chose this over "installed programs only while elevated"
    and over "remove the restart".
  - This is an amendment, not a new ADR: the user's rule since 2026-11-10 is no new redesign ADRs.
  - Revision 2 answers the independent review of revision 1 (`6f9d81618`, CHANGES: six P1s and nine P2s). The
    "Review answers (revision 2)" section at the end lists them.
  - The details below, and D-H1 to D-H9, still need the user's answer. Nothing here is built yet. ER-23 does not apply
    (no drawings).
- Amends:
  - ADR 0073 §1.3 ("Test switches are gated or registered"): the paragraphs on a relaunch through `runas`, and on "No
    data-folder override is read by an elevated or service process";
  - revision 3's DB9 note;
  - the Debrief part of "Migration per app" (item 2).

  DB9's own fix is unchanged: release builds ignore the program-path variables. It is on
  `origin/fix/debrief-env-exe-override` (`9905ea48c`, plus `bf40474ec` for `CHROMIUM_PATH`), merged into
  `origin/train/2026-11-11-5` and not on main yet. This text is merged into ADR 0073 once it is accepted.
- Also changes:
  - Debrief ADR 0017 §7: the per-plugin elevated process is retired (§5);
  - the elevated recorder of Debrief ADR 0022 §7: its symbol cache (E1).

  Each gets a one-line link to this amendment in WP-H7.
- Related:
  - Rule 75 (ADR 0092 §4, accepted 2026-11-10; its text is not in `docs/rules.md` yet, ADR 0092 WP-9).
  - ADR 0092 W2, WP-1 (`PrivateDir`) and WP-7 (the elevated bootstrap).
  - Rule 53 / ADR 0033: identity is a pid plus start time, or a held handle.
  - Rule 73 (ADR 0073); rule 55 and `docs/known-copies.toml` (one implementation per mechanism); ER-22 / rule 42
    (share first).
  - ER-18 and ER-19 (long work), ER-20 / rule 51 (parallelism and cost), ER-27 (logs), ER-33 (a probe must see),
    ER-11 (failing test first).
  - Family ADR 0053 (per-machine Windows installs; Proposed, user direction 2026-11-04); ADR 0096 (the register, and
    copies to main); Dyno ADR 0027 (the family's privileged helper).
- Evidence:
  - Read on 2026-11-11 with `git show`, `git grep`, `git merge-base --is-ancestor` and `git rev-parse` after
    `git fetch`. Paths without a ref are at `origin/main` `ea94d9f1e`.
  - "Shipped" claims were checked by reading the same file at the tag `debrief-v1.1.1` (`33eba7841`). Unless a row
    says otherwise, every S row was found there.
  - Nothing was built or run, and no user data was read.
  - Behaviour of the OS or of a library that was read but not measured is marked "(read, not measured)", and WP-H0
    measures it.

  The heads, re-read right before the commit:

```evidence-heads
read: 2026-11-11
origin/main ea94d9f1e
debrief-v1.1.1 33eba7841
origin/train/2026-11-11-5 55c056089
origin/fix/debrief-env-exe-override 82517897b
origin/feat/private-files-type b72942f7b
origin/docs/dyno-adr-0027-per-machine 3a6be49b5
engineering-rules HEAD 35701f739
```

## Trigger

ADR 0073 revision 3 found the problem. `restart_elevated` (`apps/debrief/app/src-tauri/src/system.rs:120-136`, the same
lines in `debrief-v1.1.1`) relaunches the whole app elevated through `ShellExecuteW` with `runas` (`system.rs:176`).
That made the three sidecar-path overrides (DB9) an environment-variable UAC bypass.

Gating DB9 closes those three variables, but not the rest. An elevated Debrief also starts programs that the user's
normal account can choose or replace:

| # | What an elevated Debrief starts | Where (main) | Class | How this amendment closes it |
| --- | --- | --- | --- | --- |
| S1 | The browser at `settings.browser_path` | `app/src-tauri/src/browser/runtime.rs:72,92` | rule 75 | no elevated Debrief (§5) |
| S2 | The Chromium runtime downloaded into the data folder (`installed_exe`) | `browser/runtime.rs:73,94` | rule 75 | the same |
| S3 | A custom ffmpeg the user chose | `video/runtime_commands.rs:153` (`video_runtime_set_custom`; `:155` at the tag) | rule 75 | the same |
| S4 | The ffmpeg downloaded into `<data>/video/ffmpeg/<tag>` | `video/mod.rs:52,132`; started from `release_dir` with no hash check at use (`crates/debrief-video/src/runtime/store.rs:277-295`) | rule 75 | the same |
| S5 | Third-party plugins from `<data>/plugins` | `lib.rs:638`; started contained (`sandbox_for_third_party`: low integrity, no other programs, `debrief-plugin-api/src/sandbox.rs:89-93`) and hash-checked before every start (`debrief-plugin-host/src/installed.rs:2,521`) | rule 75, **lower** (P2-1): the program and its recorded hash are both in the user's data folder, but it runs below low integrity. Whether a lowbox token made from an elevated token keeps administrator groups is not measured; WP-H0 measures it. | the same |
| S6 | VS Code found through `PATH`, `LOCALAPPDATA` and `ProgramFiles*` | `opener.rs:172-190` (`find_vscode`) | rule 75 | the same |
| S7 | A browser found from `ProgramFiles(x86)`, `ProgramFiles` and `LOCALAPPDATA` | `crates/chromium-driver/src/discover.rs:44`, read through `std::env::var` (`:94`) | rule 75 | the same |
| S8 | A browser named by `CHROMIUM_PATH` | `discover.rs:114` (`find_with`), reached from `browser/sessions.rs:882-885` (`fallback_browser`) | DB9 class (a variable that names a program) | already fixed: release builds ignore it (`bf40474ec` on `origin/fix/debrief-env-exe-override`, in train 5); the row stays until it lands |
| S9 | Programs run by bare name | `logman` (`debrief-plugin-tools/src/perfmon_control.rs:115`); `relog` (`debrief-plugin-windows/src/blg.rs:54`, `app/src-tauri/src/tools.rs:755`); `wevtutil`, `reg`, `typeperf` (`debrief-collect/src/runner.rs:87` with `collector.rs:281,304,317`); `taskkill` (`chromium-driver/src/process.rs:47`); `cmd` (`app/src-tauri/src/storage/walk.rs:202`); `explorer` (`opener.rs:74`) | rule 75 | the helper runs `logman` by absolute path (§3). For the unelevated app, see "Bare names after the per-machine install" below. |
| S10 | Every sidecar next to `current_exe()`, in the per-user install folder | Tauri's NSIS default `installMode` is `currentUser`, and Debrief sets none; ADR 0053 line 55 (`3a6be49b5`) lists Debrief as `currentUser` for 1.0.0 to 1.1.1 | rule 75 | per-machine install (D-H2) |

**S6 and S7 do not depend on the open environment question.** WP-H0 measures whether a `runas` child sees
`HKCU\Environment` values of `ProgramFiles`. That question no longer decides these rows: both also search
`LOCALAPPDATA`, which the user's account can write in any case.

**They are not DB9's class.** DB9 is a *test switch* that names a program (rule 73), and S8 is the one more of those.
S1 to S7, S9 and S10 are *normal product discovery*. They are harmless while Debrief runs as the user, and they become
rule-75 violations only in an elevated process. So rule 73 does not cover them; removing the elevated Debrief does
(§5, D-H3).

**Bare names after the per-machine install.** For a bare name with an unchanged environment, Rust's `Command` on
Windows searches in this order (read, not measured;
<https://doc.rust-lang.org/std/process/struct.Command.html#method.new>):

1. the application's folder;
2. the 32-bit system folder;
3. the Windows folder;
4. `PATH`.

After D-H2 the application's folder is `Program Files\Debrief`, which only administrators can change. Every S9 tool is
in `System32`, and `explorer` is in the Windows folder, so each one resolves before `PATH` is read. WP-H0 tests this:

- a `relog.exe` planted in a `PATH` folder the user owns must not run;
- after any `.env("PATH", …)` on a `Command`, the child's `PATH` is searched first, so a scan in WP-H8 lists every
  such call.

Two more weaknesses on the path that already elevates per plugin (Debrief ADR 0017 §7) are in `debrief-v1.1.1`:

- **E1. The elevated native recorder reads and writes symbols in user folders.**
  - `debrief-plugin-native/src/tool.rs:347` takes `paths::elevated_data_dir`, which accepts the host's data folder
    under the user's `%APPDATA%` or `%LOCALAPPDATA%` (`paths.rs:61-75`).
  - `:358` uses that folder as the symbol cache, together with the user's symbol folders and an optional symbol server
    (`debrief-symbols/src/config.rs:17-24`).
  - So an administrator's process parses PDB files the user's account can replace, and downloads into a folder that
    account controls.
- **E2. The elevated plugin process is the whole `debrief-plugins.exe`**, with all twelve built-in plugins, importers
  included (`debrief-plugins/src/main.rs:12-27`).
  - Its role refuses all but elevated tool operations (`debrief-plugin-api/src/serve.rs:96,243-247`), but the code is
    in an administrator's address space.
  - It comes from S10's per-user folder.
  - Its bootstrap is ADR 0092's W2 file (`debrief-ipc/src/elevation.rs:103-112`).

## 1. What needs administrator rights today (from the code)

| # | Operation | Code that needs the rights | How Debrief gets them today |
| --- | --- | --- | --- |
| A1 | .NET ETW capture (`etw-*` operations) | `debrief-plugin-dotnet/src/plugin/tool.rs:127-137` (`.elevated()`), `plugin/elevated.rs` | per-plugin elevated process |
| A2 | Native ETW profiler (start, status, stop) | `debrief-plugin-native/src/tool.rs:396-400` | per-plugin elevated process |
| A3 | Performance Monitor collector sets: `install`, `control` (start, stop, delete, query) and `outputs` with `logman` | `debrief-plugin-tools/src/perfmon_tools.rs:141-148`; access denied mapped at `perfmon_control.rs:44-46,127-128` | the relaunch (or the "Performance Log Users" group) |
| A4 | Reading and following the Security event log | `debrief-plugin-windows/src/eventlog/query.rs:50` (`needs_admin`) | the relaunch |
| A5 | A dump of another user's process, or of an elevated one | `debrief-plugin-dumps/src/capture.rs:203-207,252-254` | the relaunch |
| A6 | Process monitor samples of processes it cannot open | `debrief-monitor/src/tracker.rs:111-122` | the relaunch |
| A7 | Native sampling of an elevated process | `debrief-plugin-native/src/sampling/live.rs:170-172` sends the user to A2 | – |

`debrief-agent`'s service install (`debrief-agent/src/service/mod.rs:45`) is out of scope: it is a command-line tool
that tells the user to use an elevated prompt.

**Whether the helper needs `SeDebugPrivilege`.** An elevated administrator token holds it, disabled. Debrief never
enables it today: `git grep` for `SeDebugPrivilege`, `SE_DEBUG_NAME` and `AdjustTokenPrivileges` in `apps/debrief`
finds only event-log test fixtures.

- The helper starts with it disabled.
- WP-H0 measures whether A5 and A6 can open other accounts' processes and services' processes without it. ADR 0053's
  S0 spike lists the same question, unanswered.
- If they cannot, the helper enables it only around the `OpenProcess` of one `Dump` or `ProcessSample` and drops it
  again, and logs both.

## 2. Decision: one helper, started on demand, from a protected folder (D-H1, D-H2, D-H5)

- **One program, `debrief-admin.exe`.** A new crate `debrief-admin` with a **closed, typed request list** (§3).
  - It links only code that needs the rights: `debrief-etw`, the ETW halves of the .NET and native recorders, the dump
    writer, the Security log reader, the process reader and the `logman` runner with the perfmon template renderer.
  - It links no importer, plugin host, browser, symbol resolver or network crate.
  - The per-plugin elevated role (ADR 0017 §7) is retired, which closes E2.
  - Plugins keep their port, `ToolCtx::elevated()` and `ElevatedOps::invoke` (`debrief-plugin-api/src/tools.rs:592-631`).
    The host turns their calls into typed helper requests.
  - Third-party plugins still cannot declare elevated operations (`installed.rs:502-505`).
- **On demand, for the session (D-H5).** The first admin-only operation starts it, which shows one UAC prompt right
  after the user's click. It serves until Debrief closes the connection, and sits in a kill-on-close job object with a
  memory cap. It is not a service.
- **Windows only (D-H6).**
- **A fixed, protected install path (D-H2, D-H8).** Debrief 1.2 installs per machine (ADR 0053's Debrief row,
  `3a6be49b5` line 550).
  - The host reads the install folder from `InstallLocation` under Debrief's `HKLM` uninstall key, which only
    administrators can write. It never uses `current_exe()` or the environment.
  - It checks the chain from that folder to `debrief-admin.exe` with
    `paddock_service::privileged::trust::verify_chain` (`crates/paddock-service/src/privileged/trust.rs:436`) over
    `trust_real::RealTrust` (`trust_real/mod.rs:29`).
  - It holds the image with `trust_real::HeldImage::open` (`trust_real/mod.rs:164-195`) until `ShellExecuteExW`
    returns.
  - The helper repeats the chain check from its own token at start, and exits with code 12 if it fails (§7).
- **DLL loading.** The first thing the helper does is call
  `SetDefaultDllDirectories(LOAD_LIBRARY_SEARCH_SYSTEM32 | LOAD_LIBRARY_SEARCH_APPLICATION_DIR)`. It is linked with
  `/DEPENDENTLOADFLAG:0x800` (system32 only for static imports' dependents), so a DLL planted in a user folder is never
  loaded. The WP-H0 VM probe records image loads as well as process starts.

## 3. What the helper may and may not do

Every request is decoded into a typed value at the edge, checked again in the helper, bounded, and given a time limit.

| Request | Arguments, validated in the helper | Answer |
| --- | --- | --- |
| `Info` | – | API version, helper version, the chain result |
| `EtwDotnet { start, status, stop }` | target as `ProcessId` (pid and start time, `paddock_proc::ProcessId`, `crates/paddock-proc/src/identity.rs:37`); preset from a closed enum; caps no larger than the plugin's limits | output stream of NetTrace bytes, plus status |
| `EtwNative { start, status, stop }` | target `ProcessId`, `heap: bool`, the same caps | output stream of raw events; stacks as module and offset, **unsymbolised** (closes E1: the unelevated plugin symbolises) |
| `PerfmonInstall { template }` | the **typed** `PerfmonTemplate` (`debrief-plugin-tools/src/perfmon_template.rs`), never XML. The helper renders the XML itself, after setting `RootPath` and the output location to a folder of its own choosing (below); a template with a `Task`, a run-as user or a password is refused. The set name must match `^Debrief[ A-Za-z0-9_-]{0,64}$`. | the set's name and state |
| `PerfmonControl { name, action }` | the same name pattern; `action` from `start`, `stop`, `delete`, `query` | state and the parsed `logman` output, at most 64 KiB |
| `PerfmonOutputs { name }` | the same name pattern | the log files of that set, streamed as output streams. They are never handed over as paths, because the folder is the helper's own. |
| `SecurityLog { query, follow }` | a time window and a record cap (at most 100,000 per answer), or follow with credits | event records as stream items |
| `Dump { target, kind }` | target `ProcessId`; `kind` from a closed enum. Protected processes are refused by `GetProcessInformation(ProcessProtectionLevelInfo)` (anything but `PROTECTION_LEVEL_NONE`), and LSASS by image path from the system folder (D-H4). | output stream of the dump bytes |
| `ProcessSample { targets }` | at most 4,096 `ProcessId`s | the monitor's fields only |

**The helper never:**

- reads its environment: no `std::env::var`, `var_os`, `vars`, `vars_os` or `current_dir` (WP-H8's Clippy ban and scan);
- opens a path a caller chose;
- writes into a folder the user can change;
- loads a plugin;
- uses the network.

Its one external program is `%SystemRoot%\System32\logman.exe`, at an absolute path from
`known_folders::system_tool_in`. That function's list is closed (`SYSTEM_TOOLS = ["icacls", "reg", "sc"]`,
`known_folders.rs:42`), and any other name is "returned as given" (`:46`), so WP-H1 adds `logman`: a shared-crate
change by the integrating agent. The helper starts `logman` with `env_clear()` and only `SystemRoot`, taken from
`system_directory()`.

**Its folders.**

- The installer creates `%ProgramData%\Debrief\Admin\`, owned by Administrators, with no write access for Users.
  ADR 0053's Debrief row gains this line.
- Before every request that writes there, the helper checks that chain with `verify_chain`.
- Inside it, every scratch and collector-set folder is a `paddock_env::private::PrivateDir` (ADR 0092 WP-1), created and
  used by handle. `PrivateDir` is on `origin/feat/private-files-type` `b72942f7b`, which is in train 5 and not on main.
- Collector sets therefore write their logs only below that folder, and `PerfmonOutputs` streams them back.

## 4. IPC and trust (rule 75), in `paddock-ipc`

**Where it is built.** Debrief's own IPC modules are registered copies that leave by `debrief-v1.2.0`:
`docs/known-copies.toml:105-160` lists `endpoint.rs`, `frame.rs`, `peer.rs`, `wire.rs`, `launch.rs`, `child.rs`,
`link.rs`, `sandbox.rs` and `contain.rs`, with canonical `paddock-ipc` (or the planned `paddock-sandbox`). Rule 55 does
not allow building a new mechanism on a copy that is about to be deleted. So **WP-H1a builds the elevated launch in
`paddock-ipc`**, the canonical crate, next to `launch.rs` and `child.rs`. Today `paddock-ipc`'s launcher starts a child
with its bootstrap on stdin (`crates/paddock-ipc/src/launch.rs:1-5,237`) and has no elevated variant. The helper uses
`paddock-ipc` from the start. Debrief's plugins move with PH1 to PH4 as already planned. (D-H9 offers the alternative of
moving the delete date.)

**The elevated launch, `paddock_ipc::elevated`:**

- **No bootstrap file** (closes W2, ADR 0092 WP-7).
  - The host creates the pipe: a random name, first instance only, no remote clients, one instance, and the DACL
    "owner and Administrators". This is the same descriptor Debrief uses today (`debrief-ipc/src/link.rs:296-336`),
    re-built in `paddock-ipc`'s Windows endpoint.
  - `ShellExecuteExW` with `runas` starts the helper with only the pipe address and the requester's `ProcessId` as
    arguments.
  - **There is no token on the command line.** The helper connects, calls `GetNamedPipeServerProcessId`, and compares
    that pid and its start time with the requester's `ProcessId` before it sends or reads anything; a mismatch exits
    with code 11.
  - Only then does the host send the session token, and the helper echoes it in its `Hello`.
  - As defence in depth, the helper refuses (code 13) a requester whose image is not `<install folder>\debrief-app.exe`.
- **The impersonation level.** The helper opens the pipe as a client with
  `SECURITY_SQOS_PRESENT | SECURITY_IDENTIFICATION`, set explicitly through `ClientOptions::security_qos_flags`. Tokio
  documents identification as its default (read, not measured:
  <https://docs.rs/tokio/latest/tokio/net/windows/named_pipe/struct.ClientOptions.html#method.security_qos_flags>). So
  the medium-integrity host can identify the helper's token but never impersonate it. A test asserts that
  `ImpersonateNamedPipeClient` followed by `OpenThreadToken` in the host yields an identification-level token.
- **The host checks the client.** `GetNamedPipeClientProcessId` must equal the pid of the process handle that
  `ShellExecuteExW` returned. The host holds that handle, so the pid cannot be reused (rule 53).
- **Fixed parameters.** `SandboxSpec` and the frame limit, today fields of the bootstrap, are compiled into the helper.
- `paddock-ipc`'s system-endpoint identity (`identity.rs`) and the squatting check (`trust.rs`) are for a service that
  many accounts call (Dyno ADR 0027 §2). They are not used here: this is a private one-to-one child.

## 5. How results come back, and what goes

**How results come back:**

- typed replies under the frame limit;
- bulk bytes (ETW traces, dumps, collector-set logs) as output streams with credits, which the unelevated host writes
  where it chose (protocol 1.1 of ADR 0017, re-built on `paddock-ipc` in WP-H1a);
- live items as stream events;
- progress as task events;
- the helper's standard error forwarded over the connection, so the helper writes no log file.

**No elevated Debrief (D-H3).** At start, the shell reads `TokenElevationType`:

- **`TokenElevationTypeFull`** (UAC is on and Debrief was started elevated) or **root on macOS and Linux**: Debrief does
  not start its windows. It shows one native message: "Debrief does not run as administrator. Start it normally; it asks
  for administrator permission when a feature needs it." Then it exits with a logged reason.
  - This is the honest answer to rule 75: there is an unelevated user whose folders an elevated Debrief would read,
    starting with every project and import file.
  - Option (b) relaunches unelevated instead (through the shell's own token); it costs about 1 day more.
- **`TokenElevationTypeDefault`** (UAC is off, or the built-in Administrator account) runs as today. No helper is needed
  (`decide_on`, `elevation.rs:44`).
  - Every process of that user is already elevated, so there is no lower integrity level to protect from. Rule 75's
    premise does not hold, and gating S1 to S10 there would protect nothing.
  - The About page and the log say "UAC is off: Debrief runs with administrator rights".
- **`TokenElevationTypeLimited`** is the normal case.

**What is removed:**

- Rust:
  - `restart_elevated` and its line in `lib.rs:331`;
  - `relaunch_elevated`, `quoted` and `can_restart_elevated` (`system.rs:39-40,65,117-144,171-199`);
  - the `--elevated` role and the `elevate` field (`debrief-plugin-host/src/process.rs:60-63,773-777`);
  - `paths::elevated_data_dir`.
- Messages that tell the user to restart as administrator now name the helper's permission prompt:
  `debrief-plugin-dumps/src/capture.rs:206,253`, `debrief-plugin-tools/src/perfmon_control.rs:44-46` and
  `debrief-monitor/src/tracker.rs:113`.
- TypeScript:
  - `tools-backend.ts:99,209,322-323` and `tools.types.ts:11,321-322`;
  - `lifecycle.service.ts:35-36,91-93` (`canRestartElevated`, `restartElevated`);
  - the five UI entry points of §1;
  - `mock-tools-backend.ts:590-598`.
- The specs of `elevation-hint`, `perfmon`, `monitor`, `eventlog`, `lifecycle`, `mock-tools-backend` and
  `provide-tools`, and `tools-backend.spec.ts:42`, are updated.
- `lifecycle.rs:968` keeps testing `restart_app` only.
- The `elevation` query stays and answers `elevated`, `elevationType` and `platform`.

## 6. Residual risk, and what limits it (D-H1, D-H4, D-H5)

The host is a medium-integrity process that holds a session-long channel to a High process. Code that already runs as
the user can inject into the host, and then until Debrief exits, without another prompt, it can:

- dump any unprotected process;
- read the Security log;
- create and start collector sets.

ADR 0017 §7 already says UAC is not a boundary against code running as the same user. But a session-long channel turns
one consent into many operations. The limits:

- **The request list is closed and typed.** There is no "run this program" and no path argument (§3).
- **Protected processes and LSASS are refused** (D-H4), so credential memory is never dumped.
- **Mitigation option for D-H5.** For each `Dump` and each `PerfmonInstall`, the helper shows **its own** confirmation
  window ("Debrief asks to write a memory dump of <process> (pid N). Allow?"). That window runs at High integrity, and
  User Interface Privilege Isolation (UIPI) stops a medium-integrity process from sending it input, so injected code
  cannot click it. The alternative is one UAC prompt per `Dump`.
- The audit lines in the log (§8) name every request, with its correlation id.

## 7. Failure states, wording and long work (ER-18, ER-19)

Copy is in sentence case. Every state has a next step.

| State | What the user sees |
| --- | --- |
| Before the prompt | Dialog: "Windows will ask for permission to start Debrief's administrator helper. It records and reads only what you ask for here." Buttons: **Continue**, **Cancel**. |
| Waiting | Phase "Waiting for administrator permission". **Cancel** stops waiting; a helper that arrives later finds the connection closed and exits. |
| Declined (`ERROR_CANCELLED`) | "Administrator permission was declined. Nothing was changed." **Try again** |
| The helper's own confirmation declined | "You declined the dump in the administrator helper's window. Nothing was written." |
| Helper missing or not protected (exit 12, or the host's chain check) | "Debrief's administrator helper is missing or is not in a protected folder. Reinstall Debrief for all users." |
| Not verified (exit 11 or 13) | "The administrator helper could not confirm it was started by this Debrief, so it stopped. Try again; if it happens again, export a diagnostic bundle." |
| Protected target | "Windows protects this process, so it cannot be dumped." |
| Helper stopped during work | "The administrator helper stopped (exit code N). Your other work is not affected." **Try again**. A stream cut short is kept and marked "incomplete". |
| Started elevated (Full) | the native message of §5 |
| Not on Windows | "This is only available on Windows." |

Long operations (an ETW capture, a large dump, a week of the Security log, collector-set logs) run as jobs with
labelled phases: "Waiting for permission", "Recording" or "Reading", "Writing the dump", "Copying to Debrief". Each
phase counts bytes or records from the first moment. The UI never times a job out: it shows "Still working". Cancel
works in every phase: it sends `stop` and keeps what was written, marked incomplete.

**Exit codes**, a table both sides share:

| Code | Meaning |
| --- | --- |
| 10 | bad arguments |
| 11 | server not verified |
| 12 | own image not protected |
| 13 | requester image wrong |
| 14 | the admin folder refused |

## 8. Logging (ER-27)

Host, per helper start:

- the install folder read from `HKLM`;
- the `verify_chain` outcome (`Untrusted::describe`, `trust.rs:256`);
- prompt shown, accepted or declined, with milliseconds;
- the helper's `ProcessId`;
- the handshake and impersonation-level outcome;
- the exit code, mapped to text.

Every request gets a correlation id, its kind, duration, outcome and bytes or records, and records whether
`SeDebugPrivilege` was enabled for it. The helper writes its phases and refusals to forwarded stderr. User content stays
out of the logs: kinds, counts and pids only, never paths, set names or event text. Failures before the helper is
connected are logged by the host from the exit code. The start-up refusal of §5 is logged with its `TokenElevationType`.

## 9. Tests

The work starts from these failing tests (ER-11):

1. `restart_elevated` is not a command.
2. A `KnownFolders` fake and a poisoned environment (`ProgramFiles`, `SystemRoot`, `PATH`, `DEBRIEF_PLUGINS`): the
   helper's paths come only from the fake, and the install folder only from the `HKLM` port.
3. A squatting server (another pid, or the same pid with a different start time): the helper exits 11 before it writes
   a byte, and no token was ever sent.
4. A client whose pid is not the started process is refused.
5. The decoder refuses:
   - an unknown kind;
   - a set name outside the pattern;
   - more than 4,096 targets;
   - a `PerfmonTemplate` with a `Task`, a run-as user or a password;
   - a template whose rendered `RootPath` is not the helper's folder (the renderer always sets it: a property test over
     generated templates).
6. `Dump`:
   - refuses LSASS;
   - refuses any process whose protection level is not `PROTECTION_LEVEL_NONE` (a fake port; on the VM, a real PPL
     process such as `MsMpEng.exe`).
7. The helper crate has no `std::env` call, and no `Command::new` except the `logman` site: Clippy plus the WP-H8 scan,
   each with a planted positive control.
8. The start-up decision: `Full` and Unix root refuse with the message; `Default` and `Limited` start. Only if the user
   chooses D-H3 (c), **8b**: every row from S1 to S10 is refused in elevated mode, one assertion per row.
9. The host's view of the helper's token is identification-level (§4).

Windows CI (unelevated) runs the helper as a plain child. The switch for that is a `test-support` launch option, not an
environment variable (rule 73). It goes through the real pipe and the real output stream, including a crash in the
middle of a stream.

**The Windows test VM's admin snapshot** (local only, never CI) runs the real prompt:

- accept, decline and cancel-while-waiting;
- the helper's own confirmation, plus a medium-integrity `SendInput` attempt at that window, which must have no effect;
- `HKCU\Environment` poisoned (`ProgramFiles`, `ProgramFiles(x86)`, `SystemRoot`, `PATH`, `DEBRIEF_PLUGINS`), with a
  planted `relog.exe` and a planted `version.dll` in user folders;
- a full run that records every process creation with its token elevation, and every image load into an elevated
  process. Only `debrief-admin.exe`, `System32\logman.exe` and system DLLs may appear elevated.
- A1 to A6 end to end;
- Debrief started elevated is refused.

**Positive control (ER-33):** `debrief-v1.1.1`'s "Restart as administrator" with `HKCU\Environment` `DEBRIEF_PLUGINS`
set to a marker program must show the marker running elevated. The same run measures the `ProgramFiles` question, S5's
lowbox token and `SeDebugPrivilege`. The installer is built with gates on (rule 54).

## 10. Debrief 1.1.x

Nothing changes in 1.1.x. The user decided on 2026-11-09 (DB9) and on 2026-11-11 to fix this in 1.2 only. S1 to S10,
E1, E2 and W2 stay in `debrief-v1.1.1`. The 1.2 release notes list them as fixed.

## Parallelism and cost (ER-20, rule 51)

- **Prompts.** One per session, plus the helper's own confirmation per `Dump` or `PerfmonInstall` (D-H5). A prompt per
  operation would cost the user more than any CPU time here.
- **Work in the helper.** Requests run on a small worker pool: one slot per slow kind (`Dump`, `PerfmonInstall`,
  `SecurityLog` back-read) and up to four requests in all, as Dyno's helper does (Dyno ADR 0027 §2). More requests
  answer "busy" at once. The thread level is normal (rule 52): a person waits on each request.
- **Copy cost.** Dumps and traces cross the pipe once, through the existing credit window. Today a dump is written
  directly by the elevated Debrief, so this adds one pipe copy of the file.
  - Pipe throughput is not measured. WP-H0 measures a 2 GiB dump through the pipe on the VM and records the result in
    `apps/debrief/docs/performance.md` (rule 48).
  - If the pipe costs more than 20 % of the dump time, the cheaper design is to write the dump into the helper's
    `PrivateDir` and stream it once at the end, which is the same copy count as today's write plus read.
- **Symbolisation** moves out of the elevated process (E1). That costs nothing extra: it ran once either way.
- No new thread-count setting: the pool is bounded by request kinds, not by data size.

## Prerequisites and what is built

| Mechanism | State |
| --- | --- |
| `paddock-ipc` launch, child, frames, peer, endpoint (`crates/paddock-ipc/src/{launch,child,frame,peer,endpoint}`) | built on main; **no elevated launch** (WP-H1a adds it) |
| Debrief's `debrief-ipc` copies | built, registered for deletion by `debrief-v1.2.0` (`docs/known-copies.toml:105-160`); not extended by this work |
| `paddock_service::privileged::{known_folders, trust::verify_chain, trust_real::HeldImage}` | built on main; `system_tool_in` lacks `logman` (WP-H1, shared-crate change) |
| `paddock_proc::ProcessId` | built (`identity.rs:37`) |
| `paddock_env::private::PrivateDir` (ADR 0092 WP-1) | **prerequisite**: on `origin/feat/private-files-type` `b72942f7b`, in train 5, not on main |
| `GetNamedPipeServerProcessId` check (ADR 0092 WP-7) | not built anywhere; built in WP-H1a |
| Rule 75 text (ADR 0092 WP-9) | **not built**: `docs/rules.md` ends at rule 71 |
| DB9 and `CHROMIUM_PATH` gated (`9905ea48c`, `bf40474ec`) | on train 5, not on main |
| ADR 0053 and Debrief per machine | **prerequisite**: ADR 0053 is Proposed on `origin/docs/dyno-adr-0027-per-machine` `3a6be49b5`. It must be **accepted and copied to main (ADR 0096 §1) before WP-H5 starts**. Its shared parts are on main (`crates/paddock-service/src/per_machine.rs`; Dyno and Pitwall have `windows/per-machine.json`). |
| Windows test VM admin snapshot with UAC automation | **prerequisite**, being added, not on main |
| `./dev check test-doubles` (ADR 0073 WP-0) | **not built** |
| The RT-5 check host and ADR 0088's ratchet engine (for WP-H8's conformance half) | **not built** |

## Work packages

Estimates are raw days of agent work, before review rounds.

| WP | Package | Depends on | Days |
| --- | --- | --- | --- |
| WP-H0 | VM spikes on the admin snapshot: `ProgramFiles` in `HKCU`, S5's lowbox token, `SeDebugPrivilege` for A5 and A6, bare-name resolution, DLL loads, `GetNamedPipeServerProcessId` across integrity levels, the impersonation level, pipe throughput for a 2 GiB dump, the positive control; a research note and a `performance.md` block | admin snapshot | 1.5–2 |
| WP-H1a | `paddock_ipc::elevated`: the `runas` launch, the verified pipe, no command-line token, the identification-level client, the shared exit codes, the re-built output stream with credits; tests 3, 4 and 9 first; CHANGELOG entry | – | 2.5–3.5 |
| WP-H1 | `debrief-admin` core: the decoder (test 5), the typed perfmon renderer with a fixed `RootPath`, the image chain and `HKLM` folder (test 2), no environment, `SetDefaultDllDirectories` and the link flag, the admin folder and `PrivateDir`, `logman` added to `system_tool_in` | WP-H1a, ADR 0092 WP-1 | 3–4 |
| WP-H2 | The requests: the ETW halves of A1 and A2 moved; A3 (`install`, `control`, `outputs`), A4, A5 (protection level, LSASS, the helper's confirmation; test 6), A6; `SeDebugPrivilege` per WP-H0 | WP-H1 | 3.5–5 |
| WP-H2b | E1: native symbolisation moved into the unelevated plugin; `elevated_data_dir` deleted | WP-H2 | 1–1.5 |
| WP-H3 | Host routing: the launcher on `paddock-ipc`, one helper per session, `ElevatedOps` turned into typed requests, A3 to A6 in their plugins routed to the helper, the `--elevated` role removed | WP-H1 | 3–4 |
| WP-H4 | `restart_elevated` and every item of §5's removal list (test 1 first), the start-up decision (test 8), the UI states of §7, mocks, specs and i18n | WP-H3 | 2–2.5 |
| WP-H5 | Debrief per machine (ADR 0053's row, counted there), plus the `%ProgramData%\Debrief\Admin` folder line | ADR 0053 accepted and on main | (2.5–3) |
| WP-H6 | Windows CI legs and the VM run of §9; installer build (rule 54) | WP-H2 to WP-H5 | 2–2.5 |
| WP-H8 | Rule-75 check of every elevated entry point: Clippy `disallowed-methods` in a `clippy.toml` for each elevated binary (environment, `Command::new`, path-taking `std::fs` calls); a source scan that finds every `runas` and elevated launch in the family and asserts each target is registered; a planted positive control. The conformance half moves onto the RT-5 host once that exists. | – | 1–1.5 |
| WP-H7 | Text: merge into ADR 0073, links in ADR 0017 §7 and 0022 §7, the ADR 0053 Debrief-row line, lessons entry, wiki notes, release notes | – | 0.5 |
| WP-HR | Independent re-review of this amendment, and review of WP-H1a's trust code | – | 0.5–1 |

**Total: 20.5 to 28 days raw** (revision 1 said 13 to 18.5). The additions:

- the `paddock-ipc` launcher (WP-H1a);
- routing A3 to A6 (WP-H3);
- E1 (WP-H2b);
- the typed perfmon renderer and the helper's confirmation;
- the start-up decision;
- the rule-75 check (WP-H8);
- larger spikes.

The per-machine install that ADR 0053 counts (2.5 to 3 days) comes on top. ADR 0092 WP-7 drops from 3 to 1.5 days,
because the verified pipe is built here; it keeps `private_tempdir`, the IPC socket folder, Pitcrew and its test 8.

After WP-H1a and WP-H1, WP-H2 and WP-H3 run in parallel, and WP-H8 is independent. Everything except WP-H8's
conformance half gates `debrief-v1.2.0` (D-H7).

## Decisions for the user

| # | Decision | Recommendation |
| --- | --- | --- |
| D-H1 | Shape: (a) one dedicated `debrief-admin` with a closed request list, and the per-plugin elevated role retired; (b) keep ADR 0017 §7's per-plugin elevated process and add A3 to A6 to it; (c) a LocalSystem service like Dyno's | **(a)**. Residual risk under all three: same-user code injected into Debrief can use the open channel (§6). (a) limits that to seven typed requests, protected processes refused. (b) is about 3 days cheaper but keeps twelve plugins in an administrator's process (E2), with up to four prompts. (c) never asks again, so its residual risk lasts for good, not for a session. |
| D-H2 | Fixed path: (a) Debrief per machine in 1.2 (ADR 0053), the helper in its install folder; (b) Dyno's protected copy on first use (Dyno ADR 0027 R1); (c) the per-user folder | **(a)**. The direction the user set for every app on 2026-11-04. It closes S10 and makes bare names resolve to protected folders. ADR 0053 must be accepted and on main first. |
| D-H3 | A Debrief started elevated: (a) **with UAC on (`Full`) or as root, refuse to start, with the message of §5; with UAC off (`Default`), run as today**; (b) as (a), but relaunch unelevated instead of refusing (+1 day); (c) elevated mode: run, and refuse S1 to S10 with a banner | **(a)**. An elevated Debrief still parses the user's files, and that is what rule 75 forbids, so (c) is not honest. Where UAC is off, there is no lower integrity level to protect from. |
| D-H4 | Dumps: (a) refuse every protected process (by protection level) and LSASS; (b) any process, as the relaunch allows today | **(a)**. Residual risk: injected code can still dump unprotected processes of other accounts during the session. D-H5 (b) limits that. |
| D-H5 | Consent: (a) one UAC prompt per session, nothing more; (b) (a) plus the helper's own UIPI-protected confirmation per `Dump` and `PerfmonInstall`; (c) one UAC prompt per `Dump` | **(b)**. Under (a), injected code gets dumps, the Security log and collector sets for the rest of the session, with no further prompt. (b) keeps a person in the loop for the two operations that write lasting or sensitive data, at about 0.5 day. (c) trains people to click through. |
| D-H6 | macOS and Linux: no helper in 1.2; root is refused at start (D-H3) | **Yes.** All the admin-only operations are Windows only. |
| D-H7 | Gate: WP-H0 to WP-H6 and WP-H8's Clippy half block the `debrief-v1.2.0` tag | **Yes.** It is the second half of the DB9 fix the user put in 1.2. |
| D-H8 | Install folder: (a) read from the `HKLM` uninstall key and then chain-checked; (b) pin `Program Files\Debrief` in ADR 0053's Debrief row (no folder page) | **(a)**. A machine may install to another drive; the key only administrators can write, and the chain check still decides. |
| D-H9 | IPC base: (a) build the elevated launch in `paddock-ipc` now (WP-H1a); (b) build it on `debrief-ipc` and move `debrief-ipc`'s delete date past `debrief-v1.2.0` (`Copies-date: … approved: user`) | **(a)**. (b) saves about 1 day now but adds a second elevated mechanism that rule 55 must later fix in two places, and it moves a date the user set. |

## Review answers (revision 2)

The review of revision 1 is `6f9d81618`, verdict CHANGES.

| Finding | Answer |
| --- | --- |
| P1-1 inventory | S4 (downloaded ffmpeg), S8 (`CHROMIUM_PATH`, fixed on train 5 by `bf40474ec`) and S9's full bare-name list added. S6 and S7 hold anyway, because of `LOCALAPPDATA`; they are reconciled with DB9 (a different class). Bare-name resolution is stated and tested (WP-H0). Elevated mode is replaced by the start-up refusal (P1-6); test 8b covers every row if the user keeps elevated mode. |
| P1-2 `logman` XML | §3: a typed template rendered in the helper; `RootPath` fixed; `Task` and run-as refused; the `outputs` request added; the admin folder made by the installer and chain-checked before each write; decoder tests 5. |
| P1-3 copies | §4: the elevated launch is built in `paddock-ipc` (WP-H1a); D-H9 offers moving the date, with its cost. |
| P1-4 prerequisites | `PrivateDir` (ADR 0092 WP-1, train 5), the `paddock-ipc` work, and ADR 0053's acceptance and copy to main before WP-H5. |
| P1-5 residual risk | §6, and residual risk stated in D-H1, D-H4 and D-H5; the helper's own UIPI-protected confirmation as D-H5 (b); `SeDebugPrivilege` stated and measured; protected processes refused by protection level (test 6). |
| P1-6 D-H3 | The new D-H3 (a), by `TokenElevationType`. |
| P2-1 | S5's class lowered, with its containment cited. |
| P2-2 | S9 cites `perfmon_control.rs:115`. |
| P2-3 | §4: identification level, set explicitly and tested (test 9); no token on the command line, sent only after the server-pid check. |
| P2-4 | §5's removal list completed. |
| P2-5 | Re-estimated: 20.5 to 28 days. |
| P2-6 | `docs/adr/register/0092.toml` WP-7 is 1.5 days, with its kept scope. |
| P2-7 | D-H8. |
| P2-8 | §2 DLL loading; image loads in the VM probe. |
| P2-9 | A "Parallelism and cost" section; test 8's range is S1 to S10; the R-row comment cites ADR 0096 §4. |
| Rule-75 check | WP-H8. |
