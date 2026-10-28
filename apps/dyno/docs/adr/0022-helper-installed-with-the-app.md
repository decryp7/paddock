# 22. The privileged helper is installed with the app, from a protected folder

- Status: proposed (2026-10-28)
- Related: amends ADR 0016 (sections 1, 3, 5 and 6: who installs the helper, who may call, update and uninstall, when
  it is asked for) and ADR 0021 point 4 ("the install runs from the helper's command line"); ADR 0006, ADR 0010 (the
  per-user agent and its login entry stay as they are); `crates/dyno-helper` (`auth.rs`, `cli.rs`, `config.rs`,
  `install.rs`, `serve.rs`), `dyno_core::helper_wire`, `dyno_agent::helper_centre`, `dyno_platform::elevate`,
  `app/src-tauri/tauri*.conf.json`, `app/src-tauri/windows/installer-hooks.nsh`; Paddock rules 7, 9, 19, 23, 24

## Context

The user decided on 2026-10-28 that in Dyno 1.0, a read-only monitor (ADR 0021), the privileged helper is installed with
the app instead of on a click later. Today the helper is installed by the agent: `Request::InstallHelper` makes the agent
run `dyno-helper install --user <id> --agent-exe <path>` elevated (UAC `runas`, `osascript ... with administrator
privileges`, `pkexec`). That command copies the helper to a system folder, writes `config.json` (the one authorised user
and the agent's path) and a per-user token, and registers a LocalSystem service, a LaunchDaemon or a systemd unit.

What the helper does in 1.0 (`dispatch.rs`, `helper_wire.rs`): `Info`, `Audit`, `Uninstall`, and `Traffic`, which only
works on Windows (`serve.rs` answers "needs no helper on this system" elsewhere). **On macOS and Linux no feature of 1.0
needs the helper**; the planned reads (Linux DMI memory, other users' processes, process start and exit events) are not
built yet.

How Dyno is packaged today (`tauri.*.conf.json`): Windows NSIS with `installMode: currentUser` (in
`%LOCALAPPDATA%\Dyno`, no elevation); macOS `.app` in a DMG, dragged to `/Applications` (the copy then belongs to the
user who dragged it), sealed ad-hoc (rule 23); Linux `.deb` (files in `/usr/bin`, owned by root) and an AppImage (run
from a FUSE mount under `/tmp`, a different path every run). Updates come from the Tauri updater, which checks the
minisign signature (`PADDOCK_TAURI_SIGNING_PRIVATE_KEY`, public key from `PADDOCK_TAURI_UPDATER_PUBKEY`) before it
installs anything. On Windows it runs the NSIS installer with `/P /UPDATE`; on macOS it swaps the `.app` and falls back to
an `osascript` administrator prompt when the bundle is not writable (`tauri-plugin-updater` 2.14, `updater.rs`).

**Signing in the release workflow** (`.gitea/workflows/dyno-release.yml`, `app-release.yml`,
`.gitea/actions/tauri-bundle`), checked 2026-10-28:

- Tauri updater (minisign): present. Updates are checked for integrity.
- Apple: no Developer ID. `APPLE_SIGNING_IDENTITY` is a placeholder secret, "empty while the apps are ad-hoc sealed".
  Without it there is no notarisation, no hardened runtime, no signed `.pkg`, and no Team ID to check callers against.
- Windows Authenticode: none. There is no `certificateThumbprint`, no `signCommand` and no secret for it. Installers
  are unsigned, so SmartScreen warns and the UAC prompt says "Unknown publisher".

### The problems to solve

**(a) The Windows trust model.** The service pins the agent by path (`auth.rs`, `Policy::check`). With a per-user
install, that path is `%LOCALAPPDATA%\Dyno\dyno-agent.exe`, which the user, and so any program running as the user, can
replace. To be exact about what this gives an attacker:

1. **The way to SYSTEM is any elevated run of a file the user can write.** Today these are the click install and the
   "Update the helper" install: both run `dyno-helper.exe` elevated from `%LOCALAPPDATA%`. A program running as the user
   swaps the file, waits for the user to approve the UAC prompt (which says "Unknown publisher" either way), and its own
   code runs as an administrator. ADR 0016 §3 accepted this risk because such malware "could equally wait for any other
   elevation". That reasoning is weaker once the helper is installed *with every update*: the prompt then comes regularly,
   and the user expects it.
2. **The caller pin.** A replaced `dyno-agent.exe` passes the path check, so it can send every helper request: today
   that is reading data plus `Uninstall`. It cannot reach SYSTEM through the helper, because no request changes the
   machine (ADR 0021). The pin only matters once the agent's folder is one the user cannot write.
3. **No pin can stop the same user from injecting code.** Any program running as the user can open the user's own
   running agent and inject code into it (same user, same integrity level). A path, hash or signature check cannot stop
   this. The defence is that the helper answers only read-only requests.

**(b) Registering and removing the helper**, per OS, by the installer and the uninstaller.
**(c) Updates, downgrades and uninstalling** must never leave a stale or orphaned helper. The pinned path stays the same
across updates, and the helper's version changes with every update.
**(d) An honest degraded mode** when there is no helper.
**(e) Tests:** what can run in CI and what only runs in the local test VMs. The VMs are local only, never CI (rule 7,
`local-test-vms`).

## Options

### Windows

| Option | What it means | For | Against |
| --- | --- | --- | --- |
| **W1. Per-machine NSIS** (`installMode: perMachine`) with hooks that call the helper's own install plan | Dyno is installed in `%ProgramFiles%\Dyno` (HKLM). The installer runs elevated (`RequestExecutionLevel admin` in Tauri's template). `NSIS_HOOK_POSTINSTALL` registers the service and `NSIS_HOOK_PREUNINSTALL` removes it | Every program that SYSTEM or the elevated installer runs lives in a folder only administrators can write. Reuses the tested install plan. The updater already runs this installer | A UAC prompt at install and at **every update**. Dyno becomes per-machine (one copy for all accounts; each user's agent still runs as that user, ADR 0010) |
| W2. MSI (WiX) with `ServiceInstall`/`ServiceControl` | The standard Windows way: MSI owns the service, and `MajorUpgrade` handles upgrades | Rollback and upgrades come from Windows Installer | Tauri generates the component for `externalBin` files, so a `ServiceInstall` tied to that file needs a custom template or a second copy through `fragmentPaths`. Changes the installer, the update artefact and the hooks. A second registration path next to the macOS/Linux plan |
| W3. Keep per-user; the helper checks the caller's Authenticode signature | Pin the publisher instead of the path | No UAC at updates | Needs a code-signing certificate (none exists). A user-writable path lets the attacker swap the file after the process started (the check reads the file, not the code that is running). Does nothing against injection. Leaves 1 (the elevated run of a user-writable file) unsolved |
| W4. Keep per-user; the service checks a hash of the agent recorded at install | Same as W3 without a certificate | No purchase | Same swap problem. Every update must rewrite the hash, which needs elevation, which brings back 1 |
| W5. No helper on Windows | Per-program traffic shows "needs administrator rights" | No privileged code | Loses the one feature that needs the helper, against the user's decision |

### macOS

| Option | What it means | For | Against |
| --- | --- | --- | --- |
| **M0. No helper in 1.0** | Nothing in 1.0 needs one. The install plan stays ready for the first macOS read | No resident root process with nothing to do. No change to the DMG | The decision "installed with the app" does not apply on macOS until there is something to install it for |
| M1. `.pkg` with `preinstall`/`postinstall` | `pkgbuild`/`productbuild` in CI after `tauri build` (Tauri has no `pkg` target; its bundle types are deb, rpm, appimage, msi, nsis, app, dmg). `postinstall` runs `dyno-helper install --machine` as root | Root-owned bundle; installed in one step with the app | Unsigned packages are blocked by Gatekeeper ("Open Anyway" in System Settings); a signed one needs a **Developer ID Installer** certificate. The Tauri updater swaps only the `.app`, so `postinstall` does not run on updates (the helper goes stale until the next prompt). macOS has no uninstaller: dragging the app to the Bin leaves the daemon behind. `/Applications` can be written by the admin group, so the path pin is weaker than on Windows |
| M2. `SMAppService.daemon(plistName:)` from the app | Plist in `Contents/Library/LaunchDaemons`, `BundleProgram` inside the bundle; macOS 13+; the user approves it in System Settings > Login Items | Apple's current way. The daemon runs from the bundle, so it is updated with it. Removed with the app | Needs an Apple-issued signing identity for the app and the helper (Apple DTS: ad-hoc signing "will see problems like this"; ADR 0016 already found ad-hoc does not work). The daemon must be unregistered and registered again after an update (developer forum reports, including a timing gap) |
| M3. First-run prompt (today's `osascript`, at first launch instead of on a click) | The app asks for the password once | No purchase, no new artefact | Runs a file elevated from a bundle the user owns (a drag install), which is problem 1 on macOS |

### Linux

| Option | What it means | For | Against |
| --- | --- | --- | --- |
| **L0. No helper in 1.0** | Same reasoning as M0 | Least privilege | As M0 |
| L1. `.deb` maintainer scripts (`bundle.linux.deb.preInstallScript`, `postInstallScript`, `preRemoveScript`, `postRemoveScript`, all in the Tauri 2.12 schema) plus the unit as a package file; the same four keys exist for `rpm` | dpkg owns `/usr/bin/dyno-helper` and the unit file; the scripts enable, restart and stop it | The Debian way; upgrade, downgrade and purge come from dpkg; the agent path (`/usr/bin`) belongs to root | None worth noting; the scripts must handle every dpkg argument and a system without systemd |
| AppImage | No helper (the path changes every run and the file belongs to the user) | Honest | Features that need the helper show the degraded text (none in 1.0) |

### Who may call (all OS)

| Option | For | Against |
| --- | --- | --- |
| U1. One user, recorded at install (ADR 0016) | Smallest audience | An installer does not know reliably who "the user" is: a UAC prompt approved with another admin's credentials gives that admin's SID; `apt` through PackageKit gives no user; a `.pkg` from MDM has no console user. A wrong guess turns the feature off for the real user |
| **U2. Any local, interactive user, through the agent at the protected installed path** | Works with any installer. What the helper gives is read-only and almost all of it is visible to any user anyway (on Windows any user can list every TCP connection with its owning process without elevation; the helper adds byte counts) | On a shared PC, user B can see the per-program byte counts of user A's programs. The per-user token no longer protects anything and is dropped, so the gates are the endpoint ACL, the kernel's peer facts and the program check |

## Decision (proposed)

### 1. Per OS

- **Windows: W1.** The NSIS installer becomes `perMachine`, and the service runs `"%ProgramFiles%\Dyno\dyno-helper.exe"
  service` **in place**. It is not copied: the installer owns the file, so there is no second copy to go stale.
  - `NSIS_HOOK_PREINSTALL`: stop the service (`dyno-helper.exe stop-service`, which waits until it has stopped), stop
    every running `dyno-agent.exe` from `$INSTDIR`, and detect an old per-user install (the HKCU uninstall key) and run
    its uninstaller silently. There is no compatibility before v1, so the old install is not migrated.
  - `NSIS_HOOK_POSTINSTALL`: `dyno-helper.exe install --machine --agent-exe "$INSTDIR\dyno-agent.exe"`. This one command
    hardens `$INSTDIR` (see 2), writes `%ProgramData%\Dyno\Helper\config.json`, creates or reconfigures the service
    (auto start, restart on failure, quoted `binPath`), sets the service's security descriptor so that interactive users
    may *start* it (for Repair without a prompt), and starts it. It is idempotent: an update and a repair run the same
    command.
  - `NSIS_HOOK_PREUNINSTALL`: `dyno-helper.exe uninstall` (stop, delete the service, remove `%ProgramData%\Dyno\Helper`
    including the audit log), then stop the agents. A failure is logged and the uninstall carries on.
  - Tauri's template does not run the uninstall hook for `/UPDATE` installs. An interactive reinstall may run the old
    uninstaller first, which removes the helper; `POSTINSTALL` then puts it back. Both orders end with one current
    helper.
  - `CheckIfAppIsRunning` in Tauri's template looks only at the main binary, which is why the hook stops the agent and
    the service itself.
- **macOS: M0 for 1.0.** The helper is not shipped or registered on macOS until the first macOS read-only request is
  built. When that happens, the choice is M2 with a Developer ID (decision D4), or M1 with the orphan check of 4 if there
  is no Developer ID. M3 is rejected (it is problem 1 again).
- **Linux: L0 for 1.0, L1 ready.** The `.deb` does not register the helper in 1.0. When the first Linux read lands, it
  gets L1: `/usr/lib/systemd/system/dyno-helper.service` as a package file (`bundle.linux.deb.files`), and maintainer
  scripts that run `systemctl daemon-reload`, `enable` and `restart` on `configure`, `stop` and `disable` on `remove`,
  and remove the data on `purge`. All of this is skipped when `/run/systemd/system` does not exist (containers,
  chroots). `rpm` uses the same scripts if it is ever built. The AppImage never gets a helper.
- If the user prefers the helper registered on all three OS in 1.0 (decision D1), Linux takes L1 now, and macOS takes M1
  (unsigned `.pkg`, with the Gatekeeper step in the release notes' First launch section) or waits for D4.

### 2. The trust model (fixes (a))

1. **Nothing elevated ever runs from a folder the user can write.** `dyno-helper install --machine` and `serve` both
   check the **protected-location rule** for their own program and for `agent_exe`, and fail closed with a message that
   says what to do:
   - Windows: the file and every folder above it grant write, delete or change-permission rights only to SYSTEM,
     Administrators and TrustedInstaller. This matters because NSIS lets the user choose the folder, and on a second
     drive (`D:\Apps\Dyno`) the inherited ACL gives Authenticated Users the Modify right. The installer applies an
     explicit ACL to `$INSTDIR` (`icacls /inheritance:r`, SYSTEM and Administrators Full, Users Read & execute) before
     the check.
   - Unix: the same rule as sshd's StrictModes. Every component must belong to root and must not be writable by group
     or others. `/usr/bin` passes. `/Applications` does not (it is writable by the admin group), which is why macOS waits
     for a signature check (D4).
2. **Callers (U2):**
   - The endpoint ACL admits SYSTEM, Administrators and INTERACTIVE (`S-1-5-4`), denies NETWORK, and the pipe rejects
     remote clients. On Unix the socket is mode 0666 in a root-owned folder.
   - The peer's uid (Unix) and pid are recorded in the audit log.
   - The peer's program must be the recorded `agent_exe`, which is now in a protected folder.
   - The per-user token is dropped. It no longer protects anything, and pretending otherwise would mislead.
3. **Requests:** `Uninstall` leaves the wire (`HELPER_API` 2), because the package owns the helper's lifecycle. Every
   remaining request reads (`Info`, `Traffic`, `Audit`). Same-user injection into the agent (problem 3) can therefore
   only read per-program byte counts. This residual risk is accepted and written down.
4. **The elevated code is the installer.** The first install is trusted the way any download is (HTTPS from Gitea; no
   Authenticode, so SmartScreen and "Unknown publisher" appear). Every update is minisign-verified by the updater before
   it runs. A Windows certificate (D3) would change the prompt's text, not this argument.
5. A hash or signature pin of the agent is **not** added in 1.0. Once the path is protected, a hash adds nothing, and an
   Authenticode check needs D3. With Artifact Signing, the leaf certificates last about three days, so a later
   signature check would pin the subject and the issuing CA, never a thumbprint.

### 3. Updates, downgrades, uninstall, repair (fixes (c))

- **Compatibility is `HELPER_API`, not the version string.** `assess` treats a helper with the same API but a different
  version as `Ready` (with the version shown). Only a different API is `Outdated`. Mid-update the two versions differ for
  a moment, and that is not an error.
- **The install always rewrites `config.json` in its own schema and never migrates it.** An older helper after a
  downgrade therefore never reads a newer schema (it would fail closed). Downgrades stay allowed (`allowDowngrades`
  default `true`); the hooks are the same in both directions.
- **Upgrade (Windows):** the updater runs the new installer with `/P /UPDATE` (one UAC prompt). `PREINSTALL` stops the
  service and the agents, NSIS replaces the files, `POSTINSTALL` reconfigures and starts the new helper, and the agent
  starts again. `agent_exe` does not change, so the pin holds.
- **Orphan check (all OS):** at start and every 6 hours, a helper whose `agent_exe` is missing at two checks at least an
  hour apart removes itself (stop, unregister, delete its data) and writes that to the audit log. This covers a deleted
  install folder, a failed uninstall hook and, if M1 is ever used, a macOS app dragged to the Bin. The direction is safe:
  removing the helper only reduces power. The two checks stop an update's file swap from looking like an uninstall.
- **Repair (rule 19):** a stopped service is started by the agent with no prompt (the service security descriptor from
  1). A missing or damaged registration offers **Repair**, which runs `dyno-helper.exe install --machine` elevated from
  `%ProgramFiles%\Dyno` (UAC), only after the protected-location check of the binary it is about to elevate. This is
  the only elevation the agent keeps; the osascript and pkexec paths are deleted. The Repair button does not depend on
  the helper.

### 4. With no helper (fixes (d))

`HelperState` becomes:

| State | When | What the user sees |
| --- | --- | --- |
| `NotNeeded` | macOS, Linux | Settings > Helper is not shown. The About page says "Nothing in this version of Dyno needs administrator rights on this system." |
| `NotIncluded` | AppImage | "This copy of Dyno runs without its helper. Everything in this version works without it." |
| `NotRunning` | Windows, service stopped | "Per-program traffic needs Dyno's helper, which is stopped." [Start the helper] (no prompt) |
| `Missing` | Windows, no service or damaged config | "Dyno's helper is missing or damaged, so per-program traffic is not available. Repair puts it back (Windows asks for permission once)." [Repair] |
| `Outdated` | Different `HELPER_API` | As `Missing`, with the two versions |
| `Ready` | Answering | "The helper is running. It only reads and changes nothing on this computer." |

`Installing` and `NotInstalled` go away, and so does the "install the helper" dialog. The traffic page keeps its
existing reason and points at the state's button, never at itself (rule 19).

## Consequences

- Windows users see a UAC prompt at install and at every update; release notes and the First launch section say so.
  Dyno lives in `%ProgramFiles%` for every account on the PC.
- No elevated code and no SYSTEM service runs a file that a non-administrator can write. The one weakness ADR 0016
  accepted is closed on Windows. What remains is same-user injection, limited to read-only answers.
- macOS and Linux 1.0 carry no resident root process. The machinery for them is designed and tested in pure code, so
  the first read that needs it only adds the registration.
- ADR 0016's single authorised user and token are replaced by U2. Its closed request list, the audit log, the fixed
  endpoint and the peer-program check stand.

## Code changes and tests

Sizes: S up to half a day, M one to two days, L three to five days.

| # | Change | Size |
| --- | --- | --- |
| 1 | `dyno_core::helper_wire`: `HELPER_API` 2; remove `Uninstall` and `Bye`; the new `HelperState` set; `assess` compares API; new texts. `protocol`: remove `InstallHelper`/`UninstallHelper`, add `StartHelper` and `RepairHelper` | S |
| 2 | `dyno-helper` CLI: `install --machine --agent-exe`, `uninstall`, `stop-service`; `config.json` schema 2 (no user; `agent_exe`, `version`); drop the token (`NewToken` step, `load_token`) | M |
| 3 | `auth.rs`: U2 policy (any local user, program check unchanged, uid and pid audited); `paths::endpoint_access` with the Windows DACL (SY, BA, IU allowed, NU denied) | S |
| 4 | Protected-location rule: pure over a `FileFacts` port (owner, mode or ACL entries per path component); Windows ACL reading in one small FFI module (`GetNamedSecurityInfoW`, with `// SAFETY:`); used by `install`, `serve` and Repair | M |
| 5 | Install plan, Windows: in place (no copy), `icacls` on `$INSTDIR`, `sc sdset` with interactive start, quoted `binPath`, idempotent reconfigure; Linux plan as package files (kept, unused in 1.0); macOS plan kept, unused | M |
| 6 | Orphan check in `serve` (pure decision over two observations and a clock) | S |
| 7 | NSIS: `installMode: perMachine`; `installer-hooks.nsh` PREINSTALL, POSTINSTALL and PREUNINSTALL as in decision 1; the old per-user uninstall | M |
| 8 | `dyno_agent::helper_centre`: delete install and uninstall; add Start and Repair. `dyno_platform::elevate`: keep Windows `runas` for Repair only, delete the osascript and pkexec paths | M |
| 9 | UI: Settings > Helper card states and buttons, remove the helper gate and the install dialog, mock backend, specs | M |
| 10 | Build: do not bundle `dyno-helper` on macOS and Linux in 1.0 (per-OS `externalBin`), and update `check-bundle.sh` and the sidecar list in `dyno-release.yml` to match | S |
| 11 | Docs: amend ADR 0016 and 0021 by link; testing-manual helper section rewritten (H1-H7 describe the removed click install); user guide; release notes First launch (UAC at updates) | S |
| 12 | (Only with D1 = all OS) Linux L1 scripts and unit file: M. macOS M1 `pkg` step in `app-release.yml`: L. macOS M2 (`SMAppService`, needs D4): L | M/L |

Total for the recommendation (rows 1 to 11): about 9 to 13 days.

**Tests in CI** (none of them installs anything, rule 7):

- Pure plans: install, update and uninstall steps per OS, asserting the exact `sc.exe`, `icacls` and `sc sdset`
  arguments, the quoted `binPath`, and that the install is idempotent (the plan for "already installed" is the same
  plan).
- Protected-location rule over fake `FileFacts`: a user-writable component, a second drive with inherited Modify, a link,
  and `/Applications`. On the Windows runner, real ACLs on a temporary folder the test creates.
- Policy U2: another program, an unknown pid, and a remote pipe client (refused). On the Windows runner: the DACL
  string parses (`ConvertStringSecurityDescriptorToSecurityDescriptorW`).
- Config schema 2 parse, and fail closed on damage. `assess` for every state. The orphan decision with a fake clock.
- The NSIS hook file: a text test that each hook calls the expected command and nothing else, plus a check that the
  built installer requests administrator rights.
- With D1: run the `.deb` maintainer scripts in a throwaway container this job creates (install, upgrade, downgrade,
  remove, purge, with no systemd so the guard is exercised). Run `shellcheck`.
- UI specs for every state and button, with the mock backend.

**Tests in the local VMs only** (the user, before the release; recorded in docs/testing-manual.md):

- Windows 11 VM: a fresh install from the release artefact (UAC once). `DynoHelper` runs as LocalSystem from
  `%ProgramFiles%\Dyno`. Per-program traffic works for a standard account and for an admin account.
- As a standard user, replacing `dyno-agent.exe` or dropping a DLL into `$INSTDIR` is refused. An install into
  `D:\Apps\Dyno` is hardened, or refused when hardening fails.
- An update N-1 to N through the updater (one prompt, the new helper version, no second service). A downgrade with the
  N-1 installer. An interactive reinstall that chooses "uninstall first".
- Uninstall removes the service and `%ProgramData%\Dyno\Helper`. Deleting `%ProgramFiles%\Dyno` by hand leaves no
  service after the orphan window.
- Stop the service, then Start from Settings (no prompt). `sc delete`, then Repair (UAC). A per-user 0.x install present
  before the first per-machine install.
- Linux VM (and macOS VM, only with D1): the same matrix with `apt install ./dyno.deb`, `apt install --allow-downgrades`,
  `apt remove`, `apt purge`. The AppImage shows `NotIncluded`.

## Decisions only the user can make

- **D1.** Register the helper on macOS and Linux in 1.0 even though nothing there uses it yet? Recommended: no.
  Windows only, and the other OS when their first read lands.
- **D2.** Accept a per-machine Windows install, with a UAC prompt at install and at every update? Without it, the trust
  problem (a) cannot be fixed with what exists today, and the alternative is no helper on Windows (W5).
- **D3 (purchase, optional).** A Windows code-signing certificate. Microsoft Artifact Signing (the former Trusted
  Signing) is a paid Azure subscription, Basic tier about USD 10 a month (the pricing page did not show the amount when
  checked). It is open to individual developers in the United States and Canada only; elsewhere it needs an organisation
  in the listed countries. Alternatively, an OV certificate from a CA on a hardware token or cloud HSM, typically a few
  hundred USD a year (not checked). Either one gives a named publisher in UAC and SmartScreen reputation over time, and
  allows a later Authenticode caller check. The recommendation does not need it.
- **D4 (purchase, needed for a sound macOS helper).** The Apple Developer Program (USD 99 a year) for Developer ID
  Application and Installer certificates. It allows notarisation, the hardened runtime (rule 23's exception ends),
  `SMAppService`, a signed `.pkg`, and a caller check against the Team ID. It is not needed for 1.0 if D1 is "no".
- **U2** (any local user may read through the agent) replaces ADR 0016's one user. It is listed here because it changes
  what other accounts on a shared PC can see.

## Sources checked (2026-10-28)

- Tauri 2 config schema shipped with `@tauri-apps/cli` 2.12.3 (in the repository's `node_modules`): `NSISInstallerMode`
  (`currentUser` uses HKCU and needs no administrator; `perMachine` uses Program Files and HKLM and needs
  administrator), `installerHooks` (the four hooks and when they run), `allowDowngrades` (default `true`; NSIS refuses
  downgrades in silent, passive and update mode only when it is false), `DebConfig`/`RpmConfig` `preInstallScript`,
  `postInstallScript`, `preRemoveScript`, `postRemoveScript`, `WixConfig.fragmentPaths`/`componentRefs`, and
  `BundleType` (no `pkg`).
- Tauri's NSIS template, `crates/tauri-bundler/src/bundle/windows/nsis/installer.nsi` (dev branch): `RequestExecutionLevel
  admin` for `perMachine`; `$UpdateMode` from `/UPDATE`; update mode skips the uninstall-first step;
  `CheckIfAppIsRunning` looks at the main binary only.
- `tauri-plugin-updater` 2.14.0 source: Windows passes the install-mode arguments and `/UPDATE` to NSIS; macOS swaps the
  bundle and falls back to `do shell script ... with administrator privileges`.
- Tauri docs "Windows Installer" (v2.tauri.app/distribute/windows-installer) and "Debian" (v2.tauri.app/distribute/debian).
- Apple, `SMAppService` and `daemon(plistName:)` (plist in `Contents/Library/LaunchDaemons`), `Status.requiresApproval`,
  and "Updating helper executables from earlier versions of macOS" (macOS 13+, `BundleProgram`,
  `AssociatedBundleIdentifiers` needs a matching Team ID).
- Apple Developer Forums 745720 and 768592 (DTS: ad-hoc signing breaks registration; re-registration after an upgrade).
  Forum reports, not documentation.
- Microsoft Learn, Artifact Signing quickstart (individuals: US and Canada only; organisations: listed countries) and
  FAQ (paid subscription needed, no EV, SmartScreen reputation builds over time). Azure Artifact Signing pricing page
  (Basic and Premium tiers; the amount was not shown).
- Microsoft Learn, SetPerTcpConnectionEStats (as cited in ADR 0021).
