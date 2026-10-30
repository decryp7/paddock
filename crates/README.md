# Shared Rust crates

A Cargo workspace for Rust code the apps share. Members: `paddock-jobs` (ADR 0008) and the app shell crates of ADR 0012
(`paddock-commands`, `-dialogs`, `-download`, `-health`, `-menu`, `-settings`, `-startup`, `-updater`, `-window`, and
`paddock-shell`, the Tauri plugin that wires them; plan [docs/plans/workbench.md](../docs/plans/workbench.md) S0-S17). Crates are extracted from the apps one
at a time (a crate moves here when a second app needs it, rule of three for new abstractions, two for copied code).

Rules for a crate here:

- Pure logic and thin adapters, no dependency on any app crate. Edition and licence come from `[workspace.package]`.
- Library errors are typed, no `unwrap`/`expect` on user data or I/O, `cargo clippy -- -D warnings` clean,
  >= 99% line coverage.
- Native glue (code only a real desktop runs, that tests cannot reach by rule 7 or because Tauri does not let them build
  the value: a real tray icon, window effects, `RunEvent::ExitRequested`) lives in one small file per crate named
  `native.rs` under the adapter and is left out of the coverage job (`--ignore-filename-regex` in the `crates` job of
  `.gitea/workflows/ci.yml`). Native glue is never logic: each function hands over to tested code or makes one platform
  call, and it is reviewed by hand. Today: `paddock-window/src/tauri/native.rs`, `paddock-panels/src/tauri/native.rs`, `paddock-updater/src/tauri/native.rs`, `paddock-secrets/src/os/native.rs`, `paddock-diag/src/tauri/native.rs`, `paddock-service/src/native.rs`, `paddock-metrics/src/native.rs`, `paddock-build/src/native.rs`, `paddock-siblings/src/native.rs`.
- A pure core with no Tauri dependency, and a `tauri` feature for the adapter (`src/tauri.rs`, `pub mod tauri` behind
  `#[cfg(feature = "tauri")]`). Tauri comes from `[workspace.dependencies]` without default features (the app enables
  the runtime); CI builds every crate with `--all-features` on Linux, Windows and macOS.
- A crate that uses another one writes `paddock-jobs = { workspace = true }` (the workspace dependency carries path,
  version and registry, so the published crate points at the registry).
- `publish = ["decryp7"]` in every crate. A new crate starts with `[package.metadata.paddock] scaffold = true`, which keeps
  the publish job from uploading it; its work package removes the marker. A new member needs no CI step of its own: the `crate`
  job's workspace-wide steps (and the coverage job) test every member; `scripts/publish.test.mjs` and conformance C37 fail
  when a member is left out (an `--exclude`). A crate whose code changes with an optional feature off joins the job's
  "Tests with the optional features off" step.
- Published to the Gitea cargo registry by the tag workflow only (`.gitea/workflows/publish.yml`, rule 9). One repo tag
  `vX.Y.Z` releases everything; each crate bumps its own `version` only when it changes, and a version the registry
  already has is skipped. `node scripts/publish.mjs` is the local dry run (lists every crate and npm package with what a
  tag would publish).

## Consuming the crates

Apps consume the crates from the Gitea cargo registry (decision D3), never from a git tag. In the app's
`.cargo/config.toml` (no token needed to read):

```toml
[registries.decryp7]
index = "sparse+https://dev.decryptology.net/api/packages/decryp7/cargo/"
```

and in its `src-tauri/Cargo.toml` (pre-1.0: `0.N` accepts patches of that minor only), with `Cargo.lock` committed. An app
on the shell needs only the plugin; it names a shell crate itself only for the types it uses in its own code:

```toml
[dependencies]
paddock-shell = { version = "0.2", registry = "decryp7" }                       # the plugin (lockstep with @decryp7/paddock-ui)
paddock-health = { version = "0.1", registry = "decryp7", features = ["tauri"] } # e.g. ServiceSpec, Cause for its supervisors
paddock-jobs = { version = "0.1", registry = "decryp7", features = ["tauri"] }   # its own jobs
```

The app's `deny.toml` allows the registry: `allow-registry = ["https://github.com/rust-lang/crates.io-index",
"sparse+https://dev.decryptology.net/api/packages/decryp7/cargo/"]`. Renovate opens one PR per Paddock release (crates
and npm package together); it lands only through the app's own gates. `paddock-shell` and `@decryp7/paddock-ui` are
bumped together (the UI checks `shell_version` at start-up); every other crate keeps its own version.

**The shell in an app** (ADR 0012 §3): one plugin line, the app's own table next to it, and one capability set per
window class.

```rust
// src-tauri/src/main.rs
let shell = paddock_shell::Builder::new(paddock_shell::AppIdentity::new("pitwall", "Pitwall", env!("CARGO_PKG_VERSION")))
    .app_commands(&COMMANDS)                                       // the app's own command_table! (checked against the shell's)
    .dialogs(&[&dialogs::DIALOGS])                                 // the app's dialog table (ADR 0026 §3); the shell's are built in
    .health(vec![ServiceSpec::new("llm", "the model server").repair(restart_llm)])
    .section(paddock_settings::section(models_section(), apply_models))
    .updater(|_| UpdatesBuilder::new("Pitwall").public_key(option_env!("TAURI_UPDATER_PUBKEY")).changelog(include_str!("../../CHANGELOG.md")))
    .windows(|_| Windows::builder(WindowConfig::new("Pitwall")).build())
    .build();
tauri::Builder::default()
    .plugin(shell)
    .invoke_handler(paddock_commands::tauri::Guard::new(&COMMANDS).dialog_grants(paddock_dialogs::tauri::grant_of).wrap(handler!()))
