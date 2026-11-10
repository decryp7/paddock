# Amendment to ADR 0073 (2026-11-11): Debrief's elevated work moves into one narrow helper, and the elevated relaunch goes

- Status: **Proposed 2026-11-11.** The direction was decided by the user on 2026-11-11: Debrief 1.2 replaces "Restart as
  administrator" (the whole app relaunched elevated through `runas`) with a **narrow elevated helper** that does only
  the work that needs administrator rights and hands the results back. The user chose this over "installed programs
  only while elevated" and over "remove the restart". It is an amendment, not a new ADR (the user's rule since
  2026-11-10: no new redesign ADRs). The details below, and D-H1 to D-H7, still need the user's answer. Nothing here is
  built yet. ER-23 does not apply (no drawings).
- Amends: ADR 0073 §1.3 ("Test switches are gated or registered": the paragraphs on a relaunch through `runas`, and on
  "No data-folder override is read by an elevated or service process"), revision 3's DB9 note, and the Debrief part
  of "Migration per app" (item 2). DB9's own fix (gating the three executable-path overrides, WP-D) is unchanged and
  still comes first. This text is merged into ADR 0073 once it is accepted.
- Also changes: Debrief ADR 0017 §7 (the per-plugin elevated process is retired, §5 below) and the elevated recorder of
  Debrief ADR 0022 §7 (its symbol cache, E1 below). Each of those ADRs gets a one-line link to this amendment in WP-H7.
- Related: rule 75 (ADR 0092 §4, accepted 2026-11-10; its text is not in `docs/rules.md` yet, ADR 0092 WP-9), ADR 0092
  W2 and WP-7 (the elevated bootstrap), rule 53 / ADR 0033 (identity is pid plus start time, or a held handle), rule 73
  (ADR 0073), rule 55 (one implementation per mechanism), ER-22 / rule 42 (share first), ER-18 and ER-19 (long work),
  ER-27 (logs explain every failure), ER-33 (a check counts only if its probe can see), ER-11 (failing test first),
  family ADR 0053 (per-machine Windows installs; Proposed, user direction 2026-11-04), Dyno ADR 0027 (the family's
  privileged helper and its trust rules).
- Evidence: read on 2026-11-11 with `git show`, `git grep` and `git rev-parse` after `git fetch`. Paths without a
  ref are at `origin/main` `ea94d9f1e`. "Shipped" claims were checked by reading the same file at the tag
  `debrief-v1.1.1` (`33eba7841`); each row of §1 says whether it is in that tag. Nothing was built or run, and no user
  data was read. External behaviour that was read but not measured is marked as such. The heads, re-read right before
  the commit:

```evidence-heads
read: 2026-11-11
origin/main ea94d9f1e
debrief-v1.1.1 33eba7841
origin/docs/dyno-adr-0027-per-machine 3a6be49b5
engineering-rules HEAD 35701f739
```

## Trigger

ADR 0073 revision 3 found that `restart_elevated` (`apps/debrief/app/src-tauri/src/system.rs:120-136`, in
`debrief-v1.1.1` at the same lines) relaunches the whole app elevated with `ShellExecuteW` and the `runas` verb
(`system.rs:176`). That makes the three sidecar-path overrides (DB9) an environment-variable UAC bypass. Gating DB9
closes those three variables. It does not close the rest: an elevated Debrief also starts programs from places the
user's normal account can change.

