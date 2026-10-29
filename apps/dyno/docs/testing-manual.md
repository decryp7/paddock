# Manual tests

What automated tests cannot cover: a real desktop, a real OS service manager, real hardware. Run these before a release on
each OS you ship, with the release build. Dyno only watches, so nothing here changes the machine; use dummy
programs (`yes > /dev/null`, `ping -t`, `sleep 1000`) as targets.

Record: date, OS and version, Dyno version, load average at the time, result per row.

## Install and start

| # | Steps | Expected |
| --- | --- | --- |
| 1 | Install from the release artifact; open Dyno | Splash, then the dashboard within a few seconds; no blank window |
| 2 | Quit; `dyno-agent status` | Either "running" (agent kept measuring) or "No agent is running." with a clear next step |
| 3 | Open Dyno again with no agent | The app starts one; tiles fill in |
| 4 | Settings > Launch at login on, log out and in | Dyno is in the tray; `dyno-agent status` says running; history has no gap longer than the logout |
| 5 | Settings > Launch at login off | The login registration is removed (macOS Login Items, `systemctl --user is-enabled dyno-agent`, the Run value) |
| 6 | Delete `dyno.db` while stopped, start | History starts again; an alert or note says so |

## Live data

| # | Steps | Expected |
| --- | --- | --- |
| 7 | Run a CPU burner (`yes > /dev/null`) | CPU tile and the app's row rise within a few seconds; stop it and they fall |
| 8 | Unplug the power (laptop) | The battery tile shows; Dyno's intervals double (Dyno CPU drops) |
| 9 | Desktop without a battery or sensor | The tile says why ("no battery"), never zero |
| 10 | Leave the window closed for 10 minutes, reopen | History has a continuous line for the gap |
| 11 | Watch "Dyno: x % CPU, y MB" with the dashboard open for 5 minutes | Stays near the targets in the roadmap; note the load average |

## Processes

| # | Steps | Expected |
| --- | --- | --- |
| 12 | Start `sleep 1000` (or `ping -t`); find it in Processes | Grouped under its parent, with details; no action on it is offered |

## Service and updates