```

```json
// src-tauri/capabilities/shell.json (one capability per window class; the guard narrows dialogs to their contracts)
[
  { "identifier": "shell-main", "windows": ["main", "w-*"], "permissions": ["paddock-shell:main"] },
  { "identifier": "shell-popout", "windows": ["p-*"], "permissions": ["paddock-shell:popout"] },
  { "identifier": "shell-dialog", "windows": ["d-*"], "permissions": ["paddock-shell:dialog"] },
  { "identifier": "shell-tray", "windows": ["tray", "tray-*"], "permissions": ["paddock-shell:tray"] }
]
```

and in a test of the app: `assert_eq!(shell().problems(Some(include_str!("../paddock-manifest.json"))), Vec::<String>::new())`
(see `paddock-shell` below). The UI side is in [ui/README.md](../ui/README.md) ("Adopting the shell").

Crates, built and planned, with their designs in [`docs/adr/`](../docs/adr/README.md):

| Crate(s) | Purpose | Three users today? |
|---|---|---|
| `paddock-jobs` | long operations: job/phase/progress model, throttled reporter, cancel token, hash-while-writing, parallel walk/map/pipeline, job pool (ADR 0008); `time`: the family's clock port, fake clock, deadlines, restart back-off (plan X20) | yes (all five apps); built |
| `paddock-commands`, `-health`, `-dialogs`, `-startup`, `-download`, `-settings`, `-updater`, `-window`, `-menu`, `-shell` | the app shell (ADR 0012) | yes; all built (plan S1-S17), `paddock-shell` is the plugin that wires them |
| `paddock-secrets` | credentials: `Secrets` port, `Secret` that never prints, keyring adapter, `MemorySecrets`, opt-in owner-only file fallback, naming scheme (D6), one-time rename (plan X5) | yes (Debrief, Pitwall, Pitcrew, Spotter, Sighter PoC); built |
| `paddock-pitwall` | Pitwall client: discovery, pairing, keychain keys, typed requests, optional-AI state (ADR 0004) | yes (Debrief, Pacenotes, Sighter; Spotter, Dyno later); built (plan section 15, with the loopback HTTP clients of X16) |
| `paddock-assistant` | the assistant (plan W6): `LlmProvider` port, prompted tool protocol, `AssistantTool` with budgets and an approval step for writes, the bounded cancellable tool loop on an injected clock, versioned bounded conversations, the Pitwall adapter, Tauri commands and the `paddock://assistant` event | yes (Pacenotes, Sighter; Debrief's chat moves onto it); built |
| `paddock-guard` | crash containment (ADR 0001, plan X51): `catch`, `Quarantine` (3 failures in 10 minutes, shared `Clock`, persisted through paddock-env), `Guard::call` (time budget, hang abandoned, typed refusal) | yes (Debrief, Pacenotes, Dyno plugin hosts); built |
| `paddock-ipc` | the one IPC (ADR 0001 step 1, plan X13): frames, private endpoint, token + pid handshake, peer (calls, streams with credits, cancel, pushes), launcher and child loop, and the companion `link` (discovery file, reconnect back-off, replace another build) | yes (Debrief, Dyno, Pitcrew); built |
| `paddock-plugin-package` | the `.paddockplugin` archive, the plugin manifest model (permissions, contributions, namespaces), Ed25519 signing, keys, trust levels and store, installed-binary verification (ADR 0001, plan X51) | yes (Debrief, Pacenotes, Dyno tier 3); built |
| `paddock-view` | the declarative view model plugins return (ADR 0002, plan X51): `ViewDoc`, nodes, actions, patches, JSON + postcard, the limits validator, generated TypeScript | yes (Debrief, Pacenotes, Dyno tier 3); built |
| `paddock-plugin-api` | what an out-of-process plugin links (ADR 0001, plan X51): platform protocol 2.0 (`PlatformRequest`, `HostCall`), `Plugin` and `Contributions` (tools with checked schemas, settings, views, MCP tools), the `Extension` mechanism for an app's own contract, version rules with refusal texts that say who must update, host files, output files, `serve` | yes (Debrief, Pacenotes, Dyno tier 3); built |
| `paddock-plugin-host` | what an app links to run plugins (ADR 0001, plan X51): installed `.paddockplugin` packages (verified before every start), grants (default deny) and the pure `sandbox_for` mapping, a `Supervisor` per plugin (restart with back-off, quarantine through `paddock-guard`), guarded `Remote<C>` proxies, grant-checked host files, outputs and elevated operations, status for the UI | yes (Debrief, Pacenotes, Dyno tier 3); built |
| `paddock-plugin-test` | testing plugins (ADR 0001, plan X51): `FakeHost` (a plugin in this process over a real IPC pair), the conformance checks every plugin runs, golden helpers, and `paddock-plugin-fixture`, the sample plugin the host's tests start as a process | yes (every plugin, the host's tests); built |
| `cargo-paddock-plugin` | the plugin author's tool (ADR 0001, plan X51): `cargo paddock-plugin new / build / pack / keygen / sign / verify / run / test` on `paddock-cli` conventions; a project that depends on `paddock-plugin-api` alone, a deterministic optionally Ed25519-signed `.paddockplugin`, verification against trusted keys, and the built program started against `FakeHost` for the conformance checks (from Debrief `cargo-debrief-plugin`) | yes (every plugin author, plugin CI); built |
| `paddock-mcp` | MCP protocol, tool schemas, limits, paging, audit log, local transports (ADR 0007) | three (Pacenotes, Sighter, Debrief); extracted with Debrief's server, after Debrief 1.0.2 |
| `paddock-testplan` | `.sighterplan` / `.sighterresult` (ADR 0005) | two (Pacenotes, Sighter); shared schema, so extracted on purpose |
| `paddock-mcp` | local-only MCP server (ADR 0007): JSON-RPC 2.0 with the 2025-06-18 shapes, tool and resource registry (read-only by default, actions need an explicit opt-in, off until the user turns it on), schema-validated arguments, result caps, paging, time limits, redacted audit log, stdio and loopback (per-client key) transports, client request builders for Pitwall (plan X22) | yes (Debrief, Pacenotes, Sighter servers; Pitwall client); built |
| `paddock-testplan` | `.sighterplan` / `.sighterresult` as TOML (ADR 0005): typed ids and a closed `Step` enum, secrets by reference with redacting writers, canonical writer, traceability `Coverage`, plan `diff`, JUnit export, limits and a fuzz target (plan X52) | two (Pacenotes, Sighter); built |
| `paddock-archive` | safe zip/tar/tar.gz/tar.xz unpacking: limits on entries, sizes and expansion ratio, traversal and link guards, layout and filter, all-or-nothing extract through a staging folder swapped in, progress and Cancel as a paddock-jobs job, fuzz target (plan X24) | yes (Debrief AI/video runtimes, Pitwall engines, Spotter renderer, Pitcrew runners); built |
| `paddock-bench` | benchmark harness with stored budgets (rule 16): `Recorder` with an injected clock (startup, throughput, first event, query latency, memory), a results file per app, `budgets.toml` with tolerance and regression limit, `check`, and the `paddock-bench` command for the nightly workflow (plan X26) | yes (every app's benchmarks); built |

| `paddock-cli` | command-line conventions: stable exit codes, errors with a next step, `--json` as one object (errors too), tables as text/CSV/JSON, throttled progress on stderr, `--help`/`--version` style, time and size arguments, the Ctrl+C flag (plan X25) | yes (Pitwall, Dyno, Debrief CLIs; agent and service programs); built |
| `paddock-env` | per-OS data, config, cache, log and runtime folders (`<APP>_HOME` override in debug builds, portable mode), `VersionedFile` (atomic write with `.bak`, migrations, damaged or newer handled) (plan X7, D3) | yes (all apps); built |
| `paddock-files` | user files: path grants (open only paths the user chose, canonicalised), validated open/save pickers behind a port, recent lists (recorded only after a granted open, re-granted on use), reveal, open with the default program (never what runs on open) and web addresses (plan X31; from Debrief `paths.rs`, `recent.rs`, `opener.rs`) | yes (all apps; Pacenotes and Sighter from their first commit); built |
| `paddock-storage` | what an app keeps on disk and clearing it safely: declared categories (cache the app can rebuild, the user's data), a bounded size walk (entries, time on an injected clock, depth, no links, one volume), a preview that is measured first, a re-check before every delete, confirmation for data, nothing outside the app's own folders, as jobs with progress and Cancel (plan X32; from Debrief `storage/`, Dyno `cleanup/{walk,guard}.rs`, Pitwall `leftovers.rs`, Pitcrew cache clean) | yes (all apps); built |
| `paddock-live` | the live feed (plan X29): `Hub<S: LiveState>` holds the state and a sequence number, queues `Frame`s (snapshot, delta `from..=seq`, link) for bounded `Subscription`s (latest-wins merging, a slow consumer is given a snapshot), `run` keeps a `Source` connected with `Backoff` on the shared `Clock` and announces the `Link`; the `tauri` feature emits `paddock://live/<channel>` and answers `live_snapshot` | yes (Pitwall, Dyno, Pitcrew, Spotter, Debrief); built |
| `paddock-metrics` | tiered metrics store and machine sampler (plan X30): generic `TieredStore<P: Series>` with a checked `Retention` (Pitwall's 4-tier table by default, Pitcrew's sparse table), `MachineSample` series (CPU, memory, free disk, own memory), `Sampler` on the shared `Clock`, free space of a path's volume, bounded `folder_size`, `StoreFile` persistence through paddock-env | yes (Pitcrew, Pitwall, Dyno, Debrief agent); built |
| `paddock-testkit` | Rust test kit (dev-dependency): `Scratch` (work, home and tmp folders; tmp must be empty at the end), `env_lock()` + `EnvLock::hermetic`, golden files (`PADDOCK_UPDATE_GOLDEN=1`), `FakeRunner` for OS-command seams, `corpus`/`never_panics`, re-exported `FakeClock` (plan X40) | yes (all apps); built |
| `paddock-packs` | knowledge packs (rule 21): TOML packs, built-in plus the user's `packs/` folder, parsed once with limits, unknown keys reported, a bad pack refused whole without touching the rest, debounced hot reload with a swap, `user_regex` with caps (plan X15, D11) | yes (Spotter, Pitwall, Dyno, Debrief); built |
| `paddock-watch` | folder watching: `Debouncer` on the shared `Clock` (quiet time plus longest wait), the `ChangeSource` port with a `notify` adapter and a watcher thread, Debrief's settle rule (`Settle`, `Batcher`, `scan_folder`) for files that are still being written (plan X23) | yes (Debrief, Spotter, Dyno, `paddock-packs`); built |
| `paddock-log` | the app and service log: CLEF JSON lines, one file per process per day in the OS log folder with a size budget and old days removed, level at run time, `log` + `tracing` in one stream, redaction hooks (never file contents by default), error de-duplication with breadcrumbs, panic hook with crash marker, child-output capture (plan X1, D1/D2) | yes (all apps and services); built |
| `paddock-diag` | report a problem: a diagnostics zip of the logs, system facts and settings that the user previews file by file and saves where they choose; one `Redactor` for secrets, personal data and file contents; app sections; the Logging settings section; nothing is ever sent (plan X2, D9) | yes (all apps); built |
| `paddock-service` | background services and login items: one `ServiceSpec` rendered as the launchd plist (bundled or file), systemd unit, Windows `Run` value or service and NSIS hooks; registration and repair behind ports (rule 24), registered version, exe-changed restart, launch at login, health entry (plan X4, D4) | yes (Pitwall, Pitcrew, Dyno, Debrief agent); built |
| `paddock-repo-files` | a bounded, `.gitignore`-aware repository walk and SHA-256 hashing with deterministic results (ADR 0022 §6) | yes (`paddock-dev`; Pacenotes' index later); built |
| `paddock-repo-graph` | the project graph: Cargo and npm projects and edges (Tauri `frontendDist`, tsconfig, angular.json, ESLint config references), file to project, the affected closure, lockfile differences and a `History` port; git and `cargo metadata` adapters behind features (ADR 0022 §6) | yes (`paddock-dev`; Pacenotes later); built |
| `paddock-slots` | numbered slots over OS file locks the kernel releases when the holder dies, a holder protocol and a typed API: no stale reclaim, so no reclaim race (ADR 0022, the CI kit's job isolation) | yes (`paddock-dev ci slot`); built |
| `paddock-siblings` | hand-off between the family's apps: the apps and what each accepts (`docs/ecosystem.md`), a typed `Handoff` (open, import files, show an item), discovery of an installed sibling per OS, starting it detached with paths or a deep link, plain "not installed" and "needs update" answers, the receiving side's parsers (plan X45; from Pitcrew's `debrief_link.rs` and `handoff.rs`) | yes (Pitcrew, Dyno, Debrief, Spotter); built |
| `paddock-names` | identifier search (rule 20): word segmentation, name normalisation (`Path` and `Text` styles), grouping of spellings, ranked and explained matching of parts, joins, separators, case, plurals, filler words, word order and typos, clarifying question on a true tie (plan X18; from Debrief `names/` and Dyno's counter search) | yes (Debrief, Dyno; Pitwall, Spotter, Pacenotes later); built |
| `paddock-channels` | outbound notification channels: webhook, ntfy and e-mail (STARTTLS, TLS, none), secrets from `paddock-secrets` (scope `channel:<id>`), bounded payloads, retries with the family's back-off on an injected clock, test send before and after saving; ports `HttpPoster`/`MailSender` with real adapters and in-process fakes (plan X17) | yes (Pitcrew, Spotter; Dyno, Debrief later); built |
| `paddock-net` | outbound HTTP policy: one `Egress` door with https only (plain http to this computer), reach limits (public / LAN / loopback) checked on every address and redirect hop and on the addresses the socket connects to, link-local/metadata never reachable, body and redirect caps, host allow-list, offline switch, per-host gap, conditional requests; `Wire`/`Lookup` ports, `UreqWire` on paddock-download's TLS and proxy set-up with connection reuse, fakes, `paddock_download::Transport` adapter (plan X16) | yes (Pitwall, Spotter, Pitcrew, Debrief, channels); built |
| `paddock-proc` | child processes: no console window, timeouts, output caps, kill-tree, supervised children with a memory cap, process memory (PSS), login `PATH` (plan X6) | yes (all apps); built |
| `paddock-window` | native window feel, tray, single instance | yes |
| `paddock-panels` | pop-out panel windows, drag ghost, UI-state forwarding (X50 W3) | yes |
| `paddock-notify` | OS notifications | borderline (two to three) |
| `paddock-chromium` | the one Chromium driver: discovery (tests only `CHROMIUM_TEST_PATH`), one command-line builder with the start page last, `Sandbox::decide`, supervised launch with a time limit on an injected clock and a memory cap, DevTools client, pages, readiness and challenge rules, AI cue, the pinned Chrome for Testing runtime, a fake browser for tests (plan X14) | yes (Debrief, Spotter; Sighter later); built |
| `paddock-runtime` | pinned runtimes (Chromium, FFmpeg, llama.cpp): a TOML catalogue per runtime (per-platform URL, size, SHA-256), a versioned store that installs through `paddock-download` and `paddock-archive` as one job (download, unpack, self-test, install), keeps the previous release for rollback and quarantines a release that fails (plan X14) | yes (Debrief: Chromium, FFmpeg, llama.cpp; Spotter: Chromium; Pitwall: llama.cpp); built |

## `paddock-jobs` Tauri adapter (plan S9)

With the `tauri` feature, `paddock_jobs::tauri` is the jobs part of the shell (ADR 0008 open question 2, ADR 0012 §3):

```rust
tauri::Builder::default()
    .setup(|app| {
        let jobs = paddock_jobs::tauri::manage(app.handle()); // Arc<JobRegistry>; snapshots go out on `paddock://jobs`
        app.manage(AppState { jobs, pool: JobPool::new(Workers::auto()) });
        Ok(())
    })
    .invoke_handler(paddock_jobs::tauri::route(
        paddock_jobs::tauri::handler(),                    // or handler_with_grants(lookup): dialogs by their contracts
        paddock_commands::tauri::Guard::new(&COMMANDS).wrap(handler!()),
    ))
```

- `Emit` is the `EventSink` (event `EVENT` = `paddock://jobs`, every snapshot to every webview); `Jobs` is the managed state.
- `COMMANDS` (a `command_table!`): `jobs_snapshot` (query: every known job, ordered by id) and `job_cancel { id }`
  (mutation: false for an unknown or ended job). Both are async, return at once and never wait for a job; an app without
  the state gets `{ code: 'notSetUp', message, retryable: false }`. Starting work stays with the app: its command registers
  a job, submits it to its `JobPool` and returns the id.
- `route(jobs, other)` sends the table's commands to the jobs handler and the rest to the app's, for an app without the
  `paddock-shell` plugin (which serves these commands itself; `invoke_handler()` is the unguarded handler it composes).
- Generated TypeScript in `ui/projects/paddock-ui/shell/jobs/generated/` (`PADDOCK_WRITE_TS=1 cargo test -p paddock-jobs
  --features tauri --test it tauri::generated`): `jobs-commands.ts` (`JOBS_COMMANDS`, `JobsCommand`) and `jobs-fixtures.ts`
  (one snapshot of every shape, written by the real reporter, decoded by the UI's specs).

## `paddock_jobs::time` (plan X20)

One clock for the family (Rule 40 fix for tests that asserted wall-clock limits, lessons "Tests that measure the wall
clock fail under load"). `paddock-log` and `paddock-health` re-export these types; apps delete their `clock.rs`/`time.rs`
ports and back-off loops (see the plan's X20 row).

```rust
use paddock_jobs::time::{Backoff, BackoffPolicy, Clock, Deadline, FakeClock, Retry, SystemClock};
let clock: Arc<dyn Clock> = Arc::new(SystemClock);              // FakeClock in tests
let token = CancelToken::with_deadline_on(clock.clone(), limit); // or JobRegistry::register_with_deadline (registry's clock)
clock.wait(Duration::from_secs(1), &token)?;                     // Err(Cancelled) at once on cancel or deadline
let mut b = Backoff::new(BackoffPolicy::default());               // 1 s doubling to 60 s, reset after 5 healthy min, give up >5 in 10 min
match b.failed(clock.monotonic()) { Retry::After(d) => clock.wait(d, &token)?, Retry::GiveUp => report() }
```

- `Clock`: `monotonic()` (never backwards; the only reading for durations, deadlines and back-off), `wall()` (calendar
  time for stamps and file names; may jump), `wait(d, &CancelToken)`; provided `now_ms`, `wall_ms`, `sleep`.
- `FakeClock`: both readings move only with `advance`/`advance_ms` (`set_wall` moves the wall alone); waits block until
  the clock reaches their end. A test starts the work, `wait_for_waiters(n, guard)` until it is blocked on the clock,
  advances and asserts the outcome; the guard is a hang guard, never a timing assertion.
- `Deadline::after(clock, limit)`: `passed()`, `remaining()`; a limit too large to represent never passes.
- `Backoff` is a pure state machine over monotonic readings (`healthy(now)`, `failed(now) -> Retry`, `reset()`);
  `BackoffPolicy::retry_forever` for a reconnecting client.

## `paddock-commands` (plan S1)

The command table of an app or a shell plugin, and its guard (ADR 0012 §3, ported from Debrief ADR 0016 and 0054):

```rust
paddock_commands::command_table! {
    pub(crate) static COMMANDS, macro handler;       // the validated `Table`, and `handler!()` = `::tauri::generate_handler![...]`
    Query [WINDOW] => workspace::summary,           // Kind: Query | Mutation | Quiet (how it is logged)
    Mutation [MAIN | DIALOG] limit 4 * MIB => workspace::open_paths,
    Quiet [MAIN] => session::save_session,          // classes: MAIN (`main`, `w-<n>`), POPOUT (`p-<host>-<n>`), DIALOG (`d-<host>-<n>`), WINDOW (all)
}
tauri::Builder::default().invoke_handler(
    paddock_commands::tauri::Guard::new(&COMMANDS)
        .dialog_grants(|app, label| dialogs_of(app).grant_of(label))  // impl DialogGrant: the per-dialog contract (ADR 0009)
        .wrap(handler!()),
)
```

- The table is validated at compile time: a duplicate name, a command callable from no class, a name that is not
  `lower_snake_case` (1-64) or a limit outside 1 KiB-64 MiB does not compile (default limit 256 KiB).
- The guard refuses, before the arguments are parsed: a class the entry does not name, a webview outside the label
  scheme, a dialog without a grant or whose grant does not cover the command, a request over the limit. The refusal is
  the command's error (`{ code: 'denied' | 'limit', message, retryable: false, command, dialog? }`) and is logged with
  target `paddock_commands` (keys `Command`, `Window`, `Dialog`, `ErrorCode`): an error naming the dialog and the
  command for a contract breach, else a warning. Plugin commands (`plugin:...`) and unknown commands pass.
- `Table::kind_of` classifies a command for the activity log; `Table::contract_problems(base, dialogs, host_only)`
  checks dialog contracts against the table; `labels` is the shell's window label scheme (`Label`, `class_of`,
  `resolve`).
- Tray popovers (`tray`, `tray-<item>`) are their own class `TRAY` (plan S12): not part of `WINDOW`, so a popover calls
  only the commands that name it (`[WINDOW | TRAY]`), it hosts no dialogs, and the start-up watchdog does not watch it.
- In the owning crate's tests: `paddock_commands::testing::assert_async(&COMMANDS, &[src])` fails on any sync
  `#[tauri::command]` and on table entries it cannot find; `testing::sync_generated(path, text, WriteMode::from_env())`
  fails on a stale generated file (`PADDOCK_WRITE_TS=1` rewrites it).

**Generated TypeScript command names** (`paddock_commands::ts::names_module(&TABLE, "APP_COMMANDS", "AppCommand", how)`),
one file per table, written by a test of the crate that owns the table and checked on every run, so a stale file fails
CI:

```ts
// Generated from the command table by paddock-commands. Do not edit: <how to regenerate>.

/** Every command of the table: its kind, the window classes that may call it and its largest request in bytes. */
export const APP_COMMANDS = {
  open_paths: { kind: 'mutation', windows: ['main', 'dialog'], limit: 4194304 },
  summary: { kind: 'query', windows: ['main', 'popout', 'dialog'], limit: 262144 },
} as const;

/** A command name of the table. */
export type AppCommand = keyof typeof APP_COMMANDS;
```

Entries are sorted by name; an empty table is `{} as const` (name type `never`).

**Composed tables** (plan S17): `command_table! { pub static SHELL = [A, B, C]; }` folds other tables into one, validated
the same way at compile time (a command two parts declare does not compile); `tauri::Routes` sends each call to the
handler of the part that lists it, so one `Guard` covers them all. That is how `paddock-shell` serves every shell crate's
commands: each crate keeps its table and an unguarded `invoke_handler()`, and no crate's Tauri command macros (exported
at its crate root by `#[tauri::command]`) are spliced into another crate's `generate_handler!`.

The shell's files live in `ui/projects/paddock-ui/shell/generated/` and in the entry point that uses each:

| File | Content | Written by |
|---|---|---|
| `commands.ts` | the wire types (ts-rs): `CommandKind`, `WindowClass`, `CommandInfo`, `RefusalCode`, `CommandRefusal` | `paddock-commands` (`PADDOCK_WRITE_TS=1 cargo test -p paddock-commands --lib generated`) |
| `shell-commands.ts` | `SHELL_COMMANDS` and `ShellCommand`, every command of the shell plugin | `paddock-shell` (`PADDOCK_WRITE_TS=1 cargo test -p paddock-shell --lib generated`) |
| `shell.ts` | `SHELL_PLUGIN`, `SHELL_VERSION` and the `shell_version` reply (`VersionReply`, `AppIdentity`) | `paddock-shell` (same test) |
| `notify/generated/health.ts` | the events (`SERVICE_HEALTH_EVENT`, `NOTIFICATIONS_EVENT`, `NOTIFICATIONS_OPEN_EVENT`, `NOTIFICATIONS_ACTION_EVENT`), `healthCause`, and the wire types (ts-rs) of service health, notifications, window notices and `HealthError` | `paddock-health` (`PADDOCK_WRITE_TS=1 cargo test -p paddock-health --all-features --lib generated`) |
| `notify/generated/health-commands.ts` | `HEALTH_COMMANDS` and `HealthCommand`, the health and notification commands | `paddock-health` (same test) |

An app writes its own file the same way into its UI (for example `src/app/core/generated/app-commands.ts`); dialog
contracts and the contributions core type command names as `AppCommand | ShellCommand`.

## `paddock-dialogs` (plan S5)

Dialogs as real OS windows (ADR 0009, rule 17), ported from Debrief's `dialog_contracts.rs`, `dialog_placement.rs` and
`dialog_windows.rs` (Debrief ADRs 0050, 0054) without Debrief's dialog ids:

- **Contracts are Rust tables** (ADR 0026 §3). The owner of a dialog declares it with `paddock_dialogs::dialogs!` beside its
  `command_table!` (a shell crate's `pd.*` dialogs in its `lib.rs`, an app's in `src-tauri/src/dialogs.rs`):
  `"pd.mcp.key" => Form(Modal) { width: Some(Narrow), commands: &[dialog_callable::mcp_issue_key] }`. A `Shape` is kind and
  modality in one (`Decision`, `Form(Modal | Modeless)`, `Viewer`, `Tool`: a modal viewer does not type-check), widths are
  `Narrow` 400, `Medium` 560, `Wide` 720 (defaulted by kind; a tool alone names `Px(n)`), a viewer or tool may name a
  preferred `height`. Commands are the `dialog_callable::<name>` markers `command_table!` emits for every command open to
  dialogs, `prefix(dialog_callable::ALL, "settings_")` or `plugin("dialog", "open")`: a misspelt, removed or closed
  command is a compile error, and the table's rules (ids, a modal required choice, widths, heights, duplicates) are
  checked when the `static` is built. `Contracts::from_tables(&[...])` builds the run-time contracts;
  `Contracts::allows(dialog, command)` covers `BASE_COMMANDS` (`dialog_take_input`, `dialog_close`,
  `dialog_ready`, `dialog_fit`, `notifications_post`, `notifications_list`), the tables' `base` and the dialog's own list.
  `Kind::policy()` is the kind table of ADR 0026 §1 (default width and height, viewer floor 680x560 and grows only,
  `fits`, what each kind remembers), generated into `shell/generated/dialog-kinds.ts`; `ts::contracts_module` writes
  each owner's `dialog-contracts.ts` (`DIALOG_CONTRACTS`, `DialogId`, `DialogCommandOf`, `DIALOG_BASE`), checked by a
  test of the owner.
- `placement`: pure geometry in physical px. `monitor_of` picks the parent's monitor (most overlap, else nearest; never
  the primary display as such); `place` sizes and places on that monitor's work area (remembered offset from the parent
  only when the whole dialog stays there, else centred on the parent and moved inside; shrunk to fit; mixed DPI and
  negative coordinates); `screen_position` gives points on macOS and the exact physical place elsewhere;
  `memory_for` is what each kind keeps of a remembered place (a decision its offset only, a form its width and offset, a
  viewer and a tool their size and offset); `Placement::physical_size` rounds up once (`physical_ceil`).
- `Dialogs` (managed state) records open dialogs by label `d-<host>-<n>` (at most 8 per host), answers the guard
  (`grant_of(label)`, the `paddock_commands::DialogGrant`) and decides close requests (`close_request(label, now)`:
  forwarded as the dialog's Close/Cancel, closed when it did not answer within 1.5 s, a required choice kept unless the
  watchdog marked its page unresponsive).
- `window`: the wire `Measure { content, minContent, viewportHeight?, devicePixelRatio? }` (TS `DialogMeasureWire`; the page reports CSS px: `devicePixelRatio` is its `window.devicePixelRatio`, 0.25..=8, and the fit converts content, minimum and viewport to logical px with the webview zoom `zoom_of` = ratio / window scale factor, 1 when either is missing; `viewportHeight` is the page's `innerHeight`, 1..=4096: the fit adds `viewport_delta` = the window's inner height less it, clamped to 0..=400, to the content and minimum heights, for a webview shorter than its window; the delta is taken once, at the first fit (`dialog_ready`), kept on the registry entry and reused by later fits), `plan_fit` (the cap is 75 % of the work
  area less the window's own outer-minus-inner decorations and limits the preferred size only, never below `minContent`,
  which becomes the OS minimum; decisions and forms shrink and grow, a viewer only grows, a tool keeps its size) and
  **`show_mechanism(os)`, the one place the way of keeping a window unseen is chosen** (`ReadyThenShow`: hidden,
  shown on `dialog_ready`, method d of the 1.1-0 spike, measured on macOS, Linux and Windows).
- `tauri` feature: `open` (window by kind, created **hidden** at the placed size on the dialog shell's surface colour,
  no title-bar buttons, owned or independent per OS, `WindowHooks` for the app's webview settings), the commands
  `dialog_open`, `dialog_take_input`, `dialog_close`, `dialog_ready { measure }` (sizes, then shows and focuses the
  pending window), `dialog_fit { measure }` (refits), `on_window_event` (close request, move/resize, scale-factor change
  refits from the last measure, destroyed dialog or host), `page_unresponsive`, `close_all_of`, `set_visible`, `grant_of` for `Guard::dialog_grants`. Events
  `paddock://dialog-result`, `paddock://dialog-closed` (to the opener), `paddock://dialog-close-requested` (to the
  dialog). The lifecycle: a window is pending until `dialog_ready`, or until `Dialogs::show_deadline` (1 s;
  `with_show_deadline`, `paddock-startup`'s `Config::dialog_show_deadline`) passes and it is shown at the placed size
  (a late `dialog_ready` then fits it once); a modal's parent is disabled when the window is shown, never at creation (the registry records only the desired state, a parent is disabled iff a shown modal of it is open, and one queued main-thread task reads the latest of it and applies it; no window API runs under the registry lock);
  opening a pending dialog again creates no window and does not focus (show) it early. The deadline runs through a
  `Scheduler` (`ThreadScheduler`; tests inject their own and never sleep).

```rust
paddock_commands::command_table! {
    static COMMANDS, macro handler;
    Mutation [MAIN | DIALOG] limit 9 * MIB => paddock_dialogs::tauri::dialog_open,
    Query [DIALOG] => paddock_dialogs::tauri::dialog_take_input,
    Mutation [DIALOG] limit 2 * MIB => paddock_dialogs::tauri::dialog_close,
    Quiet [DIALOG] => paddock_dialogs::tauri::dialog_ready,
    Quiet [DIALOG] => paddock_dialogs::tauri::dialog_fit,
    // ... the app's commands
}
tauri::Builder::default()
    .manage(paddock_dialogs::Dialogs::from_tables(&[&app::DIALOGS])?)
    .invoke_handler(Guard::new(&COMMANDS).dialog_grants(paddock_dialogs::tauri::grant_of).wrap(handler!()))
    .on_window_event(|w, e| paddock_dialogs::tauri::on_window_event(w, e))
```

Wire types are generated into `ui/projects/paddock-ui/shell/generated/dialogs.ts` (`DialogKind`, `DialogRequest`, `DialogMeasureWire`, `DialogStart`, `DialogResult`, `DialogClosed`, `DialogError`, `DIALOG_EVENTS`,
`DIALOG_BASE_COMMANDS`; `PADDOCK_WRITE_TS=1 cargo test -p paddock-dialogs --lib generated`).

**The real-window probe** (ADR 0026 §6): `tests/show_probe.rs` (`harness = false`: a Tauri app owns the main thread) opens
the real dialog frame (`sd.rename` of the built shell-demo, served from `PADDOCK_PROBE_DIST`, normally
`ui/dist/shell-demo/browser`) in a real window created hidden, lets the page measure itself synchronously and send
`dialog_ready`, and checks that the window was shown by that ready and not by the deadline (set to 300 s) and that after
the show the page's viewport equals the measured content height, the page does not scroll and the footer is fully
visible; the parent is a visible host window and the dialog is modal, and the probe asserts the dialog window was still
hidden when its `dialog_ready` arrived. It reports from a synchronous read and a short timer, never animation frames
(a sleeping display runs none). It exits 0 at once unless `PADDOCK_REAL_WINDOWS=1` (and says so when `PADDOCK_PROBE_DIST`
is missing), so the ordinary run skips it. In `nightly.yml` the job `dialog-window-probe` runs it on Linux only, under
`xvfb-run` in the CI container (it opens real windows, which rule 7 allows only in a throwaway Linux container or a test
VM): on a manual run or when the variable `PADDOCK_REAL_WINDOW_PROBE` is `true`, and it gates nothing. The macOS and
Windows probe runs in the local test VMs (docs/dev-setup.md, "Test VMs and real browsers"). Run it there from `crates` (`--features tauri`, not
`--all-features`, which pulls in the mock runtime):
`PADDOCK_REAL_WINDOWS=1 PADDOCK_PROBE_DIST=../ui/dist/shell-demo/browser cargo test -p paddock-dialogs --features tauri --test show_probe -- --nocapture`. Exit 0 passes, 1 fails
(the reason is printed), 2 means no report within 90 s (the calls the page made are printed with it; none means the page's
calls were refused: the probe supplies the `paddock-shell` plugin and an ACL for the `d-*` windows itself). Every call prints with
its time. It opens a window on the desktop, so run it on purpose.
## `paddock-health` (plan S3)

Service health and the cause-key notification centre (ADR 0010, ADR 0012 §3), ported from Debrief ADR 0055
(`health/{model,registry}.rs`, the generic half of `health/hub.rs`, `notification_centre/`), without Debrief's
services, sources or actions:

```rust
// The app registers its services; the repair is the shell's code and never goes through the broken service (rule 19).
let health = paddock_health::tauri::init(app.handle(), paddock_health::tauri::Setup::new(vec![
    ServiceSpec::new("llm", "the model server").repair(move || supervisor.restart().map_err(|e| Cause::new("...").detail(e))),
]).notifications_file(data_dir.join("notifications.json")).os_notifications(|app, notice| show_os(app, notice)))?;
// Supervisors report; the same report again announces nothing.
health.down("llm", Cause::new("The model server stopped.").hint("Choose Restart.").code("llm.stopped"), Some(RepairAction::Restart));
// Commands behind the shell's guard (S17 lists them in the shell plugin's table).
.invoke_handler(Guard::new(&paddock_health::tauri::COMMANDS).wrap(paddock_health::tauri::invoke_handler()))
```

- One failure is one chip and one notification in every window: each real change is one `paddock://service-health`
  event (a `ServiceHealthSnapshot`) and one message with the cause `health:<id>` (posted on the outage, updated in
  place, resolved on the recovery), with a `paddock://notifications` event per window. New windows ask with
  `health_snapshot` and `notifications_list`.
- `notifications_post` (16 KiB): a window's own notice (`SHELL_PORTS.notify`) goes to the centre, so the engine stays the
  source and every window lists it (rule 34). Untrusted input parsed once (`notify::window::WindowNotice`): the severity
  is closed (no `critical`), the title one line of at most 160 characters, the body at most 1000, the kind an id of
  `[a-z0-9.-]` (never `health`), unknown fields refused; the cause is the engine's (`ui:<kind or severity>:<title>`), so
  a window never addresses another source's message and the same notice again counts up. App scope (default) reaches
  every window, window scope the caller's window and its dialogs and pop-outs. Every dialog may call it (and
  `notifications_list`): both are in `paddock_dialogs::BASE_COMMANDS`.
- `health_repair` returns at once with the service `repairing`, runs the repair on a background thread (a panic is
  contained) and ends `ok`, or `down` with the repair's cause; `openSettings` repairs are opened by the UI instead.
- Notifications: sources and action types are the app's strings (`NoticeSource`, `NoticeAction { type, ...args }`);
  the shell's own are `health`, `repairService` and `openSettings`. App messages are saved debounced and atomically in a
  versioned file (missing, damaged or newer: empty, logged); a window's project messages are saved by the app with its
  project (`notifications_record` / `notifications_restore`). A dialog or pop-out sees its host window's list.
- OS notifications (plan X8, `notify::os`; the Tauri side in `tauri::os`, the plugin adapter in `tauri::native` behind
  feature `os-notify`): one policy for every app, merged from Debrief's `notify.rs` (level, "only when the user cannot see
  it", rate limiter), Spotter's `toast.rs` (buttons, click-through, a validated payload) and `QuietHours`; Pitcrew's and
  Dyno's plain toasts had nothing to add (Dyno's sustain and cool-down decide whether an alarm *exists* and stay in
  `dyno-core`).

  ```rust
  let settings = SharedOsSettings::default();              // the user's level, quiet hours, limits (Settings: pd.notifications)
  let notifier = OsNotifier::new(settings.clone(), Arc::new(PluginToaster::new(app.clone())))
      .with_labels(ActionLabels::default().label("viewJob", "View job"));
  paddock_shell::Builder::new(identity)
      .os_notifications(settings)                          // the "Notifications" settings page over the same handle
      .health_setup(move |_, s| s.os_notifier(Arc::new(notifier)).on_notice_action("viewJob", |app, notice, action| open_job(app, action)))
  ```

  - Policy (`screen`, pure): the level (`off`, `alerts` = warning and above, `all`), then "the window the message belongs
    to is in front" (`user_sees` over focused and visible windows; an app-wide message is seen in any), then quiet hours
    (local time, across midnight; a critical failure still notifies unless the user turns that off), then the limits.
    A held-back message is still in the centre; only the toast waits.
  - Limits (`RateLimiter`, epoch ms from the injected `Clock`): one toast per cause key per cool-down (30 s) and a burst
    (5 in 5 minutes) over all causes, both clamped to ranges; bounded to 512 causes; a clock that goes backwards forgets
    instead of muting; a toast the OS refused is taken back (`Reservation`), so a failure costs no quota.
  - Buttons: a toast has up to three, one per labelled action of the message (`ActionLabels`; the shell's `repairService`
    and `openSettings` are built in). A click reports the message id and the slot; the action is read from the centre's
    message again, never from the OS payload (`route`): the toast opens the message (and marks it read), Repair runs
    `Health::repair` in the shell (rule 19), an app handler (`on_notice_action`) runs on the OS callback thread, anything
    else goes to the window as `paddock://notifications-action` (`NoticeActionEvent`; the UI runs it like the centre's own
    button). Where an OS reports no clicks the old fallback stays: the app gaining the focus within 2 minutes opens the
    last toast's message; a reported click supersedes it.
  - Quiet-hours digest: causes held back by quiet hours are counted (distinct, at most 512); when quiet hours are over
    `OsNotifier::pending_digest` says how many and `tauri::poll_digest` posts one quiet message ("N notifications held
    during quiet hours", cause `os:quiet-digest`, merged across nights) and one toast, rate-limited like any other cause. A
    user who is looking at the window gets the message in the centre only. `Setup::os_digest_every(d)` polls on a thread that
    ends with the app (quiet hours end by the clock, not an event); a window gaining the focus polls too.
  - Tests never post a notification: `FakeToaster` records toasts, plays clicks and permission answers (public, for the
    apps' wiring tests); `FixedOffset` stands for the user's zone, `FakeClock` for time.

## `paddock-startup` (plan S7 and §11)

The start-up contract and the window watchdog (ADR 0012 §3, ported from Debrief ADR 0058), plus the liveness heartbeat
after start-up. Every window reports `window_ready` once it painted its frame; a window that does not within 10 s is
reloaded once, then the user is asked natively (Reload / Restart in Safe Mode / Keep Waiting). Once ready, each visible
window answers a ping every 2 s (`paddock://ping` → `window_pong`); after 5 missed in a row it is marked unresponsive,
logged, the unresponsive hook lets its dialog be closed by the OS close, and the user is asked (Reload / Keep Waiting); an
answer clears the mark. A gap of more than 3 s between ticks means the app process itself was paused: the clocks restart.
A terminated web content process (macOS) is reloaded at most three times a minute; the reloaded page hears why with its
last UI checkpoint.

```rust
let startup = paddock_startup::tauri::Startup::builder()
    .safe_mode(|app, _label| restart_in_safe_mode(app).map_err(|e| e.to_string()))
    .on_unresponsive(paddock_dialogs::tauri::page_unresponsive)    // a dialog whose page cannot answer may be closed
    .native_dialogs()                                               // feature `dialog`; the app registers tauri-plugin-dialog
    .build()?;                                                      // `Config` timings are checked
paddock_startup::tauri::install(tauri::Builder::default(), startup) // manage + on_page_load + content-process terminate
    .invoke_handler(Guard::new(&paddock_startup::COMMANDS).wrap(paddock_startup::tauri::handler()))
```

- The pure core (`Watchdog`, `Checkpoints`, `ready_reply`, `clean_error`, `Prompt`, `Config`) takes the time from the
  caller and is tested with a fake clock; the ticker thread (started by the first watched page load; never the main
  thread) only applies the `Action`s, reloading and asking through `run_on_main_thread`.
- `COMMANDS`: `window_ready`, `window_boot_error` (16 KiB, one bounded line, at most 10 per page load) and `window_pong`
  for every window class; `ui_checkpoint` (16 MiB, an object) for main windows only. An app with its own table lists
  `paddock_startup::tauri::window_ready` and the others there instead; `DIALOG_BASE` names the three every dialog
  contract includes.
- Generated TypeScript: `ui/projects/paddock-ui/shell/startup/generated/startup.ts` (wire types) and
  `startup-commands.ts` (`PADDOCK_WRITE_TS=1 cargo test -p paddock-startup --lib generated`).

### Run lifecycle: unclean shutdown, safe mode, session restore (plan X3)

Debrief's `lifecycle.rs`, shared. The app begins the run first thing in `main` (after `paddock-log` is installed with
`with_app_dirs`, so a panic leaves its `CrashMarker` in the data folder) and hands it to the contract:

```rust
let mode = paddock_startup::Mode::from_launch(launch.safe_mode, std::env::var("PITWALL_SAFE_MODE").ok().as_deref());
let life = paddock_startup::Lifecycle::begin(paddock_startup::Start {   // feature `os-probe`
    data_dir: dirs.data(), process: "pitwall", mode,
    scope: paddock_startup::InstanceScope::of(single_instance), app_version: env!("CARGO_PKG_VERSION"),
});
if !mode.allows(paddock_startup::Feature::Plugins) { /* do not load them */ }
let startup = Startup::builder().safe_mode(restart_in_safe_mode).lifecycle(life).build()?;
```

- **Running marker** (`running.json`, a `VersionedFile`): `begin` writes it, `end` removes it on a clean exit (the shell
  calls `paddock_startup::tauri::end_run` at `RunEvent::Exit`; an app without the plugin calls it itself). Left behind, it is a
  crash when its process is gone; PID reuse is caught by the start time; a live predecessor is taken over; an unreadable or
  newer marker is a crash. `paddock-log`'s crash marker also makes the start a crash and says which thread and where.
- **Instances.** Safe mode and `--new-instance` use `instances/<pid>/` (marker, sessions), so they never rotate the main
  instance's session or report its crash; dead ones are pruned; a crash in safe mode is told to the next normal start, which
  then does not offer safe mode again.
- **Session.** `ui_checkpoint` also goes to `session/current.json` on the autosave thread (newest state per window; the
  clean exit flushes). At start the previous run's file becomes `recovered.json` and the run starts empty; an empty run
  never wipes the recovered one. Torn, damaged or newer files start clean with a notice in the offer; at most 8 windows of
  2 MiB.
- **The offer.** `recovery_status` (main window only) answers `{ safeMode, unclean: { crash, session, offers } | null }`;
  `recovery_restore` returns the windows and the asking window's state; `recovery_start_fresh` forgets the session;
  `recovery_safe_mode` runs the app's restart and settles the notice only when it worked. The UI side is
  `RecoveryStore` / `PdRecoveryNotice` / `PdSafeModeBadge` in `@decryp7/paddock-ui/shell/startup` (the pages frame places them).
- Replaces: Debrief `app/src-tauri/src/lifecycle.rs` (marker, instances, `SessionStore`, `Lifecycle`) and
  `app/src/app/lifecycle/` (`lifecycle-banners.ts`, `lifecycle.service.ts` safe-mode and crash parts, `session.service.ts`
  checkpoint part). Debrief keeps the project-file format of its sessions: its `UiCheckpoint.capture()` returns the content.

## `paddock-panels` (plan X50 W3)

Pop-out panel windows: a panel of a window's dock moved into an OS window of its own. Taken from Debrief's
`panel_windows.rs`, `drag_ghost/` and the transport half of `app/src/app/popout`. Pure core, `tauri` feature for the adapter.

- `panel`: `Panel::parse` for `<view>` / `<view>#<key>` (view: namespaced id; key 1 to 1024 characters), `popout_url`.
- `place`: `place(wanted, remembered, host, monitors)` puts a pop-out on any display, clamped to that display's work area
  (requested place, else the view's remembered place, else beside the host); mixed scales and negative coordinates tested.
- `registry`: `PopOuts` (labels `p-<host>-<n>`, at most 12 per host) and `Memory` (last place per view, a `paddock-env`
  versioned file).
- `sync`: `recipients` (only the windows of the sender's host; dialogs get `display` scope only), `check_message`
  (JSON object, 512 KiB) and `Limiter` (200/s, burst 100).
- `ghost`: the drag ghost's state machine, geometry, request parsing and static page (`ghost::page::{HTML, CSS, JS}`: the
  app serves them as `drag-ghost.html|css|js`).
- Adapter: `Panels::builder().route(..).bounds_file(..).build()` managed as state; `on_window_event` (a pop-out closing
  docks its panel back, a host closing closes its pop-outs, a drag ends with its window); `invoke_handler()` behind
  `Guard::new(&paddock_panels::COMMANDS)`. Events: `paddock://panels` (to the host: `returned`, `changed`, `moved`),
  `paddock://ui-sync`. Generated TypeScript: `ui/projects/paddock-ui/workbench/panels/generated/` (`PADDOCK_WRITE_TS=1 cargo
  test -p paddock-panels --lib generated`). OS glue (pointer, no-focus show, frameless, the tracker loop) is `tauri/native.rs`.

## `paddock-workbench` (plan X50 W2a, W7)

The Rust side of the workbench (ADR 0013 §3): the dock's layout store and the workbench Tauri plugin, built exactly like
`paddock-shell` (one guard over a composed table, generated permission sets per window class, generated TypeScript names).

```rust
let workbench = paddock_workbench::Builder::new()                 // every part optional (a missing one answers "not set up")
    .layouts(dirs.config())                                       // <config>/workbench-layout.json
    .panels(Panels::builder().route("index.html#/popout").bounds_file(data.join("popout-places.json")).build())
    .window_hooks(WindowHooks::new())                             // optional: the app's steps in building a pop-out
    .assistant(Arc::new(assistant))                               // paddock_assistant::Assistant
    .build();
tauri::Builder::default().plugin(shell.build()).plugin(workbench) // the shell first: the dialogs' grants are its state
```

- Plugin `paddock-workbench` (`links`): commands are `plugin:paddock-workbench|<command>`. `COMMANDS` folds `LAYOUT`
  (`layout_get [MAIN | POPOUT]`, `layout_set [MAIN]`), `paddock_panels::COMMANDS` (11 commands) and
  `paddock_assistant::tauri::COMMANDS` (7) behind one `Guard` (`paddock_dialogs::tauri::grant_of` for the dialog contracts)
  over `Routes` to each crate's unguarded handler. The plugin also hooks every window's events into the panel layer
  (a pop-out closing docks back, a host closing closes its pop-outs). The app serves the drag ghost's page
  (`paddock_panels::ghost`) and calls `Panels::set_exiting(true)` when it quits.
- Permissions: `build.rs` makes `allow-<command>`; `permissions/window-classes.toml` (generated from the table, checked by
  a test; `PADDOCK_WRITE_TS=1 cargo test -p paddock-workbench --all-features --lib generated` rewrites it) holds the sets
  `default` (= `main`), `popout` and `dialog`. An app's capabilities grant `paddock-workbench:default` to `main`/`w-*`,
  `paddock-workbench:popout` to `p-*` and `paddock-workbench:dialog` to `d-*`. The drag ghost window (`drag-ghost`) matches
  none of them.
- Generated TypeScript (`ui/projects/paddock-ui/workbench/generated/`): `WORKBENCH_COMMANDS` / `WorkbenchCommand`
  (`workbench-commands.ts`) and `WORKBENCH_PLUGIN` (`workbench.ts`), exported from `workbench/public-api.ts`.
- `layout::LayoutStore`: a `paddock-env` `VersionedFile` (format 1). Keys `[a-z0-9._:-]{1,64}`, values JSON objects of at
  most 1 MiB, at most 128 keys and 8 MiB in all; a damaged file gives an empty store and one `Problem` (logged), a newer
  file is left alone and saves are refused (the change stays in memory for the run). The UI owns the content (the dock's
  versioned envelope, parsed at its edge).
- Commands answer the one envelope of `paddock_commands::CommandError` (boxed): `invalid` with `field` for a bad key or a
  value that is not an object, `limit` for too large or full, `io` for a refused write; file I/O runs on the blocking pool.
- Tests: the store in temporary folders (damaged, newer, out-of-bound files); on Tauri's mock runtime through the plugin:
  every command reaches its handler from a window of a class it is open to and is refused (by Tauri's ACL) from the
  others, a dialog reaches only its contract, the drag ghost's label reaches nothing, parts left out answer "not set up";
  the generated files are current.

## `paddock-window` (plan S12)

Top-level windows, single instance and deep links, the bounded quit, native feel and the tray (ADR 0012 §3), ported
from Debrief's `window_manager.rs`, `windows.rs`, `instance.rs`, `lifecycle.rs` (exit watchdog), `native_feel.rs` and
`tray.rs`, Pitwall's `window.rs`/`tray.rs` and Dyno's `popover.rs`, with no app's names, settings or menu items:

```rust
let launch = paddock_window::launch::parse_args(std::env::args().skip(1), cwd.as_deref(), &["pitwall"]);
if paddock_window::launch::wants_single_instance(&launch, safe_mode_env) {   // first plugin
    builder = builder.plugin(tauri_plugin_single_instance::init(paddock_window::tauri::second_instance));
}
builder.setup(move |app| {
        app.manage(Windows::builder(WindowConfig::new("Pitwall").schemes(&["pitwall"]))   // .multi_window(32) for Debrief
            .prefs(settings.clone())                          // impl Prefs: close setting, open-files setting, "remember"
            .bounds_file(data_dir.join("window-state.json"))
            .teardown("server client", |app| disconnect(app)) // bounded exit steps, in order
            .build());
        app.manage(Windows::dialog_hooks(app.handle()));      // dialogs get the same feel and builder hook
        app.manage(Popovers::new().popover(None, PopoverSpec::new("index.html#/tray", "Pitwall")));
        create_tray(app.handle(), Tray::new(NativeTray::new(icons).menu_on_left_click(false), model).popover(true));
        start(app.handle(), &launch)?;                        // main window, unless `--hidden` and a tray exists
        Ok(())
    })
    .on_window_event(|w, e| { paddock_dialogs::tauri::on_window_event(w, e); paddock_window::tauri::on_window_event(w, e) })
    .invoke_handler(Guard::new(&paddock_window::COMMANDS).wrap(paddock_window::tauri::handler()))
    .build(context)?
    .run(|app, event| paddock_window::tauri::on_run_event(app, &event));
```

- Pure core: `close` (last window: quit / keep running in the tray / ask; no tray means quit; only an explicit exit
  ends a tray app; `--hidden` builds no window when a tray can bring it back), `launch` (flags `--new-instance`,
  `--new-window`, `--safe-mode`, `--hidden`, `--`; paths capped at 256; deep links of the app's schemes parsed once,
  bounded, no credentials; the open plan: an empty window is reused, links go to the window in front and never ask,
  the `openFilesIn` setting decides the rest), `bounds` (versioned file, `main` and `window` keys; restored on the
  monitor the rectangle overlaps most or the nearest, scaled, shrunk and moved inside the work area; new windows
  cascade on the parent's monitor; built on `paddock_dialogs::placement`), `exit` (`teardown_step` bounded per step,
  `end_stalled_exit`, once-only `ExitState`), `feel`, `tray` (menu model with validation and the shell's ids `pd.show`,
  `pd.new-window`, `pd.window:<label>`, `pd.quit`), `popover` (anchoring at the tray icon, fitted height, material per
  OS, the boot script with `data-popover-open` and Escape).
- `tauri` feature: `Windows` state and builder, `create_main` (from `tauri.conf.json` with `"create": false`, else
  `WindowConfig`), `open_new`, `reveal` (rebuilds a destroyed `main`), `list`, `target`, `quit`, `close_requested`,
  `on_window_event` (close setting, focus of pop-outs and dialogs for their host, bounds, destroyed windows, the
  popover's blur), `second_instance`, `opened` and `reopen` (macOS), `exit_requested`, `exit` (bounded teardown, once),
  `arm_exit_watchdog` (8 s, then teardown and process exit), `on_run_event`; `Tray` over a `TrayBackend` (redrawn only
  when the model changed), `Popovers` with `toggle_popover`, `fit_popover`, `close_popover`. `tray` feature:
  `NativeTray` (Tauri's tray icon; template images on macOS; no icon in a session without a taskbar). `wry` feature:
  WebView2's browser keys off in release builds.
- `COMMANDS`: `window_new` [MAIN | TRAY], `window_close`, `window_set_title` (4 KiB), `window_resolve_close`,
  `window_take_pending`, `window_resolve_open` [MAIN], `windows_list`, `window_focus`, `app_quit` [WINDOW | TRAY],
  `tray_fit_height`, `tray_close` [TRAY]. Events: `paddock://windows`, `paddock://window-close-requested`,
  `paddock://window-open-ask`, `paddock://window-open`.
- Generated TypeScript: `ui/projects/paddock-ui/shell/generated/window.ts` (wire types, `WINDOW_EVENTS`) and
  `window-commands.ts` (`PADDOCK_WRITE_TS=1 cargo test -p paddock-window --lib generated`).
- Tests run on Tauri's mock runtime with a fake tray backend and hooks for Quit and the process exit: no window, tray
  icon or single-instance lock reaches the desktop (rule 7). The untestable glue (the tray icon's create/update, window
  effects, the run and close events Tauri does not let tests build) is `src/tauri/native.rs`, outside coverage. `tests/native_menu.rs` has no harness, so the native menu
  is built on the main thread (macOS builds menus there only); the builder is made, never built.
## `paddock-menu` (plan S13)

One menu model for the native menu bar and the in-app menus (ADR 0012 §3, ported from Debrief's `menu.rs`, ADR 0008).
The UI builds the model from its contributions and draws it itself on Windows and Linux; on macOS it sends it with
`menu_set` once the app reported ready (later changes coalesced), and this crate shows it natively:

```rust
paddock_commands::command_table! {
    static COMMANDS, macro handler;
    Quiet [MAIN | POPOUT] limit 1 * MIB => paddock_menu::tauri::menu_set,
    // ... the app's commands
}
tauri::Builder::default()
    .setup(|app| Ok(paddock_menu::tauri::install(app.handle())))  // manages NativeMenu (product name), routes clicks
    .invoke_handler(Guard::new(&COMMANDS).wrap(handler!()))
    .on_window_event(|w, e| paddock_menu::tauri::on_window_event(w, e)) // the menu bar follows the focused window
```

- `parse` refuses a model over the limits (16 menus, 1000 entries, depth 4, ids of 128 `[A-Za-z0-9._:/-]`, labels of
  120 characters without control characters, no duplicate item id; unknown fields) and turns bindings into accelerators
  (`accelerator("Mod+Shift+O")` is `CmdOrCtrl+Shift+O`; a binding a menu cannot show keeps working in the page).
- `native_spec(model, app_name)` adds the standard macOS items; `outline` is its snapshot form (the crate's snapshot test
  pins it). `diff` answers `Unchanged`, `States` (check marks and enabled states updated in place) or `Rebuild`;
  `NativeMenu::builds` counts the builds (a start builds twice: the standard menus at `install`, then the UI's).
- A click goes as `paddock://menu` (the item id) to the focused top-level window only (a dialog or pop-out stands for its
  host); a model from a window that is not the target is answered `notFocused` and sent again on focus.
- `manifest::check(json)` over the app's `paddock-manifest.json`: every menu item routes to a command, every key converts
  to an accelerator, a command sits in one menu-bar menu. Run it in a test of the app.
- The native menu bar is a port (`NativeBar`: `TauriBar`, a fake in tests); on macOS native menus are created on the main
  thread only, so the Tauri menus are built in tests on Windows and Linux.
- Generated TypeScript: `ui/projects/paddock-ui/shell/menu/generated/menu.ts`
  (`PADDOCK_WRITE_TS=1 cargo test -p paddock-menu --lib generated`).

## `paddock-download` (plan S8)

Downloads with one in-order SHA-256 while writing (rule 15, ADR 0008 §5, ADR 0012 §3), ported from Debrief's model
downloader (`debrief-ai::models::{download, segmented}`) with the hash moved into the write path:

```rust
let downloader = Arc::new(Downloader::http(HttpConfig::default())?);  // one per app: the host caps hold across downloads
let req = Request::new(url, models_dir.join("model.gguf"), Integrity::Sha256(Sha256Digest::from_hex(&pinned)?))?;
let ctx = jobs.register("download", "Downloading the model", Some("model.gguf"));
let d = downloader.clone();
pool.spawn_job(ctx, move |ctx| Ok(d.download(ctx, &req)?))?;      // the command returns the job id at once
```

- Phases (`phases`): `connect` (items, 1), `resume` (bytes of the kept prefix, only when resuming), `download` (bytes;
  a growing count when the host gives no size), `verify` (items, 1: the digest computed while writing is compared, then
  one rename; never a second read). Each phase starts at 0 and never goes backwards; Cancel is noticed within 250 ms in
  every phase, because the job's worker only supervises the transfer threads.
- A `bytes=0-0` probe decides: a 206 host is downloaded in ranges (`Config::connections`, default 4, when the file is at
  least `segment_threshold`), positioned writes, an idle connection takes over the far half of the lowest slow range, and
  ranges ahead of the hash wait in memory (at most `max_buffer`, default 64 MiB); a host without ranges is one stream,
  and a dropped connection restarts it and skips the bytes already written.
- `HostCaps` (default 4 connections per host across all downloads; `with_cap(host, n)`); 429/503 lower the download's
  connections and hold every request to that host for `Retry-After` (seconds or an HTTP date, clamped to
  `backoff..=max_wait`); failures retry with exponential back-off; no bytes for `stall_timeout` ends as `Stalled`.
- Resume: `<name>.part.json` (versioned) keeps the written and hashed prefix; the same request resumes when the size,
  the pinned checksum and the host's strong ETag (else Last-Modified) still match; ranges carry `If-Range`.
- Bounds: `Config::max_bytes` / `Request::max_bytes` before writing and while streaming; a pinned `Request::size` must
  match. Typed `DownloadError` (`keeps_partial()` says whether a rerun resumes); `From<DownloadError> for JobError`.
- The network is a port (`Transport`); `HttpTransport` (feature `http`, default) is ureq with rustls and the OS
  certificate store, `Accept-Encoding: identity`, and the system proxy by default (`ProxyMode::System`:
  `HTTPS_PROXY`/`HTTP_PROXY`/`ALL_PROXY`/`NO_PROXY` in either case, then macOS system configuration or Windows Internet
  Settings/WinHTTP; loopback always direct; `ProxyMode::Direct`; `ProxyMode::explicit("http://user:pass@host:3128")` or
  `ProxyMode::Rules(ProxyRules::all(ProxyServer::parse(..)?.with_credentials(u, p)).with_no_proxy("*.corp,10.0.0.0/8"))`;
  logins never print). No SOCKS and no PAC scripts. Feature `tauri`: `tauri::manage` puts one
  shared `Downloader` in the app state, `tauri::downloader(app)` returns it.
- Also (0.3.0): `Request::header` (an `Authorization` token or `Accept`, values never printed), `Request::resumable()`
  (something to continue?), `Downloader::download_with_cancel` (an external `CancelToken` besides the job's),
  `get_json`/`get_text`/`get_bytes` with `Fetch` (headers, size limit, timeout, cancel, retries, shared proxy and host caps),
  `NotEnoughSpace { path }` naming the folder (and the drive on Windows), `phases::{ALL, *_LABEL, UNPACK, INSTALL,
  SELF_TEST, is_download_phase}` for phases an app adds, and `Downloaded::sink_panics` (a panicking `EventSink` is contained
  and now counted). The engine is thread-based and blocking by design (no async runtime, nothing to share with the app's):
  from async code call it inside `spawn_blocking`, or run it as a job on the `JobPool`.
- Tests run against an in-process HTTP server, an in-process CONNECT proxy and a scripted transport (no network);
  `cargo bench -p paddock-download`
  compares one stream with 2, 4 and 8 connections on a server that limits each connection.

## `paddock-settings` (plan S10)

Settings (rule 21: tunables are settings; ADR 0012 §3), ported from Debrief's `debrief_plugin_api::settings`,
`config_store.rs` and the generic half of `settings.rs`, without Debrief's sections, plugins or groups:

```rust
let settings = paddock_settings::tauri::manage(app.handle(), Settings::open(config_dir.join(paddock_settings::FILE_NAME)));
settings.register(Arc::new(paddock_settings::shell::appearance()))?;                 // the shell's section `pd.appearance`
settings.register(paddock_settings::section(models_section(), |v| apply_models(v)))?;  // `Configurable`: schema + apply
let models: ModelsSettings = settings.decode("pw.models")?;                           // the app's own typed struct
.invoke_handler(paddock_settings::tauri::route(paddock_settings::tauri::handler_with_grants(paddock_dialogs::tauri::grant_of), app_handler))
```

- Schema: `SettingsSection` (id = the settings page's id, owner `shell` or `app`) of `SettingField`s with a `FieldKind`
  (bool, int with step and unit, float, text with an anchored `regex` pattern of bounded size, enum, path, duration in ms,
  size in bytes, string list) and a default; `check` refuses a defective schema at `register` (ids, lengths, limits,
  patterns, defaults that fail their own validation, duplicates).
- Values: `validate` (refuses a set as a whole, one `FieldError` per field, messages for the user) and `sanitize` (stored
  values: each bad one becomes its default and is logged); unknown keys are dropped; `ValidValues` with typed getters and
  `decode::<T>()`.
- File: `settings.json` `{ version: 1, sections: { id: { key: value } } }`, at most 1 MiB, read once. Missing: defaults.
  Damaged (not JSON, wrong shape, too large): defaults, kept as `settings.json.corrupt` (five slots), one `LoadProblem`.
  Newer format: defaults, kept as `settings.json.v<N>`. Unreadable: defaults, left alone. Sections nobody registers are
  kept and written back. Writes are atomic (temp file, `sync_all`, rename, folder sync on Unix) and serialised.
- `Settings::set(id, changes)` lays changes over the current values (`null` = default), validates, applies (a failing or
  panicking `apply` refuses the change and nothing is saved), saves, and announces one `SettingsChanged` to the
  `ChangeSink`; the same values again announce nothing; restart-only fields set `restart_pending`. `reset(id, key?)`.
  Locks recover from poisoning. Errors are typed (`SettingsError`, on the wire `{ code, message, retryable }`).
- `tauri` feature: `manage` (the `paddock://settings` sink to every window; a start-up problem is posted once to
  `paddock-health`'s centre as `settings:file` with an Open Settings action), `COMMANDS` (`settings_overview` for every
  window; `settings_set` (1 MiB) and `settings_reset` for main windows and dialogs whose contract names them; the writes
  run on the blocking pool), `handler`, `handler_with_grants`, `route`.
- Generated TypeScript in `ui/projects/paddock-ui/shell/settings/generated/` (`PADDOCK_WRITE_TS=1 cargo test -p
  paddock-settings --all-features --lib generated`): `settings.ts`, `settings-commands.ts`, `settings-fixtures.ts`.
- Coverage: 99%+ lines; not covered are the `#[tauri::command]` wrapper lines and the blocking pool's join error (a panic
  outside `apply`, which is contained already).

## `paddock-shell` (plan S17)

The shell as one Tauri plugin (ADR 0012 §3, decision D2): `paddock_shell::Builder` (see "Consuming the crates" above).

- `COMMANDS` folds every shell crate's table into one (`command_table!` composition) plus the plugin's own
  `shell_version`; one `Guard` with the dialog grants (`paddock_dialogs::tauri::grant_of`) wraps `Routes` over each crate's
  `invoke_handler()`. Commands are `plugin:paddock-shell|<command>`; the guard and dialog contracts see the plain names.
- Set-up (in order): the app identity, the jobs registry, the downloader (when given), service health and the
  notification centre (always; the app's services, `notifications.json` in the data folder unless `in_memory`), settings
  (`settings.json` in the config folder by default) with the shell's `pd.appearance` section, the dialog contracts (an
  empty file when none is given), the start-up watchdog (defaults unless `startup` is given), windows (when given; dialog
  windows then get their feel), the native menu (`native_menu(false)` in tests: macOS builds menus on the main thread
  only), updates with the `pd.updates` section, the app's sections, then the app's own `setup` closures. A part that fails
  fails the start with a message naming it.
- Hooks: page load (the start-up watchdog), every window's events (dialogs, windows, menu, health focus and forget), run
  events (the window layer's bounded exit; at exit the pending update and the messages are written). macOS's web content
  process termination is an app-builder hook: `.on_web_content_process_terminate(paddock_startup::tauri::web_content_terminated)`.
- Update preferences live on the `pd.updates` settings section (`paddock_settings::shell::updates`): the settings are their
  one writer and source; `updates_set_preferences` stores through `SettingsPreferences` and the section's apply puts
  stored and changed values into effect (`Updater::apply_preferences`). `updates.json` keeps only the updater's own record.
- Permissions: Tauri's build step makes `allow-<command>` for every command (`src/command_names.rs`, generated from the
  table); `permissions/window-classes.toml` (generated) groups them into `main` (also `default`), `popout`, `dialog` and
  `tray`, each exactly the commands the table opens to that class. The tests resolve them through Tauri's own ACL.
- `Builder::problems(manifest)`: the app's table against the shell's (no shared names), the dialog contracts against both
  tables, and with the app's `paddock-manifest.json` (`manifest::check`): menus and keys (`paddock_menu::manifest`), every
  command a dialog declares served and open to dialogs, contracts and manifest dialogs the same set, UI health services
  equal to the Rust services, every app settings section with a page and every generic-form page with a section.
- `shell_version` answers `{ shell, app }`; the crate's version is `@decryp7/paddock-ui`'s (a test reads `package.json`,
  `scripts/publish.mjs` refuses a difference), and the UI logs a plain error when the plugin differs
  (`checkShellVersion`).
- Tests on Tauri's mock runtime: every command reaches its handler from a window of a class it is open to, each permission
  set opens exactly the table's classes, a dialog reaches only its contract, window notices reach every window, the update
  preferences flow through the settings, and `tests/demo.rs` checks the shell demo's contracts and manifest. The demo's
  real Rust side is the app `apps/shell-demo/src-tauri` (plan S18): its table writes the demo's `app-commands.ts`.

To add a crate: create `crates/<name>` (as one of the scaffolds), add it to `members` and `[workspace.dependencies]` in
`Cargo.toml` (CI's workspace-wide steps pick it up; add it to the feature-off step of the `crate` job in
`.gitea/workflows/ci.yml` when it has `cfg(not(feature))` code or a default feature).

## `paddock-updater` (plan S11)

Updates, About and What's New. With the `tauri` feature (and `install` for the installer over `tauri-plugin-updater`):

```rust
.setup(|app| {
    let key = option_env!("TAURI_UPDATER_PUBKEY"); // CI secret; else `plugins.updater.pubkey`
    let updates = paddock_updater::tauri::UpdatesBuilder::new("Pitwall")
        .public_key(key)                                   // no valid key: "Updates are not available in this build"
        .releases_page("https://dev.decryptology.net/decryp7/pitwall/releases")
        .changelog(include_str!("../../CHANGELOG.md"))     // What's New: the running version's notes only
        .legal("notices", "Third-party notices", include_str!("../resources/THIRD_PARTY_NOTICES.md"))
        .service_hooks(Arc::new(PitwallService))           // rule 24: before_install, on_version_change
        .installer(Arc::new(paddock_updater::tauri::PluginInstaller::new(app.handle().clone(), key.unwrap_or_default())));
    paddock_updater::tauri::install(app.handle(), updates)?;
    Ok(())
})
.run(|app, e| if let tauri::RunEvent::Exit = e { paddock_updater::tauri::on_exit(app); })
```

The release address is `plugins.updater.endpoints[0]` (the `stable` channel) unless the builder names channels. Checks
and downloads are jobs on the app's `paddock-jobs` registry and use the app's `paddock-download` downloader; the UI is
`@decryp7/paddock-ui/shell/about`.

## `paddock-secrets` (plan X5)

One credential store for the apps (decision D6: service = the app's display name, account = `<scope>:<key>`, e.g.
`Spotter` / `channel:w1:2:token`):

```rust
use paddock_secrets::{KeyringSecrets, MemorySecrets, Name, Secret, Secrets};

let secrets = KeyringSecrets::os("Spotter");             // feature `keyring`; tests use MemorySecrets::new("Spotter")
let name = Name::new("w1", "token")?;                    // parsed once: 1-128 printable ASCII, no ':' in the key
secrets.set(&name, &Secret::new(token)?)?;               // Secret: 1-8192 chars on one line
let header = secrets.get(&name)?.map(|s| format!("Bearer {}", s.expose()));
```

- `Secret`: `Debug` is `Secret(***)`, `Display` is `***`, zeroized on drop, `expose()` is the one way to read it. Errors name
  what to fix and never contain a value.
- `Secrets` (port): `get -> Result<Option<Secret>>`, `set`, `delete` (missing is not an error). `Vault` is the raw
  `(service, account)` layer under it (`OsVault`, `MemoryVault`); `VaultSecrets::new(service, vault)` adds the scheme.
- Fallback: `WithFileFallback::new(primary, FileSecrets::new(dir))` is the opt-in (Pitwall's admin key). The file is used
  only when the primary is *unavailable* (no Secret Service, locked keychain) or has no value; files are created 0600
  (written to a 0600 temp file and renamed) and a file with group/other bits is refused (`InsecureFile`). On Windows no mode
  exists: `dir` must be a per-user folder (the app's data dir under `%LOCALAPPDATA%`), whose ACL is owner, SYSTEM and
  Administrators only.
- Old entries: `rename_entry(vault, (old_service, old_account), (new_service, new_account))` reads old, writes new, deletes
  old (idempotent, never loses the secret); Pitcrew's `Pitcrew notification channel` / `<key>` becomes `Pitcrew` /
  `channel:<key>`, its `Pitcrew Gitea API` / `<url>` becomes `Pitcrew` / `<url>:read token`, and so on.
- Rule 7: the keyring calls (`src/os/native.rs`) panic unless `PADDOCK_SECRETS_REAL=1` when running this crate's tests or
  when an app's tests set `PADDOCK_SECRETS_FORBID_REAL=1` (put it in the app's test environment), so a test that reaches the
  real keychain fails. `PADDOCK_SECRETS_REAL=1 cargo test -p paddock-secrets --features keyring -- --ignored` runs the one
  opt-in round trip by hand.

## `paddock-mcp` (plan X22)

```rust
use paddock_mcp::{AuditLog, Policy, Server, ToolDef, ToolOutput, serve_stdio};

let audit = Arc::new(AuditLog::new(500));                                   // the app shows and clears this
let server = Server::new("debrief", env!("CARGO_PKG_VERSION"), Arc::new(SystemClock::new()), audit.clone(), Arc::new(Redactor::new()));
server.add_tool(ToolDef::read_only("search_events", "Searches the open logs.", schema, |ctx, args| {
    Ok(ToolOutput { text: rows, structured: None, next_cursor })               // ctx.cursor, ctx.cancel; capped, timed, audited
}).paged())?;
server.set_policy(Policy::read_only());                                      // from the settings page; off by default
serve_stdio(&server, &mut stdin.lock(), &mut stdout.lock())?;                // `--mcp` mode; or serve_loopback(server, keys, listener, 4)
```

Rules built in (ADR 0007): local only (stdio, or 127.0.0.1 with a handshake line holding a per-client `KeyStore` key: shown once,
only a SHA-256 kept, revocable), off by default, every call audited with redacted arguments (`query`/`content` fields are kept as
a size only), arguments validated against the tool's schema (a wrong call is a tool error the model can fix), results cut at the
tool's byte limit with a cursor for more, a call that passes its time limit is cancelled and abandoned (at most 8 still running),
a panicking tool is contained, actions (`ToolDef::action`) are hidden and refused until `Policy::allow_action(name)`. Sighter's own
rules sit on top and are not relaxed. `client` has the request builders and typed results for a host (Pitwall); the wire types are
shared. The contract test pins the message shapes against the specification's JSON Schema. `notifications/cancelled` stops a running call; a
connection runs up to 4 tool calls at once and answers them as they finish. With the `tauri` feature, `McpControl` + `paddock_mcp::tauri`
are the settings page's engine (`Builder::mcp` in the shell): the on/off switch, tools shown, actions allowed by name, client keys (issued
once, only the hash kept, the list persisted through `paddock-secrets`), the audit log and its `paddock://mcp` event. Not built: `prompts`, subscriptions,
sampling, HTTP transport (stdio and loopback only, as ADR 0007 decides).
## `paddock-archive` (plan X24)

```rust
use paddock_archive::{extract, extract_job, Layout, Links, Modes, NoControl, Options};

let opts = Options { layout: Layout::Flat { max_depth: 2 }, links: Links::Copy, modes: Modes::AllExecutable, ..Options::default() }
    .filter(|name| name == "llama-server" || name.ends_with(".so") || name.starts_with("LICENSE"));
let report = extract(&archive, &runtime_dir, &opts, &NoControl)?;          // all or nothing: staging folder, then a rename
// As a job (Jobs list, phases "Expanding"/"Unpacking" in bytes, Cancel):
pool.spawn_job(registry.register("unpack", "Unpacking the runtime", None), move |ctx| extract_job(ctx, &archive, &dest, &opts))?;
```

The format is recognised from the content. Defaults: 10 000 entries, 4 GiB per file, 8 GiB in all, at most 200 times the archive's size
(never below 64 MiB), 32 folders deep, 1024-character paths. Refused: `..`, absolute paths, drive letters, backslashes, control
characters, Windows device names, names differing only in case, links when `Links::Refuse` (the default), links that leave the tree.
`sweep_stale` removes staging leftovers of a crash. Fuzz: `cargo +nightly fuzz run archive` in `crates/paddock-archive`. Zstd is not read
(no app needs it).

## `paddock-bench` (plan X26)

Rule 16's budgets, checked the same way in every app. The app's benchmark records numbers; the harness stores, compares and fails.

```rust
use paddock_bench::{Kind, Recorder, SystemClock};

let clock = SystemClock::new();
let mut rec = Recorder::new("debrief", env!("CARGO_PKG_VERSION"));
rec.throughput(&clock, "import-clef", lines, 5, || import_all())?;       // items/s, median of 5 runs
rec.time_ms(&clock, "page-unfiltered", Kind::QueryLatency, 7, || page())?; // ms, median of 7 runs
rec.memory_bytes("rss-querying", peak_rss_bytes)?;                        // MB
rec.finish().save(&root.join("bench/debrief.results.json"))?;             // versioned, atomic
```

`bench/budgets.toml` (TOML data, bounded, never executed):

```toml
[[budget]]
name = "page-unfiltered"
kind = "query-latency"   # optional guard
limit = 500              # a maximum for times and memory, a minimum for throughput
tolerance_pct = 10       # past the limit but inside this: passes with a warning
regression_pct = 20      # worse than the previous run by more: fails even inside the limit
```

`paddock-bench check --budgets bench/budgets.toml --results bench/debrief.results.json [--previous old.json] [--json|--markdown]`
exits 0, or 5 when a budget is missed, a measurement is missing or a regression is found (1 for an unreadable file, 2 for
a wrong command line). `paddock-bench propose --results ... --margin 25` writes a first `budgets.toml`. The tests of an app's
benchmark logic use `ManualClock`, so they do not depend on machine speed. The nightly workflow template is
`.gitea/workflow-templates/bench-nightly.yml`.

## `paddock-cli` (plan X25)

One look and behaviour for every command line (Pitwall, Dyno, Debrief, the agent and service programs). Pure: writers and
times are passed in.

```rust
use paddock_cli::{finish, CliError, CommonFlags, Exit, Format, Help, Progress, Report};

let (flags, rest) = CommonFlags::extract(&std::env::args().skip(1).collect::<Vec<_>>()); // --json -q -h -V
let result: Result<Report, CliError> = run(&rest);              // Err(CliError::not_running("Pitwall", "Start the app."))
let exit = finish(flags.format, result, &mut stdout, &mut stderr); // text, or ONE JSON object (errors too)
std::process::exit(exit.code());
```

Exit codes (a contract): 0 ok, 1 failure, 2 usage, 3 not running, 4 refused, 5 findings, 130 interrupted. Data goes to
stdout, progress and notes to stderr. `Progress::update(now, line)` throttles to one line per 500 ms, drops repeats and lets a
new phase through. `Table` renders text, CSV or JSON with bounded cells and a truncation note. `parse_time`, `parse_size`,
`format_size` read command-line values once. `paddock_service::Verb` registers its words in `Help` and decides `status`'s exit.

## `cargo-paddock-plugin` (plan X51, ADR 0001)

`cargo install cargo-paddock-plugin --registry decryp7`, then `cargo paddock-plugin <command>`. Output follows `paddock-cli`:
`--json` writes exactly one object (errors too), `-q` silences notes, exit codes 0 ok, 1 failed, 2 wrong command line, 4 refused
(a package that is unsigned, signed by a stranger or changed), 5 findings.

```text
cargo paddock-plugin new my-plugin [--dir D] [--id com.you.my-plugin] [--publisher "You"] [--api-path <checkout>/crates]
cd my-plugin
cargo paddock-plugin test                       # build, start the program against FakeHost, run the conformance checks
cargo paddock-plugin run                        # start it and print what it describes
cargo paddock-plugin keygen --out ~/.paddock/me.key        # owner-only file, never overwritten; prints the public key
cargo paddock-plugin build [--target <triple>]...
cargo paddock-plugin pack [--target T]... [--program T=<file>]... [--key me.key] [--out F]   # dist/<id>-<version>.paddockplugin
cargo paddock-plugin sign dist/x.paddockplugin --key me.key [--out F]
cargo paddock-plugin verify dist/x.paddockplugin [--trust <public key text or file>]...        # exit 4 when not trusted/intact
```

- `new` writes `Cargo.toml` (dependencies: `paddock-plugin-api` and `serde_json`; dev: `paddock-plugin-test`), `manifest.toml`, a
  library with the plugin (one tool), a `main` that is one `serve` call, a conformance test, `.cargo/config.toml` (the registry) and
  `.gitignore`. Nothing existing is overwritten. `--api-path` depends on a checkout's crates by path (no network).
- `build` runs `cargo build --release --bin <manifest binary>` into `<project>/target` whatever `CARGO_TARGET_DIR` says; a target
  that is not this computer's needs `rustup target add`.
- `pack` reads `manifest.toml` (targets, program name), takes each program from `target/` or `--program`, and writes the archive
  through `paddock-plugin-package` (fixed times, sorted entries, `SHA256SUMS`, optional signature): the same inputs give the same bytes.
- `verify` opens the package (layout, limits, checksums, signature) and, with `--trust`, decides trust like the app does
  (`TrustPolicy`); without it only the integrity is checked and the output says so.
- `test` compares what the plugin describes with its manifest (id, version, protocol, tools, views, MCP tools) and runs
  `paddock_plugin_test::conformance`; each finding is `[check] problem`. `--program <file>` starts a program built elsewhere and
  `--no-build` uses the existing build.
- Pure core (`args`, `template`, plans in `pack`/`build`), the one process start in `cargo.rs` (through `paddock-proc`),
  `FakeHost::launch` in `paddock-plugin-test` starts a program like the app's launcher does.

## `paddock-names` (plan X18)

Rule 20 as a pure library: the user types "grafic bilder" and finds `CompanyA.Graphic.Builder.exe`, or "memory pages" and finds
`\\Memory\\Pages/sec`. The app's engine builds a `Catalogue` and exposes `search` as a command (the palette and the assistant
call the command; the matching is never written in TypeScript).

```rust
use paddock_names::{Candidate, Catalogue, Entry, Style, group, vocabulary_from};

// Names with counts -> one group per entity (spellings joined) -> entries.
let vocab = vocabulary_from(&candidates);                       // learns the words the data separates
let entries = group(&candidates, &vocab).iter().map(|g| Entry::from(g).kind("process")).collect();
let names = Catalogue::with_vocabulary(entries, Style::Path, vocab)?;
let found = names.search("grafic bilder", 5);                  // found.matches[0].how == How::Typo, .why says so
if let Some(q) = found.clarify { /* "By 'builder', do you mean A (process) or B (process)?" -> q.options are ids */ }

// Counters, symbols, models: plain words, other fields rank below the name.
let counters = Catalogue::text(vec![Entry::new(path, "Bytes Received/sec").fields(&["Network Interface", "lo0"])])?;
```

- `How`, best first: `UserAlias`, `Exact`, `Joined`, `Parts`, `Alias`, `Prefix`, `Words` (every word anywhere, any order),
  `Typo` (only when nothing matches better). Order is score, then weight, then id, so results are deterministic.
- `Style::Path` reads the last path component and drops extension, vendor prefix, version and architecture; `Style::Text`
  drops nothing (`Pages/sec`, `lo0`).
- Bounds: 250,000 entries (more is `NamesError::TooManyEntries`), 256 characters of a query or a name, 24 names, 8 fields and 16
  aliases per entry, 50 matches. Nothing panics on any text.
- Stays in the app: the entity catalogue, AI aliases (pass them as `Entry::aliases`), the user's corrections (`user_aliases`),
  context ranking and co-occurrence merging (layer on the returned matches).
## `paddock-channels` (plan X17)

Where the apps send alerts, from Pitcrew's `notify-channels` and Spotter's `mail.rs`:

```rust
use paddock_channels::{Dispatcher, EgressPoster, LettreSender, Message, Severity, Settings, channel_policy};
use paddock_jobs::{CancelToken, time::SystemClock};
use paddock_net::{Egress, Reach};

let egress = app_egress;                                 // paddock-net Egress built with channel_policy(Reach::Lan)
settings.validate()?;                                    // on Save: the message says what to fix; at most 8 channels
let d = Dispatcher::new(settings, secrets, Arc::new(EgressPoster::new(egress)), Arc::new(LettreSender), Arc::new(SystemClock));
let msg = Message::new("diskLow", Severity::Error, "Disk low", "C: has 2 GB left", "runner-1");
let deliveries = d.dispatch(&msg, &cancel);              // blocks: run it in a job; one Delivery per channel, in order
let test = d.test_with(&draft, Some(&typed_password), "Spotter", &cancel);   // "Send test" before Save; d.test(id, ..) after
```

- Settings hold no secret: a webhook or ntfy channel with `auth` uses the token `Name("channel:<id>", "secret")`, and so does an e-mail
  channel with a `username` (its password; `ChannelConfig::secret_name`, `secret_name_of(id)`); an old Pitcrew
  entry moves with `paddock_secrets::rename_entry`. `forget_secret` removes it when the channel is deleted.
- Bounded: title one line of at most 200 characters, body at most 64 KiB (cut on a character boundary with a marker), at most 8
  channels, 10 recipients, 64 events a channel, 8 KiB a header; the answer's body is never read.
- Settings page engine (`ChannelsControl`, feature `tauri` for the commands `channels_get`, `channels_save`, `channels_set_secret`,
  `channels_clear_secret`, `channels_test` and the `paddock://channels` event): settings in a `SettingsStore` (`FileSettings` over
  `paddock-env`), `save` validates and then deletes the secrets of removed channels, `set_secret` is called only by the secret dialog's
  window, `test` returns an id at once and reports a `ChannelsEvent::Test` (one test per channel, four at once). UI:
  `@decryp7/paddock-ui/shell/channels`.
- Network: `EgressPoster` over `paddock-net` (https only, plain http to this computer with `Reach::Loopback`, offline mode,
  connection reuse, no redirects, the answer's body never read).
- Retries: transient failures only (no connection, timeout, HTTP 408/425/429/5xx, a 4xx mail reply), at most 3 attempts by default
  (`RetryPolicy`, 5 at most), waits from `paddock_jobs::time::Backoff` (2 s doubling to 30 s), a server's `Retry-After` honoured up
  to that cap, every wait cancellable and on the injected `Clock` (tests use `FakeClock`). A bad configuration, a missing secret,
  a refused login or certificate, or a 4xx rejection is reported at once. The test send makes one attempt.
- Containment: every channel runs on its own thread under `catch_unwind`; a panic is `SendError::Panicked` and the others still
  deliver. Redirects are never followed (a 3xx is a rejection). Errors name the host only (no path, query or login) and never a
  secret. SMTP does not use the HTTP proxy.
- Tests use `FakePoster` / `FakeMail` (scripted, recording) or an in-process loopback server; the real adapters are tested
  against those servers only.

## `paddock-guard` (plan X51, ADR 0001)

Crash containment from Debrief's `debrief_core::guard`, without its plugin types. Domain-free: the owner is a string, the call
kind a label, the budget a `Duration`.

```rust
let clock: Arc<dyn Clock> = Arc::new(SystemClock);
let (quarantine, problem) = Quarantine::persistent(QuarantinePolicy::default(), clock.clone(), dirs.state("quarantine.json"));
let guard = Guard::new(Arc::new(quarantine), clock);
// A remote call: refused while quarantined, 10 s budget, a panic is Crashed, a typed error is Failed (not counted).
match guard.call("my-plugin", "import", Some(Duration::from_secs(10)), move || remote.import(file)) {
    Err(CallError::Refused(why)) => show_disabled(why),
    Err(CallError::Hung { .. } | CallError::Crashed(_)) => {} // already counted
    other => use_it(other),
}
guard.quarantine().record("my-plugin", Failure::Garbled, "oversized frame"); // what the adapter itself detects
guard.quarantine().release("my-plugin"); // the user re-enabled it
```

- `Quarantine` counts failures per owner inside the policy window on the injected clock; at `max` the owner is quarantined
  until `release`. `status`/`all` give failures, total, last error (500 characters), quarantined and hung; `set_observer` hears
  each start and lift once. Quarantined owners are kept in a versioned file (`VersionedFile`); a missing, damaged or newer file
  gives an empty quarantine and a `Problem`. Failure times are not kept (the clock restarts).
- `Guard::call` with a budget runs on a helper thread and a timer on the clock; an overrun marks the owner hung (a counted
  failure) and refuses calls until the abandoned call returns. In-process guarding cannot stop a running call or survive an
  abort; that is the process boundary's job.
- Tests drive a `FakeClock`; nothing sleeps or measures time. Locks poisoned by a panicking thread are recovered.
## `paddock-view` (plan X51, ADR 0002)

The declarative view model a plugin returns instead of web code: pure data, validated in Rust, rendered by the host's generic
components. Depends on `paddock-ipc` (for `Json`, the arguments of an action) and serde only.

- `ViewDoc { schema, title, root, etag }`; `Node { id, kind: NodeKind }` with `Stack`, `Text`, `Table`, `Tree` (flat, pre-order
  `depth`), `Graph`, `Diagram`, `Form`, `ReviewList`, `Matrix`, `Chart`, `Badge`, `Progress`, `Empty`, `NeedsAi` and `Link`.
  `ActionRef { id, label, confirm, args, writes }` carries the text label that makes every interactive node accessible;
  `Link` is an action or `OpenInBrowser { url, label }`. Icons, tones and text styles are closed enums.
- Wire: JSON (camelCase, externally tagged enums) and postcard, no `untagged`/`flatten`/internal or adjacent tags (postcard
  cannot decode those) and no `skip_serializing_if`; variants only appended. `to_json`/`from_json`/`to_postcard`/`from_postcard`
  refuse more than 1 MiB and stop nesting at 64 nodes while decoding (postcard has no recursion limit of its own).
- `validate(&ViewDoc) -> Result<(), ViewError>`: depth 16, 20,000 nodes (nodes, tree nodes, graph nodes, diagram elements,
  table rows, findings, matrix cells, fields), 1 MiB encoded, strings 8 KiB, ids 128 bytes and unique, graphs and diagrams
  5,000 nodes / 20,000 edges with edges naming existing nodes, inline rows 2,000 (more: `RowSource::Paged`), charts 20,000
  values, `u64`s below 2^53, links `http`/`https` without user information. `validate_action` bounds a `ViewAction`.
- `apply_patch(&mut ViewDoc, ViewPatch)`: `Replace { base_etag, etag, id, node }` (a stale base or an unchanged etag is refused) or
  `Doc` (a new etag); all or nothing, and the result is validated.
- Host-added action arguments: `row` (table row), `page`, `node` (tree/graph node, `expand`), `finding` (and `reason`), `values`
  (form), `column`/`descending` (`sort`).
- Generated TypeScript: `ui/projects/paddock-ui/view/generated/view.ts` (`PADDOCK_WRITE_TS=1 cargo test -p paddock-view --lib
  generated`; a test fails on a stale file). The Angular `<pd-view>` renderer is `@decryp7/paddock-ui/view` (`ui/projects/paddock-ui/view`).
- Fuzz: `crates/paddock-view/fuzz` (`cargo +nightly fuzz run decode` from `crates/paddock-view`).

## `paddock-ipc` (plan X13, ADR 0001 step 1)

The family's one IPC between an app and its own processes (workers, plugins, agents, services), from Debrief's
`debrief-ipc` (frames, peer with credits and cancel, launcher, child loop) and Dyno's `dyno-ipc` (sync threads, token,
endpoint, pushes), with Pitcrew's service pipe access list. Synchronous and thread-based, no async runtime; HTTP on loopback
stays only for Pitwall's public API (D5).

```rust
// A child that connects back (a worker or plugin).
let child = paddock_ipc::launch(&ChildSpec::new(exe, "the AI worker").arg("--worker"), Arc::new(NoCalls))?;
let answer: Reply = child.peer().request(&Request::Ask(q), Some(Duration::from_secs(30)))?;
// In the child's main: paddock_ipc::child::serve("Debrief", PROTOCOL, |_| Arc::new(typed(handle)), |_, _| {})

// A companion the app did not necessarily start (agent, service).
let server = LinkServer::start(ServerConfig::new(dirs.runtime().join("agent.json"), BUILD), handler)?; // companion
let client = LinkClient::start(ClientConfig::new(dirs.runtime().join("agent.json"), BUILD, "the agent"),
                               None, Some(launcher), Some(on_state))?;                              // app
```

- `frame`: 4-byte length + postcard `Frame` (Debrief's frame version 2 plus `Push` at the end), `FrameLimit` 1 KiB to 64 MiB
  (16 MiB default). A frame over the limit is skipped without being allocated and fails only its call (`IpcError::TooLarge`,
  or a `tooLarge` failure to the other side); a length beyond 64 MiB or garbage ends the connection. `ProtocolVersion`
  (same major, known minor) refuses with a message that says which side to update.
- `endpoint`: a Unix socket in a 0700 folder (`Listener::bind_in(AppDirs::runtime())`, temporary folder as fallback) or a
  named pipe with an `Access` list: `owner_only()` (D:P(A;;GA;;;OW), 0600) or `service_for_interactive_users()` (Pitcrew's
  SDDL for a Windows service serving the user's app); fixed endpoints for services (`Listener::bind_fixed`).
- `handshake`: `Host`/`Client`; `Hello { version, protocol, token, pid }` checked in constant time and against the pid the OS
  reports (and the launched child's pid); refusals are `Fail { id: 0 }` with code `version` or `auth`.
- `peer`: `call`/`request` (timeout, then cancel and `cancel_grace`), `stream`/`CallStream` (credits; drop cancels),
  `push`/`on_push` (bounded, dropped and counted when the listener lags), `Handler`/`typed`, `CallCtx` (`send_item`,
  `is_cancelled`, `cancel_probe`, call back into the other side), `max_concurrent_calls` (`busy`). Codes in `codes`.
- `launch`/`child`: `ChildSpec` → `paddock_proc::Supervised` (no console window, own process group, tree ended on drop,
  optional `MemoryCap`, clean environment), the `Bootstrap` JSON line on stdin (stdin stays open as the lifeline), and the
  child's `serve`/`read_bootstrap`/`connect`/`lifeline`. `Bootstrap::extra` carries what an app adds (Debrief's sandbox).
- Every limit runs on an injected clock (`PeerOptions::clock`, `Host::with_clock`, `ServerConfig::clock`); each client of a
  `LinkServer` is authenticated on its own thread within `handshake_timeout`, so a silent client delays nobody; the server
  caps `max_connections` and `max_handshakes` (`busy`), and `Subscriptions` push a topic's events to the connections that
  asked for it.
- `link`: `Discovery` (endpoint, token, pid, build, protocol; written owner-only through `paddock_env::write_atomic`, read
  bounded; `Found::Stale` for a dead pid, `Found::Untrusted` for a file others could write), `LinkServer` (accept thread,
  `peers()` for pushes, removes its file on stop), `LinkClient` (state `Connecting`/`Connected`/`Retrying`/`GaveUp`/`Stopped`,
  `paddock_jobs::time::Backoff` on the injected clock, `retry()`, `Replace::{Terminate, Ask, Never}` for a companion of
  another build; never its own process), and the pure `plan`.
- Tests start real processes through the `paddock-ipc-test-child` binary and leave none behind; nothing sleeps or measures
  time (gates, channels, a `FakeClock` for the back-off). `fuzz/` holds the frame-codec fuzz target
  (`cargo +nightly fuzz run frame`).

## `paddock-plugin-package` (plan X51, ADR 0001 "Signing and packaging")

The `.paddockplugin` package and the manifest model, from Debrief's `debrief-plugin-package` and the manifest half of
`debrief-plugin-api` (`manifest.rs`, `sandbox.rs`'s permission names). It depends on no plugin crate (only `paddock-env` for the
owner-only atomic writes and the versioned trust file); `paddock-plugin-api` re-exports its types and `paddock-plugin-host` installs
and verifies with it. No Debrief type moves: app contracts appear only as `Namespace { ns, minor }` (`debrief.import/3`).

```rust
let manifest = Manifest::parse(&std::fs::read_to_string("manifest.toml")?)?;          // limits, typed errors, unknown keys refused
let key = SecretKey::generate_to(&dir.join("publisher.key"))?;                        // 0600, never replaces an existing key
archive::build(Build { manifest, programs: vec![(target, &exe)], sign: Some(&key) }, &out)?;   // deterministic .paddockplugin
let pkg = Package::open(&out)?;                                                       // bounded read, checksums, signature
HostInfo::new("Debrief").check_protocol(id, pkg.manifest.protocol)?;                  // paddock-plugin-api: says who must update
let trust = policy.decide(&pkg.signature)?;                                           // Builtin | Signed | Unsigned (developer mode)
let sha = pkg.extract(&out, manifest::host_target(), &installed)?;                    // exec bit, re-verified while copying
archive::verify_installed(&installed, &sha)?;                                         // before every start: changed binary = Tampered
```

- `manifest`: `Manifest` (id, name, semver version, publisher, description, platform `ProtocolVersion`, binary and targets, key id,
  `Contribution`s, `PermissionRequest`s). `Contribution` is `Tool`/`View` (with `Action { id, writes }`: write-capable actions are
  badged and never exposed over MCP), `Settings`, `McpTool` (read-only by construction) and `Extension(Namespace)`. `Permission` is
  `OpenedFilePaths`, `ReadFolders`/`WriteFolders(GrantRef)`, `RunPrograms(ProgramRef)`, `Network(HostPattern)`, `Elevated(ops)`,
  `McpExpose`, `Memory { mib }`, `Cpu { percent }`; default deny, each kind once, lists of 1 to 64 names. `manifest.toml` is
  bounded (64 KiB, 256 contributions, 64 actions each, 16 targets) and **unknown keys are refused** (as in Debrief); the model
  types are plain serde (no `untagged`/`flatten`/internal tags), so the same `Manifest` crosses the postcard IPC, where the host
  runs `Manifest::check` on it. Compatibility is not here: `paddock_plugin_api::compat::HostInfo` judges a manifest's protocol and
  namespaces with the one rule of `paddock_ipc::ProtocolVersion` (`accepts`, `skew`, `introduced_in`) and words who must update.
- `names`: `PluginId`, `GrantRef`, `ProgramRef`, `HostPattern` (`*.example.com`), `Namespace`: newtypes that validate in
  `new` and in `Deserialize`, so a name from the wire is as valid as one from a file. `ProtocolVersion` is re-exported from `paddock_ipc` (the one
  implementation; the package depends on `paddock-ipc`, which is a dependency only, nothing is launched) and `GrantRef` from
  `paddock-view` (the one type a path field and a folder permission share).
- `archive`: zip with `manifest.toml`, `bin/<target>/<binary>[.exe]`, `SHA256SUMS` and an optional detached Ed25519 `signature`
  over the digest of the manifest bytes plus the sums bytes. `Package::read`/`open` (with `Limits`: 256 MiB package, 128 MiB program,
  512 MiB total, 64 entries; sizes counted while reading) refuse traversal, absolute and drive names, links, encrypted, duplicate and
  unlisted entries, a hash that differs from `SHA256SUMS`, and a signature that does not match (`Tampered`). `build` is deterministic.
  `extract` re-hashes while copying; `verify_installed` is the check the host runs before every start.
- `keys`: `SecretKey`/`PublicKey` text forms, key ids, `save` owner-only through `paddock_env::write_atomic`, `generate_to` never
  replaces a key. `trust`: `TrustPolicy` (the app's built-in keys plus the user's `TrustSettings`) decides `Trust::Builtin`,
  `Trust::Signed` or `Trust::Unsigned` (unsigned or unknown key: only with developer mode, with a warning); `TrustStore` is the
  versioned owner-only state file (missing or damaged gives nothing trusted and developer mode off).
- `fuzz/` holds the manifest and archive fuzz targets (`cargo +nightly fuzz run manifest`, `... package`); `tests/properties.rs` runs
  never-panic and never-accept-other-content properties with the tests.
## `paddock-storage` (plan X32)

The Settings > Storage mechanism in one place. The app declares what it keeps; the crate measures, previews and clears.

```rust
use paddock_storage::{Category, Item, Kind, Storage, from_this_run};

// shell: Builder::new(identity).files(..).storage(|_| Ok(Storage::new(Arc::new(categories)).within(&dirs)))
let cache = Category::new("cache", "Cache", Kind::Cache).folder(dirs.cache()).entries(|_| None).note("Rebuilt when needed.");
let logs = Category::new("logs", "Logs", Kind::Data).folder(dirs.logs())
    .entries(move |p| from_this_run(p, Some(started), "written")).note("Logs written in this session are kept.");
let models = Category::new("models", "Models", Kind::Data).folder(models_dir)
    .item(Item::path(model).removed_by(|p| engine.delete_model(p)).protected(active_reason)); // the engine stops using it first
```

- **Boundary.** `Storage::within(&dirs)` (plus `also_within` for the web view's profile or a temp folder the app owns): a category
  whose folder resolves, links included, outside those folders is shown but never cleared.
- **Preview first.** `scan` returns sizes, `clearable_bytes`, the biggest items and `capped` (lower bounds) and keeps what it saw.
  `clear` refuses without that preview (`noPreview`) and uses it up; `validate` refuses unknown, duplicate or too many categories
  before a job exists.
- **Confirmation.** A `data` category is cleared only when the request says `confirmed`; the UI gets that from a modal window.
  Nothing is ever cleared by itself (Pitwall's leftovers are a category the user clears).
- **Re-check before every delete** (`guard::recheck`, from Dyno): the item still exists, is not a link, has the kind the preview
  saw, resolves through its real parent inside the category's folder, was listed by the preview and has not changed since. What
  fails stays, with the reason, in the result.
- **Size tree** (`tree`, from Dyno's disk-space map): `storage_tree(id)` walks one category's folder into a bounded tree (three levels,
  twelve items per folder with the smaller ones folded into one line, entries and time limited, no links, one volume); `capped` says
  the sizes are lower bounds. Only folders inside the app's folders are walked.
- **Jobs.** `storage_scan`, `storage_clear` and `storage_tree` start a job (phase-labelled progress from the first moment, Cancel in every phase,
  one job at a time) and return at once; the outcome is a `paddock://storage` event. `Run` is the port, `Recorder` its fake.
- **UI.** `@decryp7/paddock-ui/shell/storage` (+ `/page`, `/dialog`): `StorageBackend` (Tauri, memory with the same rules),
  `StorageStore`, the Storage page and the `pd.storage.confirm` decision window.

## `paddock-files` (plan X31)

The rule "open only paths the user chose" in one place, plus what an app does with the user's files. Built from Debrief's
`paths.rs`, `recent.rs` and `opener.rs`; Pitcrew's `open_url`/`reveal_logs` and Spotter's and Dyno's direct calls to the dialog and
opener plugins become `Files`.

```rust
use paddock_files::{Access, Files, GrantSource, PickRequest, recent};

// shell: Builder::new(identity).files(|_| Ok(Files::for_app(&dirs, &[recent::FILES, recent::PROJECTS])))
// an app command takes the calling window's grants (a dialog or pop-out resolves to its host):
#[tauri::command]
async fn open_log(grants: paddock_files::tauri::WindowGrants, app: tauri::AppHandle, path: String) -> Result<(), CommandError> {
    let granted = grants.read_file(&path, &["log", "txt"])?;        // notGranted unless the user chose it
    // ... open it ...
    paddock_files::tauri::opened(&app, "files", &granted, None);     // recorded only after a granted open
    Ok(())
}
```

- **Grants** (`PathGrants`, `GrantedPath`): filled only on the Rust side by the native dialogs (`Files::pick`), drag and drop
  (the shell's window hook grants a drop to the window and sends `paddock://files-dropped`), the command line
  (`tauri::grant_command_line`), a window's project file and files the app produced (`GrantSource`). Per window, at most
  `MAX_GRANTS`, expiring after `GRANT_TTL` (a project does not), compared canonically so `..` and links cannot reach another
  file; a read grant on a folder covers what is inside it. The message of a refusal says what to do (choose it again).
- **Validators** (`path`): `parse` (absolute, bounded, no NUL), `canonical`, `output_file`, `child`, `confined` (a removal stays in
  a root).
- **Pickers**: `PickRequest` is validated (bounded title, letters-and-digits extensions, a plain default name) and shown by a
  `Picker`; `tauri::NativePicker` (feature `dialog`, native glue) is the `tauri-plugin-dialog` adapter. It runs on the blocking
  pool.
- **Recent lists** (`Recents`, `ListSpec`): any number of named lists with caps (`recent::FILES`, `recent::PROJECTS`), in a
  versioned `recent.json` of the config folder (damaged or newer reads as empty), announced on `paddock://files-recent`.
  `Files::regrant` grants an entry again on use.
- **Open** (`open`): reveal, open with the default program and the browser as command lines that are pure functions of the OS
  (`Os`), started by a `Launcher` (a detached child with a reaper and a timeout). Files that would run (`runs_on_open`: scripts,
  programs, installers) are refused; web addresses must be http, https or mailto.
- **Commands** (`COMMANDS`, folded into the shell plugin): `files_pick`, `files_recent_list|remove|clear|grant`, `files_reveal`,
  `files_open`, `files_open_at_line` (a granted file in VS Code at a line, 1 to ten million; the file is an argument of the editor, never run;
  the editor is found per OS and a missing one says what to install), `files_open_app_folder` (data, config, cache, logs by name: the
  webview never supplies the path) and `files_open_url`. Recent entries remember whether they were a folder, for the UI's glyph. The wire types and names are generated into `ui/projects/paddock-ui/shell/files/generated/`
  (`PADDOCK_WRITE_TS=1 cargo test -p paddock-files --all-features --lib generated`).
## `paddock-net` (plan X16)

Every outbound request goes through one `Egress`:

```rust
use paddock_net::{Egress, Policy, Reach, Request, SystemLookup, UreqWire};
use paddock_download::ProxyMode;

let policy = Policy::default().reach(Reach::Public).allow_hosts(&["api.example.org", "*.cdn.example.org"]).min_gap(Duration::from_secs(1));
let wire = Arc::new(UreqWire::new(&policy, ProxyMode::System)?);          // one connection pool per proxy and reach
let egress = Arc::new(Egress::new(policy, wire, Arc::new(SystemLookup), Arc::new(SystemClock)));
let resp = egress.send(&Request::get(url).if_none_match(&etag)?, &cancel)?; // resp.not_modified(), resp.etag(), resp.body (capped)
egress.set_offline(true);                                                  // every later request: NetError::Offline, no lookup, no socket
let transport = egress.download_transport();                               // the same policy under paddock_download::Downloader
```

- Before **every hop** (a redirect is a new request): no login in the URL; https (plain http only for `localhost`, `127.x`,
  `::1` when the reach is `Loopback`); the host on the allow-list; **every** address of the name within the reach (one bad
  address refuses the name, as a DNS-rebinding answer would be); `169.254.169.254` and all link-local, unspecified and
  multicast addresses are never reachable. The wire applies the reach again to the addresses the socket connects to.
- A redirect never goes farther than where the request started (a public start cannot reach the LAN or this computer), never
  goes from https to http, drops `Authorization`, `Cookie` and `Proxy-Authorization` when the host changes, and a 301/302/303
  after a POST becomes a GET. The wire never follows one.
- Caps: answer body (8 MiB, `max_body`), request body (1 MiB), redirects (5, at most 10), headers (32, 8 KiB each).
  `stream` returns the unread body for downloads (no whole-request limit; `body_timeout` between chunks).
- Politeness: `min_gap` between two requests to one host on the injected clock, cancellable. `Retry-After` is read with
  `Response::retry_after`; retrying is the caller's (`paddock-channels`, the downloader).
- Tests use `FakeWire` (scripted, records, counts sockets) and `StaticLookup`; the real wire is tested against a loopback server.
- Name checks resolve through the system resolver before each hop; behind a proxy that resolves names itself (internal-only
  names), pass a `Lookup` that answers from the proxy's policy instead.

## `paddock-env` (plan X7)

Folders per OS and versioned state files, the code every app wrote its own copy of:

```rust
use paddock_env::{AppDirs, AppId, Problem, VersionedFile};

let app = AppId::new("spotter", "Spotter")?;              // id: Linux folders and SPOTTER_HOME; name: macOS/Windows folders
let dirs = AppDirs::for_app(&app)?;                       // data(), config(), cache(), logs(), runtime(); dirs.ensure()?
let file = VersionedFile::<Prefs>::new(dirs.config().join("prefs.json"), 2).with_migration(1, v1_to_v2);
let loaded = file.load();                                 // never fails: value + Option<Problem> to report once
file.save(&loaded.value)?;                                // atomic; refused when the file is from a newer version
```

- Folders: macOS `~/Library/{Application Support,Caches,Logs}/<Name>`; Windows `%LOCALAPPDATA%\<Name>` for everything (decision
  D3; `legacy_roaming_data()` names the old `%APPDATA%\<Name>` for the one-time move); Linux XDG (`$XDG_*_HOME/<id>`, logs
  under the state folder, runtime in `$XDG_RUNTIME_DIR`). The full table is on `AppDirs`.
- `<ID>_HOME=<root>` puts every folder under `<root>/{data,config,cache,logs,run}`; honoured in debug builds only
  (`for_app` passes `cfg!(debug_assertions)`), ignored in release. A `portable.flag` beside the executable puts them under
  `<exe folder>/<id>-data`. Tests resolve with `AppDirs::resolve(app, os, &MapVars, exe_dir, allow_override)` and a temp folder.
- `VersionedFile`: JSON object with a top-level `version` (missing = 1). Missing gives defaults; damaged gives the valid `.bak`
  or defaults, with the file kept as `<name>.corrupt-<time>` (newest 5); newer is untouched, defaults, saves refused with a
  clear error; older runs the `with_migration(from, step)` chain once, writes the new format and keeps `<name>.v<from>.bak`.
  Saves write a temporary file, fsync, rename; the previous good file becomes `.bak` (a damaged one never replaces it).
  `write_atomic` is the same replace for non-JSON files.

## `paddock-packs` (plan X15)

Knowledge packs (rule 21), from Spotter's `pack.rs`, Pitwall's `packs.rs`, Dyno's `cleanup/pack.rs` and the two `user_regex.rs` copies:

```rust
use paddock_packs::{BuiltinPack, DirSource, Limits, Origin, PackError, PackKind, PackLoader, PackStore, Reloader, user_packs_dir};

struct Cleanup;
impl PackKind for Cleanup {
    type Raw = RawPack;                    // serde type of one file; no `deny_unknown_fields`, no `format` field
    type Pack = Vec<Category>;             // what the app uses
    const FORMAT: u32 = 1;
    fn convert(raw: RawPack, origin: Origin) -> Result<Vec<Category>, PackError> { /* the app's own rules; origin tells user from built-in */ }
}

let loader = PackLoader::<Cleanup>::new(
    vec![BuiltinPack { name: "builtin.toml", text: include_str!("builtin.toml") }],
    Box::new(DirSource::new(user_packs_dir(&dirs))),
    Limits::default(),
);
let store = Arc::new(PackStore::new(loader));
store.observe(|snapshot| emit_packs_changed(snapshot));                    // once per real change
let reloader = Arc::new(Reloader::new(store.clone(), Arc::new(SystemClock)));
reloader.reload_now();                                                      // first load
let _watch = paddock_packs::watch::spawn(reloader, &NotifySource, &user_packs_dir(&dirs))?;   // feature `watch`
let snapshot = store.current();                                             // Arc<Snapshot>: packs, problems, warnings, health()
```

- One answer to a bad pack (decision D11): refused whole with a named reason (a syntax error names line and column); keys the
  kind does not know are warnings naming the key (`unknown key "category.0.flavour" is ignored`). One bad pack never touches the
  others; a pack that turns bad while running keeps its last good version and says so; a file deleted during a reload is gone,
  not an error.
- Limits (`Limits`, all bounded before the typed parse): file bytes (read at most max + 1), entries per list, string and key
  length, nesting depth, values in the file, `*.toml` files in the folder. The folder source ignores dot-files, `~` files,
  sub-folders and symbolic links. Nothing in a pack is executed or followed.
- Reload: `Reloader` debounces on a `paddock_jobs::time::Clock` (300 ms quiet, 1.5 s at most), so a storm is one reload and
  tests move a `FakeClock`. The new snapshot is built whole, then swapped; readers keep theirs. `watch::ChangeSource` is the
  port (`FakeSource` for tests, `NotifySource` behind the `watch` feature); X23 (`paddock-watch`) replaces the adapter.
- `user_regex`: `compile`, `compile_with`, `compile_set`; `RegexLimits::DEFAULT` (Debrief: 8 MiB) and `STRICT` (Spotter: 1 MiB);
  typed `RegexError` (`TooLong`, `TooComplex`, `Invalid`) whose messages say what to fix.
- `Snapshot::health()` gives the one line (`Ok`, `Notes`, `Degraded`) for the `packs` health or notification entry.
- `tauri` feature: `paddock_packs::tauri::{Setup, install}` over `view::kind(id, label, store)` (the pack type erased); `Builder::packs(|app|
  Ok(Setup::new().kind(...).health("packs")))` in `paddock-shell`. Commands `packs_status` (every window) and `packs_reload` (main windows and the
  Settings dialog), the `paddock://packs` event to every window on a real change, and the health entry: down with the first refused pack and `Retry`
  as its repair (a reload, served by the shell, never by a pack: rule 19), ok when none is. The UI is `@decryp7/paddock-ui/shell/packs` (Settings >
  Knowledge packs).

## `paddock-watch` (plan X23)

```rust
use paddock_watch::{Debouncer, NotifySource, Settle, SettleConfig, scan_folder, spawn};

let debouncer = Arc::new(Debouncer::new(Arc::new(SystemClock), Duration::from_millis(300), Duration::from_millis(1500)));
let _watch = spawn(debouncer, &NotifySource, &folder, /* recursive */ true, move || rescan())?;   // feature `notify`; FakeSource in tests

// Debrief's watched folders: a file is ready when its size and time stopped changing.
let mut settle = Settle::new(SettleConfig::default());
let ready = settle.observe(&scan_folder(&folder, &|rel| is_log(rel)).files, clock.now_ms());   // ready.new / .changed / .skipped
settle.imported(&stamp);  settle.failed(&path, "locked", now);                                  // back-off, then given up
```

- `Debouncer`: quiet time, never later than the longest wait after the first event; `poll()` is `Idle`, `Waiting(d)` or `Now`.
  Tests move a `FakeClock`; the thread's 50 ms wake-up only bounds how late a due action runs.
- `Settle` ignores temporary names and empty files, forgets vanished files, retries a locked file with back-off (4 attempts), reports
  a too-large file once, and bounds its pending list; `Batcher` groups ready files into one job. `scan_folder` skips symbolic links
  and stops at 50 000 entries or 16 levels (`Scan::truncated`).
- Not here yet: Dyno's keep-for times (`dyno-core/src/watch.rs`) are app policy over `Settle`'s stamps and stay in Dyno.

## `paddock-log` (plan X1)

The log of every app, service and sidecar, from Debrief's `logging.rs` + `clef.rs` + the generic half of `activity/logger.rs`:

```rust
use paddock_log::{LogConfig, LogLevel, Logging};

let log = Logging::new(LogConfig::new(&app, "app", env!("CARGO_PKG_VERSION")).with_dirs(&dirs).with_commit(commit)).install()?;
log::info!("started");                         // `log` and `tracing` share one stream
log.set_level(LogLevel::Debug);                // from Settings > Diagnostics > Logging, no restart
let _e = paddock_log::ipc_span(window, op_id, command).entered();   // Window / OperationId / Command on every event inside
paddock_log::log_from_webview(window, level, message);               // the webview's log command
```

- Files: `<id>.<day>.log` (process `app`) or `<id>-<process>.<day>.log` in `AppDirs::logs()`; `LogSettings` = level, `max_file_mb`
  (20; after it only errors, with a one-time notice) and `keep_files` (7 older days). An unusable folder never stops the app:
  lines are dropped, `LogHandle::file_problem()` says why (for a health entry), the open is retried every 30 s.
- Privacy: the default `Redactor` masks secret-like keys, replaces content-like keys (`Content`, `Body`, `Line`, `Text`,
  `Query`, `Prompt`, ...) by `<omitted: N characters>` and bounds every value; apps add `with_hook` rules (paths, user names);
  `fingerprint` shows equality without the value. `Redact` is a trait so X2's report writer can reuse it.
- Errors: identical errors are written once a minute then summarised (`flush_repeats` at exit); each error carries the
  window's latest breadcrumbs (events with an `Operation` property).
- Panics: the hook writes a Fatal entry (thread, location, bounded backtrace, span fields) and `crash-<process>.json`
  (`CrashMarker`, in `AppDirs::data()`); the next start reads it (plan X3) and clears it.
- `spawn_child_capture(name, stderr)` logs a child's output line by line (4 KiB per line, `Child` + `child:<name>`).
- Tests use `Logging::scoped`, a temp folder and `FakeClock` (`paddock_jobs::time`, re-exported; `now_utc` reads it as UTC); no sleeps.

## `paddock-proc` (plan X6)

Child processes, the code seven helpers in five apps wrote their own copies of. The apps' `clippy.toml` forbids
`Command::new` and points at `paddock_proc::command`.

```rust
use paddock_proc::{Limits, MemoryCap, Supervised, command, run_bounded};

let mut git = command("git");                                  // no console window on Windows (R06)
git.args(["status", "--porcelain"]);
let out = run_bounded(git, Limits::new(Duration::from_secs(10)).max_output(1 << 20))?;   // tree ended on timeout
if out.success() { use_it(&out.stdout_text()); }

let browser = Supervised::spawn_capped(cmd, MemoryCap::new(4 << 30, "the browser").listener(listener))?;  // drop ends the tree
paddock_proc::end_all();                                       // at shutdown / in the panic hook: every supervised tree
```

- `command(program)`; `clean_env(&mut cmd, KEPT_ENV)` clears the environment to an allow-list; `lower_priority(&mut cmd)` is
  below-normal on Windows and `setpriority` before exec elsewhere (never `nice(1)`, which drops `DYLD_*` on macOS).
- `run_bounded(cmd, Limits)`: stdin closed, stdout and stderr captured up to `max_output` each (the rest is read and dropped so
  the child never blocks); a timeout, or with `stop_on_cap` the cap, ends the whole tree. `Outcome` is `Exited | TimedOut |
  OutputCap`; spawn failures are a typed `RunError` whose message names the program, not its arguments.
- `Supervised`: its own process group on Unix; `kill`, `terminate(grace)` (SIGTERM, then the tree), `wait` and drop end and
  reap the tree. Every one is in a `Registry` (`global_registry()` for `spawn`), so `end_all()` ends them without their owners.
  `MemoryCap` samples the tree every 200 ms (PSS on Linux), warns at 80%, ends it over the cap and tells a `MemoryListener`
  (`Warning | Stopped | Ended`, with the peak); `stopped_over_memory()` and `memory_peak()` read it back.
- `process_memory(pid)` / `tree_memory(root)`: PSS from `/proc/<pid>/smaps_rollup` on Linux (resident size if unreadable),
  resident size elsewhere; thread entries are skipped (rule 26). `kill_tree`, `ask` / `end` (one pid, not its group, for the
  leftovers of finished jobs), `terminate(pid, grace)`, `is_alive`, `processes_mentioning(needle)` (leak checks).
- `login_path()`: the `PATH` of the user's login shell (`$SHELL -l -i -c`, then `-l -c`; bounded, absolute entries only;
  `None` on Windows or when no shell answers); `merge_path(login, inherited, sep)`. The caller sets `PATH` once at start.
- Tests start only the test binary itself (re-executed with a mode) or `sh` scripts in a temp folder, and check afterwards
  that the pids are gone.
- Not yet: a Windows job object (`KILL_ON_JOB_CLOSE`). Windows trees end through `taskkill /T /F`, which cannot reach a
  helper whose parent already exited; the stdin lifeline and the sandbox limits of `debrief-ipc` stay with `paddock-ipc` (X13).

## `paddock-runtime` (plan X14)

Programs an app downloads on demand instead of bundling them; nothing outside the app's catalogue is ever downloaded.

```rust
let catalogue = Catalogue::parse(include_str!("ffmpeg.toml"))?;     // refused whole with a reason when anything is off
let store = RuntimeStore::in_app_data(&dirs, "ffmpeg")?;            // <data>/runtimes/ffmpeg
store.sweep(catalogue.for_this_platform().ok())?;                   // at start-up: staging leftovers, stale downloads
let ctx = jobs.register("runtime", "Downloading FFmpeg", None);
pool.spawn_job(ctx, move |ctx| Ok(store.install(ctx, &downloader, pinned, &Ffmpeg)?))?;
let exe = store.active().map(|i| i.executable);                     // the release in use, if any passed its self-test
```

- Catalogue (`format = 1`, `name`, `version`, `source`, `license`, `[[platform]]` with `key` = `<windows|macos|linux>-<x86_64|
  aarch64>`, `url` (https, or http to the loopback for tests), `size`, `sha256`): unknown keys, a name or version that is not a
  folder name, a size of 0 or over 4 GiB, a bad checksum, a platform twice and an archive that is not a zip or tar
  (`.zip`, `.tar`, `.tar.gz`/`.tgz`, `.tar.xz`) refuse it whole.
- `RuntimeKind` says what differs: `locate(root)` (the program inside the unpacked tree) and `self_test(exe, version)` (bounded,
  checks the version). `install` is one job: the download's phases (hashed while written, resumable from `downloads/`), then
  `unpack` through `paddock-archive` (its limits and guards; links kept inside the tree; `with_archive_options` to change
  them), `self-test`, `install` (uncancellable rename).
  Cancel works up to the swap and keeps the partial download.
- The release in use stays until the new one passed; it becomes `previous` for `rollback(kind)` (which re-tests it); older
  releases are removed. A release that fails its self-test, and one the app reports with `quarantine(version, reason)`, moves
  to `.quarantine/` (the newest three kept, listed by `quarantined()` for diagnostics); the previous release takes over.
- `state.json` is a `paddock_env::VersionedFile`; a lost or damaged one falls back to the newest complete folder on disk
  (each release folder has its `runtime.json` marker). One install at a time per store (`.lock`, across processes: `Busy`).

## `paddock-chromium` (plan X14)

The one Chromium driver, from Debrief's `chromium-driver` and Spotter's `spotter-render`: no application types.

```rust
let store = paddock_chromium::runtime::store(&dirs)?;               // the pinned Chrome for Testing, installed as a job
let exe = discover::find(Some("SPOTTER_CHROMIUM_PATH"), runtime::installed(&store)).ok_or(Error::NotFound)?;
sweep_profiles(&launch::profiles_dir(&dirs));                       // at start-up: profiles of crashed runs
let mut opts = LaunchOptions::new(Profile::Throwaway(launch::profiles_dir(&dirs)));
opts.window = Window::OffScreen;                                    // OnScreen(Some(bounds)) for a window the user sees
let mut browser = Chromium::launch(&exe, &opts)?;                   // supervised tree: time limit, memory cap (PSS)
let page = browser.start_page()?;                                   // the window keeps one tab
page.track_dom_changes()?;                                          // only when the app may add a script to pages
page.navigate(url, Duration::from_secs(30))?;
match page.wait_ready(Some("#results"), &Rule::default(), budget)? { Outcome::Ready => read(page.capture()?), .. }
browser.close();                                                    // Browser.close, then whatever is left; drop does the same
```

- `launch::command_line(dir, &opts, sandbox)` is the only place the arguments are built; `START_PAGE` is always last (Debrief
  1.1.1's `--no-sandbox` after the start page). `--enable-automation` stays (no stealth). `Window::{OnScreen(bounds),
  OffScreen, Headless}`, `QUIET_FLAGS` (no GPU, no component downloads, no throttling; `opts.quiet`), `disk_cache_bytes` for a
  kept profile, `extra_args`. `Sandbox::decide(env, root)`: dropped only as root with `PADDOCK_BROWSER_NO_SANDBOX=1` (CI
  containers), then also `--disable-dev-shm-usage`; `launch_with(exe, opts, sandbox)` lets tests run that branch on any OS.
- The process is a `paddock_proc::Supervised` tree (no console window, in the registry `end_all` ends; on Windows in its
  kill-on-close job, which every end closes, the time limit's included). Every wait runs on
  `opts.clock` (a `FakeClock` in tests): the lifetime limit, the start-up wait (`start_limit`), `start_page`'s wait and the
  pages' `wait_ready` polls; only `close_gracefully`'s grace for a real process uses the OS clock. `memory_cap` goes through
  `MemoryCap` (an injectable sampler); `ended_by()` says which limit ended the browser.
  A failed start says why (the end of the browser's error output, or the memory cap). `check_display` explains a headful
  launch on Linux without `DISPLAY`/`WAYLAND_DISPLAY`. Profiles: `Throwaway(base)` (removed at the end; `sweep_profiles`
  removes those of dead processes) or `Persistent(path)` with an accent theme (`write_theme`).
- `Chromium::gone(page)` tells `Exited`, `Disconnected` and `PageClosed` (the user closed the window) apart; a closed page
  fails every call with `Error::Closed`. `close_gracefully(grace)` sends `Browser.close` (no code-sign clone on macOS).
- `Page`: `navigate`, `back`, `evaluate`, `info`, `text_excerpt`, `snapshot(selector)` (URL, title, text, load state, element,
  DOM quiet time, password field), `wait_ready`, `challenge`, `capture` (16 MB cap), `screenshot_png`, `block_heavy`.
  `ready::readiness` (element and region rules of both apps) and `challenge::detect` (consent, human check, login wall) are pure.
- `discover`: `find(override_var, downloaded)` (an explicit path, then the app's download, then Edge/Chrome/Chromium);
  `find_for_tests()` returns only `CHROMIUM_TEST_PATH`, and refuses it when it names an installed browser (rule 7, T02).
- `runtime`: the embedded `chromium.toml` (Chrome for Testing 156.0.8078.47, five platforms), `install(ctx, downloader,
  store)`, `installed(store)`, `ChromeForTesting` (locate, `--version` names the pinned version).
- `fake` (feature `test-support`, Unix): a fake DevTools browser that re-runs the test binary; it plays the user (close the
  window, quit, drop the connection, stall loads) and answers page scripts from a file (`page_shows`). Apps refuse to start
  when `PADDOCK_FAKE_BROWSER` is set. `tests/real_browser.rs` drives a real Chrome for Testing headless when
  `CHROMIUM_TEST_PATH` is set and checks that nothing is left behind; with `PADDOCK_REQUIRE_TEST_BROWSER` set a missing browser
  fails it (Paddock's Linux CI job, whose `paddock-ci` image carries the build `chromium.toml` pins).
- Pins: `node conformance/ci/pin-runtime.mjs crates/paddock-chromium/src/chromium.toml --chrome-for-testing Stable` rewrites
  version, URLs, sizes and checksums (any `paddock-runtime` catalogue: `--version <v>` or as it is); the CI image's
  `CHROME_VERSION` must follow (`scripts/ci-image.test.mjs` checks it).

- Windows (plan X54): each supervised child runs in a kill-on-close job object held by the `Supervised` value; dropping it, or
  the app ending in any way, ends the child and every helper it started, orphans included (`Supervised::in_job`).

## `paddock-service` (plan X4)

Background services and login items, from the four designs in the apps: Pitwall's and Pitcrew's `service_install.rs` /
`service_repair.rs` (the R03 repair, the registered version, Login Items), Pitcrew's and Pitwall's server `platform/` and
`service/` (systemd, SCM, the restart after an update), Dyno's `service/` (a `Runner` port, the `Run` value, a per-user
agent) and `login.rs` (one launch-at-login switch), and Debrief's agent (a launch daemon an administrator installs).

```rust
use paddock_service::{Manager, ServiceSpec, ProcessType, backend::Side, native, render};

let spec = ServiceSpec::builder("net.decryptology.pitwall.service", "Pitwall", "pitwall-server")
    .bundle_id("net.decryptology.pitwall").display_name("Pitwall local LLM server")
    .supervised_env("PITWALL_SERVICE").process_type(ProcessType::Interactive).build()?;
// The files the app ships, generated and checked in a test (paddock_commands::testing::sync_generated):
let plist = render::launchd_plist(&spec, render::LaunchdProgram::Bundled);   // src-tauri/macos/<label>.plist
let hooks = render::nsis_hooks(&spec);                                        // src-tauri/windows/installer-hooks.nsh
// At run time, on a background thread (rule 10):
let backend = native::host_backend(&spec, &program, Side::App, dirs.data())?;
let mut service = Manager::new("Pitwall", backend, dirs.data(), spec.label(), env!("CARGO_PKG_VERSION"), ping);
service.on_start(&cancel);                      // rule 24, at every start (and again a minute later)
let status = service.status();                  // the UI; status.health() for paddock-health
service.repair(&cancel)?;                       // the Repair button (never depends on the service, rule 19)
```

- **One spec, every file.** `render` is pure and golden-tested (`tests/golden/`, `PADDOCK_WRITE_TS=1` rewrites): the bundled
  plist (`BundleProgram`, `AssociatedBundleIdentifiers`, the supervised variable) or a plist file (launch agent or daemon),
  the systemd unit (every word quoted against splitting and `%`/`$` expansion), the `Run` value (C runtime quoting), the
  service as `sc.exe` calls (delayed automatic start, LocalSystem, restart after 10 s, 1 min, 5 min) and the NSIS hooks (a
  per-machine installer registers the service; a per-user one only stops and removes the agent). `ServiceSpec::builder`
  refuses names, arguments and links that would break a file or a command line.
- **Backends behind ports.** `LaunchdBundled` (`SMAppService` + `launchctl print`/`print-disabled`/`bootout`),
  `LaunchdFile` (plist + `bootstrap`), `Systemd` (user or system unit), `WindowsRun` (`reg.exe`, Task Manager's
  `StartupApproved` switch), `WindowsService` (`sc.exe`, inside the elevated `<program> install`) and
  `ElevatedWindowsService` (the app's side: one UAC prompt per request). All OS access goes through `System`, `AppService`
  and `Elevator`; `testing::{FakeSystem, FakeAppService, FakeElevator}` record every command, file write and wait on a
  virtual clock. `backend::choose` picks the mechanism (pure); `native::host_backend` builds it.
- **Rule 24 as a decision and a sequence.** `plan` decides: the user's switch (Login Items, Startup, Services, `systemctl
  disable`) and a pending approval are never overridden, not even by Repair; a refused job while the service is down is
  replaced (R03); where the OS pins the build (macOS) the registration is replaced once per version change, elsewhere only
  when the definition differs (otherwise the version is just remembered); a missing or moved program is re-pointed; a
  registration a newer copy made is left to it (two copies never ping-pong); nothing is installed unasked; where every
  change needs a prompt (a Windows service from the app) a repair is offered, never started. `repair::replace` runs
  unregister → evict (`launchctl bootout`) → wait until gone (bounded) → register → check, retried with a growing pause,
  then a bounded wait for the service; approval, elevation, a declined prompt and "unavailable" stop at once; a cancel is
  honoured only where nothing is left unregistered. Every phase is a `Step` for the log and `RepairTracker` (the UI).
- **The registered version** is `service-<label>.json` in the data folder (`VersionedFile`; C21's
  `service_registered_version`), written only after a registration took; a lock beside it keeps two copies from repairing
  at once (a dead copy's lock is taken over after 10 minutes).
- **The program's side.** `watch::ExeWatch` exits a supervised program whose executable changed once it is idle (the OS
  starts the new one); `supervised::supervised_path` merges the login shell's `PATH` (`paddock_proc::login_path`, R05);
  `Verb` is the shared first argument (`install`, `uninstall`, `status`, `stop`, `print-unit`, `print-plist`, `--service`).
- **Launch at login.** `login::launch_at_login_section` (`pd.login`, one switch, off by default) and
  `apply_launch_at_login` (the app's own `Autostart` and the service together, each tried, one message).
- **The Windows service host (X4b).** `scm_host::supervise` is the `--service` supervisor over a `Sessions` port: the
  logged-on user's session (connected before disconnected, console first, never session 0; Remote Desktop users are
  found), the program started there with the user's token and environment plus the supervised variable, restarted after
  a pause, moved when the session changes, stopped gracefully (its `stop` verb, a grace period, then ended).
  `native::run_as_service(spec, exe)` is the SCM dispatcher and the Win32 side. Pitcrew's pipe SDDL is the app ↔ service
  link and stays with `paddock-ipc` (X13).
- **In the app** (`tauri` feature, `control::ServiceControl`): `service_status`, `service_repair`, `service_install`,
  `open_login_items` and the `paddock://service` event (`ServiceUpdate { status, error }`); requests run on their own
  thread, status answers from the last look while a repair holds the manager, the health entry is kept current and is
  the health registry's Repair (`RepairAction::OpenLoginItems` where macOS has the switch), start-up checks at 3 s then
  every minute. `paddock_shell::Builder::service(|app| Ok(ServicePart::new(Setup::new(manager).health("agent"))))`
  wires it and the `pd.login` "Start at login" section (only a change by the user acts). The UI is
  `@decryp7/paddock-ui/shell/service` (Settings > Background service).
- **The shell demo's agent** (crate `apps/shell-demo/src-tauri/agent`, binary `shell-demo-agent`): a heartbeat-only
  per-user agent, bundled as Tauri's `externalBin` (`agent/sidecar.mjs` builds it per target triple in
  `beforeBuildCommand`; the bundle check starts it from the built app with `--self-test`, rule 23), with its generated
  plist and NSIS hooks, so the page, the health entry and
  `scripts/shell-demo/smoke-service-update.sh` (C21: build N, then N+1; disposable Macs only, rule 7) have something real.
- Not here: Task Scheduler entries (no app uses them).

## `paddock-diag` (plan X2)

Report a problem, from Debrief's `activity/report` (the best of the three: manifest, sections, a zip the user reviews), Dyno's
`diagnostics.rs` (personal data removed, every file previewed with its size) and Pitcrew's `redact.rs` (secrets). Decision D9:
**nothing is sent**; the user saves the zip after seeing every file.

```rust
use paddock_diag::{AppInfo, Collected, Redactor, builtin};

let app = AppInfo::new("spotter", "Spotter", env!("CARGO_PKG_VERSION"), commit);
let redactor = Redactor::for_machine(&ProcessVars);                      // this machine's user, home folder and name, plus the defaults
let collected = Collected::collect(&app, &dirs, &builtin::all(), &redactor, Utc::now());
let report = collected.assemble("the timeline stays empty");             // description.txt + manifest.json
let preview = report.preview();                                          // the list the dialog shows: name, kind, size, summary
report.save(&path_the_user_chose, &cancel, &progress)?;                   // zip built in memory, then replaces the file atomically
```

- `Redactor`: `redact_secrets` (Pitcrew's patterns + Debrief's detectors: tokens, passwords, keys, JWTs, URL credentials, Luhn-checked
  card numbers), `Personal` (Dyno: user names, home folders, profile paths, machine name, e-mail and IP addresses) and
  `paddock-log`'s field rules (secret keys masked, file contents replaced by a size). A CLEF log line is parsed and each string
  property redacted, so a replacement never breaks a line. Apps add `with_pattern` (limited like user regexes: 500 characters,
  256 KiB compiled, 32 patterns) and `with_field_rules` (the same rules as in their log).
- `Section`s are the app's contributions; they return `Content` (text, JSON or log lines), never bytes, so every file is redacted.
  Built-ins: `SystemInfo`, `Settings` (the settings file, secrets masked), `Crash` (the marker), `Logs` (the newest part, read from the
  end of each file: a 1 GB log costs a seek). A section that panics, a bad or repeated name, too many files or bytes
  (`Limits`) become notes in the manifest and the preview, never a failure.
- The preview and the zip are built from the same file list. `save` checks the destination (`validate_destination`: absolute, `.zip`,
  existing folder), builds the zip in memory, checks the cancel token every 256 KiB, and replaces the destination atomically: a
  cancel or an error leaves no partial file.
- `logging`: the `pd.logging` settings section ("Diagnostics > Logging": level, MB per day, files kept) over `LogSettings`; the level is
  applied with `LogHandle::set_level`, the two limits need a restart; `logging::stored` reads the saved values before the log exists.
- `tauri` feature: `Setup` (sections, redactor, `LogHandle`), `install`, and the commands `diagnostics_preview` (collects on the blocking
  pool, reuses the collection while the user types), `diagnostics_save` (returns at once; a native save dialog asks where, then a
  job writes the zip, with Cancel from the first moment; the outcome is the `paddock://diagnostics` event), `diagnostics_cancel`, and
  `log_from_webview` (the webview's lines with their `Window`). The path is chosen only by the user (the `dialog` feature adds the
  native picker; the shell enables it with `native-dialogs`). The shell registers all of it from `Builder::diagnostics`.
- The crate has no network dependency (a test checks its manifest; `cargo tree` shows none). Tests use temp folders, a fake picker and
  Tauri's mock runtime; no sleeps.

## `paddock-pitwall` (ADR 0004, plan section 15)

The one Pitwall client, extracted from Debrief 1.1's `debrief_ai::engine::pitwall` and checked against Pitwall's own router
(`crates/pitwall-server/src/http.rs`). Blocking and bounded: apps call it on a worker thread (rule 10). Loopback only
(`127.0.0.1`, any other host is unrepresentable); no TLS, no cloud.

```rust
use paddock_pitwall::*;

let app = AppIdentity::new("debrief", "Debrief")?;
let link = Link::new(LinkConfig::new(app, Locator::system(), secrets /* Arc<dyn paddock_secrets::Secrets> */));
link.set_listener(Arc::new(|e| /* LinkEvent::Changed | Paired { use_for_app } */ {}));
let view = link.probe();                                  // discovery, health, key, a model; never pairs by itself
link.connect(true)?;                                      // the user chose "Use Pitwall": shows view.pairing.code, polls in the background
let reply = link.chat_with(ChatRequest::new(ModelSel::Auto(Caps::chat(8192)), msgs), &mut |delta| show(delta))?;
let state = Session::new(EngineChoice::Auto).state(&link);   // AiState -> NeedsAiView for <pd-needs-ai>
```

- `discovery`: `Locator::system()` reads `discovery.json` (16 KiB, strict, `baseUrl` must be `http://127.0.0.1:<port>` with a port of 1024 or more, pid alive)
  from the folders of `paddock-env` for `pitwall` (Local first and the old Roaming folder second on Windows, D3), else tries port
  11435. `PITWALL_HOME` names the data folder itself. Tests use `Locator::in_dir`.
- `keys`: `KeyVault` over `paddock_secrets::Secrets` (service = the app, account `Pitwall:API key`); `adopt_legacy` moves Debrief 1.1's
  entry; a 401 with a key deletes it (`KeyRevoked`), without one is `NeedsPairing`.
- `Client`: `health`, `begin_pairing` / `poll_pairing`, `select`, `models`, `model_ids`, `ensure_capability` (a model that cannot see
  is `Unsupported` locally, with Pitwall's reason), `jobs` / `cancel_job`, `count_tokens` / `truncate`, `task`, `embed` (batches of 64),
  `chat` / `chat_with` / `chat_stream` (an iterator; dropping it cancels). Every reply carries `Provenance` (`X-Pitwall-Model`,
  `-Engine`, `-Chosen-By`, `-Cold-Start-Ms`, `-Cache`, `-Alias`, `-Job-Id`). `CallOptions`: priority (`Interactive`, `Background`,
  `Batch`), cache policy, `CancelToken` (acts within a 50 ms read slice, even while Pitwall is silent), per-phase `Timeouts`
  (connect 2 s, first byte 120 s, idle 60 s for chat) and a whole-call deadline.
- `retry`: pure table (250 ms to 8 s, jitter, `Retry-After`; 4 attempts, 8 for batch); connect errors, `503` and `429` with
  `Retry-After` only; a stream is never retried after its first delta; probes (`health`, pairing poll) never retry.
- `PitwallError`: typed, each with `user_message(app)` and `next_step()` (`StartPitwall | InstallPitwall | Pair | InstallModel | Wait |
  UpdateApp | UpdatePitwall | ChangePermissions | None`).
- `AiState` / `NeedsAiView` (serialisable, camelCase): `Disabled | NotInstalled | NotRunning | Incompatible | NeedsPairing | Paused |
  NoSuitableModel | Unavailable | Ready`, with the actions that fix each.
- `engine`: `EngineChoice` (Automatic, Pitwall, Built-in), the pure `select`, `Session` (setting, "use the built-in engine for this
  session", AI off) and `run_or_fallback` (one engine per request, never a silent mix: `Routed::Unavailable(NeedsAiView)`).
- `health`: `service_spec` and `report`: Pitwall is `Degraded`, never `Down`, only when the setting wants it, and its Repair is
  `UseFallback` (or `OpenSettings` for an app without an engine) so it never depends on Pitwall (rule 19).
- `loopback`: the bounded, cancellable `127.0.0.1` HTTP/1.1 transport itself (header, line and body limits, chunked, injection-proof
  headers), reusable for other local servers: Debrief's llama-server client (`debrief-ai-worker/src/http.rs`) moves onto it.
- `testing` (feature `testing`): `FakePitwall`, an in-process fake with exactly the routes of `PITWALL_ROUTES`, Pitwall's answer and
  error shapes, scripted pairing, failures with `Retry-After`, held and dying streams. It waits on conditions, never on the clock.
  `tests/contract.rs` runs the client against it; with `PADDOCK_PITWALL_HTTP_RS=<pitwall>/crates/pitwall-server/src/http.rs` it also
  checks that Pitwall's router still has every route (Pitwall's CI should set it).
- `AiSnapshot` (`Session::snapshot`): the one value the shared `<pd-needs-ai>` draws (`@decryp7/paddock-ui/shell/pitwall`): state,
  the engine's sentence and buttons, the waiting pairing code, whether the app has an engine of its own. The app registers the
  four commands of `status::COMMANDS` (`pitwall_state`, `pitwall_connect`, `pitwall_cancel_pairing`, `pitwall_use_builtin`) over
  its `Link` and `Session` and emits `status::EVENT` (`paddock://pitwall`) when the snapshot changes. The TypeScript is generated
  by `PADDOCK_WRITE_TS=1 cargo test -p paddock-pitwall --all-features --lib generated`.

## `paddock-live` (plan X29)

Replaces Pitwall's `live.rs`, Dyno's `link.rs` pump, Pitcrew's `events.rs` forwarder, Spotter's push handling and the ordering gaps
of Debrief's channels. The UI half is `@decryp7/paddock-ui/live` (see `ui/README.md`).

```rust
impl LiveState for Overview {                       // the app's state
    type Delta = OverviewChange;
    fn apply(&mut self, d: &OverviewChange) { /* the one fold */ }
    fn merge(prev: &mut OverviewChange, next: &OverviewChange) -> bool { /* latest wins, or false */ }
}
let hub = Hub::new(Overview::default());
let forwarder = paddock_live::tauri::register(app.handle(), "overview", &hub)?;   // paddock://live/overview + live_snapshot
std::thread::spawn(move || paddock_live::run(&hub, &mut source, &clock, default_policy(), &cancel));
// a Source: connect, `feed.seed(state)` (link becomes connected, a snapshot goes out), then `feed.publish(delta)` until it ends
```

- Sequence: every `publish` is the next number; a frame is `from..=seq` because merged deltas stand for several numbers. A window
  applies a delta only when `from` is next; a gap, a reload or a new window asks `live_snapshot`. Frames are camelCase JSON with a
  `kind` tag (`snapshot`, `delta`, `link`).
- Bounded: each `Subscription` queues at most `with_queue_cap` frames (4 to 4096, default 256). Deltas merge into the queued one when
  `LiveState::merge` says so (a memory sample replaces the last); a consumer that still falls behind has its queue replaced by one
  snapshot (`resyncs()` counts it): memory never grows, nothing is lost silently. `reset` (a reseed) also replaces what was queued.
- `run` waits its delays on the injected `Clock` (a `FakeClock` in specs); the link announces `reconnecting { attempt, retryInMs,
  reason }` before each wait, `down` when the policy gives up and `stopped` on cancel. A connection that lasted `reset_after` starts
  the delays over. Reasons are short sentences, never content.
- A panic in an app's `apply` poisons nothing: locks recover (`PoisonError::into_inner`).
- Channel names are `[a-z][a-z0-9-]{0,31}`; the command is `Query [WINDOW]` and async (rule 10); dialog windows reach it only through a
  contract (`handler_with_grants`). Not covered: per-call streams to one caller (Debrief's `tauri::ipc::Channel` for search results
  and the live tail) stay in the app; a feed is for shared state.

## `paddock-assistant` (plan W6)

The chat that can use an app's tools, over Pitwall. Pure core plus the `tauri` feature; UI side: `@decryp7/paddock-ui/workbench/assistant`.

```rust
let provider = Arc::new(PitwallProvider::new(link));                       // or any other `LlmProvider` (decision D5)
let store = Arc::new(ConversationStore::open(dirs.state().join("assistant.json"), clock.clone()));
let tools = Tools::new(vec![Arc::new(FindEvents), Arc::new(DeleteBookmarks)])?;   // the app's `AssistantTool`s
let assistant = Assistant::new(provider, tools, store, clock, AssistantConfig::new("Pacenotes", SYSTEM));
paddock_assistant::tauri::manage(app.handle(), assistant);                  // commands: `paddock_assistant::tauri::COMMANDS`
```

- `AssistantTool`: `spec` (name, description, JSON schema), `access` (`ReadOnly` or `Write`), `budget` (calls per turn, result size),
  `describe` (the call as the user reads it), `call`. A `Write` tool is offered to the model only when the user ticked "Allow
  actions" for the message and each call waits for `assistant_approve_tool` (timeout on the clock). Tools never panic the turn.
- `run`: rounds, tool calls, tokens and wall time are budgets (`Budgets`); the last round has tools off; a call that cannot be read,
  names an unknown tool or misses a required argument is reported to the model and never run; Stop acts within a read slice.
- `ConversationStore`: `VersionedFile`; a damaged file gives defaults and one problem, a newer file is never touched; bounds on
  conversations, messages, text, data and file size.
- Commands (`<area>_<verb>`): `assistant_send`, `assistant_stop`, `assistant_approve_tool`, `assistant_conversations`,
  `assistant_load`, `assistant_delete`, `assistant_info`; event `paddock://assistant`; TypeScript generated by
  `PADDOCK_WRITE_TS=1 cargo test -p paddock-assistant --all-features --lib generated`.

## `paddock-metrics` (plan X30)

Replaces `metrics.rs` in Pitcrew (generic `TieredStore`), Pitwall (four tiers) and the tier maths of Dyno's agent store, and the four
free-space helpers (Debrief `disk.rs`, Pitcrew `probe.rs`, Pitwall `system.rs`, Dyno `sensors.rs`).

```rust
let retention = Retention::standard(30);                    // 5 s/1 h, 1 min/1 d, 10 min/1 w, 1 h/30 d; Retention::new(..) checks your own
let file = StoreFile::<MachineSample>::new(dirs.state("metrics.json"), retention.clone());
let (mut store, problem) = file.load();                     // damaged or newer file: empty store + a Problem to report once
let mut sampler = Sampler::new(SystemProbe::new(), clock).watch_disk(&runners_dir).watch_process(std::process::id());
store.push(sampler.sample());                               // every tier; a sample from the past is TooLate, never reordered
let chart = store.query(clock.wall_ms() as u64, Duration::from_secs(86_400));   // the finest tier covering the range
file.save(&store)?;
```

- A `Series` says how samples fold into a bucket (`begin`/`absorb`/`point`); the bucket may be any serializable value, so Pitwall's
  histograms fit. Memory is `Retention::capacity()` points at most (`MAX_TIERS` 8, `MAX_KEEP`, `MAX_POINTS`); a file written with another
  tier layout is dropped, a changed `keep` trims.
- `MachineSample` aggregates CPU and memory as means (with the CPU peak), free disk as the minimum. Own memory is `paddock-proc`'s
  `tree_memory`: PSS on Linux, threads skipped (rule 26).
- `volume_free` picks the mount with the longest matching mount point (verbatim Windows paths match plain ones); `Sampler::disk_space` caches
  the list of volumes (`mounts_every`, default 60 s, on the clock). `folder_size` stops at `max_entries`, `max_depth`, `max_time` (on the
  clock) or a cancel and never follows links; `folder_size_with` takes any listing for tests.
- Dyno keeps its SQLite history and uses `bucket_start`, `Retention::tier_for` and `Mean` for the tier maths.

## `paddock-testkit` (plan X40)

A `[dev-dependencies]` crate; nothing in it ships. It replaces `std::env::temp_dir()` (clippy forbids it in tests, `lint/clippy.toml`), the per-app env mutexes and the hand-made fake runners:

```rust
use paddock_testkit::{assert_golden, env_lock, FakeRunner, Reply, Scratch};

let scratch = Scratch::new();                       // work(), home(), tmp(); deleted on drop
let mut env = env_lock();                           // one lock for every test that touches the environment
env.hermetic(&scratch);                             // HOME, APPDATA, XDG_*, TMPDIR now point into the scratch folder
let cfg = scratch.write("conf/app.toml", "x = 1")?; // relative paths only; `..` and absolute paths are refused
run_the_code_under_test(&cfg);
assert_golden(&golden_dir.join("report.txt"), &report);   // PADDOCK_UPDATE_GOLDEN=1 rewrites the file
scratch.finish()?;                                  // or just drop it: anything left in tmp() fails the test
```

- `Scratch`: `work()` is for the code under test, `home()` stands in for the user's home, `tmp()` for the shared temp folder. Dropping a
  scratch whose `tmp()` is not empty panics with the names (not while the test is already failing); `finish()` returns them as an
  error to assert on. An unreadable `tmp()` counts as a leftover. `PADDOCK_TESTKIT_KEEP=1` keeps the folder of a failed test.
- `env_lock()`: a process-wide lock (a lock poisoned by a failed test is recovered); `set`/`remove` restore the original values in
  reverse order when the guard drops. `hermetic(&scratch)` is the in-process half of C14 (the wrapper `paddock-conformance run --`
  is the other half).
- Golden files: line endings normalised; the failure names the file, the first differing line and the update variable.
- `FakeRunner`: `on("launchctl", &["print"]).reply(Reply::ok("..."))`, replies queue (`.then(...)`, the last repeats), the first
  matching rule answers, an unscripted command is an error reply, `calls()`/`lines()`/`count()` record. An app implements its own
  runner trait for it in its test module (Dyno's `Runner`).
- `corpus(seed)` gives a bounded, deterministic set of hostile inputs; `never_panics(&corpus(seed), parse)` returns the inputs that
  panicked (the proptest-free half of X41).
- `FakeClock` (and `Clock`) are `paddock_jobs::time`'s, re-exported: assert on a fake clock, never on `Instant::now()` (C30).

### `CommandError` (plan X28)

`paddock_commands::CommandError` is the one error a command rejects with: `{ code, message, retryable, hint?, field?, cause?, kind? }`
in camelCase. `ErrorCode` is a short general list (`failed`, `invalid`, `notFound`, `notGranted`, `busy`, `cancelled`, `io`, `limit`,
`unsupported`, `unavailable`, `denied`, `missing`, `timeout`, `internal`), so a code means the same in every app; an app's own
distinctions (Pitcrew's `unknownRunner`, Debrief's `noModel`) go in `kind`. Constructors per code (`CommandError::invalid("...")`),
builders (`.hint()`, `.field()`, `.cause("health:store")`, `.kind()`, `.retry(bool)`), and `From` for `io::Error` (not found, denied,
io), `String`/`&str` (`failed`) and the guard's `Refusal` (`denied`, `limit`). `retryable` defaults by code (`busy`, `io`,
`unavailable`, `timeout`). The TypeScript (`ui/projects/paddock-ui/ipc/generated/ipc.ts`, with the runtime `ERROR_CODES`) is generated
by `PADDOCK_WRITE_TS=1 cargo test -p paddock-commands --lib generated` and checked on every run. Domain errors stay in their libraries
and convert once at the command edge.

## `paddock-shell` set-up checklist (plan X19)

`Builder::first_run(|app| FirstRun::new().notifications(access).pitwall(port, Caps::chat(8192)).item(tour))` turns on the first-run
checklist: the service and Start at login steps appear when the app has `Builder::service`, the notification and Pitwall steps when the
app names them, then the app's own `SetupItem`s. State: `setup.json` in the app's config folder (`{skipped, finished}`, versioned by
`paddock-env`). Commands: `setup_state` (blocking pool), `setup_act` / `setup_repair` (return at once; the step is marked working
and the outcome arrives on `paddock://setup`), `setup_skip`, `setup_finish`. Ports for tests: `ServicePort`, `NotificationAccess`
(`ToasterAccess` over the OS notifier), `PitwallPort` (`LinkPort` over `paddock_pitwall::Link`); the apps' own: `SetupItem`. The UI is
`@decryp7/paddock-ui/shell/setup`.

## `paddock-siblings` (plan X45)

```rust
let siblings = paddock_siblings::Siblings::native("dyno")?;
match siblings.open("pitcrew", &Handoff::show(Item::new("machine", "mac-mini")?))? {
    Outcome::Opened { .. } => {}
    Outcome::NotInstalled { message } | Outcome::NeedsUpdate { message } => show(message),
}
siblings.import("debrief", vec![log_path])?;        // files to import
```

The receiving app reads the same call back: `Handoff::from_paths(&args.paths, spec, &DiskProbe)` and `Handoff::from_link(&link, spec)` over
`paddock_window::launch::parse_args`. The contract per app is `FAMILY` (Debrief: any file; Spotter: `.json`; Pitcrew: `machine` items;
every app can be opened). Replaces Pitcrew's `crates/pitcrew-core/src/debrief_link.rs` and `app/src-tauri/src/handoff.rs` (the
log-extract logic stays in Pitcrew; only the finding and starting moves).
## `paddock-build` and `paddock_proc::self_test` (plan X34)

`paddock-build` is a build-dependency; the build.rs of an app is one line (`paddock_build::Build::new().sidecars(["x-service"]).run()`, feature
`tauri-build`). It embeds the one Windows application manifest (Common Controls v6) into every binary of the crate, test binaries
included, and tells tauri-build to add none of its own; it creates an empty `binaries/<name>-<target>[.exe]` for each sidecar that is not
built yet (a real file is never overwritten); and it reruns on `TAURI_UPDATER_PUBKEY`. The apps' `windows-app-manifest.xml` copies go.
App extras (Debrief's commit id) stay in the app's build.rs before the call. `paddock_proc::self_test::handle(name, version, &checks)` is
the first call in a sidecar's `main`: with `--self-test` it runs the checks, prints one line each and exits 0 or 1 (rule 23; C34 checks
that every sidecar has it, and `paddock-sidecar self-test` starts them from the built bundle).

## `paddock-testplan` (ADR 0005, plan X52)

`.sighterplan` and `.sighterresult`: the formats Pacenotes drafts and Sighter runs. Pure (bytes in, strings out, no I/O); both files are TOML.

```rust
use paddock_testplan::{parse_plan, parse_result, Coverage, NoRedaction, SecretRedactor, diff, junit, lint};

let plan = parse_plan(&bytes)?;                 // typed and validated; an error names line, key or id and the fix
let text = plan.to_toml();                      // canonical: fixed key order, so a regenerated plan diffs cleanly
let hash = plan.hash();                         // sha256 of the canonical text = a result's `plan_hash`
let warnings = lint(&plan);                     // a literal typed into a password/token/card field: warn, never fail
let redactor = SecretRedactor::for_plan(&plan, &keychain /* impl Resolver */);
let result_text = result.to_toml(&redactor);    // resolved secrets become {{secret:NAME}}; same for junit::write
let matrix = Coverage::from(&plan, &results);   // requirement x case x last status (and acceptance criteria), once for both tools
let changes = diff(&old_plan, &new_plan);       // links kept by case id and step; only edited steps need a look
```

- Limits: file 4 MiB, 5,000 cases, 200 steps a case, strings 4 KiB, ids 64, a wait 1 h, results 50,000 case results. `format` and `version`
  (major) come first; unknown keys at the same major are `UnknownKey { path, line }`; a newer major is `Version` ("needs Sighter/Pacenotes N
  or newer"); the last two majors are read (today only 1 exists).
- Ids are newtypes (`RequirementId`, `CaseId`, `ScreenName`, `PlanId`, `DataName`, `Tag`, `Column`, `SecretRef`); `#` is reserved for
  `AcceptanceRef` (`REQ-142#AC-2` is the second entry of that requirement's `acceptance` list); `RelPath` refuses `..`, absolute and
  backslash paths.
- `Step` is a closed enum (click, type, press, select, wait_for, assert_screen, assert_text, wait); in the file a step is
  `action = "..."` plus exactly the fields it needs, and a `target = { text, name, class, inside, near, nth }`. A value is plain text,
  `{{data.COLUMN}}` (a column of the case's table) or `{{secret:NAME}}`; plain text may not contain `{{`.
- Results carry evidence by relative path and sha256, never inline; `Status` is `pass | fail | blocked | needs_recording | skipped`.
  `RunResult::is_for(&plan)` says whether a result belongs to this exact version of the plan.
- The crate never resolves a secret: `Resolver` is the app's; `Redactor` is what writers take.
- Tests: fixtures with invented data, every refusal, properties (`parse(write(p)) == p`, no input panics), `fuzz/` (`cargo +nightly fuzz run
  formats`). JSON Schema export and thumbnails in results are not built (ADR open questions).

## The plugin platform: `paddock-plugin-api`, `-host` and `-test` (plan X51, ADR 0001 and 0003)

Rust only, out of process only, one IPC (`paddock-ipc`), no web code from plugins. The platform knows how to host, isolate,
call, sign and describe a plugin; each app owns what its plugins may contribute, as an **app contract** carried in
`Extension { ns, minor, body }` (the platform routes by `ns` and never decodes `body`).

```rust
// A plugin (depends on paddock-plugin-api alone); its main is one line.
struct Hello;
impl Plugin for Hello {
    fn id(&self) -> &str { "net.example.hello" }
    fn name(&self) -> &str { "Hello" }
    fn version(&self) -> &str { env!("CARGO_PKG_VERSION") }
    fn register(&self, c: &mut Contributions) -> Result<(), RegisterError> {
        c.add_tool(MyTool).add_view(MyView).add_extension::<DebriefImport>(handle_import);
        Ok(())
    }
}
fn main() -> Result<(), IpcError> { paddock_plugin_api::serve(Arc::new(Hello)) }

// The app: discovery, grants, supervision, a typed proxy per contract.
let cfg = Arc::new(HostConfig::new(HostInfo::new("Debrief").serving(NamespaceSupport::new("debrief.import.v1", 2)), clock, quarantine, outputs));
let host = PluginHost::load(PluginsDir::of(&dirs, clock), cfg);
let plugin = host.get("net.example.hello").unwrap();
let reply = plugin.remote::<DebriefImport>()?.call(&ImportRequest::Sniff { .. })?;   // guarded: faults are counted
let answer = plugin.invoke("collector", "start", json!({ "folder": path }))?;          // schema- and grant-checked
```

- **Wire** (`paddock-plugin-api::protocol`, `PLATFORM_PROTOCOL` 2.0): `PlatformRequest { Describe, ApplySettings, Invoke,
  RenderView, ViewAction, McpTool, Stop, Extension }` and, plugin to app while it works, `HostCall { ReadFile (at most
  1 MiB), Report, TaskStarted, TaskFinished, Output, Elevated (a named operation), Log }`. Values that are JSON by nature
  cross as `Json`; no wire type uses `untagged`, `flatten`, internal tags or `skip_serializing_if` (round-trip tested).
  Protocol 1 (Debrief's own) is refused by `HostInfo::check_protocol` with `Refusal::Legacy`; the shim that would serve it
  (ADR 0003 phase 3) is the app's.
- **Compatibility** (`compat`): same major and a minor the host knows, for the platform and for every contract namespace
  (the namespace carries its major in its name). Refusals say who must update: "plugin X needs Debrief 1.4 or newer" /
  "plugin X was built for an older contract; update it". `check_described` compares what a plugin says on `Describe` with the
  verified manifest; a plugin that contributes more or less than it declared is refused.
- **Faults and failures** (`paddock-plugin-host::supervisor`): a *fault* (process ended, panic in a contribution, a call past
  its timeout and the cancel grace, an oversized or garbled frame, a reply or view document that does not decode or validate)
  is counted by `paddock_guard::Quarantine` (3 in 10 minutes) and attributed to the plugin; a *typed failure* (`Fail`, a
  contract's own error) is not. A crash is restarted on the next call after `paddock_jobs::time::Backoff` on the injected
  clock; a changed program (`verify_installed`) is quarantined at once and never started.
- **Grants** (`grants`): default deny; `sandbox_for(&Manifest, &Grants) -> SandboxSpec` is a pure table-tested mapping kept as
  data. **OS-level enforcement (AppContainer, landlock/seccomp, `sandbox-exec`) stays in the app for now**; the spec is in the
  child's bootstrap (`Bootstrap::extra`). The host enforces the memory cap, the grant-checked reads of host files, tool path
  arguments (links followed), outputs only into the app's folder, and elevated operations only by an allowed name.
- **Tests** (`paddock-plugin-test`): `conformance::assert_conforms(plugin)` is one line in a plugin's tests; `FakeHost` is a
  real IPC pair inside the test process; `golden` compares what a plugin describes and renders; `fixture` is the sample plugin
  and `paddock-plugin-fixture` / `-v1` (the frozen protocol-1 program) / `-misbehaving` (garbled, silent, dying, greedy and bare plugins) are the processes the host's tests start; the parts that end or block a process (`process.rs`) and the programs' `main`s are the only files outside the coverage gate. Time is a
  `FakeClock`; nothing sleeps.
- **Not here:** Debrief's profile/document importers, dumps, the old `PluginRequest` enum (it becomes the `debrief.*`
  contract bodies, ADR 0003 phase 2), the elevated *process* (the app starts it and runs `Invoke` with the `elevated` role),
  the OS sandbox. The author's tool is `cargo-paddock-plugin` (below).


## `paddock-repo-files`, `paddock-repo-graph` and `paddock-slots` (ADR 0022 R0)

The shared layers under `tools/paddock-dev` (`./dev`), each with a pure core and a thin adapter:

- `paddock-repo-files`: `walk(root, WalkLimits)` lists the files below a folder, sorted, `/`-separated, `.gitignore`-aware
  also outside a git repository, `.git` skipped, links not followed, bounded by `max_files` and `max_depth`;
  `hash_file`/`hash_bytes`/`hash_files`/`hash_listing` give SHA-256 hex (files streamed and capped, many files on a bounded
  number of threads in the order given). Typed `RepoFilesError`.
- `paddock-repo-graph`: `Graph` (projects by folder; `node_of` the deepest project containing a path; `dependents` the reverse
  closure), `npm_graph` (`FileSource` port), `cargo_graph`/`parse_metadata` (`CargoMetadata` port), `lock_diff` (npm
  workspaces whose resolution changed between two `package-lock.json` texts) and the `History` port (tracked files, changed
  files, a file at a revision, the log). Features, both off by default: `git-history` (`GitHistory`) and `cargo-metadata`
  (`CargoCli`), which run `git` and `cargo` through `paddock-proc`. **`cargo-metadata` is never enabled under
  `apps/pacenotes/`** (cargo reads the repository's `.cargo/config.toml`, which can name programs to run; ADR 0022 §6, and the
  a conformance check that fails on it is planned in ADR 0022 §3.7 and does not exist yet; until then review is the guard). Paddock's own rules (global inputs, doc inputs, owners) are `paddock-dev`'s `plan`
  module, not this crate's.
- `paddock-slots`: `SlotId` (`s1`...), the `SlotLocks` port and `acquire_first`/`slot_states` (pure), `acquire_retry` (a few tries when every slot looked busy, because a status probe holds a lock for an instant), tokens (`fresh_token`; `release` presents the holder's token and a holder honours only its own, so a late release never frees the next job's slot), `FsLocks` over
  `<root>/slots/sN.lock` (`File::try_lock`: `flock`, `LockFileEx`), and the holder protocol `hold` (take a slot, announce it
  on a writer, keep it until the owner folder is gone or `release` asks), `release`, `wait_free`. The lock file is never read
  or written (Windows locks block other handles); `sN.owner` and `sN.release` are information and a request. Reading
  `slot_states` tries each lock for an instant, so a status call can make a concurrent acquirer skip a slot once.
  Used by `./dev ci slot acquire|release|status` (see `tools/paddock-dev/src/slots.rs` for the detached-holder design).