| # | What an elevated Debrief starts | Where (main) | In `debrief-v1.1.1` |
| --- | --- | --- | --- |
| S1 | The browser at `settings.browser_path` | `app/src-tauri/src/browser/runtime.rs:72,92` | yes (`:72`) |
| S2 | The Chromium runtime downloaded into the data folder (`installed_exe`) | `browser/runtime.rs:73,94` | yes |
| S3 | A custom ffmpeg the user chose | `video/runtime_commands.rs:153` (`video_runtime_set_custom`) | yes (`:155`) |
| S4 | Third-party plugins from `<data>/plugins` | `app/src-tauri/src/lib.rs:638` (the plugins folder given to `AppState`) | yes |
| S5 | VS Code found through `PATH`, `LOCALAPPDATA` and `ProgramFiles*` | `opener.rs:172-190` (`find_vscode`) | yes (`:172`) |
| S6 | A browser found from `ProgramFiles(x86)`, `ProgramFiles` and `LOCALAPPDATA` | `crates/chromium-driver/src/discover.rs:44`, read through `std::env::var` at `:94` | yes (`:44`) |
| S7 | `logman` by bare name (perfmon collector sets) | `crates/debrief-plugin-windows/src/command.rs:73` (`quiet_command(program)`) with the bare `logman` of `perfmon_template.rs:365-368` | yes (`:73`) |
| S8 | Every sidecar next to `current_exe()`: the per-user install folder | Tauri's NSIS default `installMode` is `currentUser`; Debrief sets none (`tauri.conf.json`, no `bundle.windows.nsis`); ADR 0053 line 55 on `3a6be49b5` lists Debrief as `currentUser` for 1.0.0 to 1.1.1 | yes |

S6 is not measured: whether a `runas` child sees `HKCU\Environment` values of `ProgramFiles` and `ProgramFiles(x86)`.
The test VM cannot automate the UAC prompt yet, and an admin snapshot is being added (WP-H0 measures it). The shared
code already assumes the worst case: `paddock_service::privileged::known_folders` says "a UAC-elevated process can
inherit the user-writable `HKCU\Environment`" and resolves folders from the OS instead (`known_folders.rs:1-8`). S7's
search order is read from the Rust standard library documentation for `Command::new` on Windows
(<https://doc.rust-lang.org/std/process/struct.Command.html#method.new>: the application's folder comes before the
system folder). That is read, not measured. With S8 it means that a `logman.exe` planted in the per-user install folder
runs elevated.

Reading the code for this amendment found two more shipped weaknesses on the path that already elevates per plugin
(Debrief ADR 0017 §7). Both are in `debrief-v1.1.1`:

- **E1. The elevated native recorder reads and writes symbols in user folders.** `debrief-plugin-native/src/tool.rs:347`
  takes `paths::elevated_data_dir`, which accepts the host's data folder when it is under the user's `%APPDATA%` or
  `%LOCALAPPDATA%` (`paths.rs:61-75`). `:358` then uses it as the symbol cache, together with the user's symbol folders
  and an optional symbol server (`debrief-symbols/src/config.rs:17-24`). So an elevated process parses PDB files that the
  user's account can replace, and downloads into a folder that account controls. Rule 75 forbids both.
- **E2. The elevated plugin process is the whole `debrief-plugins.exe`.** It links all twelve built-in plugins,
  importers included (`debrief-plugins/src/main.rs:12-27`). The elevated role refuses every request except elevated tool
  operations (`debrief-plugin-api/src/serve.rs:96`, `:243-247`), so that code cannot be called. But it is in an
  administrator's address space, it comes from S8's per-user folder, and its bootstrap is the W2 file of ADR 0092
  (`debrief-ipc/src/elevation.rs:103-112`).

## 1. What needs administrator rights today (from the code)

| # | Operation | Code that needs the rights | How Debrief gets them today |
| --- | --- | --- | --- |
| A1 | .NET ETW capture (`etw-*` operations) | `debrief-plugin-dotnet/src/plugin/tool.rs:127-137` (`.elevated()`), `plugin/elevated.rs` | per-plugin elevated process (ADR 0017 §7) |
| A2 | Native ETW profiler (start, status, stop) | `debrief-plugin-native/src/tool.rs:396-400` (`.needs_elevation()`, `.elevated()`) | per-plugin elevated process |
| A3 | Performance Monitor collector sets: import, start, stop, delete with `logman` | `debrief-plugin-tools/src/perfmon_tools.rs:141`; access denied mapped at `perfmon_control.rs:44-46,127-128` | the relaunch (or the "Performance Log Users" group) |
| A4 | Reading and following the Security event log | `debrief-plugin-windows/src/eventlog/query.rs:50` (`needs_admin`); tool text `eventlog_tool.rs:224` | the relaunch |
| A5 | A dump of a process of another user, or of an elevated one | `debrief-plugin-dumps/src/capture.rs:203-207,252-254` | the relaunch |
| A6 | Process monitor samples of processes it cannot open | `debrief-monitor/src/tracker.rs:111-122` (`AccessDenied`) | the relaunch |
| A7 | Native sampling of an elevated process | `debrief-plugin-native/src/sampling/live.rs:170-172` sends the user to ETW, which is A2 | – |

