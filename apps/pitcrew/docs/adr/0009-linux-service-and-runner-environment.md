# 9. Linux: a systemd user service, XDG paths, and the login-shell PATH for runners

- Status: accepted (2026-10-22)
- Related: ADR 0001, ADR 0003 (service model); code: `pitcrew_core::{service_paths, service_spec, runner_env}`,
  `pitcrew_service::{platform::linux, login_env, paths}`, `app/src-tauri/src/service_install.rs`

## Context

Pitcrew ran on Windows and macOS. Linux build machines need the same: a supervised runner, installed from the app
without a password prompt of Pitcrew's own. A bug found on the Mac the same day shapes part of this decision: launchd
starts the service, and so every runner and job, with the minimal `PATH` `/usr/bin:/bin:/usr/sbin:/sbin`, so jobs failed
with "Cannot find: node in PATH". The systemd user manager has the same minimal `PATH`.

## Decision

### The service is a systemd user service

Like the macOS launch agent (ADR 0003): it runs **as the user**, needs no root, and the runners get the user's
session, home folder and tools. `pitcrew-service install` writes `~/.config/systemd/user/pitcrew.service`
(`$XDG_CONFIG_HOME`), runs `systemctl --user daemon-reload` and `systemctl --user enable --now pitcrew.service`;
`uninstall` disables and stops it, removes the file and reloads. The app runs these verbs (no elevation) and bounds the
wait; the service program bounds each `systemctl` call (30 s). Errors say what to do (for example, the user instance
is not reachable over ssh without a session).

The unit (`service_spec::systemd_unit`, pure, `ExecStart` words quoted so `%`, `$` and spaces cannot be expanded):
`Restart=on-failure`, `RestartSec=10`, `KillMode=mixed` (SIGTERM goes to the service only, which stops its runners
gracefully and waits for a running job; leftovers are killed after `TimeoutStopSec=300`), `WantedBy=default.target`.

**Lingering** (running with nobody logged in) is a system setting. Pitcrew documents `loginctl enable-linger $USER`
(docs/linux.md) and shows the state in `scripts/linux-verify.sh`, but never runs it.

**AppImage:** its files live in a temporary mount. Install first copies `pitcrew-service` to
`<data>/bin/pitcrew-service` (atomically, 0755) and the unit points there; after updating an AppImage, run Turn on
again to refresh the copy. A `.deb` installs `/usr/bin/pitcrew-service`, which the unit uses directly.

A system-wide unit with `User=` for headless hosts is the possible later option (as the LaunchDaemon on macOS).

### Paths follow XDG

`service_paths` (pure, shared by the service and the app): data folder `$XDG_DATA_HOME/Pitcrew`, default
`~/.local/share/Pitcrew`. The Unix socket (mode 0600, in a 0700 folder) lives in `$XDG_RUNTIME_DIR/pitcrew` when that
is set (a per-user tmpfs cleaned at log-out), else in the data folder. Relative XDG values are ignored, as the spec
says. (Before this change the Linux fallback used `~/.config/pitcrew`; nothing was released, so there is no
migration.)

### Runners get the user's login `PATH` (macOS and Linux)

At start and at every runner (re)start the service resolves the `PATH` the user's login shell would give a terminal
(`login_env`): `$SHELL -l -i -c` (so `~/.zshrc`, where nvm usually lives, runs), then `-l -c`, each with a 5 s timeout and
bounded output; the answer sits between markers so banners are ignored, and is sanitised (absolute entries, no
duplicates, at most 128 entries / 8 KiB). The result is cached for 60 s. When no shell answers, a fallback is used:
nvm's newest Node, `~/.cargo/bin`, `~/.local/bin`, Homebrew, the system folders, then the service's own `PATH`.
The parsing, the fallback and the validation are pure (`runner_env`); the shell and the disk are the thin edge.

Per runner, an optional `[runners.env]` table in `pitcrew.toml` sets or overrides variables (a runner's own `PATH`
replaces the login one). It is validated in the core: at most 32 variables, names `[A-Za-z_][A-Za-z0-9_]*` up to 64
characters, values up to 4096 characters without control characters, and the `PITCREW_` and `GITEA_RUNNER_` prefixes
are reserved. A runner with an invalid `env` is skipped with a problem, like any invalid entry. On Windows the runner
keeps the service's or the user's own environment (the table is not applied there yet).

### The service restarts itself when its program is replaced (macOS and Linux)

launchd and systemd keep the old process after the app is updated or reinstalled, so the service would run old code. It
fingerprints its own executable (size and modification time, `self_update::BinaryStamp`) at start and compares every
30 s. When it changed (`self_update::decide`, pure): with a job or an update running it logs once and waits; when idle it
exits cleanly, and launchd (`KeepAlive`) or systemd (`Restart=always`, which still honours `systemctl stop`) starts the
new program, which adopts the runners as after any restart. A file that is momentarily missing (an update replacing it)
is ignored. On Windows the app or the installer restarts the service, so the watch is off.

## Consequences

- A stopgap `runner.envs: PATH` in a runner's `config.yaml` is no longer needed on macOS and Linux.
- Linux behaviour is tested in a Linux container (unit text, paths, the service against the fake runner); the systemd
  calls themselves need a machine with a user instance and are verified there with `scripts/linux-verify.sh`.
