# Shared Rust crates

A Cargo workspace for Rust code the apps share. Members: `paddock-jobs` (ADR 0008) and the app shell crates of ADR 0012
(`paddock-commands`, `-dialogs`, `-download`, `-health`, `-menu`, `-settings`, `-startup`, `-updater`, `-window`; scaffolds
until their work packages in [docs/plans/workbench.md](../docs/plans/workbench.md) land). Crates are extracted from the apps one
at a time (a crate moves here when a second app needs it, rule of three for new abstractions, two for copied code).

Rules for a crate here:

- Pure logic and thin adapters, no dependency on any app crate. Edition and licence come from `[workspace.package]`.
- Library errors are typed, no `unwrap`/`expect` on user data or I/O, `cargo clippy -- -D warnings` clean,
  >= 99% line coverage.
- A pure core with no Tauri dependency, and a `tauri` feature for the adapter (`src/tauri.rs`, `pub mod tauri` behind
  `#[cfg(feature = "tauri")]`). Tauri comes from `[workspace.dependencies]` without default features (the app enables
  the runtime); CI builds every crate with `--all-features` on Linux, Windows and macOS.
- A crate that uses another one writes `paddock-jobs = { workspace = true }` (the workspace dependency carries path,
  version and registry, so the published crate points at the registry).
- `publish = ["decryp7"]` in every crate. A new crate starts with `[package.metadata.paddock] scaffold = true`, which keeps
  the publish job from uploading it; its work package removes the marker. A new member also needs its step in the `crate`
  job of `.gitea/workflows/ci.yml` (`scripts/publish.test.mjs` fails without it).
- Published to the Gitea cargo registry by the tag workflow only (`.gitea/workflows/publish.yml`, rule 9). One repo tag
  `vX.Y.Z` releases everything; each crate bumps its own `version` only when it changes, and a version the registry
  already has is skipped. `node scripts/publish.mjs` is the local dry run (lists every crate and npm package with what a
  tag would publish).

Apps consume the crates from the registry. In the app's `.cargo/config.toml`:

```toml
[registries.decryp7]
index = "sparse+https://dev.decryptology.net/api/packages/decryp7/cargo/"
```

and in its `Cargo.toml` (pre-1.0: `0.N` accepts patches of that minor only), with `Cargo.lock` committed:

```toml
paddock-jobs = { version = "0.1", registry = "decryp7", features = ["tauri"] }
```

The app's `deny.toml` allows the registry: `allow-registry = ["https://github.com/rust-lang/crates.io-index",
"sparse+https://dev.decryptology.net/api/packages/decryp7/cargo/"]`.

Crates, built and planned, with their designs in [`docs/adr/`](../docs/adr/README.md):

| Crate(s) | Purpose | Three users today? |
|---|---|---|
| `paddock-jobs` | long operations: job/phase/progress model, throttled reporter, cancel token, hash-while-writing, parallel walk/map/pipeline, job pool (ADR 0008) | yes (all five apps); built |
| `paddock-commands`, `-health`, `-dialogs`, `-startup`, `-download`, `-settings`, `-updater`, `-window`, `-menu`, `-shell` | the app shell (ADR 0012) | yes; `paddock-commands` (S1) and `paddock-startup` (S7) built, the rest scaffolds, plan S2-S17 |
| `paddock-pitwall` | Pitwall client: discovery, pairing, keychain keys, typed requests, optional-AI state (ADR 0004) | yes (Debrief, Pacenotes, Sighter; Spotter, Dyno later); built first |
| `paddock-ipc`, `-guard`, `-view`, `-plugin-api`, `-plugin-host`, `-plugin-package`, `-plugin-test` | the out-of-process plugin platform (ADRs 0001 to 0003) | yes (Debrief, Pacenotes, Dyno tier 3) |
| `paddock-mcp` | MCP protocol, tool schemas, limits, paging, audit log, local transports (ADR 0007) | three (Pacenotes, Sighter, Debrief); extracted with Debrief's server, after Debrief 1.0.2 |
| `paddock-testplan` | `.sighterplan` / `.sighterresult` (ADR 0005) | two (Pacenotes, Sighter); shared schema, so extracted on purpose |
| `paddock-env` | per-OS data/config dirs, portable mode | yes |
| `paddock-service` | background service install/control per OS | yes |
| `paddock-proc` | child processes with timeouts, caps, kill-tree | yes |
| `paddock-window` | native window feel, tray, single instance | yes |
| `paddock-notify` | OS notifications | borderline (two to three) |
| `chromium-driver` | CDP driver (today in Debrief) | two to three (Debrief, Spotter, Sighter) |
| `paddock-cli` | shared CLI conventions (`--json`, exit codes, timeouts) | yes |

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

Entries are sorted by name; an empty table is `{} as const` (name type `never`). The shell's files live in
`ui/projects/paddock-ui/shell/generated/` (not yet re-exported; the `shell` entry point decides that):

| File | Content | Written by |
|---|---|---|
| `commands.ts` | the wire types (ts-rs): `CommandKind`, `WindowClass`, `CommandInfo`, `RefusalCode`, `CommandRefusal` | `paddock-commands` (`PADDOCK_WRITE_TS=1 cargo test -p paddock-commands --lib generated`) |
| `shell-commands.ts` | `SHELL_COMMANDS` and `ShellCommand`, the shell plugin's command names | `paddock-commands` with an empty table until `paddock-shell` (S17) takes it over with its own table and test |

An app writes its own file the same way into its UI (for example `src/app/core/generated/app-commands.ts`); dialog
contracts and the contributions core type command names as `AppCommand | ShellCommand`.

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

To add a crate: create `crates/<name>` (as one of the scaffolds), add it to `members` and `[workspace.dependencies]` in
`Cargo.toml`, and add its step to the `crate` job in `.gitea/workflows/ci.yml`.