Out of scope: `debrief-agent`'s service install (`debrief-agent/src/service/mod.rs:45`) is a command-line tool that
tells the user to use an elevated prompt. The app never runs it.

The UI offers the relaunch in five places: the shared error hint (`app/src/app/tools/elevation-hint.ts:14-15`), the
monitor (`tools/monitor/monitor-view.ts:45-47`), the event log view (`tools/eventlog/eventlog-view.ts:109-110`), the
command palette (`lifecycle/lifecycle-commands.ts:11`) and the menu (`shell/menu/menu-model.ts:347`). All of them go
through `tools-backend.ts:322-323` to the command `restart_elevated`, which is registered at
`app/src-tauri/src/lib.rs:331`.

## 2. Decision: one helper, started on demand, from a protected folder (D-H1, D-H2, D-H5)

- **One program, `debrief-admin.exe`.** A new crate `debrief-admin` (binary and library) with a **closed, typed
  request list** (§3). It links only the code that needs the rights: `debrief-etw`, the ETW halves of the .NET and
  native recorders, the dump writer, the event log reader for A4, the process reader for A6 and a `logman` runner. It
  links no importer, no plugin host, no browser, no symbol resolver and no network crate. The per-plugin elevated role
  of `debrief-plugins.exe` (ADR 0017 §7) is retired, so E2 goes. Plugins keep their port: `ToolCtx::elevated()` and
  `ElevatedOps::invoke` (`debrief-plugin-api/src/tools.rs:592-631`) stay, but the host routes them to the helper as
  typed requests instead of to a second copy of the plugin. Third-party plugins still cannot declare elevated
  operations (`debrief-plugin-host/src/installed.rs:502-505`, unchanged).
- **On demand, for the session.** The host starts the helper the first time an admin-only operation is asked. That is
  one UAC prompt, right after the user's own click. The helper serves until Debrief closes the connection, and then
  exits. It is not a service and does not run at boot (D-H5). It puts itself into the kill-on-close job object with its
  memory cap, as the elevated plugin process does today (ADR 0017 §7).
- **Windows only.** Off Windows, `debrief_ipc::elevation::decide_on` keeps answering `Denied` with "start Debrief with
  administrator rights (for example with sudo)" (`debrief-ipc/src/elevation.rs:41-50`). There is no setuid helper
  (D-H6).
- **A fixed, protected install path (D-H2).** Debrief 1.2 installs per machine, following ADR 0053's Debrief row
  (`3a6be49b5`, line 550: "in its next feature release (1.2)", 2.5–3 days). The helper is
  `<FOLDERID_ProgramFiles>\Debrief\debrief-admin.exe`. The host builds that path from
  `paddock_service::privileged::KnownFolders::program_files()` (`known_folders.rs:22-38`), never from `current_exe()`
  or the environment. Before the prompt, the host:
  - checks the chain with `paddock_service::privileged::trust::verify_chain` (`trust.rs:436`, the protected-chain rule
    of `trust.rs:1-18`) over `trust_real::RealTrust` (`trust_real/mod.rs:29`);
  - holds the image open with `trust_real::HeldImage::open` (`trust_real/mod.rs:164-195`) until `ShellExecuteExW`
    returns.

  The helper checks its own image the same way at start, from its token. It refuses to serve if the image is not
  protected, and exits with code 12 (§6).

## 3. What the helper may and may not do

**The request list (`ADMIN_API` 1).** Every request is decoded into a typed value at the edge (parse, don't
validate), checked again in the helper, bounded, and run with a time limit.

