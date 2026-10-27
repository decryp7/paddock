# Pitcrew in one page

Pitcrew supervises CI runners (Gitea's `gitea-runner`, formerly act_runner; Forgejo later) on Windows and macOS:
keeps them alive (stall watchdog, restart backoff, crash-loop protection), updates them between jobs (download →
two-source SHA-256 → drain → graceful stop → swap → confirm → automatic roll-back), cleans their folders, and shows
everything in a tray app.

- `crates/pitcrew-core` — pure rules, no I/O, time passed in: `controller` (a runner's life: start/stop/restart/drain,
  backoff, stalls, hold for updates), `jobs` (current job from "task started" + child processes, 20 s idle grace),
  `update::{policy,machine}`, `housekeeping` (what/when to clean), `history`, `metrics`, `notify`, `session`,
  `config` (pitcrew.toml, versioned, damage tolerant), `state` (state.json), `protocol` (app ↔ service frames),
  `gitea_api`, `redact`, `layout` (paths and command lines).
- `crates/pitcrew-service` — the service: one supervisor thread (`supervisor.rs`) owning all runner state, worker
  threads for slow work (`work.rs`, `cleanup.rs`), launch adapters (`launch.rs`, `launcher.rs`,
  `platform/win_session.rs` = the only Win32 FFI), IPC (`ipc.rs`: named pipe with ACL / 0600 Unix socket),
  `host.rs` (run mode), SCM (`platform/windows.rs`), launchd (`platform/macos.rs`).
- `app/src-tauri` — the tray app: `service_client.rs`, `events.rs` (one subscription → UI events, tray, native
  notifications), `commands.rs`, `tray.rs`, `credentials.rs` (OS credential store), `gitea.rs`, `service_install.rs`
  (UAC / SMAppService), `updater.rs`, `diagnostics.rs`.
- `app/src` — Angular UI over the `BACKEND` token (Tauri backend or mock); state in SignalStores.

Never: kill a running job, read/log/store `.runner` or tokens, trust the webview or the app from the service.
