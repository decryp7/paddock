# Dyno on Linux: build, install, verify

Dyno targets desktop Linux with systemd (x86-64 and ARM64): GNOME, KDE and others with WebKitGTK 4.1. It is a first-class
platform next to Windows and macOS.

## 1. Build

```sh
sudo apt-get install libwebkit2gtk-4.1-dev libgtk-3-dev librsvg2-dev libayatana-appindicator3-dev patchelf file xdg-utils
cd app && npm ci
npx tauri build --bundles deb,appimage -- --profile dist
bash ../scripts/check-bundle.sh ../target/dist/bundle      # run paths and that dyno-agent and dyno are packaged
```

The `.deb` depends on `libwebkit2gtk-4.1-0`, `libgtk-3-0` and `libayatana-appindicator3-1`. The AppImage bundler needs FUSE or
`APPIMAGE_EXTRACT_AND_RUN=1` (CI sets it).

## 2. Install

```sh
sudo apt install ./Dyno_<version>_amd64.deb      # or run the AppImage
```

Open Dyno; in **Settings** turn on **Launch at login**: Dyno installs a systemd **user** unit
(`~/.config/systemd/user/dyno-agent.service`: `Restart=always`, `RestartSec=5`, `Nice=10`, `CPUWeight=20`,
`IOSchedulingClass=idle`, `MemoryHigh=256M`) and enables it with `systemctl --user enable --now`. No root is needed. To keep the
agent measuring when nobody is logged in, run `loginctl enable-linger $USER` once; that is your decision, Dyno never runs it.

Without systemd (or when the unit is not installed) the app starts the agent itself and it stops with your session.

The tray needs a status-notifier host (GNOME: the AppIndicator extension). Without a tray, closing the window quits the app.

## 3. Verify

```sh
dyno-agent status                                   # running: pid …, version …, protocol …
dyno-agent service status                           # installed, running
systemctl --user status dyno-agent                  # active (running)
journalctl --user -u dyno-agent -f                  # the agent's output
dyno-agent once | head -30                          # one measurement as JSON
dyno-agent bench 25 processes                       # Dyno's own CPU (share of ONE core) and memory
```

### Verifying the Linux adapters on any machine

`scripts/linux-verify.sh` builds and tests `dyno-platform` in a native Linux container (OrbStack or Docker) against a real
`/proc` and `/sys`: tests and `clippy -D warnings` as root, then the tests again as uid 1000 (which exercises the "other users'
processes are out of sight" paths), then the portable parsers natively on the host. `scripts/linux-container.sh <command>` runs
any command in the same container; `--clean-volumes` deletes its two named volumes. CI runs the same tests on `ubuntu-latest`.

## What Dyno can see and do

- Sensors: sysinfo plus PSI (`/proc/pressure`), `/proc/diskstats`, hwmon temperatures and fans, GPU where exposed.
- Process details from `/proc` (command line, working directory, open files, autostart origin); other users' processes are out
  of sight without root.
- Network traffic per program: cumulative TCP bytes per socket from the kernel's `sock_diag` (`tcp_info`, kernel 4.1 or later, any
  user may ask) matched to programs through `/proc/<pid>/fd`. Other users' programs are not attributed without root; UDP has no
  byte counters; bytes of a connection that closed between two reads are not counted. Without netlink access the Network page says so.
- Startup list: XDG autostart entries and systemd user units.
- On Linux sysinfo lists threads as processes; Dyno filters them and never signals a thread id as a process.

## After updating Dyno

Install the new package; the app asks the old agent to exit and `Restart=always` starts the new one. If the unit points at a
removed AppImage path, `dyno-agent service ensure` rewrites it.

## Logs and files

`~/.local/share/dyno` (`dyno.db`, `settings.json`, `agent.json`, `logs/app.log`), the agent's output in the user journal.

## What Dyno can read without root

Dyno 1.0 installs no helper and no system service. With `hidepid=0` (the default) `/proc` gives CPU and owner for every
process (memory for other accounts' processes is resident memory only, which overstates shared memory, so it says *Unavailable* with that reason); disk I/O and the program path need ptrace rights, so for system processes and other accounts they say *Unavailable: Linux
shows this only for your own processes*. System processes (uid below `UID_MIN` in `/etc/login.defs`, default 1000) are listed one
by one; other accounts' processes are one **Other users** row with the number of processes and CPU. With `hidepid=1` or `2` the
list says in one line that the system hides other accounts' processes. DMI memory details need root and say so.

**Run as administrator** (session only) asks for your password through polkit (`pkexec`), which needs an authentication agent in
your session, and restarts the agent as root until you quit Dyno. Nothing is installed.

## Removing Dyno

Turn off Launch at login (or `dyno-agent service uninstall`), `sudo apt remove dyno`, delete the data folders above.

## What was verified where

The Linux adapters are tested in a Linux container (root and uid 1000) and on the `ubuntu-latest` CI job. The `systemctl`
calls, the tray and the bundles have **not** been verified on a real Linux desktop yet; use the table in
[testing-manual.md](testing-manual.md) there.