| Request | Arguments, validated in the helper | Answer |
| --- | --- | --- |
| `Info` | – | API version, the helper's own version, whether its image chain passed |
| `EtwDotnet { start, status, stop }` | target as `ProcessId` (pid and start time, `paddock_proc::ProcessId`, `crates/paddock-proc/src/identity.rs:37`), preset from a closed enum, duration and byte caps no larger than the plugin's limits | an output stream of NetTrace bytes, plus status |
| `EtwNative { start, status, stop }` | target `ProcessId`, `heap: bool`, the same caps | an output stream of raw events: stacks as module and offset, **not symbolised** (closes E1: symbols are resolved by the unelevated plugin) |
| `Logman { import, start, stop, delete, query }` | the set name, matched against `^Debrief[ A-Za-z0-9_-]{0,64}$`; for `import`, the template XML as **bytes** (at most 256 KiB), written by the helper into its own scratch folder (below) | exit code and output, at most 64 KiB |
| `SecurityLog { query, follow }` | a time window and a record cap (at most 100,000 per answer), or follow with credits | event records as stream items |
| `Dump { target, kind }` | target `ProcessId`; `kind` from a closed enum; LSASS and protected processes refused (D-H4) | an output stream of the dump bytes |
| `ProcessSample { targets }` | at most 4,096 `ProcessId`s | the fields the monitor reads today, nothing more |

**The helper never:**

- reads its environment: there is no `std::env::var`, `var_os`, `vars`, `vars_os` or `current_dir` in the crate.
  Clippy `disallowed-methods` enforces this in a crate-level `clippy.toml`, and a source-scan test checks it;
- opens a path a caller chose;
- writes into a folder the user can change;
- loads a plugin;
- talks to the network;
- runs a program by bare name.

It runs exactly one external program, `logman.exe`, by absolute path from `known_folders::system_tool_in`. That
function's list is closed (`SYSTEM_TOOLS = ["icacls", "reg", "sc"]`, `known_folders.rs:42`), and any other name is
"returned as given" (`:46`), so WP-H1 adds `logman`. That is a shared-crate change, made by the integrating agent.
The helper starts `logman` with `env_clear()` and only `SystemRoot`, taken from `system_directory()`.