| # | Steps | Expected |
| --- | --- | --- |
| 21 | `dyno-agent service status` after Launch at login | "installed, running" |
| 22 | Install a newer build over the old one | The agent restarts into the new build; the app reconnects; no manual step (macOS: re-registration happens by itself) |
| 23 | `kill` the agent | It comes back within seconds (macOS, Linux); on Windows the app starts it |
| 24 | Install and update in a throwaway VM | The plan [below](#install-and-update-in-a-throwaway-vm), rows 24a to 24i |

### Install and update in a throwaway VM

Local only, never in CI. Run it before tagging, on the build the tag would ship. Builds happen on the host; the VM only
installs and runs them.

**Machines.** A fresh standard-user (non-administrator) VM per run, reverted to its clean snapshot afterwards: macOS in
Tart (a clone of the base image), Windows in the unactivated Windows test VM (no licence is needed to run the tests; the
desktop watermark is expected). Linux is covered by the `.deb` and AppImage checks in rows 1 to 20 and is not part of this plan.

**Keys.** Test keys only. Generate a throwaway pair on the host (`npx tauri signer generate -w <scratch>/dyno-test.key`, a
throwaway password). The real family key (`PADDOCK_TAURI_*`) never leaves CI: it is not on the host, in a VM, or in a
shared folder. Test bundles are never uploaded, tagged or published (ER-15); delete them with the VM clone.

**Builds, on the host** (the same `tauri build` as CI, plus `--config` to swap the key and the feed; the committed
`tauri.conf.json` is not edited):

- `1.0.0-test` and `1.0.1-test`: in a scratch worktree only, set the workspace `Cargo.toml` and `app/src-tauri/tauri.conf.json` to `1.0.0-test`, build, then to `1.0.1-test` and build again (and `cargo update -w` the lock). The agent, the `dyno` command and the app must all carry the version: the app replaces a running agent only when its own version differs from the agent's (`link.rs` `plan()`), so a `--config` version override, which leaves the crates at the workspace version, would give two builds with the same agent and rows 24d and 24f could not pass. `createUpdaterArtifacts` on, `pubkey` = the
  test public key, `plugins.updater.endpoints` = the local feed URL (an `http://` URL on the host needs
  `plugins.updater.dangerousInsecureTransportProtocol: true` in the same override; fine for a test build, never for a release).
- **Windows test bundles (NSIS), built on the Mac** (tried on the Mac mini on 2026-10-29 at the workspace version, with a `version` override: it produced `Dyno_0.1.0_x64-setup.exe`, 9.7 MiB, unsigned. The `1.0.0-test` and `1.0.1-test` builds described here, with the crate versions bumped, are **untried**, and no installer was run in the VM yet). Needs `rustup target add x86_64-pc-windows-msvc`, `cargo install cargo-xwin` (downloads the Windows SDK and
  CRT on first use), `brew install makensis llvm`, and a `mt.exe` on `PATH` (the linker asks for the manifest tool; a symlink
  to `llvm-mt` does it: `mkdir -p <scratch>/shim && ln -s /opt/homebrew/opt/llvm/bin/llvm-mt <scratch>/shim/mt.exe`, a folder in your scratch area, never in the repository; put it on `PATH` only inside the subshell that builds). The plain `cargo` in
  `sidecar.mjs` cannot link for Windows, so build the sidecars with xwin and copy them, then run Tauri without that step:

  ```sh
  (
  export PATH=<scratch>/shim:/opt/homebrew/opt/llvm/bin:$PATH
  cd apps/dyno
  export CARGO_TARGET_DIR=<scratch build dir> CARGO_BUILD_JOBS=2
  cargo xwin build -p dyno-agent -p dyno-cli --profile dist --target x86_64-pc-windows-msvc
  for b in dyno-agent dyno; do cp $CARGO_TARGET_DIR/x86_64-pc-windows-msvc/dist/$b.exe app/src-tauri/binaries/$b-x86_64-pc-windows-msvc.exe; done
  cd app && npx tauri build --runner cargo-xwin --target x86_64-pc-windows-msvc --bundles nsis \
    --config '{"build":{"beforeBuildCommand":"npm run build && node ../../../conformance/ci/check-index.mjs dist/dyno-app/browser/index.html"},"plugins":{"updater":{"pubkey":"<test pubkey>","endpoints":["http://<host-only IP>:8089/latest.json"],"dangerousInsecureTransportProtocol":true}},"bundle":{"createUpdaterArtifacts":true}}'
  )
  ```

  with `TAURI_SIGNING_PRIVATE_KEY` / `_PASSWORD` set to the throwaway key (never the family key). The installer lands in
  `$CARGO_TARGET_DIR/x86_64-pc-windows-msvc/release/bundle/nsis/Dyno_<version>_x64-setup.exe`; copy it (and the `.sig`) to the
  feed folder. It is not Authenticode-signed (expected: SmartScreen applies as for the release). If the cross build fails on your Mac, build in the Windows
  test VM instead (Git for Windows, Node from `.nvmrc`, rustup with the msvc toolchain, Visual Studio Build Tools with the
  "Desktop development with C++" workload and a Windows SDK, NSIS from Tauri's own download), with the same commands in
  PowerShell and only the throwaway key copied in.
- `latest.json` for the feed is written by `conformance/release/latest-json.mjs` from the 1.0.1-test installers and their
  `.sig` files; serve the folder from the host with `python3 -m http.server 8089 --bind <host-only IP> --directory <feed folder>` (the address of the VM network's host side, for example Tart's `192.168.64.1`, never `0.0.0.0`; stop it when the run ends). Share bundles
  into the VM through a read-only folder, not a copy of the repository.
- The **CI-built signed 1.0.0** (the installers of the manual `dyno-release` run with `gates: true` on the commit to be tagged) carries
  the real public key and the public feed baked in, so it cannot read a local feed or accept a test signature. It is used
  for the install rows only (24a, 24b, 24h); the update rows (24c to 24g) start from `1.0.0-test`, which is built with the
  same feature flags and sidecars but not CI's bundle checks (`check-bundle.sh`, `macos-verify`), which run only in CI on the real build.

| # | Steps | Expected |
| --- | --- | --- |
| 24a | Windows VM, standard user: run the CI-built `Dyno_1.0.0_x64-setup.exe` | Installs per user with no administrator prompt; Dyno starts; SmartScreen shows the documented "More info > Run anyway" path |
| 24b | macOS VM, standard user: open the CI-built `.dmg`, drag Dyno to Applications, right-click > Open once | Starts after the Open Anyway step; the agent is registered (`dyno-agent service status`: installed, running); no administrator prompt |
| 24c | Revert each VM to its clean snapshot (24a and 24b left Dyno installed). Install `1.0.0-test`, turn on update checks, let the dashboard fill for a minute, note the agent's process id | History is recording; About reads `1.0.0-test` |
| 24d | Windows: with the feed running, Settings > Updates > Check for updates, then Install | The NSIS installer runs **passively** (progress only, no wizard pages, no prompt); Dyno restarts on `1.0.1-test` (About); the earlier history is still there |
| 24e | macOS: the same on the Tart VM | The `.app.tar.gz` is downloaded, verified and swapped into place; the app relaunches on `1.0.1-test`; `codesign --verify --deep` still passes on the new bundle; no second copy is left behind |
| 24f | After 24d and 24e: `dyno status`, the process list of the agent | The agent runs from the new build (new process id; `dyno-agent --version` shows the new version), the app reconnected by itself, nothing was started by hand; kill the agent: it comes back within seconds (macOS) or the app starts it (Windows) |
| 24g | Reset the VM to the `1.0.0-test` snapshot. Corrupt one byte of the `.sig` in the feed folder (or point `latest.json` at a `.sig` of another version), then check and install | The update is refused with a signature message; the installed version, the agent and the history are untouched. Put the good `.sig` back: the update installs. Repeat once with a `latest.json` that names the `1.0.1-test` bundle but a `.sig` made with a different test key |
| 24h | Uninstall on each VM (Windows: Apps > Dyno > Uninstall; macOS: quit Dyno, then `dyno-agent service uninstall` if offered, drag the app to the Bin) | The agent process is gone; Windows: no `Dyno` entry under `HKCU\...\Run`, no folder under `%LOCALAPPDATA%\Dyno` program files; macOS: no `~/Library/LaunchAgents/net.decryptology.dyno.agent.plist` or Login Items entry; history kept or removed as the uninstaller says |
| 24i | Revert both VMs. Check the host: no test key in the repository or the worktree, no test bundle in a release folder, the feed server stopped | Nothing to publish or clean up; record date, VM image, macOS and Windows versions, and the result per row |

## Privileges

Dyno 1.0 has no privileged helper and no way to run elevated. These checks run once per OS on the release build, with a
standard (non-administrator) account.

| # | Steps | Expected |
| --- | --- | --- |
| P1 | Install as a standard user; start Dyno | No administrator prompt appears at install or at start (Linux `.deb`: only `apt` asks for root); the agent runs as you |
| P2 | Open Processes | Your own and the system's processes are listed one by one; other accounts' processes are one **Other users** row (a second account logged in, or fast user switching, makes it appear) |
| P3 | Look at a system process's path, owner, CPU, memory and disk (per the OS table in ADR 0022) | Every figure the OS withholds says *Unavailable* with a plain reason; none shows 0 |
| P4 | Linux: remount `/proc` with `hidepid=2` (a VM) | The list says in one line that the system hides other accounts' processes |
| P5 | Check the process owner of `dyno-agent` and the app, and look for services, LaunchDaemons and systemd units named Dyno | The agent runs as you; no service or daemon exists |

## Window, tray, native feel

| # | Steps | Expected |
| --- | --- | --- |
| 25 | Close the window; tray Open | A fresh window, not blank; macOS Dock reopen works |
| 26 | Linux without a tray host, close the window | The app quits |
| 27 | Right-click chrome; press F5, Ctrl +/- , Ctrl+P; select text in a path | No browser menu, no reload, no zoom, no print; the path can be selected and copied; Copy only inside the selection |
| 28 | Drop a file on the window | Nothing navigates |
| 29 | Dark and light themes, system switch | Follows the OS; no unreadable text |
| 30 | Resize to the minimum (900 x 600) | No clipped controls |

## Safety and robustness

| # | Steps | Expected |
| --- | --- | --- |
| 31 | Corrupt `settings.json` while stopped | Dyno starts with the backup or defaults and says so |
| 32 | Make the data folder read-only | Dyno keeps working with in-memory history and says so |
| 34 | Inspect the data folder permissions | `agent.json` is readable by the owner only |
| 35 | Search the logs for a file path or command line you used | None present |

## Extras and backup health (WP-13, WP-14)

| # | Steps | Expected |
| --- | --- | --- |
| 36 | Extras > Sleep while a video call or music plays (macOS, Linux) | The app appears under "Keeping this computer awake" with its reason; on Windows without administrator rights the tab says only an administrator can list blockers |
| 37 | Extras > Boot after a restart and after a second one | Two rows; the verdict appears from the fourth boot on. Linux: `systemd-analyze blame` entries; Windows: slow apps from the Diagnostics-Performance log |
| 38 | Extras > Files, choose a folder with two identical large files | Both listed as one duplicate group; nothing can be removed from Dyno |
| 40 | Extras > Inventory > Export report, open the file | Hardware and software lists; nothing else |
| 41 | Extras > Maintenance: a window that is closed now, wait/inspect | "Next window opens ..."; no log read or backup check starts outside it; Rescan on Issues still works |
| 42 | Backup health with Time Machine on (macOS) | Healthy with the last backup time; unplug the disk: no-backup/cannot-check sentence with a next step |
| 43 | Backup health on Windows with File History or Windows Backup enabled | Verify the event IDs used (4 and 201) against your PC; report mismatches |
| 44 | Install restic through Homebrew, export RESTIC_REPOSITORY and RESTIC_PASSWORD_FILE in the shell profile, restart the agent | Found through the login-shell PATH; healthy or stale |
| 45 | Set the stale limit to 1 hour with a day-old backup | One alarm "... backup is out of date"; a new backup clears it at the next check |

