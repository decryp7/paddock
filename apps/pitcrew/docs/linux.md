# Pitcrew on Linux: build, install, register a runner, verify

Pitcrew on Linux is the app (window and tray icon) plus the Pitcrew service running as **your own systemd user service**
(ADR 0009): it starts when you log in, needs no root, and runs the runners in your session, so jobs get your home
folder and tools. Supported: x86-64 and ARM64 with systemd, a desktop with a status-notifier tray (GNOME needs the
AppIndicator extension; KDE, XFCE and others work out of the box).

## 1. Build

```sh
git clone https://dev.decryptology.net/decryp7/Pitcrew.git && cd Pitcrew
bash scripts/linux-bootstrap.sh
```

It checks Rust, Node 24 (`app/.nvmrc`) and the development packages (it prints the `apt` command for what is missing),
runs `npm ci` and `npx tauri build --bundles deb,appimage`, and prints where the `.deb` and the AppImage are
(`target/dist/bundle/`). It installs nothing. On Debian/Ubuntu the packages are:

```sh
sudo apt install libwebkit2gtk-4.1-dev libgtk-3-dev librsvg2-dev libayatana-appindicator3-dev patchelf file xdg-utils build-essential pkg-config
```

CI builds the same two bundles in the `bundle` job on tags, nightly and manual runs.

## 2. Install

- **.deb:** `sudo apt install ./Pitcrew_<version>_amd64.deb` (depends on WebKitGTK 4.1, GTK 3 and
  libayatana-appindicator). The service program is `/usr/bin/pitcrew-service`.
- **AppImage:** `chmod +x Pitcrew_*.AppImage` and run it. Turning the service on copies the service program to
  `~/.local/share/Pitcrew/bin/pitcrew-service` (an AppImage's own files disappear when it closes); after updating the
  AppImage, turn the service on again to refresh the copy.

Then in Pitcrew: **Settings > Service > Turn on**. Pitcrew writes `~/.config/systemd/user/pitcrew.service` and runs
`systemctl --user daemon-reload` and `systemctl --user enable --now pitcrew.service`. Nothing is elevated.

**Keep the runners going when nobody is logged in** (a headless build machine): once, as yourself,

```sh
loginctl enable-linger "$USER"
```

This is a system setting, so Pitcrew documents it but never runs it. Without it the service (and the runners) stop at
log-out and start again at the next log-in.

## 3. Register the runner

Stop whatever supervised the runner folder before, or two supervisors fight over one runner (`systemctl --user stop
<old unit>`, or `pkill` the old supervisor script). Then **Runners > Add runner**: server URL, the registration token
(never stored by Pitcrew), a name and labels such as `ubuntu-latest:host`. An existing registered folder (it has a
`.runner` file) is adopted as it is. Pitcrew downloads `gitea-runner-<v>-linux-amd64` or `-arm64`, verifies its SHA-256
against two hosts, writes its configuration and starts it.

### The environment jobs get

The systemd user manager starts services with a minimal `PATH`. Pitcrew therefore asks your login shell
(`$SHELL -l -i -c`, 5 s timeout) for the `PATH` a terminal has and gives it to every runner at each (re)start, so `node`,
`cargo` and friends found in a terminal are found by jobs. If the shell does not answer, a fallback is used (nvm's newest
Node, `~/.cargo/bin`, `~/.local/bin`, Homebrew, system folders). To set variables for one runner, add a table to
`pitcrew.toml` (validated: at most 32 variables, `PITCREW_*` and `GITEA_RUNNER_*` are reserved):

```toml
[[runners]]
name = "build"
# ...
[runners.env]
NODE_ENV = "ci"
PATH = "/opt/toolchain/bin:/usr/bin:/bin"   # replaces the login shell's PATH for this runner
```

A `runner.envs: PATH` stopgap in the runner's `config.yaml` is no longer needed.

## 4. Verify

```sh
bash scripts/linux-verify.sh
```

It checks the service program, the user unit and its state, lingering, the socket (`$XDG_RUNTIME_DIR/pitcrew/pitcrew.sock`),
the service's answer, and the end of each runner's log. Then push a commit to a repository with a job such as:

```yaml
jobs:
  hello:
    runs-on: ubuntu-latest   # or the label you registered
    steps:
      - run: uname -a && node --version
```

## Logs and files

| What | Where |
| --- | --- |
| Configuration (`pitcrew.toml`), state | `~/.local/share/Pitcrew/` (`$XDG_DATA_HOME/Pitcrew`) |
| Service log | `~/.local/share/Pitcrew/logs/service.log`; also `journalctl --user -u pitcrew.service` |
| Runner folder, log | `~/.local/share/Pitcrew/runners/<name>/`, `.../logs/runner.log` |
| Service socket | `$XDG_RUNTIME_DIR/pitcrew/pitcrew.sock` (else the data folder); only you can connect |
| Unit | `~/.config/systemd/user/pitcrew.service` |

## Removing Pitcrew

Settings > Service > **Turn off** (disables and stops the unit, removes the file; runners stop after their jobs), then
remove the package or the AppImage. The runner folders stay.

## What was verified where

See the README status table. The Linux unit text, the XDG paths, the service against the fake runner (start, graceful
SIGTERM stop of the runner's process group, the environment passed) and the Rust test suite run in a Linux container
(Debian, arm64) during development and on the `ubuntu-latest` CI jobs; the `systemctl` calls and the tray need a real
desktop session and are checked with `scripts/linux-verify.sh` on the target machine.
