# 22. The privileged helper is installed with the app, from a protected folder

- Status: Proposed for Dyno 1.1 (deferred from 1.0 on 2026-10-28: the user chose no helper in 1.0). Dyno 1.0 ships
  without the privileged helper. It runs with the user's rights only, is signed with the family updater key, and
  updates per user through the normal Tauri updater. The design below, including the decisions it records for "1.0"
  (D1, D2, D5, D6), now applies to Dyno 1.1 and must be confirmed again for it.
- Revision 2 (2026-10-28): adds the user's decisions of 2026-10-28 (all processes and administrator-only counters in
  1.0, the helper on all three OS, per-machine Windows, no signing purchases, any local user) and the first
  independent review (P1: updates ran an elevated installer from `%TEMP%`; the protected-location rule was not exact).
- Revision 3 (2026-10-28): records D5 (the helper installs Dyno's own signed, newer updates) and D6 (scope U3) as
  decided by the user on 2026-10-28, and answers the second independent review:
  - P1: how the helper opens the caller's update file;
  - P1: the installer's temporary folder;
  - P1: a `%ProgramData%\Dyno` folder that another user created first;
  - P1: a Dyno-only update key;
  - P2: update state and limits, the signature grammar, macOS launch jobs and the hardened runtime, the polkit rule,
    U3 for traffic, and the ADR 0024 waivers.
- Related: amends ADR 0016 (sections 1, 3, 4, 5 and 6: who installs the helper, who may call, what it does, update and
  uninstall, when it is asked for) and ADR 0021 point 4 (new read-only requests, and one named exception: the helper
  installs a signed, newer Dyno). ADR 0006 and ADR 0010 (the per-user agent and its login entry) stay. Paddock ADR 0024
  (background priority, no polling, per-pid monitoring). `crates/dyno-helper`, `dyno_core::helper_wire`,
  `dyno_agent::helper_centre`, `dyno_platform::{elevate, processes}`, `app/src-tauri/{tauri*.conf.json, updater.rs,
  windows/installer-hooks.nsh}`. Paddock rules 7, 9, 19, 21, 23, 24, 26.

## Dyno 1.0 without a helper

Dyno 1.0 has no privileged helper:

- no `dyno-helper` in the bundles;
- no install, update or repair flow for a helper;
- the NSIS installer stays per-user, and the DMG and `.deb` stay as they are.

What follows is what 1.0 cannot show, and how the UI marks it.

> **Placeholder:** the per-OS facts (what an unelevated agent cannot read on Windows, macOS and Linux) come from the
> researcher's note, and are filled in here when it arrives.

| OS | Not available in 1.0 (user rights only) | How the UI marks it |
| --- | --- | --- |
| Windows | _to be filled in from the research note_ | _to be filled in_ |
| macOS | _to be filled in from the research note_ | _to be filled in_ |
| Linux | _to be filled in from the research note_ | _to be filled in_ |

**How the UI marks it,** on every OS:

- A reading Dyno cannot get is `Unavailable` with its reason (`Reading<T>`), never zero or blank. The reason names the
  limit plainly, for example "Windows shows this only to administrators" or "Other accounts' processes are hidden from
  your account".
- Where only part of a list can be read (other accounts' processes), the list says so in one line above it.
- No page offers to install anything. No text calls the missing parts "planned", and none points at the page it is
  shown on (rule 19).

## Open for 1.1

Resolve these before this record is accepted for 1.1:

- **P1 (round-3 review): the update key and where updates come from.** The user keeps the family updater key for now.
  Every Paddock workflow can read that key (`PADDOCK_TAURI_SIGNING_PRIVATE_KEY`) and `RELEASE_TOKEN` through repository
  secrets. A helper that installs any update signed with that key as SYSTEM or root is therefore **unsafe**: a
  malicious or compromised workflow on any branch could sign a package the helper would run. The separate repository
  of section 3 does not fix this alone, because it checks the artefacts against a Tauri signature and a SHA-256 that
  any workflow can forge. Before `Update` ships, one of these must hold:
  - **A Dyno-only key** that no paddock workflow can read.
  - **Or provenance checks:**
    - `dyno-v*` is a protected tag, and the tagged commit is on protected main.
    - The expected hashes come only from the `dyno-release` run for that tag: a push event, `head_sha` equal to the
      tag commit, and the workflow path checked through the Gitea API.
    - Assets created or changed after that run ended are refused.
    - The signed version equals the tag and the package's own version.
    - The job runs on a fresh, throwaway container runner registered for that repository only, with no shared cache,
      and with every tool pinned by digest.
    - `RELEASE_TOKEN` belongs to an account that can neither push to main nor create `dyno-v*` tags.

  Each check needs a test. The trust that remains must be stated: the tag build itself in paddock CI, including caches
  that other workflows can fill, and Dyno's own dependencies.
- **P2 (round 3), not yet folded in:**
  - `DuplicateHandle`:
    - use one process handle (`PROCESS_QUERY_LIMITED_INFORMATION | PROCESS_DUP_HANDLE`) for both the image check and
      the duplication;
    - request only `FILE_READ_DATA | SYNCHRONIZE`;
    - check that the object type is File;
    - read at explicit offsets (`ReadFile` with `OVERLAPPED` offsets, `pread`), never the shared file position.
  - File systems: replace the deny-list (NFS, SMB, FUSE) with an allow-list of local types, for example ext4, xfs,
    btrfs and f2fs on Linux, apfs and hfs on macOS, and NTFS and ReFS on fixed Windows drives. overlayfs, 9p and
    virtiofs then fail too. Say which stalled read still needs the abandon path (a hanging local block device), and
    keep the update lock while an abandoned worker lives.
  - `%ProgramData%\Dyno`:
    - create it with create-new and never open an existing folder for writing;
    - retry a few times, then fail closed;
    - rename a bad folder aside with a random suffix;
    - show a message when the rename fails because another account's program holds the folder open.
  - ACLs: `Users` may currently read `Helper\`, and so the audit log on disk, which contradicts "Audit is
    administrators only". Make `Dyno\` and `Helper\` SYSTEM and Administrators only (users need nothing there now that
    the token is gone), make the Unix data folders 0700 and the audit log 0600, and state the ACL of `Dyno\` itself.
  - Size bands: rows 3, 5, 7, 14, 15 and 19 to 22 fall outside their S/M/L bands. Fix the bands (for example S up to
    1 day, M 1 to 3, L 3 to 6, XL 6 to 10) or the rows. Add 2 to 3 days for the provenance work, and add the row again.

## Context

The user decided on 2026-10-28:

- The privileged helper is **installed with the app on Windows, macOS and Linux**.
- In Dyno 1.0 the helper **reads all processes** (other users' and the system's) **and administrator-only counters**,
  including DMI memory details on Linux.
- Windows becomes a **per-machine install** in `%ProgramFiles%`, with a UAC prompt at install. Updates have no prompt,
  because the helper installs them (D5).
- **No signing purchases**: no Apple Developer ID, no Authenticode.
- **Any local user** may use the helper. Each caller sees what scope U3 allows (D6, section 6).
- **D5**: the helper installs Dyno's own signed, newer updates.
- **D6**: U3.

Dyno stays a read-only monitor (ADR 0021).

### Today

**Install.** The agent installs the helper on a click. `Request::InstallHelper` makes the agent run `dyno-helper install
--user <id> --agent-exe <path>` elevated, using UAC `runas`, `osascript ... with administrator privileges` or `pkexec`.
That command:

- copies the helper to a system folder;
- writes `config.json` (one authorised user and the agent's path) and a per-user token;
- registers a LocalSystem service, a LaunchDaemon or a systemd unit.

**What the helper answers** (`dispatch.rs`): `Info`, `Audit`, `Uninstall`, and `Traffic`. `Traffic` works only on
Windows.

**Packages** (`tauri.*.conf.json`):

- Windows: NSIS with `installMode: currentUser`, in `%LOCALAPPDATA%\Dyno`.
- macOS: a DMG with an ad-hoc sealed `.app` (rule 23). It is dragged to `/Applications` and then belongs to that user.
- Linux: a `.deb`, whose files belong to root in `/usr/bin`, and an AppImage, which runs from a FUSE mount under `/tmp`.

**Updates.** The Tauri updater checks the minisign signature and then installs (`tauri-plugin-updater` 2.14 source):

- Windows: it writes the installer to a temporary folder under the user's `%TEMP%` and starts it with `ShellExecuteW`.
  UAC elevates it.
- macOS: it swaps the `.app`, with an `osascript` administrator fallback.
- Linux `.deb`/`.rpm`: it writes the package to a temporary folder and runs `pkexec dpkg -i` or `pkexec rpm -U`.

**Signing in the release workflow** (`dyno-release.yml`, `app-release.yml`, `.gitea/actions/tauri-bundle`, checked
2026-10-28):

- The Tauri updater key (minisign) is present.
- `APPLE_SIGNING_IDENTITY` is an empty placeholder, so the apps are ad-hoc sealed.
- There is no Authenticode configuration or secret.
- The user decided on 2026-10-28 to keep it that way for now.

### The problems to solve

**(a) Trust.** Three separate weaknesses:

1. **Elevated runs of files the user can write lead to SYSTEM or root.** This covers:
   - the click install and "Update the helper" today (they run `dyno-helper.exe` from `%LOCALAPPDATA%`);
   - **every update through the Tauri updater**, on Windows and on Linux.

   The updater checks the signature, then writes the file into the user's temporary folder, then runs it elevated.
   Same-user malware can replace the file between the check and the run. With no Authenticode, the UAC prompt looks the
   same either way, and the user expects a prompt at every update.
2. **The caller pin is only as good as the folder.** The helper trusts the agent by path. A per-user path, or a path
   under `/Applications` (writable by the admin group), can be replaced.
3. **Same-user injection into the running agent cannot be stopped by any path, hash or signature check.** The defence
   is that the helper's answers are read-only and scoped (section 6).

**(b) Registration and removal** per OS, by the installer and the uninstaller, with no Developer ID on macOS.

**(c) Updates, downgrades, uninstall:** never a stale or orphaned helper.

**(d) An honest degraded mode.**

**(e) Tests:** what runs in CI and what runs only in the local test VMs (local only, never CI; rule 7).

**New with the user's decisions:**

- **(f) The read requests:** bounded shapes, validated in the helper, at the right priority, with no polling (Paddock
  ADR 0024).
- **(g) Privacy** when any local user can read every process on a shared PC.

## Options

### Windows

| Option | What it means | For | Against |
| --- | --- | --- | --- |
| **W1. Per-machine NSIS** (`installMode: perMachine`) with hooks | Installed in `%ProgramFiles%\Dyno` (HKLM); the installer runs elevated (`RequestExecutionLevel admin` in Tauri's template); hooks register and remove the service | Programs that SYSTEM runs live where only administrators write. The user chose it (D2) | UAC at install. Per-machine (one copy for all accounts) |
| W2. MSI (WiX) with `ServiceInstall` | Windows Installer owns the service | Rollback, `MajorUpgrade` | Tauri generates the `externalBin` component, so `ServiceInstall` needs a custom template; a second registration path |
| W3/W4. Keep per-user; check the caller's Authenticode signature or hash | Pin the publisher or bytes instead of the folder | No UAC | W3 needs a certificate (declined). Both can be beaten by swapping the file after the process starts. Both leave problem 1 open |

### macOS (no Developer ID)

| Option | What it means | For | Against |
| --- | --- | --- | --- |
| **M1'. Unsigned `.pkg`, programs outside the bundle** | `pkgbuild`/`productbuild` in CI (Tauri has no `pkg` target). The payload puts `dyno-agent`, `dyno-helper` and `dyno` in `/Library/Application Support/Dyno/bin/` (root:wheel, 0755), `Dyno.app` in `/Applications`, and the daemon plist in `/Library/LaunchDaemons`. `postinstall` runs `dyno-helper install --machine` | Works with an ad-hoc seal. Every program root runs, and the pinned caller, sit on a path that only root can change | The first open goes through Gatekeeper's blocked-package flow (section 1). No uninstaller (handled by the orphan watch). The app must find its agent outside its bundle |
| M1. Unsigned `.pkg`, programs inside `/Applications/Dyno.app` | As above, but the agent stays in the bundle | Fewer moving parts | `/Applications` is `root:admin 0775` (checked on the build Mac), so the protected-location rule fails and an admin-group process may replace the bundle. Rejected |
| M2. `SMAppService.daemon` | The plist in `Contents/Library/LaunchDaemons`, approved in Login Items | Apple's current way | Needs an Apple-issued signature (DTS: ad-hoc signing breaks registration). Declined with D4 |
| M3. First-run password prompt | Today's `osascript`, at first launch | Nothing new to build | Elevates a file from a user-owned bundle (problem 1). Rejected |

### Linux

| Option | What it means | For | Against |
| --- | --- | --- | --- |
| **L1. `.deb` maintainer scripts** (`bundle.linux.deb.preInstallScript`, `postInstallScript`, `preRemoveScript`, `postRemoveScript`, in the Tauri 2.12 schema) plus the unit as a package file; the same four keys for `rpm` if it is ever built | dpkg owns `/usr/bin/dyno-helper` and `/usr/lib/systemd/system/dyno-helper.service` | The Debian way; dpkg handles upgrade, downgrade and purge | The scripts must handle every dpkg argument and a system without systemd |
| AppImage | Never a helper (the path changes every run and the file belongs to the user) | Honest | Shows the degraded text (section 4) |

### Updates (problem 1 for updates; all OS)

| Option | What it means | For | Against |
| --- | --- | --- | --- |
| **P1. The helper installs updates.** The agent downloads the package and its update signature into the user's cache and asks the helper `Update`. The helper copies the bytes into a folder only it can write, **verifies the copy** against a public key compiled into the helper, checks that the signed version is newer, and runs the copy as SYSTEM or root | Nothing is checked or run in a user-writable place. No prompt at updates on any OS. Works without any certificate. On macOS it also solves "a DMG/app update cannot run `postinstall`": the update *is* the `.pkg`, run by `installer` as root | The helper gains one write action (installing a signed, newer Dyno), a named exception to ADR 0021 (D5, decided). Package-manager details per OS (below) |
| P2. Keep the Tauri updater's install | Unchanged | No new code | Problem 1 at every update, on Windows and Linux; on macOS the helper and the programs outside the bundle go stale. Only D3/D4 (signing) would make the prompt honest, and even then the swap race stays |
| P3. Updates only by downloading the installer by hand | No in-app update | Simple | Same race (the file sits in Downloads); worse experience |

### Who may call, and what they see (all OS)

| Option | What a caller sees | For | Against |
| --- | --- | --- | --- |
| U1. One user, recorded at install (ADR 0016) | Everything | Smallest audience | An installer cannot reliably tell who the user is (over-the-shoulder UAC gives the admin's SID; PackageKit gives no user; MDM gives no console user) |
| U2. Any local user sees everything | Every process of every account, with names, paths, owner and resources; per-program traffic | Simple; the user's literal choice | On a shared PC or over RDP or SSH, any account sees what every other person runs and when (section 6) |
| **U3 (decided, D6). Any local user may call; a caller sees its own processes and the system's in detail, other people's only as one total; administrators see everything** | Own and system/service accounts: full detail. Other interactive accounts: one row "Other users: N processes, CPU, memory, disk". Administrators: all detail | Same "any local user" install and caller rule; answers "why is my PC slow" (services, drivers, updaters), and hides other people's activity from non-administrators | One more check per caller (is it an administrator?); an administrator account sees everyone, which an administrator can do anyway |

## Decision (proposed)

### 1. Per OS

#### Windows: W1, with the helper in place

The service runs `"%ProgramFiles%\Dyno\dyno-helper.exe" service`. It is not copied anywhere.

- **Install folder.** `NSIS_HOOK_PREINSTALL` aborts unless `$INSTDIR` is under `$PROGRAMFILES64`: "Dyno installs only
  into Program Files, so that its system service runs a file other accounts cannot change." Hardening a chosen folder
  such as `D:\Apps\Dyno` cannot be enough, because the user can rename `D:\Apps` (review P1-2). The protected-location
  rule (section 2) checks the result again.
- **`NSIS_HOOK_PREINSTALL`** does not call the old `dyno-helper.exe`, because its interface may differ. It:
  1. runs `sc.exe stop DynoHelper`;
  2. waits up to 30 s with `sc.exe query`, then ends the service's PID from `sc.exe queryex` with `taskkill /F /PID`;
  3. ends every `dyno-agent.exe` and `dyno-app.exe` running from `$INSTDIR`, **in all sessions**.

  Other accounts' agents stop with it. They start again when that person next opens Dyno or logs in. An update the
  helper runs (section 3) starts them again at once.
- **Partial trees.** If a file is still in use in `/P` mode, NSIS can leave a partial tree. `POSTINSTALL` is idempotent,
  so running the installer again or using Repair (section 3) recovers.
- **`NSIS_HOOK_POSTINSTALL`** runs `dyno-helper.exe install --machine --agent-exe "$INSTDIR\dyno-agent.exe"`. It:
  - applies an explicit ACL to `$INSTDIR`: SYSTEM and Administrators Full, Users Read & execute, no inheritance;
  - **makes the data folder trustworthy before it writes anything** (review P1-4). Any user may create folders in
    `%ProgramData%`, so another account could have created `%ProgramData%\Dyno` first and would own whatever is
    written there. The install therefore:
    - opens the folder without following reparse points;
    - if it fails the protected-location rule (owner, ACL, or a junction or link anywhere inside), renames it to
      `%ProgramData%\Dyno.untrusted-<time>` and records that in the audit log;
    - creates `%ProgramData%\Dyno` and `Dyno\Helper` anew with an explicit security descriptor set at creation: owner
      SYSTEM, SYSTEM and Administrators Full, Users Read only for `Helper` and nothing for `Helper\updates`, and no
      inheritance from `%ProgramData%`;
  - writes `%ProgramData%\Dyno\Helper\config.json` in its own schema;
  - removes an old click-installed 0.x service and its `%ProgramFiles%\Dyno\Helper\` copy explicitly, by service name
    and path, not through the HKCU hive;
  - creates or reconfigures the service: auto start, restart on failure, quoted `binPath`, and a security descriptor
    that lets interactive users *start* it;
  - starts the service.
- **Old per-user 0.x copies.** Under over-the-shoulder UAC the installer's HKCU is the administrator's hive, so the
  installer does not look there. Instead, the 1.0 agent finds a per-user 0.x install in its own HKCU at first start. It
  offers to remove it, running that uninstaller as the user, unelevated. There is no compatibility before v1.
- **`NSIS_HOOK_PREUNINSTALL`** checks `$UpdateMode`:
  - On an update, or on an "uninstall first" reinstall (Tauri passes `/UPDATE`), it only stops, as `PREINSTALL` does.
  - On a real uninstall it runs `dyno-helper.exe uninstall`, which stops and deletes the service and removes
    `%ProgramData%\Dyno\Helper`, including the audit log.

  A failure is logged and the uninstall carries on.

#### macOS: M1', an unsigned `.pkg`

- **Payload**, laid down by Installer as root, so nothing is copied after the fact:
  - `/Library/Application Support/Dyno/bin/{dyno-agent, dyno-helper, dyno}`, each ad-hoc signed **with the hardened
    runtime** (`codesign -s - -o runtime`, no `get-task-allow`) and checked with `codesign --verify --strict` in CI.
    The hardened runtime blocks `DYLD_INSERT_LIBRARIES` and attaching a debugger to these programs, so same-user
    injection into the agent (problem 3) needs more than an environment variable.
    - This is a narrow exception to rule 23 ("while ad-hoc, the hardened runtime is off"). Rule 23's reason is that
      ad-hoc code under the hardened runtime cannot load vendor-signed libraries. These three programs link only system
      libraries, so the reason does not apply to them.
    - CI checks that with `otool -L`: only `/usr/lib` and `/System` may appear.
    - `Dyno.app` itself stays as rule 23 says.
    - Even so, the path pin is not a full boundary on macOS. A root or debugger-entitled process can still inject;
      section 6 bounds what that gains.
  - `/Applications/Dyno.app` (the UI only; it starts the agent at the fixed path above);
  - `/Library/LaunchDaemons/net.decryptology.dyno.helper.plist`;
  - the agent's launch-agent plist pointing at the fixed path (ADR 0010's opt-in stays).
- **`preinstall`:** `launchctl bootout system/net.decryptology.dyno.helper`, then stop the agents.
- **`postinstall`:** `dyno-helper install --machine --agent-exe "/Library/Application Support/Dyno/bin/dyno-agent"`
  (config, data folder, `launchctl bootstrap`).
- **The DMG goes away on macOS.** A dragged app would have no helper and a second agent.
- **Gatekeeper.** An unsigned package downloaded in a browser carries the quarantine flag, and Gatekeeper blocks it:
  - On macOS 15 and later, the dialog offers only *Done* and *Move to Bin*. The user then opens **System Settings >
    Privacy & Security > Open Anyway** and authenticates. The Control-click > Open bypass was removed in macOS 15.
  - On macOS 13 and 14, Control-click > Open still works.
  - Installer then asks for an administrator password.
  - The release notes' First launch section shows these steps, and `xattr -d com.apple.quarantine Dyno.pkg` as the
    terminal alternative.
  - Whether the installed `Dyno.app` inherits quarantine is to be checked in the VM. It should not: Installer does not
    pass the package's flag on to the payload.
- **Updates** never use the `.app` swap (section 3).
- **Uninstall.** macOS has no package uninstaller:
  - Settings > About > **Remove Dyno** (for administrators) runs `dyno-helper uninstall --all` from the protected path
    with the system password dialog. That is safe, because the file it elevates is root-owned on a root-only path.
  - The orphan watch covers dragging `Dyno.app` to the Bin (section 3).

#### Linux: L1 for `.deb`

- **Package files:** `/usr/bin/dyno-helper` and `/usr/lib/systemd/system/dyno-helper.service` (`bundle.linux.deb.files`).
- **`postinst configure`:** `dyno-helper install --machine --agent-exe /usr/bin/dyno-agent`, then `systemctl
  daemon-reload`, `enable` and `restart`.
- **`prerm remove`:** `systemctl stop` and `disable`.
- **`prerm upgrade`:** `stop` only.
- **`postrm purge`:** removes `/var/lib/dyno-helper`.
- **Without systemd:** if `/run/systemd/system` does not exist (containers, chroots), each step logs and skips, and the
  scripts exit 0.
- **The polkit rule** `/usr/share/polkit-1/rules.d/50-dyno-helper.rules` (a package file) allows exactly one thing:
  starting the helper's unit, from a local, active session.

  ```js
  polkit.addRule(function (action, subject) {
    if (action.id == "org.freedesktop.systemd1.manage-units" &&
        action.lookup("unit") == "dyno-helper.service" &&
        action.lookup("verb") == "start" &&
        subject.local && subject.active) {
      return polkit.Result.YES;
    }
  });
  ```

- **`rpm`:** takes the same scripts if it is ever built.
- **AppImage:** never a helper. It keeps the Tauri updater's own in-place AppImage update, which replaces the user's own
  file with no elevation, so problem 1 does not arise. To see other accounts' processes and the administrator-only
  counters, the user installs the `.deb` (the `NotIncluded` text says so).

### 2. The trust model (fixes (a))

#### 1. Nothing elevated ever runs from, or is checked in, a place a non-administrator can change

This now covers updates as well (section 3). Every program the helper runs, and the pinned agent, must pass the
**protected-location rule**. `install`, `serve`, Repair and `Update` check it, and it fails closed with a message that
says what to do. It is a pure function over a `FileFacts` port.

**Windows.** For every component from the file up to the volume root:

- The **owner** is trusted: SYSTEM (`S-1-5-18`), Administrators (`S-1-5-32-544`) or TrustedInstaller
  (`S-1-5-80-956008885-...`).
- Only *allow* ACEs that apply to the object itself count. Inherit-only ACEs and CREATOR OWNER/GROUP placeholders are
  ignored, because they do not apply to the object. Conditional ACEs count as granting, so the check fails closed.
- An untrusted principal may hold none of these rights:
  - **on the file:** `FILE_WRITE_DATA`, `FILE_APPEND_DATA`, `DELETE`, `WRITE_DAC`, `WRITE_OWNER`;
  - **on the file's own folder:** `FILE_ADD_FILE` and `FILE_ADD_SUBDIRECTORY` (DLL planting), `FILE_DELETE_CHILD`,
    `DELETE`, `WRITE_DAC`, `WRITE_OWNER`;
  - **on every ancestor:** `DELETE`, `WRITE_DAC` and `WRITE_OWNER` on the component, and `FILE_DELETE_CHILD` on its
    parent. These are the rights that rename or replace a path.
- Create-folder and append rights on ancestors are allowed. This is what keeps stock `C:\` (Authenticated Users:
  create folders and append, plus inherit-only Modify) passing for `C:\Program Files\Dyno`.

**Unix.** For every component:

- The owner is root.
- On the file and its folder: no write for group or others.
- On ancestors: no group or other write, unless the sticky bit is set and the child is owned by root.
- `/Library/Application Support` (`root:admin 0755`) and `/usr/bin` pass. `/Applications` (`root:admin 0775`) fails,
  which is why macOS keeps its programs outside the bundle.

**Every file the helper relies on is checked by handle before use** (review P1-4): `config.json`, `policy.toml`, the
audit log, everything in `updates\`, and the programs it starts. The helper opens the file, then applies the rule to
the open handle (owner and ACL through `GetSecurityInfo`, `fstat`) and to the path's components. A file that fails is
ignored with an audit line, and the helper fails closed. The Unix data folders (`/var/lib/dyno-helper`,
`/Library/Application Support/Dyno/Helper`, `/etc/dyno`) have root-only parents, so another user cannot create them
first. They are still checked the same way.

**Tests:** stock `C:\Program Files`, a user-writable second drive, a renamable parent, a planted DLL folder,
inherit-only and CREATOR OWNER ACEs, a conditional ACE, `/Applications`, and a sticky `/tmp`.

#### 2. Callers

- **The endpoint admits local callers only.**
  - The Windows pipe grants SYSTEM, Administrators and INTERACTIVE (`S-1-5-4`), denies NETWORK, and rejects remote
    clients. INTERACTIVE includes Remote Desktop sessions, so a person logged in over RDP is a local caller.
  - On Unix the socket is 0666 in a root-owned folder. SSH logins are local users too.
- **The kernel's facts identify the caller.**
  - macOS: the audit token from `LOCAL_PEERTOKEN`, its pid and pidversion, then `proc_pidpath_audittoken`. This needs
    no Team ID and does not race pid reuse.
  - Linux: `SO_PEERCRED`, then `/proc/<pid>/exe` read through a pidfd.
  - Windows: `GetNamedPipeClientProcessId`, then `QueryFullProcessImageNameW` on a handle kept open for the check.
- **The caller's program must be the recorded `agent_exe`.** It sits on a protected path on every OS, so file
  replacement is closed. Injection (problem 3) remains, and section 6 bounds what it can read.
- **The caller's account and administrator status** come from the same facts and decide the answer's scope (section 6):
  - Windows: the peer's token; membership of Administrators counts even when UAC has filtered it.
  - macOS: uid 0 or membership of `admin`.
  - Linux: uid 0 or membership of `sudo` or `wheel`. Supplementary groups come from `SO_PEERGROUPS` (Linux 4.13+),
    with `getgrouplist` for the peer's uid as a fallback. On macOS, membership is checked with `mbr_check_membership`
    (`cr_groups` holds only 16 groups).
- **The per-user token is dropped.** It protects nothing once any local user may call.

#### 3. The request list

`HELPER_API` becomes 2.

- **Reads:** `Info`, `Traffic`, `Processes`, `Counters` and `MemoryModules` (section 5).
- **Administrators only:** `Audit`.
- **The one write:** `Update` (section 3).
- **Removed:** `Uninstall`, because the package owns the helper's lifecycle.

#### 4. The first install

It is trusted the way any download is: HTTPS from Gitea, no Authenticode, so SmartScreen and Gatekeeper warn. After that,
every update is checked by the helper itself.

### 3. Updates, downgrades, uninstall, repair (fixes (c) and problem 1 for updates)

#### `Update(UpdateRequest { file: FileRef, signature: String })`

**One update at a time.** A lock and the state file `updates/state.json` enforce it. A second request answers "An
update is already running."

**1. Receiving the file, not a path** (review P1-2). A path chosen by the caller would make SYSTEM or root open it:

- a UNC or `\\?\` path makes the machine account authenticate over SMB (NTLM relay);
- a FIFO or an oplock can stall the helper;
- a junction or link can redirect it.

So the caller passes an open file instead:

- **Unix:** the agent opens the downloaded file and sends the descriptor over the socket with `SCM_RIGHTS` (`dyno-ipc`
  gains descriptor passing).
- **Windows:** the agent sends its handle value. The helper duplicates it out of the verified peer process
  (`OpenProcess(PROCESS_DUP_HANDLE)` on the pid from the pipe, then `DuplicateHandle`). The helper never resolves a
  name the caller wrote.

The helper then checks the handle it holds:

- **Windows:** `GetFileType` is `FILE_TYPE_DISK`. `FileAttributeTagInfo` shows no reparse point. The handle's volume
  (`GetFinalPathNameByHandleW` with `VOLUME_NAME_GUID`, then `GetDriveTypeW`) is a local fixed drive.
- **Unix:** `fstat` says `S_ISREG`. The file system is local (`fstatfs`: not NFS, SMB or FUSE; on macOS `MNT_LOCAL`).
  For files the helper opens itself, it uses `O_NOFOLLOW | O_NONBLOCK`.
- **Size:** at most 512 MB.

**2. Copying.** The helper copies the bytes into a fresh file created with create-new in `updates/` (protected-location
rule; SYSTEM or root only). `updates/` is `%ProgramData%\Dyno\Helper\updates\`,
`/Library/Application Support/Dyno/Helper/updates/` or `/var/lib/dyno-helper/updates/`.

- The copy runs on a worker with a 120 s deadline. On Windows the deadline uses `CancelIoEx`. A read stuck in a FUSE
  file on Linux cannot be cancelled: the worker is abandoned, and with one update at a time at most one worker is lost.
- Before copying, the helper checks free space: at least three times the package size on the `updates/` volume and on
  the install volume.
- The source is never read again.
- A rejected copy is deleted at once. After a run, only the current update's folder is kept, until the next start.

**3. Verifying the copy** with minisign against `UPDATE_PUBKEY`, the **Dyno-only update key** (review P1-5),
compiled into the helper. It is never the family's Tauri updater key, which every Paddock app's workflow can read. A
build without the key refuses every update: "This build cannot update itself." The signature text the helper accepts
is at most 1,024 bytes.

**4. Checking the signed trusted comment.** The trusted comment is covered by the global signature. It must match this
grammar exactly: ASCII, single spaces, fields in this order, nothing else.

```
dyno-update/1 app=dyno version=<MAJOR>.<MINOR>.<PATCH> target=<windows|macos|linux>-<x86_64|aarch64> kind=<nsis|pkg|deb|rpm>
```

- `MAJOR`, `MINOR` and `PATCH` are decimal numbers without leading zeros.
- **Pre-releases and build metadata are refused.** Update installs stable releases only; a pre-release is installed by
  hand.
- `app` must be `dyno`.
- `target` and `kind` must match this machine and its install.
- The version must be strictly greater than the helper's own (SemVer 2.0 precedence on the three numbers).

This prevents downgrade and replay, and it stops another app's or another OS's package from being run.

**5. Running the verified copy through a runner** that outlives the helper. The runner is a copy of the helper's own
program, placed in `updates/<id>/` so the installer can replace the real one. It runs the package, waits for it, and
writes the exit code into `updates/state.json`. The child's `TEMP`, `TMP` and `TMPDIR` point at `updates/<id>/tmp/`
(review P1-3): an NSIS installer run as SYSTEM would otherwise unpack its plugins into `C:\Windows\Temp`, where any user
can plant DLLs.

- **Windows:** the runner starts outside the service's job, and runs the installer with `/S /UPDATE`. `PREINSTALL`
  then stops the service, which ends the helper but not the runner.
- **macOS:** a one-shot launchd job. The helper writes `updates/<id>/net.decryptology.dyno.update.plist` (root-owned,
  `RunAtLoad`, `LaunchOnlyOnce`, program = the runner, which runs `/usr/sbin/installer -pkg <copy> -target /`) and
  bootstraps it into the system domain. `preinstall`'s `bootout` of the helper does not touch it. The new helper boots
  out any leftover update job at start.
- **Linux:** `systemd-run --unit=dyno-update-<id> --collect --property=PrivateTmp=yes` runs the runner, which runs
  `apt-get install -y --no-install-recommends <copy>` (or `dnf install -y <copy>`). It lives in its own transient unit,
  so `prerm`'s `systemctl stop` does not kill it.

**6. Recording.** The state file records the update id, the version, the time, the sessions to restart (Windows: the
sessions with a running agent; Unix: the uids whose agents were running), the runner's pid and, at the end, the exit
code. The update goes into the audit log before it starts, then the helper answers the caller.

**7. Finishing at the next start.** The new helper (`POSTINSTALL`, `postinstall`, `postinst`) reads the state file:

- **Success:** restart the agents in the recorded sessions (Windows: `WTSQueryUserToken` and `CreateProcessAsUser` of
  the agent in `%ProgramFiles%`; Unix: the launch agent or user unit is kicked for each recorded uid). Then clean up
  `updates/`.
- **Failure:** an exit code other than 0, or a runner that ended without writing one (its pid is gone and the state has
  no code). The helper writes the code and the package version to the audit log, keeps the state for Settings, and
  shows "The update to <v> did not finish (code N). Dyno <old> is still installed. Repair, or install <v> by hand."
- **Old helper still running** (the install never reached `PREINSTALL`): it reads the same state at its next start and
  reports it the same way.

#### Where the second signature comes from

The release's normal artefacts and `latest.json` stay as they are. The macOS entry of `latest.json` now points at the
`.pkg`, signed with the Tauri updater key as usual. The Tauri updater's `check` and `download` never look inside the
file.

A second workflow then publishes `dyno-update.json` on the release and on `dyno-latest`:

```
{ "version", "platforms": { "<target>": { "url", "kind", "signature" } } }
```

`signature` is the minisign signature by the Dyno-only key, with the trusted comment above.

The agent:

1. downloads the package through the Tauri updater (`check`, `download`);
2. fetches `dyno-update.json` and takes the entry for its target;
3. checks that `version`, `url` and `kind` agree with the `latest.json` entry;
4. passes the open file and the signature to `Update`.

If `dyno-update.json` is missing or older, the agent shows "The update is not ready yet" and tries again at the next
check. It never falls back to the Tauri install.

**The Dyno-only key, and how it is kept apart.** Gitea Actions secrets exist at user, organisation and repository level
only (docs.gitea.com, checked 2026-10-28). There is no per-workflow or per-environment scope. A repository secret in
`decryp7/paddock` could therefore be read by any workflow file on any branch of that repository, including a Renovate
branch. The key therefore lives in **a separate repository, `decryp7/dyno-update-signing`**:

- It holds the secret `DYNO_UPDATE_SIGNING_KEY` and its password at **repository** level.
- Only the user can push to it, and it has one workflow: `workflow_dispatch` with a `dyno-v*` tag as input. It is run
  after `dyno-release.yml` publishes.
- That workflow downloads the release's artefacts and verifies each against its Tauri updater signature and its
  published SHA-256, which proves they came from the tag build. It then signs each with the trusted comment, and
  uploads `dyno-update.json` with a token that can write releases.
- It checks out no code from `paddock`.

The public key is a plain constant in `crates/dyno-helper` (`UPDATE_PUBKEY`), reviewed like code. An offline backup of
the secret key is the user's to keep. Setting up the repository and the key is a one-time step for the user (D7).

**The Tauri updater** keeps `check` and `download` (it checks the Tauri signature early) and is never asked to install,
except for the AppImage (above). The agent hands the downloaded file to the helper. The UI shows the update as a job with phases
(downloading, checking, installing, restarting) and never times it out (rules 11 and 12).

**Who may trigger `Update`:** any local caller. The worst it can do is install a genuine, newer Dyno at a time that does
not suit someone. The audit log records who asked.

**Downgrades** are only possible by hand, with an older installer: NSIS keeps `allowDowngrades` true, `installer` installs
an older `.pkg`, and `apt` uses `--allow-downgrades`. Every install rewrites `config.json` in its own schema and never
migrates it, so an older helper never reads a newer schema. The scripts are the same in both directions.

**Compatibility is `HELPER_API`, not the version string.** A helper with the same API but a different version is `Ready`
and shows its version. Only a different API is `Outdated`.

#### The orphan watch (no polling; Paddock ADR 0024 §2)

At start, and on a file event for the app's install location, the helper checks whether the app is still there. The
event comes from `paddock-watch` on the parent folder of `%ProgramFiles%\Dyno`, `/Applications/Dyno.app` or
`/usr/bin/dyno-agent`.

If the app is missing, the helper sets a single one-shot deadline an hour later (`Clock::wait`) and checks again. If the
app is still missing, it removes itself:

- stop, unregister, delete its data;
- on macOS also the programs in `/Library/Application Support/Dyno/bin`, the launch agents, and `pkgutil --forget`.

It writes the removal to the audit log, which goes too. An update's file swap never lasts an hour. Removing the helper
only reduces power, so this direction is safe.

The one-hour deadline is a single timed wait, not a loop. It is declared to ADR 0024's check C41 with the waiver kind
`delay` and this ADR as its reason.

#### Repair (rule 19)

- **Stopped service:** the agent starts it, with no prompt where the OS allows it.
  - Windows: the service security descriptor lets interactive users start it.
  - Linux: a polkit rule shipped in the `.deb` (`/usr/share/polkit-1/rules.d/`) allows `systemctl start
    dyno-helper.service`, and nothing else, for local active sessions.
  - macOS: launchd has no such grant. `KeepAlive` restarts the daemon after a crash, and a daemon that was booted out
    is a `Missing` case for Repair.
- **Missing or damaged registration:** **Repair** runs `dyno-helper install --machine` elevated:
  - Windows: UAC, from `%ProgramFiles%\Dyno`.
  - macOS: password dialog, from `/Library/Application Support/Dyno/bin`.
  - Linux: `pkexec`, from `/usr/bin`.

  Repair runs only after the protected-location check of the file it is about to elevate.
- These are the only elevations the agent keeps. The old install, uninstall and update paths, and their osascript and
  pkexec strings for anything else, are deleted.

### 4. With no helper (fixes (d))

`HelperState`:

| State | When | What the user sees |
| --- | --- | --- |
| `NotIncluded` | AppImage; a development build | "This copy of Dyno runs without its helper, so it shows only your own processes and the counters any account can read. Install Dyno's package to see all processes." |
| `NotRunning` | The helper is stopped | "Dyno's helper is stopped, so other accounts' processes and administrator-only counters are not shown." [Start the helper] (no prompt) |
| `Missing` | No registration, or a damaged config | "Dyno's helper is missing or damaged. Repair puts it back (the system asks for permission once)." [Repair] |
| `Outdated` | A different `HELPER_API` | As `Missing`, with both versions |
| `Ready` | Answering | "The helper is running. It reads for Dyno and changes nothing except installing Dyno's own signed updates." |

**On every page that needs the helper:**

- Process lists and the disk-health and memory panes say what is missing ("Processes of other accounts are not shown:
  the helper is stopped") and use the state's button. They never point at the page they are shown on (rule 19).
- Readings the helper would give are `Unavailable` with that reason, never zeros (`Reading<T>`).

### 5. The new read requests (fixes (f))

All requests are validated in the helper after decoding. Unknown enum values or field bits are decode errors. Every
list has a cap, and the reply's postcard size stays under the IPC's 4 MB frame limit.

#### Shared rules for every read

- Each request carries a `class` (`User` for a visible view, `Background` for history sampling).
- The helper runs it on a worker thread at that class (`priority::apply_current`, ADR 0020 and Paddock ADR 0024 §1).
  The helper serves interactive views, so it is launched at `User`.
- External tools run through `paddock_proc::command(...).class(Background)` with a timeout.
- **The helper has no loops and keeps no samples** (Paddock ADR 0024 §2). It reads when asked and returns cumulative
  counters, and the agent's pure core turns them into rates.
- A machine read is cached for 1 s, keyed on the **helper's own monotonic clock** (never a time the caller sends), so
  two agents on one PC cause one read. The cache is a timestamp, not a timer.
- The cache holds the unscoped read inside the helper only. Each caller's scope (U3, section 6) and redaction are
  applied to its own copy of the answer, after the read.

#### `Processes(ProcessQuery)`

**Request:**

- `after: Option<Pid>`: the paging cursor, in pid order.
- `limit: u16`: 1 to 1,024, default 512.
- `fields: ProcessFields`: a closed bit set.
- `class`.

**Answer:** `ProcessPage { taken_at, items, next: Option<Pid>, others: Option<OthersTotal> }`.

- At most 16,384 processes per sweep, so 16 pages.
- Each page is a fresh read.
- The agent matches entries by `ProcessRef` (pid plus start time, so a reused pid is never confused).

**Allowed fields only:**

- pid, start time, parent;
- name (at most 256 characters);
- program path (at most 1,024 characters, redacted as below);
- owner kind (`Caller`, `System`, `Service(name)`, `OtherUser`) and the account name for `Caller`, `System` and
  `Service`;
- run state, thread count;
- cumulative CPU time;
- memory in the family's real-memory measure (rule 26: Linux PSS, macOS physical footprint, Windows private working
  set);
- cumulative disk bytes read and written.

**Never, for any process, including the caller's own:**

- command line, environment, working directory;
- open files and handles, loaded modules, window titles, memory contents, network endpoints.

Other users' command lines and environments routinely carry passwords and tokens, and nothing Dyno shows needs them.
The agent still reads the caller's own command lines itself, as today. The helper adds none.

**Redaction of program paths:**

- A path inside another account's profile or home folder becomes `<file name> (in another account's folder)`. The
  folders are the real profile roots, not a guessed pattern:
  - Windows: `ProfileImagePath` for each SID under `HKLM\SOFTWARE\Microsoft\Windows NT\CurrentVersion\ProfileList`;
  - Unix: each account's home folder from the password database (`getpwent`).

  The caller's own profile is not redacted.
- System and Program Files paths stay.

**What the helper adds per OS:**

- Linux: PSS, I/O counters and program paths of other accounts' processes (owner-only in `/proc`).
- macOS: footprint, CPU and disk counters (`proc_pid_rusage`) and paths of other accounts' processes.
- Windows: protected and service processes, other accounts' private working sets and owner names.

**Linux `hidepid`:** when `/proc` is mounted with `hidepid=` or `subset=pid`, the helper respects the administrator's
choice. It answers only the caller's own processes, with no total for other accounts. If the mount has a `gid=` option
and the caller is in that group, the caller is treated as allowed by that choice and gets the normal U3 answer.

**Paddock ADR 0024 §4:** this is a whole-machine table by design. It is declared with the waiver kind `machine-table`,
the one Dyno's process list already uses.

#### `Traffic`

Per-program counters are scoped like `Processes`:

- The caller's own programs and system or service programs are in detail.
- Other accounts' programs are one "Other users" total of bytes sent and received.
- Administrators see everything.
- At most 4,096 entries, which is the current `MAX_COUNTERS`.

#### `Counters(CounterQuery { ids: Vec<CounterId>, class })`

At most 8 ids per request, from a closed list. Each answer is `Reading<Value>` with a reason when unavailable.

| Id | Windows | macOS | Linux |
| --- | --- | --- | --- |
| `DiskHealth` (at most 32 disks) | NVMe/ATA health through `IOCTL_STORAGE_QUERY_PROPERTY` (protocol-specific) | – (unsupported in 1.0) | NVMe SMART log through `NVME_IOCTL_ADMIN_CMD` |
| `Thermal` (at most 64 zones) | `MSAcpi_ThermalZoneTemperature` | `powermetrics -n 1 -i 500 --samplers thermal,cpu_power -f plist`, 5 s timeout | – (sysfs needs no helper) |
| `CpuEnergy` | – | from the same `powermetrics` run | RAPL `energy_uj` (root-only since the 2020 Platypus fix) |

Disk fields are limited to model, capacity, temperature, percentage used, media errors, power-on hours and unsafe
shutdowns. **No serial numbers.**

#### `MemoryModules`

Linux only; elsewhere the agent reads this without the helper. The helper parses DMI type 17 from
`/sys/firmware/dmi/tables/DMI` (root, 0400) with a bounded parser that never panics:

- at most 64 modules;
- fields limited to slot locator, size, type, speed, form factor, manufacturer and part number;
- **no serial or asset tag**.

#### Audit

- Refusals and `Update` are written one line each.
- Reads are counted per connection and written as one line when the connection closes, so a 2 s sampler does not flood
  the log.
- `Audit` replies go to administrators only, because the log names other callers' pids and accounts.

### 6. Privacy with "any local user" (fixes (g))

On a single-person PC, U2 and U3 show the same thing. On a shared PC, a family computer, a Windows PC with RDP, or a
Linux box with SSH users, they differ.

**Under U2,** any account running Dyno sees, for every other person's processes:

- which programs they run, and when they started;
- how much CPU, memory and disk those programs use;
- their per-program network bytes (Windows).

That is enough to tell that someone is gaming at 2 a.m., runs a VPN client or a particular browser, or is idle. U2 never
shows their arguments, documents, URLs, window titles or files.

**What each OS already shows without a helper:**

- Windows lists every process name and pid to any account (`tasklist`).
- Linux `ps aux` shows every account's command lines unless `/proc` is mounted with `hidepid`, so Dyno under U2 shows
  less than `ps`, but adds resource numbers.
- macOS shows other accounts' process names but not their resources.

**Under U3,** other people's processes collapse into one "Other users" total for non-administrators. Administrators still
see everything, which they could get anyway.

**Decided: U3** (D6, the user, 2026-10-28). Any local user may install, call and use Dyno. A non-administrator sees in
detail only their own processes and traffic and those of system and service accounts. Other people appear as one
"Other users" total.

**What administrators see:** every process and every program's traffic of every account, in detail and with the same
field allow-list and redaction, plus the audit log. An administrator could read all of that anyway.

**A machine policy file** lets an administrator change the default without a rebuild: `%ProgramData%\Dyno\Helper\policy.toml`
or `/etc/dyno/helper.toml`, writable by administrators only, parsed with limits.

- `others = "total" | "detail" | "hidden"`.
- The default is `total` (U3).
- The helper reads it at start and on a file event, after checking it with the protected-location rule (section 2).
- **Exception to rule 21** ("tunables are Settings"), declared here: this tunable is a file an administrator edits, not
  a Settings page. Settings runs as the user, and letting a user's Settings widen what that user can see of others
  would defeat the setting.

## Consequences

**Install and update experience:**

- Windows users see UAC at install. Updates run through the helper with **no prompt** on any OS, and only signed, newer
  Dyno packages install. Dyno lives in `%ProgramFiles%` for every account.
- macOS users install a `.pkg` through Gatekeeper's Open Anyway flow once. After that, updates arrive with no password.
  The DMG is retired on macOS.

**Security:**

- No program run as SYSTEM or root, and no file the helper checks, sits where a non-administrator can change it.
  Problem 1 is closed for install, update and repair, without any certificate.
- What remains is same-user injection into the agent, bounded by the read scope (U3) and by `Update` accepting only
  signed, newer packages.
- The helper gains one write (installing Dyno's own signed updates, D5), a named exception to ADR 0021 point 4,
  alongside the existing `SetPerTcpConnectionEStats` exception.
- Root and SYSTEM trust exactly one signing key, kept in a separate repository that only the user can push to. No
  Paddock workflow can produce a package the helper accepts.

**Records to update:** ADR 0016's single authorised user, its token and `Uninstall` request are replaced. Its closed
request list, the audit log, the fixed endpoint and the peer-program check stand.

## Code changes and tests

Sizes: S is up to half a day, M one to two days, L three to five days. The ranges are in days.

| # | Change | Size | Days |
| --- | --- | --- | --- |
| 1 | `dyno_core::helper_wire`: `HELPER_API` 2, the new requests and replies and their caps, `HelperState`, `assess` on API, texts. `protocol`: remove `InstallHelper`/`UninstallHelper`; add `StartHelper`, `RepairHelper`, `UpdateDyno` (a job) | M | 1-2 |
| 2 | `dyno-helper` CLI: `install --machine --agent-exe`, `uninstall [--all]`; `config.json` schema 2 (no user); drop the token | M | 1-2 |
| 3 | Callers: the endpoint ACL; peer facts per OS (macOS audit token, Linux pidfd, Windows pipe pid and handle); administrator status per OS; U3 scoping and `others` policy | M | 1.5-2.5 |
| 4 | Protected-location rule (section 2), pure over `FileFacts`; Windows ACL reading in one FFI module (`GetNamedSecurityInfoW`, `// SAFETY:`) | L | 2-4 |
| 5 | Install plans per OS in place: Windows `icacls` and service security descriptor, 0.x service removal; macOS paths outside the bundle and the one-shot launchd update job; Linux unit as a package file | M | 1.5-2.5 |
| 6 | Orphan watch: start check, file event, one-shot confirmation, full removal (macOS too) | M | 1-2 |
| 7 | NSIS: `perMachine`, Program Files guard, `sc` stop, wait and kill, `$UpdateMode`, idempotent `POSTINSTALL` | M | 1.5-2.5 |
| 8 | `dyno_agent::helper_centre`: delete install and uninstall; add Start and Repair. `dyno_platform::elevate`: keep elevation for Repair only; delete the other pkexec and osascript strings; update `docs/linux.md` and `docs/macos.md` | S | 0.5-1 |
| 9 | UI: helper card states and buttons, degraded notes on the process and counter pages, the update job, mock backend, specs | M | 1-2 |
| 10 | Bundles: helper `externalBin` per OS, `check-bundle.sh`, the sidecar list | S | 0.5 |
| 11 | Docs: amend ADR 0016 and 0021 by link; rewrite the testing-manual helper section; user guide; First launch (Gatekeeper, UAC) | S | 0.5-1 |
| 12 | `Processes` per OS (field allow-list, path redaction, paging, `hidepid`, rule 26 memory), plus a fuzz target for the decoder | L | 3-5 |
| 13 | `Counters` (Windows disk health and thermal, macOS `powermetrics` parser, Linux RAPL and NVMe) and `MemoryModules` (DMI parser with a fuzz target) | L | 3-5 |
| 14 | Agent: merge helper pages into the sampler, rates in the core, the "Other users" row, class per request | M | 1.5-2.5 |
| 15 | `Update` in the helper on 3 OS: handle checks, copy with deadline and free-space check, verify, grammar and version, runner copy with protected `TEMP`, per-OS launch (detached process, one-shot launchd job, `systemd-run`), state file, finishing at the next start, restarting agents per session or uid, clean-up | L | 6-9 |
| 16 | App updater: `check` and `download` only, hand the file to the agent and the helper | S | 0.5-1 |
| 17 | macOS `.pkg` in CI (`pkgbuild`/`productbuild`, payload layout, scripts, ad-hoc signing of the programs outside the bundle, verification); the app and the launch agent use the fixed agent path; the DMG retired | L | 2.5-4 |
| 18 | Linux `.deb` maintainer scripts and the unit | S | 0.5-1 |
| 19 | CI test tools: `.deb` scripts in a throwaway container, `.pkg` expansion checks, Windows ACL tests | M | 1.5-2.5 |
| 20 | `dyno-ipc`: descriptor passing (`SCM_RIGHTS`) on Unix; handle duplication from the verified peer on Windows | M | 1.5-2.5 |
| 21 | Release: the `dyno-update-signing` repository and workflow (verify the artefacts, sign, publish `dyno-update.json`); the `.pkg` in `latest.json` and signed with the Tauri key; the agent reads `dyno-update.json` | M | 2-3 |
| 22 | Policy file: parse with limits, check by handle, reload on a file event, U3 default, tests | S | 1-1.5 |
| 23 | Trustworthy data folders: `%ProgramData%\Dyno` checks and recreation with an explicit descriptor; config, policy and updates checked by handle (Unix too) | S | 0.5-1 |

**Totals:**

- Implementation: **35.5 to 60 days** (rows 1 to 23; row 15 re-estimated for three OS; rows 20 to 23 added in revision
  3).
- The local VM matrix below: **6 to 8 days** (a first pass of about 5 days on three OS, then a re-run after fixes).
- Together: **41.5 to 68 days**.
- For comparison, rows 1-11 of the first draft summed to 8.5 to 14.5 days (review P2).

**Tests in CI** (nothing is installed on a runner; rule 7):

- **Plans and hooks:**
  - pure install, update and uninstall plans per OS, with the exact `sc.exe`, `icacls` and security-descriptor
    arguments, quoting, and idempotence;
  - an NSIS hook text test (Program Files guard, `$UpdateMode`, no old-CLI call);
  - a check that the built installer requests administrator rights.
- **The protected-location rule:**
  - every case listed in section 2, over fake `FileFacts`;
  - real ACLs on temporary folders the test creates, on the Windows runner;
  - real modes, on the Unix runners.
- **Data folders:** a pre-created `%ProgramData%\Dyno` owned by a user, and one with a junction inside, on the Windows
  runner in a temporary root the test creates. Both are renamed aside and recreated.
- **Callers and policy:**
  - U3 scoping for a caller, a system account, another user, an administrator, and the `hidden` and `detail` policies;
  - remote pipe clients refused;
  - Audit refused for non-administrators;
  - U3 for `Traffic`;
  - redaction by profile roots read through a fake `ProfileList` and a fake `getpwent`;
  - `hidepid` with and without a `gid=` match;
  - supplementary groups (fake `SO_PEERGROUPS`);
  - the polkit rule file: a text test for the exact unit, verb and subject conditions;
  - the cache uses the helper's clock and scopes per caller (a test with two callers in 1 s).
- **Request shapes:**
  - every cap (limit 0 and 1,025, 9 counter ids, unknown field bits);
  - postcard size under the frame limit at the caps;
  - path redaction on each OS's path style;
  - no command-line field exists (a test over the type's fields).
- **Parsers:** the DMI, `powermetrics` and NVMe parsers on recorded sample bytes with invented values (never real user
  data), plus fuzz targets in the existing fuzz workflow.
- **`Update`:**
  - a tampered copy, a wrong key, an older version, a wrong target or kind;
  - a link or a 513 MB file as the source;
  - a UNC, `\\?\` or device handle, a FIFO, a FUSE file and a network volume, all refused by handle;
  - an oplock or a stalled read: the deadline fires and the update is refused;
  - a second concurrent `Update`;
  - too little free space;
  - a 1,025-byte signature;
  - the grammar: a missing field, fields in the wrong order, `app=pitwall`, a pre-release, build metadata, a leading
    zero, an equal version;
  - a signature made with the family Tauri key (refused);
  - the child's environment: `TEMP`, `TMP` and `TMPDIR` point into `updates/<id>/tmp`;
  - the state file: success, a failure code, and a runner that died, each finished at the next start;
  - the source swapped after the copy, which must not change what runs;
  - a build without a key;
  - all against a fake runner, with nothing installed.
- **Watch and ADR 0024:**
  - the orphan watch with a fake clock and fake file events;
  - ADR 0024's C41 finds no sleep loop in `dyno-helper`;
  - a counting shim shows one machine read for two callers within 1 s.
- **Packages:**
  - the `.deb` scripts in a container this job creates and removes (install, upgrade, downgrade, remove, purge, no
    systemd), plus `shellcheck`;
  - the `.pkg` expanded with `pkgutil --expand`, checking payload paths, owners and scripts;
  - `codesign -d` showing the `runtime` flag on the three out-of-bundle programs, and `otool -L` showing only system
    libraries.
- **Release:** the signing workflow refuses an artefact whose Tauri signature or SHA-256 does not match, using
  invented test keys in a dry run.
- **UI:** specs for every state, the degraded notes and the update job.

**Tests in the local VMs only** (the user, before the release; written into docs/testing-manual.md):

- **Windows 11 VM**, with a standard account, an admin account and an RDP session:
  - fresh install (UAC once); an install into `D:\Apps` refused;
  - the service runs as LocalSystem from `%ProgramFiles%\Dyno`;
  - all processes shown to the admin; the "Other users" total shown to the standard account;
  - disk health and thermal;
  - as the standard account, replacing `dyno-agent.exe`, planting a DLL in `$INSTDIR`, or swapping the downloaded
    update file during `Update` must all fail to take effect;
  - update N-1 to N through the helper: no prompt, both accounts' agents back;
  - an update that fails part-way (the VM's disk filled by the tester): the message on the next start, then Repair;
  - a pre-created `%ProgramData%\Dyno` from a standard account before the install;
  - Process Monitor shows the installer's plugins unpacked under `updates\<id>\tmp`, not `C:\Windows\Temp`;
  - downgrade by hand; an "uninstall first" reinstall; a 0.x per-user copy and a 0.x click-installed service present;
  - uninstall: the service and `%ProgramData%\Dyno\Helper` are gone;
  - delete `%ProgramFiles%\Dyno` by hand: the orphan removal happens after an hour;
  - Start and Repair.
- **macOS VM** (13 and 15):
  - the Gatekeeper flow for the unsigned `.pkg`;
  - the installed app opens without a quarantine prompt;
  - the daemon runs from `/Library/Application Support/Dyno/bin`;
  - all processes and `powermetrics` work;
  - update N-1 to N through the helper with no password;
  - `DYLD_INSERT_LIBRARIES` on the agent has no effect;
  - a second account sees the total only;
  - dragging Dyno to the Bin leads to full removal within an hour;
  - Remove Dyno.
- **Linux VM** (Ubuntu; Fedora only if `rpm` is built):
  - `apt install`, upgrade, downgrade with `--allow-downgrades`, remove, purge;
  - update through the helper (`systemd-run`);
  - `pkcheck` for start (allowed) and stop (asked) of `dyno-helper.service` from a desktop session and over SSH (asked);
  - DMI memory, RAPL, NVMe;
  - a second account over SSH sees the total only;
  - `hidepid=2` respected;
  - the AppImage shows `NotIncluded`.

## Decisions only the user can make

Decided on 2026-10-28:

- **D1:** all processes and administrator-only counters in 1.0; the helper on all three OS.
- **D2:** a per-machine Windows install.
- **D3 and D4:** no signing purchases.
- **Callers:** any local user.
- **D5:** the helper installs Dyno's own signed, newer updates (P1). This is a second named exception to ADR 0021.
- **D6:** scope U3. Other people appear as one total to non-administrators. The policy file can change it.

Still open:

- **D7 (setup, one time).** Create `decryp7/dyno-update-signing`, generate the Dyno-only minisign key, store it as that
  repository's secret, and keep an offline backup. The public key goes into `crates/dyno-helper`.
- **Rule 23 exception.** The hardened runtime on the three ad-hoc signed programs outside the macOS bundle (section 1).
  It is proposed here; the user confirms it with this ADR.
- **Later, unchanged:** D3 (Windows signing: Artifact Signing is for individuals in the US and Canada only, on a paid
  Azure subscription, Basic tier roughly USD 10 a month, amount not shown on the pricing page; or an OV certificate) and
  D4 (Apple Developer Program, USD 99 a year). Either would add a named publisher and make Gatekeeper and SmartScreen
  quiet. Neither is needed by this design.

## Sources checked (2026-10-28)

- **Tauri config schema** shipped with `@tauri-apps/cli` 2.12.3 (in the repository's `node_modules`):
  - `NSISInstallerMode` (`perMachine` uses Program Files and HKLM);
  - `installerHooks` (the four hooks);
  - `allowDowngrades` (default `true`);
  - `DebConfig`/`RpmConfig` maintainer-script keys;
  - `WixConfig.fragmentPaths`/`componentRefs`;
  - `BundleType` (no `pkg`).
- **Tauri's NSIS template**, `crates/tauri-bundler/src/bundle/windows/nsis/installer.nsi` (dev branch):
  - `RequestExecutionLevel admin` for `perMachine`;
  - `$UpdateMode` from `/UPDATE`, and `/UPDATE` passed to the uninstaller;
  - update mode skips "uninstall first";
  - `CheckIfAppIsRunning` covers the main binary only.
- **`tauri-plugin-updater` 2.14.0 source:**
  - Windows: a temporary file, then `ShellExecuteW`;
  - Linux: a temporary folder, then `pkexec dpkg -i` or `rpm -U`;
  - macOS: a bundle swap with an `osascript` fallback.
- **Tauri docs:** "Windows Installer" and "Debian" (v2.tauri.app/distribute).
- **Apple documentation:**
  - `SMAppService` and `daemon(plistName:)`;
  - `Status.requiresApproval`;
  - "Updating helper executables from earlier versions of macOS".
- **Apple Developer Forums** 745720 and 768592 (forum reports from DTS and developers, not documentation).
- **macOS 15 Gatekeeper change:** The Hacker News (2024-08) reporting Apple's statement; Macworld and troz.net for the
  Privacy & Security > Open Anyway flow. These are secondary sources. Apple's own support page was not found in the
  search.
- **Folder modes** on the build Mac (macOS, `stat`): `/Applications` `root:admin 0775`; `/Library/Application Support`
  `root:admin 0755`; `/Library/PrivilegedHelperTools` `root:wheel 1755`; `/Library/LaunchDaemons` `root:wheel 0755`.
- **Microsoft Learn:**
  - Artifact Signing quickstart (individuals: US and Canada) and FAQ;
  - Azure pricing page (tiers; the amount was not shown);
  - SetPerTcpConnectionEStats (as cited in ADR 0021).
- **Paddock ADR 0024** (accepted 2026-10-28) §1, §2 and §4; `dyno-ipc` `MAX_FRAME` (4 MB).
- **Gitea documentation**, "Secrets" (docs.gitea.com/usage/actions/secrets): user, organisation and repository levels
  only. No per-workflow or environment scope is documented.
- **Not verified, left to the VM checks:**
  - that `/usr/sbin/installer` run as root applies no Gatekeeper check to an unquarantined package;
  - that Installer does not pass quarantine on to the payload;
  - `powermetrics` sampler names on macOS 13 and 15;
  - whether Tauri's signer already writes a usable trusted comment (the design does not rely on it: CI signs a second
    time).