**Its scratch folder** is the only place it writes. It creates the folder itself under `%ProgramData%\Debrief\Admin\`
(from `KnownFolders::program_data()`), with a High mandatory label and an owner-only DACL, and uses it only through the
handle it created. That is rule 75's third channel. The folder holds the `logman` template and, where the OS needs a
seekable file, the dump while it is written. Each file is deleted after its bytes have been streamed.

## 4. IPC and trust (rule 75)

- **No bootstrap file.** That closes W2 for this helper; it is ADR 0092 WP-7, built here once.
  - The host creates the pipe, as today: a random name, `first_pipe_instance(true)`, `reject_remote_clients(true)`,
    `max_instances(1)`, and the DACL `D:P(A;;GA;;;OW)(A;;GA;;;BA)` (`debrief-ipc/src/link.rs:296-336`).
  - The helper's arguments carry the pipe address, the requester's `ProcessId` and the session token.
  - The helper calls `GetNamedPipeServerProcessId` on the connected pipe and compares it with the requester's pid and
    start time **before it sends or reads anything**. A mismatch exits with code 11.
  - The helper also opens the requester by that verified identity, and refuses (code 13) if its image is not
    `<ProgramFiles>\Debrief\debrief-app.exe`. This is defence in depth only: UAC is not a boundary against code that
    already runs as the user (ADR 0017 §7, last paragraph).
  - `SandboxSpec` and `max_frame`, today fields of the bootstrap, become constants compiled into the helper.
- **The host checks the client** as today. The pid that `GetNamedPipeClientProcessId` reports must equal the pid of the
  process handle `ShellExecuteExW` returned (`link.rs:158-165`, `elevation.rs:266-270`). The host holds that handle, so
  the pid cannot be reused (rule 53). The token must match (`link.rs:152`).
- **Peer identity beyond the pid** is the token on the helper's side and the held process handle on the host's side.
  `paddock-ipc`'s `identity` (`crates/paddock-ipc/src/identity.rs`) and the squatting check in `trust.rs` are built for
  a **system** endpoint, a service that many accounts call (Dyno ADR 0027 §2). The helper is a private one-to-one child.
  So Debrief keeps `debrief-ipc` for the wire and the launcher, and reuses the family's `paddock-service` modules for the
  protected-chain and known-folder checks (ER-22). It does not reuse `paddock-ipc`'s system endpoint. Moving Debrief
  onto `paddock-ipc` is not part of this amendment.

## 5. How results come back, and what goes

- **Small answers** are typed replies under the frame limit.
- **Bulk bytes** (ETW traces, dumps) use the existing output stream with credits: the helper streams, and the
  unelevated host writes into the folder it chose (`debrief-plugin-host/src/output.rs`, the protocol 1.1 amendment of
  ADR 0017).
- **Live items** (Security log follow, ETW status) are stream events.
- **Progress** is reported as task events, as the elevated process does today.
- **The helper's standard error** is forwarded over the connection (`serve.rs:35-36`), so it writes no log file of its
  own.

**What is removed:**

- The `restart_elevated` command, its line in `lib.rs:331`, `relaunch_elevated`, `quoted` and `can_restart_elevated`
  (`system.rs:39-40,65,117-144,171-199`);
- the TypeScript call and type (`tools-backend.ts:99,209,322-323`, `tools.types.ts:11,321-322`), the mock
  (`mock-tools-backend.ts:590-598`), and the five UI entry points of §1;
- the `--elevated` role of `debrief-plugins` and the `elevate` field (`debrief-plugin-host/src/process.rs:60-63,
  773-777`);
- `paths::elevated_data_dir`.

The `elevation` query stays and answers `elevated` and `platform`. Tests that name the command are updated:
`lifecycle.rs:968` and `tools-backend.spec.ts:42`.

**A Debrief that the user started elevated by hand** (UAC switched off, "Run as administrator", or `sudo`) needs no
helper: `decide_on` already takes a plain spawn when the process is elevated (`elevation.rs:44`). Under D-H3 (a) it
runs in **elevated mode**. It does not start S1 to S7:

- no configured or downloaded browser, and no browser found through the environment;
- no custom ffmpeg;
- no third-party plugins;
- no VS Code;
- `logman` only by its system path.

A browser installed under Program Files is used only if `verify_chain` passes, with folders from `KnownFolders`. A
banner says so.

## 6. Failure states, wording and long work (ER-18, ER-19)

Copy is in sentence case. Every state has a next step.

| State | What the user sees |
| --- | --- |
| Before the prompt | Dialog: "Windows will ask for permission to start Debrief's administrator helper. It records and reads only what you ask for here." Buttons: **Continue**, **Cancel**. |
| Waiting for the prompt | Phase "Waiting for administrator permission". **Cancel** stops waiting; a helper that arrives later finds the connection closed and exits. |
| Declined (`ERROR_CANCELLED`, `elevation.rs:250,273-276`) | "Administrator permission was declined. Nothing was changed." **Try again** |
| Helper missing or not protected (chain check, exit 12) | "Debrief's administrator helper is missing or is not in a protected folder. Reinstall Debrief for all users." |
| Pipe not verified (exit 11 or 13) | "The administrator helper could not confirm it was started by this Debrief, so it stopped. Try again; if it happens again, export a diagnostic bundle." |
| Helper stopped during work | "The administrator helper stopped (exit code N). Your other work is not affected." **Try again**. A stream cut short is kept and marked "incomplete", as the `admin-save-crash` test op already checks (`debrief-plugin-host/src/bin/fake_plugin.rs:111-112`). |
| Not on Windows | "This needs root. Start Debrief with sudo to use it." |
| Elevated mode | Banner: "Debrief runs as administrator, so programs from folders your account can change are not started (browser, ffmpeg, plugins, VS Code)." |

Long operations (an ETW capture, a dump of a large process, reading back a week of the Security log) run as jobs with
labelled phases: "Waiting for permission", "Recording" or "Reading", "Writing the dump", "Copying to Debrief". Each
phase counts bytes or records from the first moment. The UI never times a job out: it shows "Still working". Cancel
works in every phase: it sends `stop` and keeps what was written, marked incomplete.

## 7. Logging (ER-27)

Host, per helper start:

- the path launched and the `verify_chain` outcome (`Untrusted::describe`, `trust.rs:256`);
- prompt shown, accepted or declined, with milliseconds;
- the helper's pid and start time;
- the handshake outcome;
- the exit code, mapped to text by a table both sides share (10 bad arguments, 11 server not verified, 12 own image not
  protected, 13 requester image wrong, 14 scratch folder refused).

Every request gets a correlation id, its kind, duration, outcome and bytes or records. The helper writes its phases
and refusals to forwarded stderr. User content stays out of the logs: no paths, set names in full or event text, only
kinds, counts and pids. The decisions that happen before the helper is connected are logged by the host from the exit
code, because the helper cannot reach the host then.

## 8. Tests

The work starts from these failing tests (ER-11):

1. `restart_elevated` is no longer a command: `ipc(main, "restart_elevated", …)` reports an unknown command. This
   fails today, because the command exists.
2. A `known_folders` fake with a poisoned environment (`ProgramFiles`, `SystemRoot`, `PATH`, `DEBRIEF_PLUGINS` set to
   a scratch folder): the helper's launch path and `logman` path come only from the fake's answers.
3. A squatting server, a pipe created by a process other than the requester: the helper exits 11 before it writes a
   byte.
4. A client whose pid is not the started process: the host refuses it. Existing, kept.
5. The request decoder refuses an unknown kind, a set name outside the pattern, more than 4,096 targets and a template
   over 256 KiB.
6. The `Dump` decision refuses LSASS by image path from the system folder.
7. The helper crate has no `std::env` call: Clippy, plus a source scan with a planted positive control.
8. Elevated mode: with `process_elevated` true, S1 to S6 are refused with the banner's reason.

Windows CI (unelevated) runs the helper as a plain child. The switch for that is a `test-support` launch option, not an
environment variable (rule 73). It goes through the real pipe and the real output stream, including a crash in the
middle of a stream.

**The Windows test VM's admin snapshot** (local only, never CI) runs the real prompt:

- accept, decline and cancel-while-waiting;
- `HKCU\Environment` poisoned with `ProgramFiles`, `ProgramFiles(x86)`, `SystemRoot`, `PATH` and `DEBRIEF_PLUGINS`,
  then a full run that lists every elevated process created (process-creation events with their token elevation),
  which must be only `debrief-admin.exe` and `%SystemRoot%\System32\logman.exe`;
- A3 to A6 end to end.

**Positive control (ER-33):** on the same snapshot, `debrief-v1.1.1`'s "Restart as administrator" with
`HKCU\Environment` `DEBRIEF_PLUGINS` set to a marker program must show the marker running elevated. Without that, the
process-list probe has not shown it can see the thing. The same run measures S6. The installer is built with gates on
(rule 54).

## 9. Debrief 1.1.x

Nothing changes in 1.1.x. The user decided on 2026-11-09 (DB9) and again on 2026-11-11 to fix this in 1.2 only. S1 to
S8, E1, E2 and W2 stay in `debrief-v1.1.1` and are written in the 1.2 release notes as fixed.

## Prerequisites and what is built

| Mechanism | State on `origin/main` `ea94d9f1e` |
| --- | --- |
| `debrief_ipc::elevation` (`decide_on`, `run_as`, cancel code), `link.rs` pid check and pipe DACL, the output stream, forwarded stderr, `ElevatedOps` | built (cited above) |
| `paddock_service::privileged::{known_folders, trust::verify_chain, trust_real::HeldImage}` | built; `system_tool_in`'s list is closed and lacks `logman` (WP-H1 adds it, shared-crate change) |
| `paddock_proc::ProcessId` | built (`identity.rs:37`) |
| `GetNamedPipeServerProcessId` check (ADR 0092 WP-7) | **not built** (no use in `apps`, `crates` or `tools`); built here in WP-H1 |
| Rule 75 text (ADR 0092 WP-9) | **not built**: `docs/rules.md` ends at rule 71 |
| Debrief per-machine install (ADR 0053's Debrief row) | **prerequisite**: ADR 0053 is Proposed on `origin/docs/dyno-adr-0027-per-machine` `3a6be49b5`; its shared parts are on main (`crates/paddock-service/src/per_machine.rs`; Dyno and Pitwall have `windows/per-machine.json`) |
| Windows test VM admin snapshot with UAC automation | **prerequisite**, being added, not on main |
| `./dev check test-doubles` (ADR 0073 WP-0), which sees the `test-support` launch option | **not built** (no `test-doubles` in `tools` or `conformance`) |

## Work packages

Estimates are raw days of agent work, before review rounds.

| WP | Package | Depends on | Days |
| --- | --- | --- | --- |
| WP-H0 | VM spikes on the admin snapshot: S6 measured, the positive control, `GetNamedPipeServerProcessId` across integrity levels, the job object under `runas`; a research note | admin snapshot | 1–1.5 |
| WP-H1 | `debrief-admin`: the request enum and decoder, the verified pipe (tests 3 and 5 first), the image chain check, no environment (tests 2 and 7), the scratch folder, exit codes, `logman` added to `system_tool_in` | – | 3–4 |
| WP-H2 | The requests: the ETW halves of A1 and A2 moved (native symbolisation moved to the unelevated plugin, E1), A3 to A6, the LSASS refusal (test 6) | WP-H1 | 3–4.5 |
| WP-H3 | Host: launch from the protected path with `HeldImage`, one helper per session, `ElevatedOps` routed to it, the `--elevated` role and `elevated_data_dir` removed | WP-H1 | 2–3 |
| WP-H4 | `restart_elevated` removed (test 1 first), the UI states of §6 in all five places, elevated mode (test 8), mocks and i18n | WP-H3 | 1.5–2 |
| WP-H5 | Debrief per machine (ADR 0053's row, counted there, not here) | ADR 0053 | (2.5–3) |
| WP-H6 | Windows CI legs and the VM run of §8; installer build (rule 54) | WP-H2 to WP-H5 | 1.5–2 |
| WP-H7 | Text: merge into ADR 0073, link lines in ADR 0017 §7 and 0022 §7, lessons entry, wiki notes, release notes | – | 0.5 |
| WP-HR | Independent review of this amendment, and of WP-H1's trust code | – | 0.5–1 |

**Total: 13 to 18.5 days raw**, plus 2.5 to 3 for the per-machine install that ADR 0053 already counts. ADR 0092 WP-7
(3 days) shrinks by about 1.5: the verified pipe is built in WP-H1, and only the IPC socket folder and Pitcrew remain
there. WP-H2 and WP-H3 can run in parallel after WP-H1. Everything gates `debrief-v1.2.0` (D-H7).

## Decisions for the user

| # | Decision | Recommendation |
| --- | --- | --- |
| D-H1 | Shape: (a) one dedicated `debrief-admin` with a closed request list, and the per-plugin elevated role retired; (b) keep ADR 0017 §7's per-plugin elevated process and add A3 to A6 as elevated operations; (c) a LocalSystem service like Dyno's | **(a)**. (b) is about 3 days cheaper, but it keeps twelve plugins, importers included, in an administrator's process (E2) and can mean up to four prompts per session. (c) is always on as SYSTEM for work Debrief needs rarely, and adds install and update paths. |
| D-H2 | Fixed path: (a) Debrief per machine in 1.2 (ADR 0053), helper in `Program Files\Debrief`; (b) Dyno's switched-on protected copy (`Program Files\Debrief Helper`, copied on first use; Dyno ADR 0027 R1 risk); (c) leave the per-user folder | **(a)**. The user already set this direction for every app on 2026-11-04. It closes S8 for every sidecar, and it has no first-run R1 window. (c) does not close S8. |
| D-H3 | A Debrief started elevated by hand: (a) elevated mode (S1 to S7 refused, with a banner); (b) refuse to start elevated; (c) unchanged | **(a)**. (b) breaks machines with UAC switched off, where every process is elevated. (c) leaves S1 to S7 open. |
| D-H4 | Dumps through the helper: (a) refuse LSASS and protected processes; (b) any process, as the relaunch allows today | **(a)**. A same-user program that drives Debrief should not get credential memory through one prompt. |
| D-H5 | Helper lifetime: (a) the session, started on first use; (b) one prompt per operation; (c) exit after 30 idle minutes | **(a)**. One prompt per session, right after a click. (b) trains click-through. |
| D-H6 | macOS and Linux: no helper in 1.2; admin operations keep "start Debrief with sudo", and elevated mode applies to root | **Yes.** ETW, `logman`, the Security log and the A5 dumps are Windows only. |
| D-H7 | Gate: WP-H0 to WP-H6 block the `debrief-v1.2.0` tag | **Yes.** It is the second half of the DB9 fix the user put in 1.2. |
