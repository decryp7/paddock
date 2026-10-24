# Dyno on macOS: build, install, verify

Dyno needs macOS 13 or newer (Apple Silicon or Intel). The build is ad-hoc signed, not notarised.

## 1. Build

Prerequisites: Xcode command line tools (`xcode-select --install`), Rust stable (1.88+), Node 24.

```sh
cd app
npm ci
npx tauri build --bundles app,dmg -- --profile dist
```

`beforeBuildCommand` builds the `dyno-agent` sidecar and the UI first. The bundle is
`target/dist/bundle/macos/Dyno.app` and `target/dist/bundle/dmg/Dyno_<version>_aarch64.dmg` (or under `CARGO_TARGET_DIR`).
`tauri.macos.conf.json` sets `signingIdentity "-"` (ad-hoc seal), `hardenedRuntime false` (rule 23) and bundles
`Contents/Library/LaunchAgents/net.decryptology.dyno.agent.plist`. Set `CI=true` to stop the DMG step from scripting Finder.

```sh
codesign -dv --verbose=2 target/dist/bundle/macos/Dyno.app 2>&1 | grep -E 'Signature|Identifier'   # Signature=adhoc
bash scripts/check-bundle.sh target/dist/bundle                                                      # run paths, seal, Team IDs, sidecars start
```

## 2. Install

Open the `.dmg`, drag Dyno to Applications and open it the first time with right-click > Open (it is not notarised). In
**Settings**, turn on **Launch at login**: Dyno registers the agent with `SMAppService` and macOS may ask you to allow it under
**System Settings > General > Login Items & Extensions**. No administrator rights are needed.

Why ad-hoc signing matters: `SMAppService` and launch agents refuse a linker-signed bundle (error -67056). After an update
launchd may keep the previous code requirement and refuse the new agent ("needs LWCR update", spawn failed); Dyno detects that
and registers again by itself.

## 3. Verify

```sh
dyno-agent status                                   # running: pid …, version …, protocol …
dyno-agent service status                           # installed, running (CLI route, see below)
launchctl print gui/$(id -u)/net.decryptology.dyno.agent | head -30   # state = running
dyno-agent once | head -30                          # one measurement as JSON (changes nothing)
dyno-agent bench 25 summary                         # Dyno's own CPU (share of ONE core) and memory
tail -f ~/Library/Logs/Dyno/agent.log               # the agent's output (when started by the launch agent)
```

`dyno-agent` is `Dyno.app/Contents/MacOS/dyno-agent`. Take overhead numbers only on a quiet machine
(`sysctl -n vm.loadavg`) and write the load average next to them.

Outside a bundle (a development build) `dyno-agent service install` writes `~/Library/LaunchAgents/net.decryptology.dyno.agent.plist`
and bootstraps it (`launchctl bootout` first, so a stale definition never survives); `uninstall` removes both. Do not mix the two
routes on one machine.

## Permissions and what Dyno can see

- Your own processes: everything (command line, working directory, open files through `lsof`, signer through `codesign`).
- Other users' and system processes: name and resource use; changing them needs administrator rights, which Dyno never takes by
  itself. The optional helper (Settings > Helper) makes the few changes that need it: it is installed with the macOS password dialog as a LaunchDaemon (`net.decryptology.dyno.helper`, program in `/Library/PrivilegedHelperTools`, data in `/Library/Application Support/Dyno/Helper`), serves your account and Dyno's agent only, and Remove deletes all of it. It does not use SMAppService yet: that needs a Developer ID signature.
- Efficiency mode is the Darwin background policy (`setpriority` with `PRIO_DARWIN_BG`); CPU caps are a 100 ms SIGSTOP/SIGCONT
  duty cycle; there is no CPU affinity on macOS.
- The protected list covers `kernel_task`, `launchd`, `WindowServer`, `loginwindow`, `coreaudiod`, `logd`, `securityd`,
  `syspolicyd`, XProtect and more, plus Dyno itself.

## After updating Dyno

Replace the app. If the agent does not come back, `dyno-agent service ensure` (or turn Launch at login off and on). Check with
`launchctl print` that the job runs the new binary.

## Logs and files

`~/Library/Application Support/Dyno` (`dyno.db`, `settings.json`, `rules.json`, `process-control.json`, `agent.json`,
`logs/app.log`), `~/Library/Logs/Dyno/agent.log`.

## Removing Dyno

Turn off Launch at login (or `dyno-agent service uninstall`), quit the app, delete `Dyno.app` and, if you want, the data
folder above. If the agent is still registered, remove it in System Settings > Login Items & Extensions.

## Running the tests on a Mac

`cargo test --workspace` includes real macOS platform tests: they spawn their own dummy processes and act only on those. Do
not run them while you depend on a process whose name matches a test dummy; they never touch anything else.
