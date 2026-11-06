# Manual tests

What automated tests cannot cover: a real desktop, a real OS service manager, real hardware. Run these before a release on
each OS you ship, with the release build. Use dummy
programs (`yes > /dev/null`, `ping -t`, `sleep 1000`) as targets.

Record: date, OS and version, Dyno version, load average at the time, result per row.

## Install and start

| # | Steps | Expected |
| --- | --- | --- |
| 1 | Install from the release artifact; open Dyno | Splash, then the dashboard within a few seconds; no blank window |
| 2 | Quit; `dyno-agent status` (Windows: `"C:\Program Files\Dyno\dyno.exe" agent status`; the agent program has no console and `dyno` is not on PATH) | Either "running" (agent kept measuring) or "No agent is running." with a clear next step |
| 3 | Open Dyno again with no agent | The app starts one; tiles fill in |
| 4 | Settings > Start Dyno at login on, log out and in | Dyno is in the tray; `dyno-agent status` (Windows: `"C:\Program Files\Dyno\dyno.exe" agent status`) says running; history has no gap longer than the logout |
| 5 | Settings > Start Dyno at login off | The login registration is removed (macOS Login Items, `systemctl --user is-enabled dyno-agent`, the Run value) |
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
| 21 | macOS: `launchctl print gui/$(id -u)/net.decryptology.dyno.agent` after Start Dyno at login (the bundled `dyno-agent service status` refuses inside the app on macOS: it shares this launchd label with the `SMAppService` agent). Linux: `dyno-agent service status`. Windows: `reg query HKCU\Software\Microsoft\Windows\CurrentVersion\Run /v Dyno` and `"C:\Program Files\Dyno\dyno.exe" agent service status` | macOS: `state = running`; Linux: "installed, running"; Windows: the value `Dyno` is `"C:\Program Files\Dyno\dyno-app.exe" --hidden` (the app starts at logon and starts the agent), and `"C:\Program Files\Dyno\dyno.exe" agent service status` says not installed. On macOS 26 Login Items shows two Dyno rows, both ours: one under "Open at Login" (the app) and one under "Allow in the Background" (the agent); no "dyno-app" row and no "Item from unidentified developer" |
| 22 | Install a newer build over the old one | The agent restarts into the new build; the app reconnects; no manual step (macOS: re-registration happens by itself) |
| 23 | `kill` the agent | It comes back within seconds (macOS, Linux); on Windows the app starts it |
| 24 | Install and update in a throwaway VM | The plan [below](#install-and-update-in-a-throwaway-vm), rows 24a to 24o |

### Install and update in a throwaway VM

Local only, never in CI. Run it before tagging, on the build the tag would ship. Builds happen on the host; the VM only
installs and runs them.

**Machines.** A fresh standard-user (non-administrator) VM per run, reverted to its clean snapshot afterwards: macOS in
Tart (a clone of the base image), Windows in the unactivated Windows test VM (no licence is needed to run the tests; the
desktop watermark is expected). The Windows snapshot also holds a second standard account (for 24m) and the VM's
administrator account (for 24l), each signed in once before the snapshot was taken. Linux is covered by the `.deb` and AppImage checks in rows 1 to 20 and is not part of this plan; its helper is checked on a Linux VM ([helper-verification.md](helper-verification.md)).

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
- The **CI-built signed `<version>`**, the build the tag would ship (the installers of the manual `dyno-release` run with `gates: true` on the commit to be tagged) carries
  the real public key and the public feed baked in, so it cannot read a local feed or accept a test signature. It is used
  for the install rows only (24a, 24b, 24j, 24m, 24k, 24h, 24l); the update rows (24c to 24g) start from `1.0.0-test`, which is built with the
  same feature flags and sidecars but not CI's bundle checks (`check-bundle.sh`, `macos-verify`), which run only in CI on the real build.

| # | Steps | Expected |
| --- | --- | --- |
| 24a | Windows VM, standard user: run the CI-built `Dyno_<version>_x64-setup.exe` (the version the tag would ship) | One User Account Control prompt asks for an administrator; Dyno installs once for every user in `C:\Program Files\Dyno\` and starts as you; SmartScreen shows the documented "More info > Run anyway" path |
| 24b | macOS VM, standard user: open the CI-built `.dmg`, drag Dyno to Applications, open it, then Done > Privacy & Security > Open Anyway > Open Anyway | An administrator name and password sheet appears (macOS 26); Dyno starts after it; the agent is registered (`launchctl print gui/$(id -u)/net.decryptology.dyno.agent`: running) |
| 24j | Right after 24a and 24b, on each VM: Settings > Dyno > Administrator readings > turn the helper on. Linux is not in this plan: check the helper on a Linux VM by the steps of [helper-verification.md](helper-verification.md) | One OS prompt (Windows User Account Control, macOS password or Touch ID, Linux polkit); the card says the helper runs; Processes lists every account's processes; the helper's audit log lists the calls |
| 24m | Windows, before 24k: sign out, sign in as a second standard user, open Dyno | One per-machine install serves both: the second account gets its own agent, its own data folder and its own `Dyno` value under its `HKCU\...\Run`; the helper answers it only as its policy allows (the owner and administrators in full, other accounts scoped, owner-only requests such as the audit log refused) |
| 24k | Turn the helper off on the same card | One OS prompt; the helper is stopped and removed (Windows: no `DynoHelper` service and no `C:\Program Files\Dyno Helper` folder; macOS: no `net.decryptology.dyno.helper` in `launchctl print system`; Linux: `/usr/lib/systemd/system/dyno-helper.service` stays, as the `.deb` ships it, but `systemctl is-enabled` says disabled and `is-active` says inactive); figures that need it say *Unavailable* again |
| 24h | Uninstall on each VM (Windows: Apps > Dyno > Uninstall; macOS: quit Dyno, turn off Settings > Start Dyno at login, drag the app to the Bin; do not run `dyno-agent service uninstall`, it refuses inside the app on macOS) | The agent process is gone; Windows: the uninstaller asks for an administrator once; checked in the standard user's own hive (the uninstaller runs elevated as the administrator), `reg query HKCU\Software\Microsoft\Windows\CurrentVersion\Run` has no `Dyno` and no `Dyno agent` value; no `C:\Program Files\Dyno\` folder; macOS: no Login Items entry (neither the "Open at Login" nor the "Allow in the Background" row); history kept or removed as the uninstaller says. The uninstaller clears only the hives that are loaded, and the second account of 24m is signed out during the uninstall: sign in as that account afterwards and check its `reg query HKCU\Software\Microsoft\Windows\CurrentVersion\Run /v Dyno` too; a `Dyno` value left there is recorded as a known gap. Known gap (macOS): with the helper still on, deleting the app leaves `/Library/PrivilegedHelperTools/net.decryptology.dyno.helper` and its launchd job; the row passes only after 24k turned the helper off, and a leftover is recorded as the known gap |
| 24c | Revert each VM to its clean snapshot (24a to 24h leave traces). Install `1.0.0-test`, turn on update checks, let the dashboard fill for a minute, note the agent's process id | History is recording; About reads `1.0.0-test` |
| 24d | Windows: with the feed running, Settings > Updates > Check for updates, then Install; answer the prompt with an administrator's name and password. Then reset to the `1.0.0-test` snapshot, repeat and decline the prompt | One Windows permission prompt (a standard user types an administrator's name and password); the NSIS installer then runs **passively** (progress only, no wizard pages); Dyno restarts on `1.0.1-test` (About); the earlier history is still there. Declined: Dyno starts again on `1.0.0-test`, says the update did not install and offers **Install again** |
| 24e | macOS: the same on the Tart VM | The `.app.tar.gz` is downloaded, verified and swapped into place; the app relaunches on `1.0.1-test`; `codesign --verify --deep` still passes on the new bundle; no second copy is left behind |
| 24f | After 24d and 24e: `dyno status` (Windows: `"C:\Program Files\Dyno\dyno.exe"` `status`), the process list of the agent | The agent runs from the new build (new process id; `dyno-agent --version`, on Windows `"C:\Program Files\Dyno\dyno.exe"` `agent --version`, shows the new version), the app reconnected by itself, nothing was started by hand; kill the agent: it comes back within seconds (macOS) or the app starts it (Windows) |
| 24g | Reset the VM to the `1.0.0-test` snapshot. Corrupt one byte of the `.sig` in the feed folder (or point `latest.json` at a `.sig` of another version), then check and install | The update is refused with a signature message; the installed version, the agent and the history are untouched. Put the good `.sig` back: the update installs. Repeat once with a `latest.json` that names the `1.0.1-test` bundle but a `.sig` made with a different test key |
| 24l | Windows, before the tag, the 1.0.0 offer without a published feed. First the administrator case: revert to the clean snapshot, sign in as the VM's administrator account, install the published per-user `Dyno_1.0.0_x64-setup.exe`, quit it (tray > Quit), run `Dyno_<version>_x64-setup.exe /UPDATE /P /R` (the arguments the updater passes) and accept the prompt. Then revert again and, in one standard-user session: install 1.0.0, let it record for a few minutes, quit it (tray > Quit), run the same command and answer the prompt with an administrator's name and password; then uninstall 1.0.0 (Apps > Dyno) with **Delete the application data** unchecked and run `Dyno_<version>_x64-setup.exe` | Administrator case: the User Account Control prompt, then "Uninstall the older Dyno first (Settings > Apps), then run this installer again." with no name, and 1.0.0 starts again. Standard user: the prompt, then the same message followed by " It is installed for <name>.", and 1.0.0 starts again. After the manual uninstall and install: one User Account Control prompt, Dyno in `C:\Program Files\Dyno\`, the 1.0.0 history still there (the data folder is per user and kept); `reg query HKCU\Software\Microsoft\Windows\CurrentVersion\Run` has one `Dyno` value naming `dyno-app.exe --hidden`, no `Dyno agent` value and no 1.0.0-style `Dyno` = `dyno-agent.exe run` |
| 24i | Revert both VMs. Check the host: no test key in the repository or the worktree, no test bundle in a release folder, the feed server stopped | Nothing to publish or clean up; record date, VM image, macOS and Windows versions, and the result per row |
| 24n | After publishing, on a Mac with the published 1.0.0 (not a VM test build): Settings > Updates > Check for updates against the public feed | 1.0.0 on macOS reads the same feed. Record what it does: whether it offers 1.1.0, whether the install completes, and whether Dyno restarts on 1.1.0 with the history kept and the agent re-registered; if it fails, record the message and check that the manual steps (replace the app from the `.dmg`) work |
| 24o | After publishing, on a Windows PC or VM with the published 1.0.0: Settings > Updates > Check for updates against the public feed, accept | 1.0.0 offers 1.1.0; accepting shows the User Account Control prompt, then the message of 24l (with " It is installed for <name>." for a standard user who typed an administrator's credentials, without it for an administrator's own account), and 1.0.0 starts again; it records no failure and offers the update again at the next check |

## Privileges

Dyno runs with the user's rights; the optional administrator helper stays off until it is turned on (rows 24j and 24k). These checks run once per OS on the release build, with a standard (non-administrator) account and the helper off.

| # | Steps | Expected |
| --- | --- | --- |
| P1 | Install as a standard user; start Dyno | Windows: one User Account Control prompt at install; Linux `.deb`: `apt` asks for root; macOS: the Open Anyway sheet of 24b. No prompt at start; the agent runs as you |
| P2 | Open Processes | Your own and the system's processes are listed one by one; other accounts' processes are one **Other users** row (a second account logged in, or fast user switching, makes it appear) |
| P3 | Look at a system process's path, owner, CPU, memory and disk (per the OS table in ADR 0022) | Every figure the OS withholds says *Unavailable* with a plain reason; none shows 0 |
| P4 | Linux: remount `/proc` with `hidepid=2` (a VM) | The list says in one line that the system hides other accounts' processes |
| P5 | Check the process owner of `dyno-agent` and the app, and look for services, LaunchDaemons and systemd units named Dyno | The agent runs as you; with the helper off, no Dyno service or daemon runs (Linux: the `.deb`'s `dyno-helper.service` is present but disabled and inactive; with the helper on, only `DynoHelper`, `net.decryptology.dyno.helper` or `dyno-helper.service` runs) |

## Power and AI access

Automated tests never hold, sleep or wake a real machine (they use fakes behind `paddock_power::PowerOs`), so these rows are by
hand, once per OS, on the release build. Use a VM or a machine you may put to sleep; note what was left running. On Windows,
`dyno` means `"C:\Program Files\Dyno\dyno.exe"`.

| # | Steps | Expected |
| --- | --- | --- |
| W1 | Power > Keep awake: 30 minutes; then Until I stop it; then while busy (`yes > /dev/null`) and while a program runs (`sleep 1000`); stop each | The OS does not sleep while held (Windows `powercfg /requests`, macOS `pmset -g assertions`, Linux `systemd-inhibit --list`); the hold ends when stopped, when the busy work ends, or when the program exits; `dyno awake status` agrees |
| W2 | `dyno awake on --for 168h`, then `dyno awake off` | Accepted (the page offers up to 4 hours, the command up to 7 days as `168h`; `7d` is refused with a usage message); off ends it |
| W3 | On a real machine (Tart guests do not sleep, wake on a timer or take Wake-on-LAN): Power > Sleep and shut down: an idle rule to sleep after 5 minutes; move the mouse, then leave the machine idle; when the countdown shows, `dyno idle snooze 5` (typing the command counts as activity, so the idle time starts again); when it shows again, Cancel; then leave it idle once more | A countdown with Cancel shows first; snooze delays it and the countdown shows again after 5 minutes; Cancel stops it; left alone, the machine sleeps by the OS's normal path |
| W4 | On a real machine (Tart guests do not sleep, wake on a timer or take Wake-on-LAN): The same rule with Hibernate (Windows, Linux); then turn the idle rule off | Windows and Linux hibernate (wake the machine afterwards); macOS does not offer it and says macOS decides |
| W5 | On a real machine, with the idle rule of W3 and W4 off: a rule to shut down at a time of day two minutes ahead; when the countdown shows, `dyno idle snooze 5`; when it shows again, Cancel; then set it again and leave it | Snooze delays the timed action and the countdown shows again 5 minutes later; Cancel ends it for today (a one-time action for good); left alone, the OS shuts down normally (no forced close). Start the machine again before the next row |
| W6 | On a real machine (Tart guests do not sleep, wake on a timer or take Wake-on-LAN): on macOS and Linux turn the helper on first (24j); Settings > Allow scheduled wake on; Power > Schedule: a wake in five minutes; sleep the machine. Then add a wake in five minutes, shut the machine down (Windows: `shutdown /s /t 0`, or turn Fast Startup off, so it is a full shutdown), and start it after that time | It wakes (macOS and Linux through the helper, which must be on; Windows through the agent's timer); a wake whose time passed while the machine was off is shown as missed and never run late |
| W7 | On a real machine (Tart guests do not sleep, wake on a timer or take Wake-on-LAN): Power > Wake devices: save a second VM or machine, test it, then wake it while it sleeps | The test says whether it answered; the wake reaches it; the check under This computer says whether this one can be woken |
| W8 | Settings > AI access on; issue a key; connect a client that starts commands (for example Claude Desktop) with `dyno mcp` and `DYNO_MCP_KEY`; call a reading tool, then `keep_awake` and `wake_device` | Readings answer; the two actions are refused until each is allowed by name, then work (keep awake at most 12 hours); every call is listed on the page; revoking the key closes the connection |

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

## Extras (WP-13)

| # | Steps | Expected |
| --- | --- | --- |
| 36 | Extras > Sleep while a video call or music plays (macOS, Linux) | The app appears under "Keeping this computer awake" with its reason; on Windows without administrator rights the tab says only an administrator can list blockers |
| 37 | Extras > Boot after a restart and after a second one | Two rows; the verdict appears from the fourth boot on. Linux: `systemd-analyze blame` entries; Windows: slow apps from the Diagnostics-Performance log |
| 38 | Extras > Files, choose a folder with two identical large files | Both listed as one duplicate group; nothing can be removed from Dyno |
| 40 | Extras > Inventory > Export report, open the file | Hardware and software lists; nothing else |
| 41 | Extras > Maintenance: a window that is closed now, wait/inspect | "Next window opens ..."; no log read starts outside it; Rescan on Issues still works |

