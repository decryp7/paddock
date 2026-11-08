# Pitwall on Linux

Pitwall on Linux is the app (tray icon and window) plus the Pitwall server running as a **systemd user service**. It needs no
root rights.

> **Not yet verified on a real Linux machine.** The Linux path is covered by unit tests and the fake-engine end-to-end tests,
> and container tests are being done by the shell work; CUDA and Vulkan builds have not been run. Treat this page as the intended
> behaviour and run the [manual tests](testing-manual.md) to confirm it. The Tauri app is not in the repository yet; the server
> and API exist and can be run with `pitwall-server`.

## Install

- **`.deb`** (Debian, Ubuntu): `sudo apt install ./pitwall_<version>_amd64.deb`.
- **AppImage:** `chmod +x Pitwall_<version>_amd64.AppImage` and run it. The server must not run from the AppImage's temporary
  mount (it disappears when the app closes), so the unit that the app installs points at a stable copy of the program.
- Builds are unsigned until the signing secrets exist ([releasing](releasing.md)).
- **Requirements:** a desktop with WebKitGTK for the window; for the tray an AppIndicator-capable shell (GNOME needs the
  AppIndicator extension). With no tray, closing the window quits the app (the service keeps serving).

## The server as a systemd user service

- Unit **`pitwall.service`** in `$XDG_CONFIG_HOME/systemd/user` (default `~/.config/systemd/user`), written by the app:

  ```ini
  [Unit]
  Description=Pitwall local LLM server
  After=network-online.target
  Wants=network-online.target

  [Service]
  Type=simple
  ExecStart="/path/to/pitwall-server"
  WorkingDirectory="/path/to"
  Environment=PITWALL_SERVICE=1
  Restart=always
  RestartSec=10
  TimeoutStopSec=60

  [Install]
  WantedBy=default.target
  ```

- `Restart=always` brings the server back after a crash and after it exits itself following an update (only when idle), but
  not after `systemctl --user stop pitwall`.
- It runs while you are logged in. To run with nobody logged in, enable lingering yourself: `loginctl enable-linger $USER`
  (Pitwall does not do it for you).
- The server asks your login shell for `PATH` (systemd's is minimal) and uses it for llama.cpp.
- Commands: `systemctl --user status pitwall.service`, `journalctl --user -u pitwall.service`, `systemctl --user restart
  pitwall.service`. Logs are also in `~/.local/share/pitwall/logs/`.

## Files

Data folder: **`$XDG_DATA_HOME/pitwall`**, else **`~/.local/share/pitwall`** (`PITWALL_HOME` overrides it).

| Item | What |
|---|---|
| `settings.toml` | Settings (with a backup copy) |
| `apps.json` | Apps and the hashes of their keys |
| `admin.key` | The secret the window uses (mode 0600; the data folder is made 0700, and the log says so when it tightens it) |
| `discovery.json` | pid, port and base URL while the server runs |
| `models/<owner>/<repo>/` | Downloaded GGUF models |
| `engines/<build>/` | Installed `llama-server` builds (`cpu`, `vulkan`, `cuda-12`, `cuda-13`, ...) |
| `partial/` | Unfinished downloads |
| `packs/` | Your model catalog packs |
| `logs/` | Service logs, no prompts or responses |

**In a container.** The data folder holds `admin.key`, so it must belong to the user the server runs as: the server makes
it 0700 and refuses a data folder or `admin.key` owned by another user (a volume mounted with another uid, or `root`
from the image). Run the container as the owner of the volume (`--user "$(id -u):$(id -g)"`), or point `PITWALL_HOME` at a
folder that user owns. A refused key is logged with the path and the reason; the server then keeps its new key in memory
only, and the window cannot connect until the folder is fixed.


## llama.cpp and GPUs

- **CPU** works everywhere. **Vulkan** serves NVIDIA, AMD and Intel GPUs with the vendor's Vulkan driver. **CUDA 12 or 13**
  needs an NVIDIA GPU and a driver new enough for that CUDA version. **ROCm**, **SYCL** and **OpenVINO** builds exist in the
  catalog for x64 and can be chosen by hand; they are never chosen automatically.
- Pitwall reads NVIDIA memory with `nvidia-smi`; with no GPU information the fit check assumes the CPU.
- The suggested build is the first that matches the hardware; the self-test (`--list-devices`) shows what the build can really
  use, and software renderers such as llvmpipe are not counted as GPUs. A build that fails its self-test is discarded and the
  previous one stays.
- Linux arm64 has CPU, Vulkan and CUDA 13 builds in the catalog (untested).

## Containers and headless machines

For a headless server run `pitwall-server --data-dir /var/lib/pitwall-data` under your own unit or container; it still binds
`127.0.0.1` only (there is no network sharing yet), so reach it through an SSH tunnel (`ssh -L 11435:127.0.0.1:11435 host`).
Use the CPU build in a container without GPU access.

## Uninstall

`systemctl --user disable --now pitwall.service`, remove `~/.config/systemd/user/pitwall.service`, `sudo apt remove pitwall`
(or delete the AppImage), and optionally delete `~/.local/share/pitwall` (models, keys, settings) and the credential-store
entries named Pitwall.

## Verified and not verified

Nothing on Linux has been run for real yet. The code that is platform-specific (paths, the unit text, quoting, PATH merging) is
pure and unit tested; the Linux container tests are in progress.
