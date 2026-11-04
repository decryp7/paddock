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

Open Dyno; **Launch at login** is on by default (Settings can turn it off) and the first run installs a systemd **user** unit
(`~/.config/systemd/user/dyno-agent.service`: `Restart=always`, `RestartSec=5`, `Nice=10`, `CPUWeight=20`,
`IOSchedulingClass=best-effort`, `IOSchedulingPriority=7`, `MemoryHigh=256M`) and enables it with `systemctl --user enable --now`. No root is needed. To keep the
agent measuring when nobody is logged in, run `loginctl enable-linger $USER` once; that is your decision, Dyno never runs it.

Without systemd (or when the unit is not installed) the app starts the agent itself and it stops with your session.

The tray needs a status-notifier host (GNOME: the AppIndicator extension, package `gnome-shell-extension-appindicator`). Dyno reads the host from the session bus (`org.kde.StatusNotifierWatcher`, `IsStatusNotifierHostRegistered`) at the start and every 2 seconds after: without one, Settings > Menu bar and the status bar say what to turn on, a start at login opens the window, and closing the window quits the app; when a host appears the icons are made again and the note goes. The log lines start with `tray host:`.

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
- Process details from `/proc` (command line, working directory, open files, autostart origin) for your own processes. With
  `hidepid=0` system processes are listed one by one and other accounts' processes are one **Other users** row; both show name, owner and CPU, and their path and disk I/O are *Unavailable* (see "What Dyno can read without root" below).
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

Dyno installs no helper and no system service unless you turn on Administrator readings (the `.deb` ships `dyno-helper.service` disabled, with a polkit rule; the AppImage has no helper). With `hidepid=0` (the default) `/proc` gives CPU and owner for every
process (memory for other accounts' processes is resident memory only, which overstates shared memory, so it says *Unavailable* with that reason); disk I/O and the program path need ptrace rights, so for system processes and other accounts they say *Unavailable: Linux
shows this only for your own processes*. System processes (uid below `UID_MIN` in `/etc/login.defs`, default 1000) are listed one
by one; other accounts' processes are one **Other users** row with the number of processes and CPU. With `hidepid=1` or `2` the
list says in one line that the system hides other accounts' processes. DMI memory details need root and say so.

Dyno 1.0 runs only with your own rights; elevation is planned for 1.1 (ADR 0022).

## Benchmarks (ADR 0030, work package B2)

The benchmark runner (`dyno-bench`) is started on a click, never on its own. On Linux with a systemd user manager it runs as a
named transient unit, outside the agent's cgroup and nice value, so its result is the machine's speed and not the agent's
lowered share:

```
systemd-run --user --unit=dyno-bench-<run id>-<kind> --collect --wait --pipe --service-type=exec -- dyno-bench <kind> ... --wait-go
```

- Cancel, "Not responding" Cancel, a safety stop (the computer switched to battery without consent) and the agent's shutdown all
  run `systemctl --user stop dyno-bench-<run id>-<kind>.service`; killing the `systemd-run` client would leave the unit running.
- Before the runner measures, the agent writes the unit's name and `InvocationID` to `bench-units.json` beside the benchmark
  history and then writes `go` on the runner's stdin. On its next start the agent stops a `dyno-bench-*` unit only if a record
  has its name and its InvocationID (rule 53: a reused name could be someone else's unit); any other is left alone and logged.
- Without a user manager the runner is an ordinary supervised child of the agent, and the run records "limited: nice 10, memory
  limit 256 MiB" and is shown as low confidence.

Verified on a real systemd (Ubuntu 24.04 arm64, systemd user manager, OrbStack test machine `paddock-gate-arm`, 2026-11-03; the
agent side was run at nice 10 to show the unit does not inherit it):

| Check | Result |
|---|---|
| Nice value and memory limit of the unit | `Nice=0`, `MemoryHigh=infinity`, `MemoryMax=infinity` (shown by `systemctl show`); the runner sees `nice 0` and `memory.high = max` from inside |
| `--pipe`, stdin `go` and the InvocationID | The runner printed nothing before `go`; `go` written to the client's stdin reached it; `systemctl --user show -p InvocationID --value` returned a 32-digit id while the unit ran |
| Cancel on an all-core burner (8 busy loops) | `systemctl --user stop` took 7 ms by hand; `ProcessLauncher::run` returned in under 2 s after the decision; no process left, unit inactive |
| Restart cleanup | A recorded leftover unit was stopped; a unit with the same prefix and a foreign InvocationID stayed running; the records were forgotten |

These are `crates/dyno-agent/tests/bench_units.rs` (a scripted runner, no benchmark work). They are `#[ignore]`d in a plain
`cargo test` and FAIL without a user manager (never a silent pass). Run them with `scripts/linux-units-check.sh` (a release
checklist step, `docs/releasing.md`), which is `cargo test -p dyno-agent --test bench_units -- --ignored --test-threads=1` on
`paddock-gate-arm`. On a headless machine start the manager first
(`sudo loginctl enable-linger $USER`, then `XDG_RUNTIME_DIR=/run/user/$(id -u)`). Unit tests of the pure parts (command
lines, parsers, cleanup decisions) are in `dyno_core::bench::unit`, and the agent's logic runs against a scripted child.
`LINUX_UNITS_VERIFIED` in `bench.rs` switches the unit mode off in one place if it ever has to be.

## Removing Dyno

Turn off Launch at login (or `dyno-agent service uninstall`), `sudo apt remove dyno`, delete the data folders above.

## What was verified where

The Linux adapters are tested in a Linux container (root and uid 1000) and on the `ubuntu-latest` CI job. The `systemctl`
calls, the tray and the bundles have **not** been verified on a real Linux desktop yet; use the table in
[testing-manual.md](testing-manual.md) there.
