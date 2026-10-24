# Plan: what the apps share next

- Status: plan (2026-10-24). The user decided to build **every** candidate below; each has a work package, a recommended
  option for every open question (§6), a place in the order (§4) and an app that adopts it (§5).
- Question: "check what else all apps should share". The shell (ADR 0012, `paddock-*` 0.2.0) and the workbench (ADR 0013)
  cover the frame, start-up, notifications and health, dialogs, jobs, downloads, settings, updates, menu, tray, single
  instance and deep links. This plan covers what is still copied or diverging **below and beside** the shell.
- Sources, read-only: `origin/main` of Debrief `64a17847`, Pitwall `fd87a01`, Pitcrew `8676bd6`, Spotter `b670246`,
  Dyno `bfbacbb`, Pacenotes `9feec3a`, Sighter `4f83f04` and its `poc/*` branches, Paddock `957dfd7`.
  "Diff N" means `diff` reported N changed lines between two copies.
- Rules: 39 (shared code lives in Paddock, not copied), 40 (a recurring class gets one mechanism), and the crate rules in
  [crates/README.md](../../crates/README.md) (pure core, `tauri` feature, at least 99% lines, typed errors, `native.rs` for
  native glue only).

## 1. Summary

| Id | Candidate | Home | Who has it today | Priority | Effort (agent-days) | Release |
|---|---|---|---|---|---|---|
| X1 | App and service log: format, rotation, folder, level, panic hook | new `paddock-log` | 5 different (Debrief CLEF; Pitcrew, Spotter, Dyno small text files; Pitwall none in the app) | now | 3 | 0.3.0 |
| X2 | Report a problem: diagnostics zip, redaction, preview | new `paddock-diag` + `shell/diagnostics` | Debrief, Pitcrew, Dyno (3 designs); Pitwall, Spotter none | now | 3 | 0.3.0 |
| X3 | Unclean shutdown, safe mode, UI session checkpoints | extend `paddock-startup` | Debrief only | now | 2 | 0.3.0 |
| X4 | Background service registration and repair, login items, login-shell `PATH`, restart after update | new `paddock-service` | Pitwall, Pitcrew (copy, diff 16 and 552), Dyno, Debrief agent (4 designs) | **now** (R03 reached users) | 5 | 0.3.0 |
| X5 | Credential store | new `paddock-secrets` | Debrief, Pitwall, Pitcrew, Spotter, Sighter PoC (5 copies) | now (`paddock-pitwall` needs it) | 1 | 0.3.0 |
| X6 | Child processes: no console, timeouts, caps, kill tree, process memory (PSS) | new `paddock-proc` | 7 helpers in 5 apps; PSS in Spotter only | now | 3 | 0.3.0 |
| X7 | Folders per OS and versioned state files (atomic, backup, migrations) | new `paddock-env` | 5 layouts; Windows Roaming vs Local differs | now | 2.5 | 0.3.0 |
| X8 | OS notifications: policy, rate limit, quiet hours, buttons | extend `paddock-health` | Debrief, Spotter, Pitcrew, Dyno (4 adapters) | now | 1 | 0.3.0 |
| X9 | Release scripts and the first conformance checks | new `@decryp7/paddock-conformance` (conformance proposal) | copies in 2 to 6 apps, 5 versions of `latest-json.mjs` | **now** | 5 | 0.3.0 |
| X10 | Shared CI steps and workflows | `paddock/.gitea/actions/*` + reusable workflows | 5 `build.yml` of 421 to 1,174 lines, none shared | now | 2.5 | 0.3.0 |
| X11 | Lint presets: ESLint (rules 27 to 39), clippy and workspace lints | new `@decryp7/paddock-eslint`, `paddock/lint/` | Debrief's is richer; four near-identical 27-line copies | now | 1.5 | 0.3.0 |
| X12 | Small UI parts: path text with middle ellipsis, empty state | `@decryp7/paddock-ui` root | Debrief only (rule 18) | now | 0.5 | 0.3.0 |
| X13 | IPC to an app's own companion process (agent, service, worker) and its discovery file | `paddock-ipc` (ADR 0001) + `link` module | Debrief, Dyno, Pitcrew (3 transports) | with the next app move | 4 | 0.4.0 |
| X14 | Chromium driver | new `paddock-chromium` | Debrief `chromium-driver`, Spotter `spotter-render` (diverged) | with Spotter's move | 3 | 0.4.0 |
| X15 | Knowledge packs (TOML, limits, hot reload) and user regexes | new `paddock-packs` | Spotter, Pitwall, Dyno (3 loaders); 2 `user_regex` copies | with the next app move | 2.5 | 0.4.0 |
| X16 | Outbound HTTP policy: https only, caps, redirects, private-address guard, offline | new `paddock-net` (shares `paddock-download`'s transport) | Pitwall, Spotter, Pitcrew, Debrief (4 clients on 2 HTTP stacks) | with the next app move | 3 | 0.4.0 |
| X17 | Outbound notification channels (webhook, ntfy, e-mail) | new `paddock-channels` | Pitcrew `notify-channels` ("meant to be copied"), Spotter `mail.rs` | with Spotter/Pitcrew moves | 1.5 | 0.4.0 |
| X18 | Identifier search (rule 20) | new `paddock-names` | Debrief `names/`, Dyno counter search; others none | later (Pacenotes) | 2 | 0.4.0 |
| X19 | First run: a set-up checklist (service, start at login, notifications, Pitwall) | `shell/setup` + `paddock-shell` | Pitwall service consent; Debrief tour; others none | later | 2 | 0.4.0 |
| X20 | Clock port, fake clock, restart back-off | `paddock-jobs::time` | 4 `time.rs`, 3 `clock.rs`, 4 back-off loops | later | 1 | 0.4.0 |

Total: about 31 agent-days for 0.3.0 (X1 to X12) and 19 for 0.4.0 (X13 to X20). The critical path for 0.3.0 is
X7 → X1 → X2, about 9 days with four agents (§4).

Already planned elsewhere and not repeated here: `paddock-pitwall` (ADR 0004, plan §15), `paddock-download` adoption
(plan §16), the plugin platform `paddock-plugin-*` (ADR 0001; X13 is its first step), `paddock-mcp` (ADR 0007, see §7),
`paddock-testplan` (ADR 0005). Left per app on purpose: §7.

## 2. Findings by area

### 2.1 Logging (X1)

| App | Implementation | Format and limits | Folder |
|---|---|---|---|
| Debrief | `app/src-tauri/src/logging.rs` (CLEF layer over `tracing`, `tracing-appender` daily files), `activity/logger.rs` (breadcrumbs, error dedup), `clef.rs` | CLEF JSON lines, `keep_files` days, `max_file_mb` per day then errors only, level settable at run time, IPC spans carry `Window`/`Command` | `dirs::log` (`app_log_dir()`) |
| Pitcrew | `app/src-tauri/src/applog.rs`; `crates/pitcrew-service/src/logfile.rs` (runner output, rotates between lines) | plain text, one line per event, 512 KiB | `~/Library/Logs`, `%LOCALAPPDATA%` |
| Spotter | `app/src-tauri/src/applog.rs` (near copy of Pitcrew's) | plain text, 512 KiB | `$XDG_STATE_HOME`, `%LOCALAPPDATA%`, `~/Library/Logs` |
| Dyno | `app/src-tauri/src/logging.rs` | plain text, 256 KiB, one older generation | `<data>/logs/app.log` |
| Pitwall | no app log; the server's stdout goes to `<data>/logs/service.log` (`crates/pitwall-server/src/service/spec.rs:150`), `%ProgramData%\Pitwall\logs` for the Windows service | whatever the server prints, no rotation in the app | data folder |

Only Debrief installs a panic hook (`lifecycle.rs`); in the other four a panic in the app goes to stderr, which is lost
when the app starts from Finder or at log-in (their own `applog.rs` says so). Bug classes: a support case cannot be
answered without asking the user to run from a terminal; five log folders to explain; logs that cannot be read by
Debrief itself (only Debrief's are CLEF). Debrief's privacy rule "never file contents" (`activity/privacy.rs`) exists as
prose in the other four.

**Recommendation:** `paddock-log` from Debrief's `logging.rs` + `clef.rs` + the generic half of `activity/logger.rs`:
CLEF everywhere (§6 D1), one file per process per day, per-OS folder from X7, limits from a shell settings section,
`tracing` and `log` bridged, a panic hook that writes the panic (with the thread name and the last breadcrumbs) to the
log and a crash marker (X3), and a `Redact` helper for fields (X2). Services and sidecars use the same crate without the
`tauri` feature.

### 2.2 Report a problem (X2)

- Debrief `app/src-tauri/src/activity/report.rs` (1,028 lines) and `report/{manifest,children,recovery,sample,open,cli}.rs`:
  a zip the user reviews and saves, nothing sent; logs, children's logs with `processes.json`, error snapshots with
  breadcrumbs, steps to reproduce, system facts, crash marker, recoverable sessions, a versioned `manifest.json`; UI
  `app/src/app/diagnostics/report-problem-dialog.ts`.
- Dyno `crates/dyno-core/src/diagnostics.rs` (731 lines, pure) + `crates/dyno-agent/src/diagnostics.rs`: removes user
  names, home folders, machine name, e-mail and IP addresses, and **previews every file with its size** before writing.
- Pitcrew `app/src-tauri/src/diagnostics.rs` (60 lines) over the service's redacted sections, with
  `crates/pitcrew-core/src/redact.rs` (secrets: tokens, `Authorization`, long hex).
- Pitwall: `crates/pitwall-core/src/redact.rs` redacts prompts for the inspector, no bundle. Spotter: nothing.

Three redactors with three different ideas of "private" (secrets; personal data; content). **Recommendation:**
`paddock-diag` (pure: manifest, sections, the union of the three redactors as one tested `Redactor`, preview) plus a
`tauri` adapter (collect, write the zip as a job, reveal) and a `shell/diagnostics` dialog (Help ▸ Report a Problem),
with sections as app contributions (Pitcrew adds runner logs, Dyno its agent, Pitwall its engine logs). §6 D9.

### 2.3 Crash, safe mode, session restore (X3)

Debrief only: `app/src-tauri/src/lifecycle.rs` (1,592 lines: safe mode, unclean-shutdown detection, persisted plugin
enable/disable, session autosave/restore format 2, crash handler) and `app/src/app/lifecycle/`. `paddock-startup` already
offers "Restart in Safe Mode" but leaves what safe mode **is** to the app (`Startup::builder().safe_mode(hook)`). The other
apps have no unclean-shutdown detection (AGENTS.md "Recoverable sessions" is a family rule in prose only).
**Recommendation:** extend `paddock-startup` with a running marker (written at start, removed on a clean exit by the
shell's exit path), `--safe-mode` handling (flag in the launch plan of `paddock-window::launch`, which settings sections
and plugins read), and a generic `Checkpoint<T>` store for the UI checkpoint it already sends to reloaded pages, so an
app only declares what goes into its session.

### 2.4 Background services, login items, login `PATH` (X4)

| App | Registration | Repair (rule 24) | Notes |
|---|---|---|---|
| Pitwall | `app/src-tauri/src/service_install.rs` (743, "Adapted from Pitcrew's"), `service_repair.rs` (719); server side `crates/pitwall-server/src/service/{mod,spec,systemd,windows,exec,watch,login_path}.rs` | yes, stored registered version | Windows SCM service, LocalSystem, starts the server in the user's session |
| Pitcrew | `app/src-tauri/src/service_install.rs` (669), `service_repair.rs` (719, diff 16 from Pitwall's); `crates/pitcrew-service/src/platform/{macos,linux,windows,win_session}.rs` | yes | self-restart when its exe changed: `crates/pitcrew-core/src/self_update.rs` (only here) |
| Dyno | `crates/dyno-agent/src/service/{mod,spec}.rs` (launch agent, systemd user unit, Windows `Run` entry), `app/src-tauri/src/login.rs` (autostart + agent), `crates/dyno-helper/src/install.rs` (privileged helper as a typed plan) | `launchctl bootout` present, no stored version | per-user, never session 0 |
| Debrief | `crates/debrief-agent/src/service/{launchd,systemd,windows}.rs` | none | admin-installed system daemon (`print-plist`, `sudo tee`) |
| Spotter | none (scheduler runs in the app); `tauri-plugin-autostart` | n/a | |

Login-shell `PATH` has three copies: `crates/pitwall-server/src/service/login_path.rs` (177),
`crates/pitcrew-service/src/login_env.rs` (187), `crates/dyno-core/src/toolpath.rs` (151, plus per-tool override folders).
`tauri-plugin-autostart` is wired separately in Pitwall, Pitcrew, Spotter and Dyno.
Bug classes: catalogue R03 (EX_CONFIG loop after an update; reached users in released Pitcrew and Pitwall builds, fixed by
copying), R04, R05; lessons "macOS service registration needs visible repair", "Service after an update". Dyno and
Debrief's agent do not have the rule-24 sequence at all, so they will meet R03 at their first ad-hoc update.
**Recommendation:** `paddock-service` with a pure `ServiceSpec` (label, program, args, kind: per-user agent or system
service, §6 D4) that renders the launchd plist, systemd unit and Windows service or `Run` entry; the `RepairOps` state
machine from `service_repair.rs` (once); `registered_version` storage and the unregister → bootout → wait → register
sequence; an exe-changed watcher for the service side; `login_path` (moved to X6, used here); a `launch_at_login` setting
section (app autostart and agent together, as Dyno's `login.rs` does); a `ServiceSpec` health entry for `paddock-health`
with a Repair that never depends on the service (rule 19). All OS calls behind a `Runner` port so tests never touch
launchd, systemd or the registry (rule 7, Dyno's pattern).

### 2.5 Credential store (X5)

`keyring = "4.2"` in Debrief (`app/src-tauri/src/ai/pitwall.rs`, service `Debrief`), Pitwall
(`app/src-tauri/src/credentials.rs`, 222 lines, file fallback when no Secret Service or a locked keychain; also
`crates/pitwall-cli/src/discover.rs`), Pitcrew (`credentials.rs` 63, `channels.rs` with service "Pitcrew notification
channel"), Spotter (`credentials.rs` 80 + the port `crates/spotter-engine/src/secrets.rs` with `MemorySecrets`), Sighter
PoC (`poc/app/core/src/credentials.rs`). The same `map_err("the credential store is not available")` text in three apps.
Only Spotter tests through a port; the others call `keyring` directly, so a test can reach the real keychain (rule 7).
**Recommendation:** `paddock-secrets`: a `Secrets` port, the keyring adapter, `MemorySecrets` for tests, a `Secret`
newtype that never prints (redacted `Debug`, zeroized on drop), the fallback rule (owner-only file only when the app
opts in, as Pitwall's admin key), and one naming scheme (§6 D6). `paddock-pitwall` depends on it.

### 2.6 Child processes and process memory (X6)

Every app forbids `std::process::Command::new` in `clippy.toml`, each pointing at its own helper:
`debrief_core::process::quiet_command` and `crates/debrief-ipc/src/process.rs` (717: job object, stdin lifeline, cleared
environment, below-normal priority), `crates/chromium-driver/src/process.rs`, `crates/pitwall-server/src/process.rs`
(64), `crates/pitcrew-service/src/process.rs` (146) + `terminate.rs` (one pid vs. the group), `crates/spotter-render/src/process.rs`
(364: kill tree on drop, timeout, memory cap), `crates/dyno-platform/src/process.rs` (161). Process memory: PSS from
`smaps_rollup` only in Spotter; threads skipped in Pitcrew (`probe.rs`), Pitwall (`system.rs`), Dyno (`processes.rs`),
Debrief `debrief-monitor`, but Debrief's child memory cap (`debrief-ipc` `process.rs` + `memory_watch.rs`) uses RSS.
Bug classes: R06 (console windows, RDP session), R07 (17 GB idle Chrome, rule 26), T03 (processes left behind).
**Recommendation:** `paddock-proc`: `command()` (no console, cleared env + allow-list, priority), `run_bounded` (timeout,
output cap, kill tree), `Supervised` child (job object on Windows, process group on Unix, kill on drop, optional memory
cap with `MemoryWatch`), `terminate(pid)` with grace, `process_memory(pid)` (PSS on Linux, threads skipped), `login_path`
(from the three copies). The shared `clippy.toml` (X11) points at it. `paddock-ipc` (X13) and `paddock-chromium` (X14)
build on it.

### 2.7 Folders and versioned state (X7)

Windows data folder: Pitwall `%APPDATA%` (Roaming) for data and `discovery.json`; Dyno, Spotter, Pitcrew `%LOCALAPPDATA%`;
Debrief uses both through `dirs::` (`app_dirs.rs`, which also has a dev override `DEBRIEF_DATA_DIR`). Logs: four places
(2.1). Atomic writes: Pitwall `crates/pitwall-server/src/paths.rs` (temp + rename + backup), Pitcrew
`crates/pitcrew-service/src/store.rs`, Debrief `config_store.rs`/`settings.rs`, Spotter `spotter-engine` (`NamedTempFile`
in `avatar.rs`), Dyno helper, `paddock-health`'s notification file; each with its own rule for "missing, damaged or newer
version". Bug classes: Roaming profiles copying multi-GB models (Pitwall); a newer file read by an older build silently
reset (AGENTS.md "versioned, tolerate missing/corrupt"); tests writing to the real data folder (T01) when an app has no
override variable.
**Recommendation:** `paddock-env`: `AppDirs::for_app(id)` (data, config, cache, logs, runtime, per OS, §6 D3), one
override variable `<APP>_HOME` for tests and debug builds (refused in release builds), portable mode; `VersionedFile<T>`
(atomic write with fsync and `.bak`, `version` field, missing → default, damaged → default + logged + kept as
`.corrupt-<time>`, newer → read-only with a notice, a migration chain `fn(vN) -> vN+1`). Settings, notifications and
every app file use it.

### 2.8 OS notifications (X8)

Debrief `app/src-tauri/src/notify.rs` (policy: notify only when the user cannot see it; rate limiter), Spotter
`app/src-tauri/src/toast.rs` (buttons and click-through per OS), Pitcrew `events.rs`, Dyno `shell.rs` + the sustain and
cool-down rules in `crates/dyno-core/src/alerts.rs`; Pitwall none. `paddock-health::tauri::Setup::os_notifications` takes
an app closure, so each app would still write its adapter. **Recommendation:** a default adapter in `paddock-health`
(feature `os-notify`): focus-aware policy, rate limit per cause key, quiet hours (Spotter's `ClockTime`), buttons mapped to
notification actions, click opens the cause's view; plus the permission request in X19.

### 2.9 Release tooling and CI (X9, X10)

| File | Debrief | Pitwall | Pitcrew | Spotter | Dyno | Pacenotes |
|---|---|---|---|---|---|---|
| `scripts/release/publish.mjs` | same | same | same | same | same | same |
| `scripts/release/latest-json.mjs` | 5 different versions, 118 lines each | | | | | none |
| `scripts/commit-lint.mjs` | 67 (own) | 59 | 59 | 59 | 59 | none |
| `check-index.mjs` | 70 | 22 | 22 | 22 | 22 (other) | |
| `check-icons.mjs` | same | same | none | same | none | |
| `macos-verify-bundle.sh` | 77 | 31 | none | none | `check-bundle.sh` | |
| key strip before bundling | in `build.yml` | `scripts/ci/tauri-bundle.sh` | none | in `build.yml` | in `build.yml` | |
| `export-diagrams.sh` / `diagrams/src/common.mjs` | same / 122 | same / 27 | same / 22 | same / 21 | other / 62 | same / none |
| `.gitea/workflows/build.yml` lines | 1,174 | 421 + `release.yml` | 476 | 575 | 603 | |

App-only guards that every app needs: Debrief `check-temp-leaks.mjs`, `scan-fixtures.mjs`, `check-quarantine.mjs`,
`startup-bench.mjs`, `macos-smoke.sh`; Spotter `pin-chromium.mjs`, `install-test-browser.mjs` (Debrief has another);
Dyno `check-capabilities.mjs`; Pitwall `privacy-check.sh`, `ui-coverage-guard.mjs`. No app uses a shared workflow or
action. Bug classes: B03, B04, B08, B09, B11, B13 to B16 (catalogue) and the lessons "Release checklist"; each is fixed in
one app and missing in the next. **Recommendation:** build the conformance package's first two rollout steps
([conformance-proposal.md](../conformance-proposal.md): C03, C04, C06, C07, C08, C14, C15, C16, plus the release scripts
under `conformance/release/`, used unmodified) as X9; X10 adds composite actions (`setup-rust`, `setup-node`,
`strip-signing-keys`, `tauri-bundle`, `macos-verify`, `publish-release`, `conformance`) and one reusable
`app-release.yml`, so an app's workflow is its own test jobs plus a few `uses:` lines (§6 D7).

### 2.10 Lint presets (X11)

Every app has `app/eslint.config.js`; Debrief's (83 lines) adds `@ngrx/signals` rules, the other four are near-identical
27-line copies with different hashes. Debrief alone has `app/scripts/lint-effects.mjs` (rule 28) and `lint-styles.mjs`.
`clippy.toml`: five different `disallowed-methods` lists (Pitwall also forbids `reqwest::Client::new`); `[workspace.lints]`
(`unsafe_code`, `missing_docs`) in four, not Debrief (catalogue S02). Rules 27 to 39 have no check outside Debrief
(catalogue A01 to A06). **Recommendation:** `@decryp7/paddock-eslint` (flat config preset with rules 27 to 39, Debrief's
two scripts as ESLint rules) and `paddock/lint/{clippy.toml,lints.toml}` that apps extend (C09, C13).

### 2.11 Small UI parts (X12) and e2e helpers

Middle ellipsis for paths (rule 18) exists only in Debrief (`app/src/app/shared/format/middle-ellipsis.ts`,
`shared/path-text.ts`). `withRequestStatus` (rule 30) is in Debrief and in `paddock-ui/shell/store`; no other app uses
either. Only Spotter depends on `@decryp7/paddock-ui` (`^0.1.0`), none on `@decryp7/paddock-e2e`; Debrief still has
`app/e2e/{app,axe,mock-url,menu-support}.ts` and `retried-reporter.mjs`, Pitwall `app/e2e/app.ts`, which the e2e package
replaces. That is adoption, not new code (§5).

### 2.12 IPC to an app's own processes (X13)

Three transports for the same job (a private endpoint, a hello with a per-launch token, bounded frames):
`crates/debrief-ipc` (postcard frames, peer with credits and cancel, endpoint, link, launch, sandbox, elevation),
`crates/dyno-ipc` (postcard frames, peer, endpoint, token, job, queue; app ↔ agent and agent ↔ plugins), Pitcrew
`crates/pitcrew-service/src/ipc.rs` (named pipe with an SDDL, Unix socket 0600, `pitcrew_core::protocol` frames). Discovery
files: Dyno `agent.json` (`crates/dyno-agent/src/discovery.rs`, owner-only, token inside), Pitwall `discovery.json`
(`crates/pitwall-core/src/discovery.rs`, no secret, HTTP). The app-side client with reconnect, back-off and "replace an
agent of another build" exists twice: Dyno `app/src-tauri/src/link.rs` (843), Pitcrew `service_client.rs`. ADR 0001
already moves `debrief-ipc` into `paddock-ipc` for plugins; this plan makes the companion link part of the same step
and moves Pitcrew and Dyno onto it (§6 D5).

### 2.13 Chromium driver (X14)

Debrief `crates/chromium-driver` and Spotter `crates/spotter-render` (both say they are meant to be shared) have the same
modules and have diverged: `launch.rs` diff 643, `process.rs` 92 vs 364 lines (Spotter's has the memory cap, kill tree and
PSS), `install.rs` 254 vs 403 (Spotter's has a manifest `chromium-manifest.toml` and progress), `discover.rs` 184 vs 290.
Debrief's `install.rs` re-reads the file to hash it (audit 2026-10-23 row 5, rule 15). Sighter's PoCs drive VMs over VNC,
not CDP, but its plan names the browser for web targets. Bug classes: T02 (installed Chrome, 441 GB of clones), R07.
**Recommendation:** `paddock-chromium` from Spotter's driver modules plus Debrief's `cue.rs` and `fake.rs`, with install
through `paddock-download` and processes through `paddock-proc`.

### 2.14 Knowledge packs and user regexes (X15)

Spotter `crates/spotter-core/src/pack.rs` + `packs/*.toml`, Pitwall `crates/pitwall-core/src/packs.rs` (unknown keys warn,
a bad pack is skipped), Dyno `crates/dyno-core/src/cleanup/pack.rs` (a bad pack is refused whole). Three different
answers to "what happens with a bad pack", and none of the three pack modules watches the `packs/` folder, so rule 21's
"reloaded without a restart" holds only where an app re-reads on its own schedule. `user_regex.rs` exists twice
(`crates/debrief-plugin-api/src/user_regex.rs`, `crates/spotter-core/src/user_regex.rs`, diff 66). **Recommendation:**
`paddock-packs`: built-in + user packs, size and count limits, one rule for errors (a pack is refused whole with a named
reason, unknown keys are warnings), a debounced folder watcher that reloads and swaps (build new, then swap), a `packs`
health/notice entry, and `user_regex` as a module.

### 2.15 Outbound HTTP (X16) and notification channels (X17)

Two HTTP stacks: `ureq` (Debrief, Pitcrew, `paddock-download`) and `reqwest` (Pitwall, Spotter). Four policies:
Pitwall `crates/pitwall-server/src/egress.rs` (only client, https, allow-listed hosts, every redirect checked, offline
mode), Spotter `crates/spotter-engine/src/fetch.rs` + `guard.rs` (per-host gap, ETag, caps, private-address guard on every
hop), Pitcrew `crates/pitcrew-service/src/net.rs` (https, caps, OS certificates), Debrief `crates/debrief-ai/src/web/`
and `browser/policy.rs`. **Recommendation:** `paddock-net` on `paddock-download`'s `ureq` + rustls transport (§6 D8):
`Egress` with https only, timeouts, body caps, redirect cap with the guard per hop, optional host allow-list, offline
switch, per-host politeness and conditional requests; clippy forbids other clients.
Channels: Pitcrew `crates/notify-channels` (webhook, ntfy, SMTP; its doc says "meant to be copied into an app") and
Spotter `crates/spotter-engine/src/{mail,channels}.rs` (lettre, password in the credential store). **Recommendation:**
`paddock-channels` from `notify-channels` with Spotter's SMTP, secrets through X5, HTTP through X16; Dyno's alerts and
Debrief's live-tail alerts can use it later.

### 2.16 Identifier search (X18)

Rule 20 is implemented in Debrief only (`crates/debrief-core/src/names/` segmentation, normalisation, grouping;
`find/`), with a smaller matcher in Dyno (`crates/dyno-core/src/counters/search.rs`) and a TypeScript `fuzzy.ts` in
Debrief's palette (which ADR 0013 moves to the workbench). Pitwall model search, Spotter product search and Pacenotes'
symbol search need the same. **Recommendation:** `paddock-names` with the pure parts of `names/` (no entity catalogue,
which stays in Debrief), exposed to the palette through an engine command (rule: Rust is the source of truth).

### 2.17 First run (X19)

Pitwall asks for the service (`app/src/app/features/service/service-consent.ts`), Debrief has a tour and welcome page,
Pitcrew, Spotter and Dyno start on an empty page. Each app that has a service, notifications or optional Pitwall asks
the same things in different places. **Recommendation:** a `shell/setup` checklist page and a `setup_state` command:
items contributed by the app (service registration from X4, start at login, notification permission from X8, Pitwall
pairing from `paddock-pitwall`), each with its own done check, reopened from Help ▸ Set-up.

### 2.18 Clock and back-off (X20)

Time as values (`time.rs`) in Pitwall (85), Pitcrew (150), Spotter (172), Dyno (65); clock ports in
`spotter-engine/src/clock.rs`, `dyno-plugin-host/src/clock.rs`, `debrief-agent/src/clock.rs`; restart back-off in
`pitwall-core/src/backoff.rs`, `dyno-plugin-host/src/supervision.rs`, `debrief-ipc/src/supervisor.rs`,
`pitcrew-core/src/restart.rs`. The post-v1 test architecture asks for a fake clock. **Recommendation:** a `time` module in
`paddock-jobs` (which every app already depends on): `Clock` port, `SystemClock`, `FakeClock`, `Backoff` (exponential,
reset after healthy time, give up after N in a window). Domain formatting stays in each app.

## 3. Work packages

One agent each, sizes in agent-days including tests (±50%). Every package: pure core with a `tauri` feature where
needed, at least 99% lines, `cargo check --target x86_64-pc-windows-msvc`, Rust tests on Linux, macOS and Windows CI,
`[package.metadata.paddock] scaffold = true` until done, its step in `.gitea/workflows/ci.yml`, generated TypeScript in
its entry point, a `crates/README.md` section, and the shell-demo shows it where it has UI (rule 3: affected tests only
in the branch). "Owned" is in the Paddock repo.

| WP | Scope | Owned | Depends | Acceptance | Effort |
|---|---|---|---|---|---|
| **X7** `paddock-env` | `AppDirs` per OS (data, config, cache, logs, runtime), `<APP>_HOME` override (debug and tests only), portable mode; `VersionedFile<T>` (atomic + fsync + `.bak`, missing/damaged/newer rules, migration chain) | `crates/paddock-env/` | none | table tests of every folder on every OS from injected env; a damaged file is kept as `.corrupt-*` and defaults load; a newer version is read-only with a notice; a migration chain v1→v3 runs once; kill during write leaves the old file (fault-injected writer); a release build ignores `<APP>_HOME` | 2.5 |
| **X6** `paddock-proc` | `command`, `run_bounded`, `Supervised` (job object / process group, kill on drop, memory cap), `terminate`, `process_memory` (PSS, threads skipped), `login_path` | `crates/paddock-proc/` (ported from `debrief-ipc/process.rs`, `spotter-render/process.rs`, `pitwall-server/.../login_path.rs`) | none | a child and its grandchild are gone after drop, timeout and cap (on all three OSes); no console window flag on Windows; output cap holds with an endless writer; PSS read from a fixture `smaps_rollup`; thread entries are skipped (fixture); `login_path` with a shell that hangs, prints junk, or is missing falls back; no process left after the test run (C14 census) | 3 |
| **X5** `paddock-secrets` | `Secrets` port, keyring adapter, `MemorySecrets`, `Secret` newtype, opt-in owner-only file fallback, naming scheme and one-time rename of old entries | `crates/paddock-secrets/` | none | `Debug`/`Display` of a `Secret` never shows it (test); the keyring adapter is never called in tests (a test fails if `keyring` is reached without `PADDOCK_SECRETS_REAL=1`); fallback only when opted in and the file is 0600 | 1 |
| **X8** OS notifications | `paddock-health` feature `os-notify`: policy (focused window sees it → no OS toast), per-cause rate limit, quiet hours, actions, click-through to the cause | `crates/paddock-health/src/notify/os.rs`, `shell/notify` | X5 none; after X7 for the settings file | policy table tests; a burst of 100 posts gives one toast per cause in the window; quiet hours across midnight; a click event routes to `notifications_open` | 1 |
| **X1** `paddock-log` | CLEF layer, daily files with `keep_files`/`max_file_mb`, level at run time, `log` bridge, IPC span fields, panic hook + crash marker, settings section "Diagnostics ▸ Logging", child-process log capture (`stderr_pipe` from `debrief-ipc`) | `crates/paddock-log/` | X7 | Debrief's `logging.rs` tests pass against the crate; a panic on a worker thread lands in the file and writes the marker; budget: after `max_file_mb` only errors; a line from the webview arrives with `Window`; files older than `keep_files` are removed; a sidecar without Tauri uses it | 3 |
| **X4** `paddock-service` | `ServiceSpec` rendering (plist, unit, Windows service or `Run` entry), `RepairOps` state machine, `registered_version` + rule-24 sequence, exe-changed watcher, `launch_at_login` settings section, health entry with Repair, `Runner` port | `crates/paddock-service/` (from Pitwall's `service_install.rs`/`service_repair.rs`, Pitcrew's `platform/`, Dyno's `service/spec.rs`) | X6, X7 | the unit text of each OS is golden-tested; the repair machine reproduces R03 (stale LWCR, EX_CONFIG) in a fake `Runner` and recovers; version change triggers unregister → bootout → wait → register exactly once; nothing touches real launchd/systemd/registry (C14); the macOS smoke in X10 installs build N then N+1 of `shell-demo`'s sample agent and asserts it runs | 5 |
| **X2** `paddock-diag` + `shell/diagnostics` | manifest, section contributions, `Redactor` (secrets, personal data, content), preview, zip as a job, "Report a Problem" dialog (decision kind: preview list, description, Save) | `crates/paddock-diag/`, `ui/projects/paddock-ui/shell/diagnostics/` | X1, X6, X7 | the union of Debrief's, Pitcrew's and Dyno's redaction tests pass; preview equals the written zip (file list and sizes); a 1 GB log is cut to the bounded window; Cancel leaves no partial zip; axe clean; nothing is sent anywhere (no network in the crate: `cargo deny` bans) | 3 |
| **X3** lifecycle | running marker and unclean-shutdown notice, `--safe-mode` in the launch plan, `Checkpoint<T>` store (versioned through X7), restore offer | `crates/paddock-startup/src/{lifecycle,checkpoint}.rs`, `shell/startup` | X1 (crash marker), X7 | killing the process leaves the marker and the next start offers Restore/Safe Mode; a clean Quit removes it; safe mode reaches settings sections and is shown in the title bar; a damaged checkpoint starts clean with a notice | 2 |
| **X9** conformance + release | `@decryp7/paddock-conformance` skeleton (CLI, waivers, findings, job summary), C03, C04, C06, C07, C08, C14, C15, C16; `conformance/release/{publish,latest-json,strip-keys,macos-verify-bundle}`; `commit-lint` as C23's first part | `conformance/` (new npm package), `.gitea/workflows/ci.yml` step | none | each check has a good and a bad fixture (`node --test`); run against every app's `origin/main` and the findings match the catalogue (Pitcrew: no icon check, no key strip; Dyno: hard-coded version); `latest-json` output byte-equal to Pitwall's for the same input | 5 |
| **X10** CI actions | composite actions (`setup-rust` with sccache, `setup-node`, `strip-signing-keys`, `tauri-bundle`, `macos-verify`, `macos-smoke`, `publish-release`, `conformance`) and a reusable `app-release.yml` | `.gitea/actions/*`, `.gitea/workflows/app-release.yml` | X9 | `shell-demo-release.yml` is rewritten onto them and releases a canary; a test workflow in a scratch repo consumes them by tag; Renovate config pins them | 2.5 |
| **X11** lint presets | `@decryp7/paddock-eslint` (rules 27 to 39; `lint-effects` and `lint-styles` as rules), `lint/clippy.toml`, `lint/workspace-lints.toml` | `eslint/`, `lint/` | none | each rule has valid/invalid cases; Debrief's two scripts' tests pass as rule tests; the preset on `shell-demo` is clean; a sample app extending it cannot drop `disallowed-methods` (C13) | 1.5 |
| **X12** UI parts | `pd-path-text` (middle ellipsis, full value in the tooltip), `pd-empty-state` | `ui/projects/paddock-ui/src/` | none | Debrief's middle-ellipsis specs move over and pass; the 320 px layout test of the gallery shows no horizontal scroll | 0.5 |
| **X13** `paddock-ipc` + link | ADR 0001 step 1 (frame, peer, endpoint, token, launch, child) plus `link` (discovery file with pid/build/token, owner-only; client with reconnect back-off and "replace other build"); Windows pipe SDDL from Pitcrew | `crates/paddock-ipc/` | X6, X20 | `debrief-ipc` and `dyno-ipc` test suites pass on it; a stale discovery file (dead pid) is ignored; a second build replaces the first; frames over the limit fail one call, not the link; fuzz target for the frame codec | 4 |
| **X14** `paddock-chromium` | Spotter's driver modules + Debrief's `cue`, `fake`; install via `paddock-download` (hash while writing), processes via `paddock-proc` | `crates/paddock-chromium/` | X6 | both apps' driver tests pass; the real-browser test uses only Chrome for Testing (C15) and closes with `Browser.close`; no code-sign clone or process left (C14) | 3 |
| **X15** `paddock-packs` | loader (built-in + user, limits, refuse whole with a reason, unknown keys warn), watcher with debounce and swap, `user_regex` | `crates/paddock-packs/` | X7 | a pack added, changed or broken while running is applied or refused within 2 s without a restart (watcher fake + one real-FS test); limits on size, count and regex size; Spotter's, Pitwall's and Dyno's pack fixtures load through it | 2.5 |
| **X16** `paddock-net` | `Egress` client on the shared transport: https only, timeouts, caps, redirect guard (private, loopback, link-local), allow-list, offline, per-host gap, ETag | `crates/paddock-net/`, `paddock-download/src/transport.rs` (shared) | none | Spotter's `guard.rs` and Pitwall's `egress.rs` tests pass; a redirect to 169.254.169.254 is refused; offline refuses before a socket opens (test transport counts sockets) | 3 |
| **X17** `paddock-channels` | webhook, ntfy, SMTP; settings model; test message; secrets via X5, HTTP via X16 | `crates/paddock-channels/` | X5, X16 | Pitcrew's `notify-channels` tests and Spotter's `mail_delivery.rs` pass on it (local fake SMTP, no real network) | 1.5 |
| **X18** `paddock-names` | segmentation, normalisation, typo-tolerant match and rank from `debrief-core/src/names` | `crates/paddock-names/` | none | Debrief's names tests pass; "grafic bilder" finds `CompanyA.Graphic.Builder.exe`; Dyno's counter search cases pass; property test: never panics, deterministic order | 2 |
| **X19** set-up checklist | `shell/setup` page and `setup_state` command; contributions for service, start at login, notification permission, Pitwall pairing | `ui/projects/paddock-ui/shell/setup/`, `crates/paddock-shell/src/setup.rs` | X4, X8, `paddock-pitwall` | shell-demo shows the checklist on first start and not after; each item's Repair works when its target is down (rule 19) | 2 |
| **X20** clock and back-off | `paddock_jobs::time::{Clock, SystemClock, FakeClock, Backoff}` | `crates/paddock-jobs/src/time.rs` | none | Pitwall's `backoff.rs` cases; fake clock drives a job's timeouts without sleeping | 1 |

## 4. Order and parallel groups (at most four agents)

```
0.3.0   G1 (now)        X7 env | X6 proc | X5 secrets → X8 os-notify | X9 conformance (5 d, runs into G2)
        G2              X1 log (X7) | X4 service (X6, X7) | paddock-pitwall §15 (X5) | X9 cont.
        G3              X2 report (X1, X6, X7) | X3 lifecycle (X1, X7) | X10 CI (X9) | X11 lint → X12 UI
        tag v0.3.0 (crates + paddock-ui + paddock-e2e + paddock-conformance + paddock-eslint), with the merged 0.2.1 UI fixes
0.4.0   G4 (Pacenotes start, Spotter/Pitcrew moves)   X20 clock → X13 ipc+link | X14 chromium | X15 packs | X16 net
        G5              X17 channels (X5, X16) | X18 names | X19 set-up (X4, X8, pitwall)
        tag v0.4.0
```

- Critical path for 0.3.0: X7 (2.5) → X1 (3) → X2 (3) ≈ 8.5 days; X4 (5) runs beside it after X6/X7.
- `paddock-pitwall` is in G2 because it needs X5; it takes one of the four slots.
- Rule 41: heavy Rust packages (X4, X6, X13, X14) are not more than two at a time with `CARGO_BUILD_JOBS=2`.
- 0.3.0 is the release the Pitwall shell move consumes, so everything Pitwall uses is in it; 0.4.0 is what Pacenotes
  starts on and what Spotter, Pitcrew and Dyno need at their moves.
- Versioning (crates/README): each crate starts at 0.1.0 and is published by the one repo tag; `paddock-shell` and
  `@decryp7/paddock-ui` stay in lockstep (0.3.0); the new npm packages start at 0.1.0.

## 5. Adoption

| App | When | Takes | Deletes |
|---|---|---|---|
| Pitwall | its shell move (next) | X1 to X12 from 0.3.0; X16 at 0.4.0 | `service_install.rs`, `service_repair.rs`, `credentials.rs`, server `service/` rendering and `login_path.rs`, `process.rs`, `egress.rs` (0.4.0), `scripts/ci/*` and `scripts/release/*`; data folder moves to Local (§6 D3) |
| Debrief | migration M1/M2 (after Pacenotes starts); X9 to X12 at once (rollout step 1 of the conformance proposal) | X1, X2, X3 (it is the source: keeps its sections and checkpoint content), X5, X6, X7, X8; X13 and X14 at 0.4.0 (ADR 0001 phase), X18 | `logging.rs`, generic half of `activity/report*`, `lifecycle.rs` mechanism, `app_dirs.rs`, `notify.rs`, `debrief_core::process`, `crates/chromium-driver`, `debrief-ipc` (via ADR 0001), its e2e helpers |
| Pitcrew | its shell move | 0.3.0 set; X13 (service link), X16, X17 at 0.4.0; gains single instance from the shell (it has none today) | `applog.rs`, `diagnostics.rs`, `credentials.rs`, `service_install.rs`, `service_repair.rs`, `platform/*` rendering, `login_env.rs`, `process.rs`, `terminate.rs`, `self_update.rs`, `redact.rs`, `ipc.rs`, `crates/notify-channels`, scripts |
| Spotter | its shell move | 0.3.0 set; X14, X15, X16, X17 at 0.4.0 | `applog.rs`, `credentials.rs`, `toast.rs`, `spotter-render` driver modules, `pack.rs` loader, `user_regex.rs`, `fetch.rs`/`guard.rs`, `mail.rs`, scripts |
| Dyno | its shell move | 0.3.0 set (gains rule-24 repair); X13, X15, X20 at 0.4.0 | `logging.rs`, `diagnostics` collection, `login.rs`, `service/spec.rs` rendering, `toolpath.rs` login part, `process.rs`, `dyno-ipc`, `cleanup/pack.rs` loader, scripts |
| Pacenotes, Sighter | first commit | everything, no waivers | Sighter PoC `credentials.rs`, `pitwall.rs` are not carried over |

## 6. Decisions (recommended option first; the coordinator takes these)

- **D1 Log format: CLEF JSON lines for every app, service and sidecar.** Debrief opens them natively, the activity log
  and report a problem already use them, and structured fields make redaction possible. Alternative: plain text (what
  four apps do) loses fields and needs a second redaction path.
- **D2 Log folder: the OS log place.** macOS `~/Library/Logs/<App>`, Windows `%LOCALAPPDATA%\<App>\logs`, Linux
  `$XDG_STATE_HOME/<app>/logs`; system services `%ProgramData%\<App>\logs` and `/var/log/<app>`. Seven days, 20 MB a day.
- **D3 Windows data folder: `%LOCALAPPDATA%` for every app.** Models and caches must not roam. Pitwall is released (1.0.3),
  so `paddock-env` moves its folder once (copy, verify, then delete, as a job with progress) and `paddock-pitwall` looks
  for `discovery.json` in both places for one minor version.
- **D4 Background process kind: per-user agent by default** (launch agent, systemd user unit, per-user `Run` entry on
  Windows); a Windows SCM service only when the work must run without a logged-on user (Pitcrew's runners, Pitwall's
  headless mode). `ServiceSpec` declares the kind; both share the repair and version logic.
- **D5 App ↔ own process: `paddock-ipc` (postcard over a private endpoint).** HTTP on loopback stays only for Pitwall's
  public API, which other apps and third-party clients call. Pitcrew and Dyno move their links onto it.
- **D6 Credential names: service = the app's display name (as today), account = `<scope>:<key>`.** Users see these names in
  Keychain Access and Credential Manager; keeping the display name means only Pitcrew's "Pitcrew notification channel"
  entries are renamed (read old, write new, delete old).
- **D7 CI sharing: composite actions plus one reusable release workflow, pinned by tag through Renovate.** Confirm first
  that the Gitea version on dev.decryptology.net resolves cross-repository `uses:` for both (a one-hour spike at the
  start of X10); if reusable workflows across repos do not work, composite actions only.
- **D8 One HTTP stack: `ureq` + rustls with the platform verifier** (what `paddock-download` uses). Pitwall's server
  keeps `axum`/`hyper` for serving; outbound `reqwest` goes away with X16.
- **D9 Report a problem: nothing is sent, the user saves the zip after a preview of every file** (Debrief's model with
  Dyno's preview). No upload endpoint.
- **D10 i18n: every app extracts its strings (`$localize`, `paddock-i18n`) at its shell move and ships English only**
  until a translation exists. Today only Debrief does.
- **D11 Bad pack: refused whole with a named reason; unknown keys are warnings.** (Dyno's rule for errors, Pitwall's for
  keys.)
- **D12 Small pure helpers live in the crate that needs them** (user regexes in `paddock-packs`, clock in `paddock-jobs`),
  not in a grab-bag `paddock-core`.

## 7. Left per app, and doc fixes

- **Privileged helpers and elevation** (Debrief `debrief-ipc/src/elevation.rs`, Dyno `dyno-platform/src/elevate.rs` +
  `dyno-helper`): two users with different needs (one elevated child vs an installed helper). Revisit when Sighter needs
  one.
- **Hardware and "will it fit"** (Pitwall `pitwall-core/src/{hardware,fit}.rs`, Dyno `hardware.rs`, Debrief's AI fit check,
  catalogue L09): Pitwall owns model fit; apps ask Pitwall through `paddock-pitwall` instead of sharing the code. Dyno's
  hardware view is its domain.
- **Domain redaction** (Debrief `debrief-core/src/redact`, Pitwall's prompt inspector) stays; X2's `Redactor` is for
  diagnostics only.
- **Deep links**: `paddock-window::launch` parses them; no app registers a scheme yet. Nothing to share until one does.
- **Time formatting** stays in each core (the wording is the app's); only the clock moves (X20).
- **Doc fixes found:** `docs/adr/README.md` lists ADR 0007 (MCP servers) but the file is on no branch of this repo; it
  needs restoring before `paddock-mcp` work starts. `crates/README.md` lists `paddock-env`, `-service`, `-proc` and
  `-notify` as planned; this plan replaces those rows (`paddock-notify` becomes X8 inside `paddock-health`) and adds
  `paddock-log`, `-diag`, `-secrets`, `-chromium`, `-packs`, `-net`, `-channels`, `-names`; update the table when the
  first of them lands.

## Shell gaps found by Pitcrew's move (2026-10-24, for 0.3.0)

- Job results: a shared result payload / read-once `job_result` in `paddock-jobs` (Pitcrew added its own).
- External jobs: a generic ingest so jobs run by an app's background service appear in the Jobs list with Cancel (Pitcrew mirrors them by hand).
- `provideShellPages` should enable `withComponentInputBinding` so detail routes need no wrapper components.
- Dialog-window providers can't reach root-provided app stores; a per-dialog `providers` loader for the app's backend that works in dialog windows.
- Browser-build stand-ins: a persistent memory settings backend and a memory health backend that runs real repair callbacks.
- `ShellTypes` augmentation needs `resolveJsonModule`, localize types and the polyfill via `buildTarget` (the unit-test builder rejects `polyfills`) — document or remove the need.
- Tray model: checked/toggle items.
- `uiTimeoutProblems`: false positive on identifiers that merely contain "setTimeout" (e.g. `setTimeoutSecs`).
- Health `Cause` hints should use the repair button's real label.

## Shell gaps found by Pitwall's move (2026-10-24, for 0.3.0)

- Frame elements use unprefixed classes (`.page`, `.status`) that app global CSS breaks: prefix every shell class `pd-`.
- A quit started through `exit_app` can't be called off (an app's "Ask each time" cancel leaves the window layer "quitting").
- `uiTimeoutProblems` false positives on setters named `...timeout...(` (also seen in Pitcrew).
- No shared UI native-feel guard (browser keys, context menu); Pitwall keeps its own `core/native-feel.ts`.
- `pd.updates` missing from `settings_overview` in mock-runtime test mode.
- paddock-download: async-friendly transport (or documented private runtime), custom headers, a JSON GET helper, `NotEnoughSpace` naming the path/drive, an external cancel token, constants for caller-added phases, EventSink panics surfaced, a "resumable?" accessor.
- `JobsStore.follow` in windows without jobs providers.
- In-memory about/settings backends need shell-only fixtures.
- The dialog-command checker must run in apps (ship it with its inputs in the npm package).

## Release scope change (user, 2026-10-24 night)

All apps wait for one shell release before they move, so **0.3.0 contains all 20 work packages (X1–X20)**, plus
`paddock-pitwall`, the gaps found by the Pitcrew and Pitwall moves, and the Shell Demo fixes already on main. There is
no 0.2.1 and no separate 0.4.0. Groups G1–G5 keep their order; G4/G5 start as soon as their dependencies land.

## More shared parts for 0.3.0 (user, 2026-10-24 night)

The user added these to 0.3.0 ("All"), and asked for a full survey to find and add every remaining candidate (§ below
filled by the survey).

| Id | Candidate | Home | Who has it today |
|---|---|---|---|
| X21 | UI building blocks: data table (sort, filter, virtual scroll, column choice), ECharts wrapper with the family palette, forms helpers | `@decryp7/paddock-ui` (`table`, `chart`, `forms`) | Debrief, Pitwall, Pitcrew, Dyno, Spotter (own tables and charts) |
| X22 | MCP server: limits, logging, read-only tools, local only (ADR 0007) | new `paddock-mcp` | needed by Pacenotes, Sighter, Debrief; Pitwall is the client |
| X23 | Folder watching with debounce (watched folders, sources, pack reload) | new `paddock-watch` | Debrief, Spotter, pack reloaders |
| X24 | Safe archive unpacking (zip/tar, size and count limits, path traversal guard) | new `paddock-archive` | Debrief imports, Pitwall engines, Spotter assets |
| X25 | CLI conventions: JSON output, exit codes, progress on stderr | new `paddock-cli` | Pitwall, Dyno, Sighter CLIs |
| X26 | Benchmark harness with stored budgets (rule 16) | new `paddock-bench` + nightly workflow | each app's own benchmarks |

## Full survey (2026-10-24 night): X27 and later

Method: `origin/main` checkouts of Debrief `ce263cb3`, Pitcrew `cddd3f5`, Pitwall `37c6111`, Spotter `c9e9faa`, Dyno
`f8de282`, Sighter `daa0863`, Pacenotes `e484acb` compared area by area against Paddock `df3efa6` (hashes and `diff` line
counts of same-named files, greps for the same functions, types, constants and dependencies). Read-only; nothing built.
"Diff N" is the number of changed lines; "identical" means the same hash. Items already in X1 to X26, §15/§16, ADR 0001,
0005, 0007, 0012 and 0013 are not repeated; where they miss something it is listed under "Gaps in planned items".

| Id | Candidate | Home | Who has it today | Effort (agent-days) |
|---|---|---|---|---|
| X27 | Repo baseline: synced config files, Renovate preset, tsconfig base, Cargo profiles, and a drift check | `paddock/templates/repo/`, Renovate preset `local>decryp7/paddock//renovate/app`, new `@decryp7/paddock-tsconfig`, conformance check C28 `sync` | all 7 repos: `renovate.json` (6 identical, Pitwall +1 rule), `rustfmt.toml` (7 identical), `LICENSE-*` (8 identical), `.editorconfig` (7 identical, Debrief own), `deny.toml` and `cliff.toml` (identical but the app name; Debrief's `deny.toml` 101 lines), `lefthook.yml` (4 identical, Pitwall + privacy + pre-push), `app/tsconfig.json` (4 identical), `app/.nvmrc` (5 identical), `[profile.dev]`/`[profile.dist]` in 5 `Cargo.toml` (identical), `scripts/export-diagrams.sh` (6 identical copies of Paddock's, Dyno other) | 1.5 |
| X28 | App IPC kit: one error envelope, generated wire types, backend selection, call timeouts, mock kit | `paddock-commands` (`CommandError`, ts-rs export via `testing::sync_generated`) + `@decryp7/paddock-ui/ipc` and `/ipc/testing` | `CommandError` in Pitcrew/Spotter/Dyno `app/src-tauri/src/dto.rs`, Pitwall `crates/pitwall-core/src/mgmt.rs`, Debrief `app/src-tauri/src/error.rs` (`CmdError`), `paddock-jobs` `tauri.rs`; TS `core/command-error.ts` (Pitcrew 26, Pitwall 28, Spotter 21, Dyno 50; diff 24 to 46) and Debrief `core/cmd-error.ts` (82); `core/call-timeout.ts` (Pitcrew 47, Spotter 53, diff 36), Pitwall `core/timeout-backend.ts` (Proxy); hand-written `core/api.types.ts` of 527 to 1,793 lines in the four manager apps (Debrief generates with ts-rs); mock switches `?fail=`/`?mock=`/`latency` in Dyno `core/mock-backend.ts` and Debrief `core/mock/mock-options.ts` + `e2e/mock-url.ts`, `MockOptions` in Pitcrew | 3 |
| X29 | Live feed: Rust pushes state to windows, UI folds it (sequence, coalescing, bounded tails, reconnect) | new `paddock-live` (tauri feature) + `@decryp7/paddock-ui/live` | Pitwall `app/src-tauri/src/live.rs` + `core/live.store.ts` + `core/live-reducer.ts`; Dyno `app/src-tauri/src/link.rs` → `state/live.store.ts`; Pitcrew `events.rs` + `state/status.store.ts` (`watch()`); Spotter `state/dashboard.store.ts` (push coalescing); Debrief `core/tauri-channel.ts` and live tail | 2 |
| X30 | Tiered metrics store and machine sampler (CPU, memory, free disk of a path's volume, folder size) | new `paddock-metrics` | Pitcrew `crates/pitcrew-core/src/metrics.rs` (509, generic `TieredStore<P: Series>`), Pitwall `crates/pitwall-core/src/metrics.rs` (515, four tiers), Dyno `crates/dyno-core/src/metrics.rs` + `dyno-agent/src/store.rs` (SQLite tiers); free space: `debrief-agent/src/disk.rs`, `pitcrew-service/src/probe.rs`, `pitwall-server/src/system.rs`, `dyno-platform/src/sensors.rs` | 2.5 |
| X31 | User files: path grants (only paths the user chose), open/save pickers, recent files, reveal, open with, open URL | new `paddock-files` (tauri feature) + `shell/files` | Debrief `app/src-tauri/src/paths.rs` (801), `recent.rs` (400), `opener.rs` (417), `core/recent/*`; Pitcrew `commands.rs` (`open_url`, `reveal_logs`); Spotter and Dyno call `tauri-plugin-dialog`/`-opener` straight from `commands.rs`; Pacenotes and Sighter will open projects and plans | 2.5 |
| X32 | Storage page and safe delete: what the app keeps, bounded size walk, clean with preview and a re-check before each delete | new `paddock-storage` + a `shell/settings` "Storage" section | Debrief `app/src-tauri/src/storage.rs` + `storage/{model,plan,stores,walk}.rs`; Pitwall `pitwall-server/src/leftovers.rs` + cache page (`e2e/cache.e2e.ts`); Pitcrew `cache_report`/`cache_clean` + `shared/policy/disk-guard-editor.ts`; Dyno `dyno-agent/src/cleanup/usage.rs` (bounded tree) and `cleanup/guard.rs` (re-check before delete) | 2.5 |
| X33 | Schedules and pause windows: every N, times of day, cron, catch-up after sleep, DST; editor component | `paddock-jobs::schedule` (next to X20 `time`) + `pd-schedule-editor` | Spotter `crates/spotter-core/src/schedule.rs` (719) + `shared/schedule.ts`, `shared/cadence.ts`; Debrief `crates/debrief-agent/src/schedule.rs` (208) and `debrief-ai/src/updates/policy.rs`; Pitcrew `crates/pitcrew-core/src/pause.rs` (236) + `shared/policy/pause-editor.ts` | 2 |
| X34 | Tauri build and sidecar kit: `build.rs` helper, Windows manifest for test binaries, sidecar build script, `--self-test` | new `paddock-build` (build-dependency), `windows-app-manifest.xml` in it, `paddock-sidecar` bin in conformance, `paddock_proc::self_test` | `app/src-tauri/build.rs` in all 5 (21 to 35 lines, same three jobs), `windows-app-manifest.xml` (5 identical; Shell Demo has none), `scripts/sidecar.mjs` Pitcrew 24 / Pitwall 25 ("adapted", diff 11) / Dyno `app/src-tauri/scripts/sidecar.mjs` 28, Debrief `app/scripts/build-sidecars.mjs` 121; `--self-test` only in `debrief-store-worker` | 1.5 |
| X35 | UI utilities: number, size, duration and relative-time formatting, a visibility-aware clock signal, safe local preferences, copy menu | `@decryp7/paddock-ui/format`, `/prefs`, root `pd-copy-menu` | `shared/format.ts` Pitcrew 125, Pitwall 124, Spotter 108, Dyno 194, Debrief `core/format.ts` + `shared/format/units.ts` (six APIs for `formatBytes`/duration/relative, diff 175 to 247); clock: Pitcrew/Spotter `core/clock.service.ts`, Pitwall/Dyno `shared/now.ts`; `core/storage.ts` (Pitcrew = Spotter), Pitcrew `preferences.service.ts`, Dyno `core/view-prefs.ts`, Debrief `ui-prefs.store.ts`; copy menu Pitcrew/Spotter `shared/ui/copy-menu.ts` (diff 6), Dyno `shared/copy-menu.ts` (diff 36), Spotter `shared/clipboard.ts` | 1.5 |
| X36 | Generic icon set (`pd:` namespace): the ~50 glyphs every app redraws | `@decryp7/paddock-ui` icons + outline helper | `shared/icons.ts` Pitcrew `pc:` 111, Pitwall 100, Spotter 86, Dyno 115, Debrief `shared/material/debrief-icons.ts`: refresh, play, pause, stop, trash, search, copy, plus, close, check, more, sun, moon, globe, alert, info, lock, shield, clock, settings, download, folder, edit, bell, external, pin, chevrons drawn up to five times with different paths | 1 |
| X37 | Control styles and the Angular Material bridge (buttons, fields, selects, chips, segmented, tabs, kbd; Material themed through `--pd-*`) | `paddock-ui/scss/_controls.scss` + `_material.scss` | Pitcrew `src/styles/_controls.scss`, Spotter `src/styles/_controls.scss`, Pitwall `styles.scss` (996), Dyno (1,191), Debrief (755); distinct Material selectors/tokens overridden: Debrief 53, Dyno 52, Pitcrew 42, Pitwall 33; `paddock-ui` 0 | 1.5 |
| X38 | Component test kit: jsdom setup, DOM helpers, fake chart | `@decryp7/paddock-ui/testing` (setup file + helpers) | `src/test-setup.ts` (Pitcrew = Spotter = Dyno, 14 lines; Debrief 57 with ResizeObserver, pointer capture, Tauri stub clean-up); helpers Pitcrew/Spotter `testing/test-helpers.ts` (diff 21), Pitwall `testing/render.ts`, Debrief `testing/helpers.ts` + `must.ts`; fake charts Pitwall `FakeChart`, Pitcrew `fakeChart()`, Debrief `testing/fake-echarts.ts` | 1 |
| X39 | E2E config preset and documentation screenshot runner | `@decryp7/paddock-e2e` (`paddockConfig()`, `captureDocScreens()`) | `app/playwright.config.ts` in 5 (31 to 71 lines, all different); screenshot specs Pitcrew `e2e/screenshots.spec.ts` (214), Spotter (256), Dyno (457), Pitwall `e2e/screenshots/capture.shot.ts`, Debrief `app/scripts/screenshots/capture.ts` + `visual-qa/` | 1.5 |
| X40 | Rust test kit: scratch folders with a leftover check, env lock and in-process hermetic env, golden files, fake OS runner, never-panics helper | new `paddock-testkit` (dev-dependency) | 39 `std::env::temp_dir()` uses in the manager apps' crates; env locks in Debrief (3) and Spotter (`serial()`); golden files via `paddock_commands::testing::sync_generated` only; fake `Runner` seam in Dyno | 1.5 |
| X41 | Hardening workflows: cargo-fuzz targets, mutants, proptest never-panics, corpus | reusable `paddock/.gitea/workflows/{fuzz,mutants}.yml` + X40 helper | Debrief `.gitea/workflows/{fuzz,mutants,corpus}.yml`, `fuzz/`, `docs/fuzzing.md`; proptest in Pitcrew, Pitwall, Dyno only; Debrief's, Pitcrew's and Pitwall's AGENTS.md promise fuzz targets for every parser | 1.5 |
| X42 | Third-party notices (cargo-about + npm licences → `THIRD_PARTY_NOTICES.md`, shown by About) | `@decryp7/paddock-conformance` bin `paddock-notices` + shared `about.toml` | Debrief `scripts/notices/{about.toml,generate.mjs,generate.sh}`; the shell's About already serves legal texts (`about_legal`), but no other app produces the notices | 1 |
| X43 | Shared doc blocks: AGENTS.md common sections, suite lessons, releasing and first launch, macOS/Linux notes, README skeleton | `paddock/docs/blocks/*.md`, synced by C23's marker mechanism; new-app template | AGENTS.md sections "Design principles", "Where logic lives", "Working in parallel", "Style", "Working practices" in all 7 (diff 5 to 47 per section between manager apps); `docs/suite-lessons.md` in 6 (4 to 65 lines, all different); `docs/releasing.md` in 5; `docs/macos.md`, `linux.md` in Pitcrew, Pitwall, Dyno | 1 |
| X44 | Undo and an activity journal (README principle "changes have an undo") | `shell/undo` (stack, Edit ▸ Undo/Redo, toast with Undo) + `paddock-jobs::journal` | Debrief `app/src/app/undo/` (undo stack, snapshot/query/range undo); Dyno `crates/dyno-core/src/activity.rs` + `dyno-agent/src/handlers.rs` (reversible system changes) + `state/activity.store.ts`; Pitcrew, Pitwall, Spotter none | 2 |
| X45 | Sibling apps: find an installed family app and hand it files or a view | new `paddock-siblings` | Pitcrew `crates/pitcrew-core/src/debrief_link.rs` + `app/src-tauri/src/handoff.rs`; ecosystem.md plans Dyno → Debrief, Spotter, Pitcrew | 1 |
| X46 | Window weight budget (rule 35: initial chunk per window, tightened each release) | conformance check C29 | Debrief `app/scripts/window-weight.mjs` (`check:window-weight`, `check:dialog-weight`); none elsewhere | 0.5 |
| X47 | Dev and host tooling: per-app build bootstrap, installed-app verify, Linux container runner, build-folder pruning | `paddock/tools/` (scripts with app parameters) + an X10 `host-cleanup` action | Pitcrew/Pitwall `scripts/linux-bootstrap.sh` (63/61), Pitcrew `macos-bootstrap.sh`, `linux-verify.sh`/`macos-verify.sh` (Pitcrew 62/56, Pitwall 57/106, "adapted"), Pitwall `linux-container-verify.sh`, Dyno `linux-container.sh` + `linux-verify.sh`, Dyno `ci-prune.sh`, Pitwall `scripts/ci/macos-cleanup.sh`, Debrief `clean-build-dirs.ps1` | 1.5 |

Total: about 34 agent-days (X27 to X47), plus about 3 for the gaps below.

### Notes per item

- **X27.** Base: Pitcrew's files (the plain template the others copied), Pitwall's `lefthook.yml` (adds the pre-push gates)
  and Pitwall's Renovate Node rule. Renovate supports a shared preset from a repo, so each app's `renovate.json` becomes
  `extends` plus app rules; the preset also groups the `decryp7` registry crates and npm packages into one PR per Paddock
  release (ADR 0012 §8 asks for that; no app config does it today). Templated files keep one app-name placeholder.
  `new-app.mjs` copies only `app/` today; it should also write the repo root from the template. The check fails on drift
  and `--write` repairs it.
- **X28.** Base: Pitcrew's `CommandError` (code enum + message, `bad`/`internal` constructors) in Rust; Debrief's ts-rs
  export and the shell's `sync_generated` stale check for TypeScript. Pitwall's Proxy wrapper is the best timeout shape
  (no per-command tables to forget), but it must skip job-starting commands (rule 12; reuse `uiTimeoutProblems`). Mock kit
  from Dyno's URL switches plus Debrief's lazy mock registry (mocks stay out of the release bundle). App DTOs stay in
  each app; only the envelope, the export mechanism and the TS plumbing are shared.
- **X29.** Same purpose, different designs. Base: Pitwall's pure `live-reducer.ts` (fold, bounded tail, one point per
  step) and its reconnecting Rust task; add a sequence number so a window that reloads asks for a snapshot. Depends on
  X20 (back-off) and X13 for a companion source.
- **X30.** Base: Pitcrew's generic `TieredStore<P: Series>` with `Retention`; Pitwall's tier table as the default. Dyno's
  SQLite history stays (domain) but takes the tier maths. The sampler reads process memory through X6 (PSS, threads
  skipped) and persists through X7.
- **X31.** Base: Debrief's `paths.rs` (grants filled only by native dialogs, drag and drop and the command line,
  canonicalised; family security rule "open only paths the user chose"), `recent.rs` (recorded only after a granted open,
  re-granted on use) and `opener.rs` (command lines as pure functions per OS). workbench.md row 57 deferred `paths.rs`
  "until a second app needs it": Pacenotes and Sighter do from their first commit, and three apps call the dialog and
  opener plugins without a grant today.
- **X32.** workbench.md keeps Debrief's storage page in Debrief; the mechanism is not Debrief's: three more apps list and
  clean their own folders. Base: Debrief's categories and plan, Dyno's bounded walk (depth, entries, time, no links, same
  volume) and re-check guard, Pitwall's "never deletes by itself". Categories come from X7's `AppDirs`.
- **X33.** Base: Spotter's `schedule.rs` (pure, tested for catch-up after sleep and DST); Pitcrew's pause windows as a
  second schedule kind; quiet hours for X8 reuse the same type.
- **X34.** The three `build.rs` jobs (embed the Windows manifest into every binary so test binaries load, write an empty
  sidecar placeholder for `cargo check`, rerun on `TAURI_UPDATER_PUBKEY`) become one call. Base: Pitcrew's `build.rs`
  and `sidecar.mjs`. Rule 23 requires CI to start every sidecar with `--self-test`; only Debrief's store worker has it.
- **X35.** Formatting is presentation (engine values in, text out) and uses `Intl` with the active locale. Base: Debrief's
  `units.ts` (most complete), Pitcrew's `ClockService` (pauses while hidden), Pitcrew/Spotter `storage.ts`, Pitcrew's
  copy menu. Domain wording (Pitwall `tokensText`, Dyno `formatGhz`) stays in the apps.
- **X36.** One outline helper (24 px, `currentColor`, stroke 1.75 as Pitcrew) and one drawing of each common glyph;
  apps keep only domain glyphs in their namespace. Needs the independent drawing review (rule 22).
- **X37.** `paddock-ui/scss/_base.scss` covers the page but no controls; each app styles buttons and Material by hand.
  Base: Pitcrew's `_controls.scss` (split already) and Debrief's Material overrides (the most complete).
- **X38.** Base: Debrief's `test-setup.ts` (superset) and helpers; `provideShellTesting` and `ManualClock` already exist
  in `shell/testing`, so this adds the DOM side. The fake chart ships with X21's chart wrapper.
- **X39.** All five screenshot runners do the same thing: a screens table × both themes at 1440×900 @2x, a frozen clock,
  long pages framed, output to `docs/screenshots/<theme>/`. Base: Dyno's (the most complete) on `captureStable`; it
  writes the hashes C24 checks.
- **X40.** Base: the leftover check ADR 0012 §7 names (not built yet) and Dyno's `Runner` seam; this is the in-process
  half of C14.
- **X41.** Base: Debrief's workflows and `docs/fuzzing.md`. Apps with parsers of untrusted input first: Spotter
  (feeds, pages), Pitwall (API bodies, GGUF headers), Dyno (OS command output), Pacenotes (source files).
- **X42.** Base: Debrief's generator (deterministic, checked in CI); its licence list must match the shared `deny.toml`
  of X27.
- **X43.** Only the rules block is synced today (C23). The common sections drifted (same purpose, different words); the
  Paddock copy becomes the source and each app keeps its own sections (Pitwall "Privacy", Dyno "Safety").
  `suite-lessons.md` becomes a link to `paddock/docs/lessons.md`.
- **X44.** Same purpose, different design: Debrief undoes UI state, Dyno undoes system changes. Shared: the stack, the
  Edit menu items and the toast; each app contributes undoable actions. Dyno's reversible actions stay in Dyno.
- **X45.** Base: Pitcrew's `debrief_link.rs` (CLI per OS, legacy name, a running instance takes the files over, "not
  installed" is a plain answer). §7 says there is nothing to share for deep links until one app registers a scheme. The
  CLI hand-off already exists, and ecosystem.md plans three more links.
- **X47.** The verify scripts read what X4 registers (unit, launchd state, discovery file), so they follow X4. Docker
  and OrbStack use only containers the script created (rule 7).

### Gaps in planned items

- **Native feel (shell gap from Pitwall's move):** it is not only Pitwall. All five apps have `core/native-feel.ts`
  (234 to 253 lines, diff 39 to 66 against Debrief's) and `src-tauri/src/native_feel.rs` (94 to 123 lines; Pitcrew,
  Spotter and Dyno differ only in the app name), although `paddock-window/src/feel.rs` exists. The shell package should
  ship the UI guard and every app should drop both copies at its move (about 1 day).
- **X4:** add the Windows NSIS hooks (`src-tauri/windows/installer-hooks.nsh` in Pitcrew, Pitwall and Dyno, diff 18 to
  27) and the launchd plist templates (`src-tauri/macos/*.plist` in the same three) to `ServiceSpec` rendering.
- **X9:** the release notes' "First launch" block is a heredoc in five workflows (Debrief, Pitcrew, Spotter, Dyno
  `build.yml`, Pitwall `release.yml`) plus `docs/releasing.md`: ship it as one template. C03 checks only
  `src-tauri/icons`; it should also check the tray sets (`paddock/icons/<app>/tray/` against Spotter's 24 and Dyno's 16
  files, Pitwall's own `scripts/gen-tray-icons.mjs`). `paddock-updater`'s About reads the build commit and date from
  `option_env!`, but no CI sets them (Debrief does it in the UI with `app/scripts/build-info.mjs`).
- **X10:** add Debrief's `plan` job (skip a nightly when main is unchanged since a green run, skip a main push that is
  tagged) and `nightly-mark`, the restore/save-always cache pattern, the `licences` job (cargo deny + npm audit) that all
  five have, and the e2e job with Chrome for Testing. Pitwall still runs on `catthehacker/ubuntu:act-latest`, not the
  `garage` image the others pin.
- **X12:** the empty state is not Debrief-only: Pitcrew and Pitwall have `shared/ui/empty-state.ts` (44 and 60 lines).
- **X21:** add row keyboard navigation and row menus (Pitwall `shared/ui/row-nav.ts`, `row-menu.ts`; Spotter the same
  two, diff 141), the chart options per kind (`shared/charts/chart-options.ts` in Pitcrew 339, Pitwall 406, Spotter 149),
  the token readers for charts (Pitwall `shared/theme-tokens.ts`, Dyno `shared/tokens.ts` + `color.ts`) and the fake
  chart for tests (X38).
- **X23:** also Dyno's watched folders with keep-for times (`dyno-core/src/watch.rs`, `dyno-agent/src/watch.rs`) and
  Debrief's "file has settled" rule (`debrief-core/src/watch.rs`).
- **X24:** also the runtime unpackers (`debrief-ai/src/runtimes/unpack.rs`, `debrief-video/src/runtime/unpack.rs`) and
  Pitcrew's runner archives.
- **X25:** also Debrief's `debrief-cli` (`output.rs`, `progress.rs`, `time_arg.rs`), the agent and service CLIs
  (`debrief-agent`, `pitcrew-service` `cli.rs`, `dyno-agent` `cli.rs`).
- **X14 and §16:** the pinned-runtime pattern (catalogue JSON with per-platform URL, size and SHA-256; store with
  versions; self-test; macOS quarantine) exists three times: Debrief `debrief-video/src/runtime/` (FFmpeg) and
  `debrief-ai/src/runtimes/`, Pitwall `pitwall-core/src/runtimes/` (catalogue diff 56 against Debrief's), and Spotter's
  `chromium-manifest.toml`. Pacenotes (language servers) and Sighter (training runtime) need it. X14 should build its
  install on a generic `paddock-download` "runtime" module rather than its own manifest.
- **X16:** Debrief's two hand-written loopback HTTP/1.1 clients (`debrief-ai-worker/src/http.rs`,
  `debrief-ai/src/engine/pitwall/http.rs`) go with `paddock-pitwall` (§15), not `paddock-net`; say so in X16.
- **Adoption, not new code:** the manager apps' toasts (`layout/toasts.ts`, `state/toast.store.ts`), connection banners,
  confirm dialogs, `state/loadable.ts` (Pitcrew = Spotter) and Dyno's `async-state.ts`, sidebars, theme services and
  window chrome are covered by `shell/notify`, `shell/dialogs`, `shell/store` (`withRequestStatus`), `shell/pages` and
  the theme; the §5 adoption table should list them for deletion.

### Skipped as app-specific

Domain catalogues and stores (`catalog.rs`/`store.rs` in Spotter sources, Dyno counters and cleanup categories, Debrief
dumps and profiling), per-app usage and limits (Pitwall `usage.rs`/`limits.rs`, Pitcrew runner `usage.rs`), Pitwall's
request scheduler (`scheduler.rs`, LLM priorities), Pitcrew's Gitea API client (`gitea.rs`), Spotter's HTTP source
logic and SSRF guard (in X16 already), the app's own mock data and `api.types.ts` DTOs (only the plumbing is X28), and
hardware, elevation and domain redaction (§7). Sighter and Pacenotes have no code yet; their needs are counted where
their plans name them.

## Scope decision on the survey (user, 2026-10-24 night: "find all and add all")

All of X27–X47 and every "Gap in planned items" go into 0.3.0. That settles the open calls: X31 path grants and X32
storage page move to Paddock now (overriding the workbench.md deferral), and X45 sibling hand-off is shared (overriding
§7). Every app's `core/native-feel.ts` / `native_feel.rs` is replaced by the shell's native feel.

## Shell gaps from Pitwall's design review (2026-10-24 night, for 0.3.0)

- `pd-waiting-for`: draw the designed offline state (not a spinner) when the service is down.
- Status-bar jobs item: phase label plus a determinate ring.
- Bell count in the danger colour, not a neutral pill.
- Settings window: themed (non-native) selects in Appearance; "Show advanced" shown once; the "N advanced settings
  hidden" line not clipped; nav sub-groups; an API for the window's footer notice.
- Service labels keep the app's casing (no forced capital on the first word).

## Shell gaps found by Spotter's move (2026-10-24 night, for 0.3.0)

- **paddock-download: system proxy** (env `HTTPS_PROXY`/`NO_PROXY` and the OS proxy settings). Spotter's old client used
  it; without it users behind a corporate proxy regress. Must land before any app ships on the shared downloader.
- Typed bridge from an app's own job/phase type to `paddock-jobs` (phases + Cancel), instead of hand-written adapters.
- Dialog titles set at runtime (a decision dialog shows its real title, not a static one).
- Already listed and confirmed again: `withComponentInputBinding` in `provideShellPages`; dialog-window providers for
  app-wide stores.
- `PADDOCK_SHELL_PERMISSION_FILES` wiring documented in the starter so every app gets the capability ACL test.

## Two more for 0.3.0 (coordinator, 2026-10-24 late)

| Id | Candidate | Home | Who has it today |
|---|---|---|---|
| X48 | i18n kit: `$localize` setup, extraction, catalog build and `--check`, locale switch in Settings | `paddock-ui/i18n` + conformance check | Debrief `build-locales`, Spotter partial; others none |
| X49 | Accessibility checks: axe in e2e, keyboard-only flows, contrast/CVD checks for tokens | `paddock-e2e` + conformance | Debrief a11y e2e only |

Kept after 0.3.0 on purpose (large, needed by Debrief/Pacenotes first): the workbench layer (ADR 0013, W0–W8), the rest
of the plugin platform (ADR 0001 beyond X13), `paddock-testplan` (ADR 0005), and the shared CI image and cache server.

## Everything joins 0.3.0 (user, 2026-10-24 late: "if we are focusing on paddock, we should add all")

The items kept back above also go into 0.3.0:

| Id | Candidate | Source |
|---|---|---|
| X50 | Workbench layer: dock and layout planner, activity bar, Explorer, palette, assistant pane | ADR 0013, workbench.md W0–W8 |
| X51 | Plugin platform beyond X13: `paddock-guard`, `-view`, `-plugin-api`, `-plugin-host`, `-plugin-package`, `-plugin-test` | ADR 0001 |
| X52 | `paddock-testplan` | ADR 0005 |
| X53 | Shared CI image and cache server for every app | ci-speed notes, X10 |
| X54 | `paddock-proc` follow-up: Windows job object (`KILL_ON_JOB_CLOSE`) so orphaned helpers end too | X6 report |

## Follow-ups from finished packages (for 0.3.0)

- X3: wire `Lifecycle::begin` into the Shell Demo app for a real-Tauri smoke of the recovery flow; place the
  unclean-shutdown notice and safe-mode badge in the workbench frame (X50); a real killed-child test.
- X6: Windows job object (X54).
- Tooling (user approved all, 2026-10-24): `cargo-hakari`, lighter dev debuginfo, `cargo-machete` check;
  measure bun/pnpm vs npm in X53.
