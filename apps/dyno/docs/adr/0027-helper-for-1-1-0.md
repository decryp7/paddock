# 27. Dyno 1.1.0: the privileged helper, switched on by the user, started with the OS

- Status: Proposed (2026-11-03). Open decisions D1 to D8 below; the recommendation for each is marked.
- Date: 2026-11-03
- Related:
  - Amends ADR [0016](0016-privileged-helper-install-and-ipc.md) (install on a click and the closed request list stay; the
    single authorised user, the per-user token and the private `dyno-ipc` are replaced) and ADR
    [0022](0022-helper-installed-with-the-app.md) (Proposed, revision 3). This record is the 1.1.0 plan; it adopts 0022's
    read set, scope and trust rules, and defers 0022's packaging and helper-run updates (D1).
  - Keeps ADR [0006](0006-privileged-helper-and-observe-only.md) point 3 (the app and the agent never run elevated) and ADR
    [0021](0021-monitoring-only.md) (monitoring only: the helper changes nothing except installing or removing itself and the
    one tool in D3).
  - Family: ADR 0001 and `docs/known-copies.toml` (the `dyno-ipc` copies expire at `dyno-v1.1.0`), ADR 0022 G.7 (no `crates/`
    change inside a release window), ADR 0024 (no polling), ADR 0034 / rule 55 (one implementation per mechanism), the accepted
    redesign ADRs 0037 (composition contract), 0039 (release gates inspect the artifacts), 0040 (Windows GUI test session) and
    0041 / rule 60 (deterministic timing), each read on its branch (`cad10f67b`, `8bf509f99`) on 2026-11-03.
  - Rules 7 (ER-14), 19, 23, 24, 26, 55; ER-27 (logs).
- Trigger: the user's feedback on Dyno 1.0.0 (2026-11-03): "Run as administrator, also at start-up", "smartmontools for all
  os", and the network plugins (firewall). The user confirmed on 2026-11-03 that 1.1.0 includes the helper on all three OS.

## Context

### What 1.0 cannot read, found in the code (`origin/main` `9da2ccf59` = `dyno-v1.0.0`, and `fix/dyno-disk-startup` `17bae21b1`)

Every consumer below today returns `Unavailable` / `AccessDenied` with a reason. Paths are under `apps/dyno/crates/`.

| # | Reading | Where 1.0 marks it | Windows | macOS | Linux |
| --- | --- | --- | --- | --- | --- |
| C1 | Other accounts' and system processes: path, owner, memory, CPU, disk | `dyno-platform/src/readability.rs` (`UnavailableReason::{NeedsAdmin, NotPermitted, ResidentMemoryOnly}`), `dyno-core/src/others.rs`, `dyno-agent/src/sampler.rs` | path, owner, private working set | CPU, footprint, disk | disk I/O, path, PSS |
| C2 | Per-program network traffic | `dyno-platform/src/windows/traffic.rs`, `parse/win_estats.rs` | needs admin (`SetPerTcpConnectionEStats`) | n/a | n/a |
| C3 | Which program owns a socket of another account | `dyno-core/src/network.rs:202` (`UNKNOWN_REASON`) | readable (pid from the TCP table) | readable from `pcblist_n` (ADR 0026) | needs root (`/proc/<pid>/fd` of others) |
| C4 | Which processes hold a file | `dyno-platform/src/linux/locks.rs:111` ("Run Dyno as root") | n/a | n/a | needs root |
| C5 | Login items and app-registered agents (BTM) | `dyno-platform/src/macos/startup.rs` `BtmSource`, only impl `NeedsHelperBtm` (branch `17bae21b1`) | n/a | `sfltool dumpbtm` asks for `system.privilege.admin`: measured in a macOS 26.6 VM, it hangs and leaves a SecurityAgent dialog | n/a |
| C6 | SSD wear / SMART | `dyno-platform/src/hardware.rs` (`SmartctlWear`, fixed candidate paths), `parse/hardware.rs:70`, `linux/facts.rs:100` | needs admin and smartmontools | smartmontools; rights to be measured (WP-0) | needs root and smartmontools |
| C7 | Temperatures, fans, CPU energy | `windows/pdh.rs:17`, `macos/probe.rs:137` | thermal zones need admin | `powermetrics` needs root | RAPL `energy_uj` is root-only |
| C8 | Memory slot table (DMI type 17) | `dyno-core/src/inventory.rs:189` | readable | readable | root-only |
| C9 | What keeps the PC awake; Windows Backup versions | `dyno-platform/src/extras.rs:59, :424` | admin on many versions | n/a | n/a |
| C10 | Firewall state and rules (network plugins) | nothing yet | profiles and rules readable without admin (secondary sources, WP-0 checks) | Application Firewall readable as a user; `pf` needs root | `nft` needs `CAP_NET_ADMIN` |
| C11 | Defender exclusions | `dyno-core/src/facts.rs:55` | admin (deliberately hidden by Windows) | n/a | n/a |
| C12 | System event logs, journal | `issues/windows.rs:191`, `issues/linux.rs:138` | Event Log Readers group | n/a | `systemd-journal` group |
| C13 | Folders the disk map cannot read | `dyno-core/src/disk/reason.rs` | admin | Full Disk Access (not admin) | root |
| C14 | Energy per app | `extras.rs:173` (ETW) | admin, ETW session | n/a | n/a |

The helper answers C1 to C10. C11 to C14 stay outside it (section 6, D6).

### What exists to build on

- `crates/dyno-helper` (2,288 lines, excluded from the workspace in `apps/dyno/Cargo.toml:16`): `serve`, `install`,
  `uninstall`, a Windows service host, an audit log, ADR 0016's token and single user. `dyno-core/src/helper_wire.rs`:
  `HELPER_API = 1`, `Info`, `Traffic`, `Audit`, `Uninstall`.
- `crates/paddock-service`: one `ServiceSpec` with `Kind::SystemService`, rendered plist / systemd unit / `sc.exe` calls, backends
  `LaunchdFile`, `LaunchdBundled` (`SMAppService`), `Systemd`, `WindowsService`, `ElevatedWindowsService`, all over a fake
  `System` port.
- `crates/paddock-ipc`: endpoints with an SDDL (Pitcrew's service pipe: `SY`, `BA`, `OW`, `IU`) and `peer_uid`. Dyno still runs
  on its copy `dyno-ipc`, registered in `docs/known-copies.toml` with `delete_by = "dyno-v1.1.0"`.
- `crates/paddock-runtime` (pinned catalogue, `paddock-download` with hashing while writing) and `paddock-net` (opt-in egress).
- ADR 0022 (Proposed): the read requests `Processes`, `Traffic`, `Counters`, `MemoryModules` with caps, field allow-list and
  path redaction; scope U3 and the policy file; the protected-location rule; `HelperState`; per-machine packaging; helper-run
  updates with a Dyno-only key (open P1: "Keep the family key for now" makes `Update` unsafe).

### What "run as administrator, also at start-up" means here

The app and the agent stay unprivileged (ADR 0006 point 3; the session-only "Run as administrator" was dropped from 1.0 on
2026-10-28 after a security review). "Run as administrator" is the helper switched on: a resident, idle system service that
answers a closed list of read requests. "Also at start-up" is that service starting with the OS, so when the agent starts at
login (ADR 0010's switch) the full readings are there with no prompt.

### Upgrade from 1.0 constrains the packaging

1.0 is installed per user on Windows (NSIS `currentUser`), dragged from a DMG on macOS, and a `.deb` or AppImage on Linux. ADR
0022's packaging (per-machine NSIS that refuses anything outside Program Files, a `.pkg` replacing the DMG) cannot be reached by
the Tauri updater from a 1.0 install: the per-machine installer's guard aborts on a `%LOCALAPPDATA%` target, and a DMG app
cannot run a `.pkg`. Every 1.0 user would reinstall by hand, and macOS users would go through Gatekeeper's Open Anyway flow
for the unsigned `.pkg`.

## Options (D1)

| | **L. Switched on by the user (recommended for 1.1.0)** | P. Installed with the app (ADR 0022 as written) |
| --- | --- | --- |
| Packaging | unchanged: per-user NSIS, DMG, `.deb`, AppImage | per-machine NSIS, unsigned `.pkg`, `.deb` scripts |
| Install | Settings > Administrator readings > Turn on: one OS prompt; the helper copies itself to a protected folder and registers a system service | at install time, by the installer |
| Upgrade from 1.0 | the normal Tauri update; the switch is offered once | a manual reinstall on Windows and macOS |
| Helper updates | from the app's new copy, one prompt, only when needed (section 4) | the helper installs Dyno's signed updates with no prompt |
| Signing keys | none new | a Dyno-only update key in a separate repository (D7 of 0022, reversed by the user on 2026-10-28) |
| Residual risk | the one elevated run starts from the app folder, which the same user can write (section 3, R1) | none for install and update; first install trusted like any download |
| Agent-days | 43 to 65 (Work packages) | add 13 to 20.5 (0022 rows 7, 15, 17, 21, plus the 1.0 migration) |

**Recommendation: L for 1.1.0.** It ships the user's request without a forced reinstall, a second signing key or a new
Gatekeeper flow, and every part that P adds can be added later without changing the wire or the scope (P's packaging builds on
L's protected copy and service). Revisit P when a signing identity exists (0022's D3/D4), because then `SMAppService` on macOS
and a signed per-machine installer on Windows become sound.

## Decision (proposed, assuming D1 = L)

### 1. Per OS: install, start, uninstall

Registration goes through `paddock-service` (`Kind::SystemService`), not through `dyno-helper`'s own install plan (rule 55, one
implementation per mechanism). `dyno-helper install` is the elevated program; it builds a pure install plan and runs it through
`paddock-service`'s backends.

| | macOS | Windows | Linux (`.deb`) |
| --- | --- | --- | --- |
| The OS prompt | `osascript` "do shell script … with administrator privileges" running `dyno-helper install` from the bundle | `ShellExecuteExW` verb `runas` (UAC) on `dyno-helper.exe install` from the install folder | `pkexec /usr/bin/dyno-helper install` |
| Protected copy | `/Library/PrivilegedHelperTools/net.decryptology.dyno.helper` (root:wheel 0755; folder `root:wheel 1755`) | `%ProgramFiles%\Dyno Helper\dyno-helper.exe`, explicit ACL: SYSTEM and Administrators Full, Users Read & execute, no inheritance | none needed: `/usr/bin/dyno-helper` is owned by root and by dpkg |
| Registration | `paddock-service` `LaunchdFile`: `/Library/LaunchDaemons/net.decryptology.dyno.helper.plist`, `launchctl bootstrap system`, `RunAtLoad`, `KeepAlive` on failure | `paddock-service` `WindowsService`: `DynoHelper`, LocalSystem, auto start, restart on failure, quoted `binPath`, **required privileges** (`SERVICE_CONFIG_REQUIRED_PRIVILEGES_INFO`: `SeDebugPrivilege`, plus any privilege WP-0 shows `wbadmin` or the SMART installer needs; the SCM removes the rest from the token), a service DACL that lets interactive users *start* it | unit `/usr/lib/systemd/system/dyno-helper.service` shipped **disabled** in the package; `install` runs `systemctl enable --now` |
| Data | `/Library/Application Support/Dyno/Helper/` (0700) | `%ProgramData%\Dyno\Helper\`, created with create-new and an explicit descriptor; an existing folder that fails the protected-location rule is renamed aside (0022 §1 and its P2 list) | `StateDirectory=dyno-helper` (0700) |
| Endpoint | `/var/run/dyno-helper/helper.sock` 0666 in a root-owned 0755 folder | `\\.\pipe\net.decryptology.dyno.helper`, SDDL `SY`, `BA` full, `IU` read/write, `NU` denied, `PIPE_REJECT_REMOTE_CLIENTS` | `RuntimeDirectory=dyno-helper`, socket 0666 |
| Start at boot | launchd at boot | SCM auto start | systemd `WantedBy=multi-user.target` |
| Turn off | Settings > Turn off: the authenticated `Uninstall` request (owner or administrator only); `dyno-helper uninstall` elevated when the helper does not answer | the same; the per-user NSIS uninstaller's `PREUNINSTALL` hook asks for one UAC prompt to run the protected copy's `uninstall` | the same through `systemctl disable --now`; `prerm remove` stops and disables; `postrm purge` removes the state folder |
| Orphan | the orphan watch of 0022 §3 over every recorded app location (one-shot one-hour deadline, `paddock-watch` events, no loop) | same | dpkg owns it |

**Linux hardening** (systemd.exec(5), freedesktop.org, read 2026-11-03): `ProtectSystem=strict`, `ProtectHome=read-only`,
`PrivateTmp=yes`, `NoNewPrivileges=yes`, `RestrictAddressFamilies=AF_UNIX AF_NETLINK`, `IPAddressDeny=any`,
`CapabilityBoundingSet=CAP_SYS_PTRACE CAP_DAC_READ_SEARCH CAP_SYS_RAWIO CAP_SYS_ADMIN CAP_NET_ADMIN` (ptrace-class `/proc`
reads, root-only files, SMART pass-through, `nft`), `SystemCallFilter=@system-service` and `~@mount` (the manual pairs
`ProtectSystem` with denying mounts, because `CAP_SYS_ADMIN` is needed for NVMe admin commands). The polkit rule of 0022 §1
(start only, local active session) ships as a package file. **AppImage: no helper** (`NotIncluded`): root cannot read another
user's FUSE mount, and its path changes every run.

**macOS: why not `SMAppService` here (D2).** Dyno is ad-hoc signed (rule 23). Apple DTS answers on the developer forums say a
daemon registered through `SMAppService` should be signed with an Apple-issued identity shared with the app, and ad-hoc signing
"will see problems like this" (threads 733046, 761249, 757463, read 2026-11-03; forum answers, not documentation). The daemon
would also run from inside `/Applications/Dyno.app` (`root:admin 0775`, measured on the build Mac on 2026-10-28), which an
admin-group process can change, and every update's new signature hits the launch-constraint loop of lesson 2026-10-23
(rule 24). A plist file plus a root-owned copy avoids all three. Like any LaunchDaemon, it appears in System Settings > Login
Items & Extensions; switching it off there is the `NotRunning` state with its own text.

### 2. Who may call, and what each caller sees

- **Peer facts from the kernel** (in `paddock-ipc`, WP-1): macOS `LOCAL_PEERTOKEN` audit token (pid, pid version, uid) and
  `proc_pidpath_audittoken`; Linux `SO_PEERCRED`, `SO_PEERGROUPS` and `/proc/<pid>/exe` through a pidfd; Windows
  `GetNamedPipeClientProcessId`, the client's token (SID, Administrators membership even when UAC filtered it) and
  `QueryFullProcessImageNameW`. If a fact cannot be read the connection is refused (fail closed).
- **No token, any local user.** ADR 0016's per-user token is dropped (0022 §2.2).
- **Scope (0022 §6, U3) plus the owner.** Full detail for administrators and for the **owner**, the account that turned the
  helper on (recorded by `install --for <uid|SID>`, which the agent passes; a different account turning it on again becomes the
  owner, guarded by the OS prompt). Every other account: its own processes and system/service accounts in detail, other people
  as one "Other users" total. The administrator-edited policy file (`others = "total" | "detail" | "hidden"`) stays (D4).
- **The caller's program** is recorded in the audit log. It is a boundary only where its path is protected (`.deb`:
  `/usr/bin/dyno-agent`); with L's per-user and `/Applications` installs a same-account program can call the helper, and it gets
  exactly what that account may see (R2). That is why the answers are read-only and scoped by the account, never by the program.

### 3. Trust and residual risks

- **Nothing elevated runs from, or is read in, a place a non-administrator can change, except the one install run.** The
  service runs the protected copy only; `serve` and every tool run check 0022 §2.1's protected-location rule (pure, over a
  `FileFacts` port) on the program, the config, the policy file and the tools folder, by handle.
- **R1 (accepted, as ADR 0016 §3 did):** the install and "Update the helper" runs elevate the program in the app folder, which
  the same account can replace before the prompt. A program already running as that account can raise any prompt it likes, so
  this does not widen what it can do; the prompt names the program, and the helper copies itself by handle and records the
  copy's SHA-256 in the audit log. Prompts are rare (section 4), so they do not train the user to click through.
- **R2:** same-account injection or impersonation gets that account's scope. Bounded by the field allow-list (never command
  lines, environment, window titles, open files or memory; 0022 §5) and by redaction of other accounts' profile paths.
- **The helper never opens a path a caller chose.** Files come as open descriptors or duplicated handles (WP-1), devices for
  SMART come from the helper's own scan, file-holder queries carry `(device, inode)`, never a path.
- **The helper has no network access** (`IPAddressDeny=any` on Linux; nothing in the code on any OS). Downloads happen in the
  agent through `paddock-net`, opt-in.
- **No plugins in the helper, and plugins never talk to it.** The agent brokers typed views to plugins (ADR 0007) if the plugin
  design asks for them.

### 4. Upgrade, helper updates, version skew

- 1.0 to 1.1.0 is the normal Tauri update. At the first start of 1.1.0 the Health page shows one dismissable line: "Dyno can
  show every process and the readings that need administrator rights. Turn on administrator readings" (a button, not a link to
  the page it is on; rule 19).
- **Compatibility is `HELPER_API` (now 2), not the version.** Same API: `Ready`, and Settings shows both versions. Different API,
  or the installed helper is older than the app's declared `MIN_HELPER_VERSION` (raised only when a helper fix must reach users):
  `Outdated`, with "Update the helper" (one prompt, the install again). Otherwise the newer bundled helper is offered, never
  forced, so most app updates ask nothing.
- `.deb`: the package upgrades `/usr/bin/dyno-helper` and `postinst configure` runs `deb-systemd-invoke try-restart` (only if the
  user enabled it). No prompt.
- A 0.x helper registered under the same label (ADR 0016 development builds) is replaced by `install`, which is idempotent and
  takes over the label, the service name and the data folder (re-created under the protected-location rule).
- Rule 24 applies to the agent's own login service, not to the daemon: the daemon is a plist file run from a root-owned copy, so
  no launch constraint pins an old build.

### 5. The request list (`HELPER_API` 2)

All requests are decoded into typed values (parse, don't validate), checked again in the helper, capped, run on a worker at the
caller's class (`User` or `Background`, ADR 0024 §1), with a timeout for every tool, and answered under the 4 MB frame limit.
The helper keeps no samples and has no loops; a machine read is cached for 1 s on the helper's own clock (0022 §5).

| Request | Answers | OS | How |
| --- | --- | --- | --- |
| `Info` | version, API, OS, scope of this caller, tool states | all | |
| `Processes(ProcessQuery)` | C1, paged, scoped, redacted | all | 0022 §5 unchanged |
| `Traffic` | C2, scoped | Windows | existing `dyno-helper` code |
| `SocketOwners` | C3: socket inode to `ProcessRef`, scoped | Linux | `/proc/<pid>/fd` links, bounded (16,384 processes, 65,536 sockets) |
| `FileHolders { dev, ino }` | C4: `ProcessRef`s holding that file, scoped | Linux | `/proc/<pid>/fd` and `/proc/locks`; the helper never opens the file |
| `LoginItems` | C5: typed `BtmItem`s of the **caller's uid** plus system-wide items | macOS | `/usr/bin/sfltool dumpbtm`, fixed arguments, 15 s timeout, parsed by `parse/btm.rs` (`parse_dumpbtm(text, uid)`, branch `17bae21b1`) inside the helper; the raw dump never leaves it, other users' records are only counted |
| `Counters(ids)` | C7: `Thermal`, `CpuEnergy`, `Fans` | all | 0022 §5 (`MSAcpi_ThermalZoneTemperature`, `powermetrics`, RAPL); its native `DiskHealth` is replaced by `Smart` |
| `MemoryModules` | C8 | Linux | 0022 §5 (DMI parser, no serials) |
| `Smart(SmartQuery)` | C6: per-disk model, capacity, temperature, percentage used, media errors, power-on hours, unsafe shutdowns; **no serial numbers or WWN** | all | `smartctl --scan-open -j`, then `smartctl -a -j <device>` for devices from that scan only; existing `parse/hardware.rs` parsers; 20 s per disk, at most 32 disks |
| `InstallTool(Smartmontools, file)` | C6: puts the downloaded upstream package into the helper's tools folder | Windows, macOS | section 7 |
| `Firewall` | C10: typed, bounded view (state per profile or table, default policies, at most 4,096 rules with name, direction, action, program, ports, addresses) | macOS `pf`, Linux | `pfctl -s info`, `pfctl -s rules`; `nft -j list ruleset` (JSON, capped at 4 MB), firewalld/ufw state derived from it. Windows profiles and rules, and the macOS Application Firewall, are read by the agent without the helper (WP-0 confirms) |
| `Extras(WakeRequests \| BackupVersions)` | C9 | Windows | `powercfg /requests`, `wbadmin get versions`; fixed arguments, 15 s |
| `Audit(n)`, `LogTail(n)` | the audit log, the helper's log for the diagnostics bundle | all | owner and administrators only |
| `Uninstall` | removes the helper | all | owner and administrators only |

Removed from ADR 0016: the single authorised user and the token. Not added: anything that changes a setting.

**`BtmSource` changes shape.** `dump() -> String` becomes `items() -> Result<Vec<BtmItem>, PlatformError>`, so parsing happens
at the edge that has the rights (the helper) and the agent never sees other accounts' records. Implementations: `NeedsHelperBtm`
(unchanged message, minus "coming in Dyno 1.1", rule 19) and `HelperBtm` (the agent's helper link). The agent's 5-minute cache
from `17bae21b1` stays.

### 6. Not in the helper (D6)

- **Defender exclusions (C11):** Windows hides them from non-administrators on purpose (malware looks for excluded folders).
  Handing them to any same-account program through the helper undoes that.
- **Event logs and the journal (C12):** they carry other people's activity in free text; the existing advice (Event Log Readers,
  `systemd-journal`) stays.
- **Disk map of unreadable folders (C13):** root walking caller-chosen trees is file I/O on user-chosen paths, and the sizes of
  other people's folders are private.
- **Energy per app (C14):** an ETW session is a write-shaped, resident collector; not in 1.1.0.

### 7. smartmontools on demand (D3)

- **Licence.** smartmontools is GPL-2.0-or-later. Dyno never links it, never bundles it and never mirrors it: the user downloads
  the upstream release on a click, and Dyno runs `smartctl` as a separate program. This is a named exception to the licence
  allow-list, like Sighter's CUDA runtime (2026-10-29). About lists it as a downloaded tool with its licence and source link.
- **Upstream artefacts** (GitHub API, `RELEASE_7_5`, published 2025-05-12, checked 2026-11-03): `smartmontools-7.5.win32-setup.exe`
  (1,508,408 bytes, NSIS, carries 64-bit binaries), `smartmontools-7.5.dmg` (1,619,514 bytes), `smartmontools-7.5.tar.gz`, each
  with a `.asc` PGP signature and an `.md5`. **There is no upstream Linux binary.**
- **Pinning.** A `paddock-runtime` catalogue entry per OS (URL, size, SHA-256). The SHA-256 is computed once by the developer
  after checking the `.asc` against the smartmontools signing key, and the same table is compiled into the helper, which
  checks the bytes itself: the agent's catalogue is never trusted by root.
- **Windows and macOS.** The agent downloads (opt-in, `paddock-net` egress, `github.com` and its release-asset host, a job with
  phases) and passes the open file to `InstallTool`. The helper copies it by handle into `tools/.staging/`, checks size and
  SHA-256, then:
  - Windows: runs the copy as `/S /SO <components> /D=%ProgramFiles%\Dyno Helper\tools\smartmontools-7.5` with `TEMP`/`TMP`
    pointing into the staging folder (0022's P1-3). The installer's own options are
    `[/S] [/SO component,...] [/D=INSTDIR]` (`os_win32/installer.nsi`, tag `RELEASE_7_5`, read 2026-11-03); components chosen so
    no `smartd` service, no Start-menu entries and no uninstaller registry key are written (the exact list is fixed in WP-0).
  - macOS: `hdiutil attach -readonly -nobrowse -mountrandom <staging>`, `pkgutil --expand-full` of the package inside into
    staging, copy `smartctl` and `drivedb.h` into `tools/smartmontools-7.5/`, detach. Nothing is installed system-wide. If WP-0
    shows `smartctl` reads SMART on macOS without root, the agent instead runs it from its own `paddock-runtime` store and the
    helper is not used for SMART on macOS.
- **Linux.** The agent asks PackageKit to install the distribution's `smartmontools` package (`InstallPackages` over D-Bus; the
  user authenticates through PackageKit's own polkit action). The helper then runs `/usr/sbin/smartctl` (protected path). Without
  PackageKit, the text gives the distribution's command.
- **Updates of the tool** come with Dyno: a new pinned version in the catalogue is offered as "Update smartmontools".

### 8. Failure modes: degrade, never fail

`HelperState` of 0022 §4 (`NotIncluded`, `NotRunning`, `Missing`, `Outdated`, `Ready`) plus `Off` (never turned on: "Turn on
administrator readings" with what it shows and the one prompt). Every reading the helper would give is `Unavailable` with the
state's reason and its button, never 0.

| Failure | What happens |
| --- | --- |
| Prompt declined or dismissed | nothing changes; the card says "Not turned on" with the button |
| Service stopped, or switched off in Login Items (macOS) | `NotRunning`; Start (no prompt on Windows and Linux; on macOS, Repair) |
| Copy, config or data folder fails the protected-location rule | the helper refuses to serve and logs why; `Missing` with Repair (one prompt) |
| Peer facts unreadable | that connection refused; the agent shows "The helper could not check who is asking" and logs the OS error |
| A tool missing, failing or timing out (`sfltool`, `smartctl`, `powermetrics`, `pfctl`, `nft`, `powercfg`) | that request answers `Unavailable` with the tool, its exit code and the next step; the process is ended by its handle (rule 53); other requests are unaffected |
| `sfltool` still asks for authorization as root (WP-0) | `LoginItems` answers `Unavailable` ("macOS would not list login items even for the helper") and C5 stays as in 1.0 |
| Download refused, interrupted or hash mismatch | the job fails with the phase and the reason; nothing reaches the helper; Retry |
| Tool install fails | staging is deleted; the state stays "not installed"; the audit log has the step and exit code |
| Policy file invalid | defaults (U3) and an audit line naming the line and field |
| Reply over a cap | truncated with `truncated: true` and the count; the UI says "showing N of M" |
| App removed, helper left | orphan watch removes the helper after the one-hour confirmation (0022 §3) |

### 9. Logs (ER-27)

- The helper logs through `paddock-log` to its data folder (0600, rotated, administrators only), and `LogTail` puts the last
  entries into the agent's diagnostics bundle.
- Lines: start (version, API, OS, policy, protected-location verdicts), every connection (correlation id, peer pid, account,
  administrator yes/no, scope, image path), every request (kind, class, duration, item count, truncation, outcome), every tool run
  (the exact command line, exit code, last 2 KB of stderr, duration, timeout), install and uninstall phases with durations, and
  each refusal with its reason.
- Never in a log: process command lines (never read), other accounts' paths (redacted as in the answers), BTM item names (counts
  only), firewall rule addresses (counts only), disk serials (never read into the reply).
- The agent logs every `HelperState` change with its cause, each call's correlation id and duration, and the tool jobs' phases.

### 10. Tests and release gates

**CI (nothing installed, no real service; rule 7 / ER-14; fake clocks only, rule 60 / ADR 0041):**

- `paddock-service` goldens for the daemon plist, the hardened unit, the `sc.exe` calls including required privileges and the
  start DACL; the elevated `install` and `uninstall` plans over the fake `System`, including taking over a 0.x registration.
- The protected-location rule over fake `FileFacts` (every case of 0022 §2.1) and on real temporary folders per runner.
- Scope: owner, administrator, other account, system account; policy `total`/`detail`/`hidden`; refused peers; Windows filtered
  administrator tokens through a fake token port.
- Request caps and decoding: every limit and one over it, unknown variants, the 4 MB frame at the caps.
- Parsers on invented sample bytes with fuzz targets: `parse/btm.rs` (several uids; only the caller's survive), `smartctl -j`
  (serials stripped), `nft -j`, `pfctl`, `powermetrics`, DMI, `powercfg`, `wbadmin`.
- Tool runs through a fake runner: timeouts fire on a `FakeClock`, exit codes and stderr reach the reply and the log.
- `InstallTool`: wrong hash, wrong size, a link, a FIFO, a network volume, a second concurrent install, `TEMP` inside staging.
- `paddock-ipc`: descriptor and handle passing, peer facts with fakes per OS; the Windows pipe refuses a remote client on the
  Windows runner with a pipe the test creates.
- `.deb`: maintainer scripts in a throwaway container (install, upgrade with and without the helper enabled, remove, purge, no
  systemd), `shellcheck`, the polkit rule text.
- UI: every `HelperState`, the switch, the first-start line, the SMART job; the composition contract of ADR 0037 for the new
  commands and their state.

**Local VMs only (never CI, never the user's machine):** a written matrix in `docs/testing-manual.md`, run by the scripts of WP-14:

- macOS 15 and 27 (GUI vm-smoke): turn on (password dialog), the daemon runs from `/Library/PrivilegedHelperTools`, survives a
  reboot and gives full readings after login with no prompt; BTM through the helper; SMART; `pf`; a second, standard account
  sees the total only; switch it off in Login Items; update 1.0 to 1.1.0; drag Dyno to the Bin and see the orphan removal.
- Windows 11 (ADR 0040 GUI session): UAC once; the service's token holds only the declared privileges (Process Explorer); sign out and
  in: full readings, no prompt; a standard account in a second session (RDP) sees the total; SMART install leaves no `smartd`
  service and no Add/Remove entry; uninstall through the per-user uninstaller removes the service.
- Ubuntu: `apt install`, enable from Settings with `pkexec`, reboot, `pkcheck` for start and stop, PackageKit install of
  smartmontools, `nft`, an SSH user sees the total, upgrade and purge; AppImage shows `NotIncluded`.

**Release gates (ADR 0039 `apps/dyno/release.toml`):** `dyno-helper` declared as an executable on every OS (`min_bytes`,
`macos_identifier = "net.decryptology.dyno.helper"`, `probe = ["--self-test"]` started from the extracted artifact), the `.deb`
carries the unit and the polkit rule at their paths with the unit disabled, the Windows `dyno-helper.exe` has the PE version and
the `asInvoker` manifest (it is elevated by the `runas` verb, never by its manifest), and `cargo deny` sees no GPL crate (smartmontools
is never a dependency). The tag waits for the VM matrix above.

## Work packages

Agent-days are ranges for one agent. **crates/** marks packages that change shared crates: they land outside a release window
(ADR 0022 G.7) and update every app that uses the crate in the same change (rule 43).

| WP | Package | Depends on | crates/ | Days |
| --- | --- | --- | --- | --- |
| 0 | VM spikes: `sfltool dumpbtm` as root from a LaunchDaemon (no SecurityAgent?); `smartctl` rights on macOS; the NSIS `/SO` component names; firewall reads without admin on Windows and the macOS Application Firewall; how macOS 15 and 27 show a plist-file daemon in Login Items | – | | 2 |
| 1a | `paddock-ipc`: a fixed system endpoint (Unix path and mode; pipe name, SDDL, remote clients refused), peer facts per OS (audit token, `SO_PEERCRED`/`SO_PEERGROUPS`/pidfd, client token and Administrators membership), descriptor passing (`SCM_RIGHTS`) and handle duplication from the verified peer | – | yes | 2.5–3.5 |
| 1b | Dyno moves from `dyno-ipc` to `paddock-ipc` (agent, CLI, plugin host, helper); `dyno-ipc` deleted and its `known-copies.toml` entries removed (due `dyno-v1.1.0` anyway) | 1a | | 2–3 |
| 2 | `paddock-service`: daemon plist for a root-owned copy, Windows required privileges and the start DACL, systemd hardening keys from the spec, goldens | – | yes | 1.5–2.5 |
| 3 | `dyno_core::helper_wire` v2: `HELPER_API` 2, the requests of section 5 with caps, `HelperState` with `Off`, scope types, `MIN_HELPER_VERSION` | – | | 1.5–2 |
| 4 | `dyno-helper` core: `install --for`, protected copy by handle, config schema 2 with the owner, protected-location rule (pure plus Windows ACL FFI), trusted data folders, scope and policy file, audit, `LogTail`, `Uninstall`, orphan watch, `--self-test` | 2, 3 | | 6–10 |
| 5 | `Processes` per OS (0022 row 12: field allow-list, redaction, paging, `hidepid`, rule 26 memory) | 3, 4 | | 3–5 |
| 6 | `Counters` and `MemoryModules` without native disk health | 3, 4 | | 2–3.5 |
| 7 | `LoginItems`: the helper side with `parse/btm.rs`, `BtmSource::items`, `HelperBtm` in the agent (needs `fix/dyno-disk-startup` merged) | 0, 4 | | 1–1.5 |
| 8 | smartmontools: catalogue entries and the pinning step, the agent's opt-in download job, `InstallTool` per OS, Linux PackageKit, `Smart` reads, UI job (if `paddock-runtime` needs an artefact-only mode: crates/) | 0, 1a, 4 | maybe | 4–6 |
| 9 | Firewall: the agent's unprivileged readers (Windows, macOS Application Firewall), the helper's `pf` and `nft`, one typed view for the plugin design | 0, 4 | | 2.5–4 |
| 10 | `SocketOwners`, `FileHolders` (Linux), `Extras` (Windows) | 4 | | 1.5–2.5 |
| 11 | Agent: the helper link, helper pages merged into the sampler (the "Other users" row off when the scope is full), Turn on / Start / Repair / Update / Turn off as jobs with phases, logs | 1b, 3 | | 2–3 |
| 12 | UI: the Administrator readings card and its states, the first-start line, degraded notes per page, the SMART job, mock backend, specs, ADR 0037 declarations | 3, 11 | | 2–3 |
| 13 | Packaging: helper `externalBin` per OS, `.deb` unit, polkit rule and maintainer scripts, the NSIS `PREUNINSTALL` step, `check-bundle`, `release.toml` entries | 4 | | 1.5–2.5 |
| 14 | Test tooling: VM matrix in vm-smoke (macOS GUI, Windows GUI per ADR 0040, Ubuntu), `.deb` container tests, testing-manual section | 13 | | 3–4 |
| 15 | Docs: ADR 0016/0022 status lines, user guide, `docs/{macos,linux}.md`, About and notices text for the downloaded tool, privacy text | all | | 1 |
| – | VM runs before the tag (a first pass, then a re-run after fixes) | 14 | | 4–6 |

**Total: 39 to 59 agent-days of implementation, plus 4 to 6 days of VM runs: 43 to 65.** The crates/ packages (1a, 2, maybe 8)
total 4 to 6 (plus 8 if it applies) and go first, in one window between the Spotter 1.0 and Pitwall 1.1 tags; 1b is required
by `known-copies.toml` whatever this record decides. With up to five agents (the Mac's limit) the critical path is
1a → 1b → 11 → 12 → 14 → VM runs, about 15 to 22 working days.

## Consequences

- Dyno 1.1.0 shows every process and the administrator-only readings, from login on, after one prompt; updates rarely ask again.
- Dyno's packaging, the Tauri updater and the family key are unchanged; ADR 0022's per-machine install and helper-run updates
  wait for a signing identity (D1, D7).
- A resident, idle root/SYSTEM process exists once the user turns it on; Settings shows it and removes it.
- R1 and R2 (section 3) are the accepted residual risks and are stated in the user guide.
- `dyno-ipc` is gone and the helper's registration is `paddock-service`'s, so the family keeps one implementation each.

## Decisions for the user

| # | Question | Recommendation |
| --- | --- | --- |
| D1 | How the helper is installed in 1.1.0: L (switched on in Settings, packaging unchanged) or P (ADR 0022: per-machine, `.pkg`, helper-run updates) | **L.** No forced reinstall from 1.0, no new key, no Gatekeeper `.pkg` flow; P later with a signing identity |
| D2 | macOS mechanism: a LaunchDaemon plist file with a root-owned copy, or `SMAppService.daemon` | **Plist file.** `SMAppService` daemons want an Apple-issued identity and would run from the admin-writable bundle; revisit with a Developer ID |
| D3 | smartmontools (GPL-2.0-or-later): downloaded from upstream on a click on Windows and macOS, the distribution package through PackageKit on Linux, never bundled or mirrored, as a named licence exception | **Yes** |
| D4 | Scope: 0022's U3 plus the owner (the account that turned it on) in full detail, with the administrator policy file | **Yes** |
| D5 | Helper updates: a prompt only when `HELPER_API` changes or a fix raises `MIN_HELPER_VERSION`; otherwise offered, not forced | **Yes** |
| D6 | Leave Defender exclusions, event logs and the journal, the disk map of unreadable folders and per-app energy out of the helper | **Yes**, reasons in section 6 |
| D7 | ADR 0022's helper-run updates and the Dyno-only key: deferred with P | **Defer** |
| D8 | Linux: helper for the `.deb` only; the AppImage shows `NotIncluded` | **Yes** |

## Sources (read 2026-11-03 unless stated)

- Code: `origin/main` `9da2ccf59` (`dyno-v1.0.0`) and `fix/dyno-disk-startup` `17bae21b1` (its commit message records the
  `sfltool` measurement in a macOS 26.6.2 Tart VM); `fix/dyno-agent-via-launchd` `b3f611269` (macOS 27.2 measurements);
  `docs/known-copies.toml`; ADR 0022 revision 3 (`e401a6967`).
- freedesktop.org, systemd.exec(5): <https://www.freedesktop.org/software/systemd/man/latest/systemd.exec.html>
  (`ProtectSystem`, `ProtectHome`, `CapabilityBoundingSet`, `NoNewPrivileges`, `PrivateTmp`, `StateDirectory`,
  `RuntimeDirectory`, and the advice to deny mounts with `ProtectSystem`).
- Microsoft Learn, `SERVICE_REQUIRED_PRIVILEGES_INFOW`:
  <https://learn.microsoft.com/en-us/windows/win32/api/winsvc/ns-winsvc-service_required_privileges_infow> ("the SCM will
  remove the privileges that are not required from the process token"; `SeChangeNotifyPrivilege` is never removed).
- Apple Developer Forums (DTS answers, not documentation): <https://developer.apple.com/forums/thread/733046>,
  <https://developer.apple.com/forums/thread/761249>, <https://developer.apple.com/forums/thread/757463>,
  <https://developer.apple.com/forums/thread/799933>. Apple's `SMAppService` reference was read for ADR 0022 on 2026-10-28;
  its page body did not load on 2026-11-03.
- smartmontools: GitHub releases API for `RELEASE_7_5` (asset names and sizes),
  <https://github.com/smartmontools/smartmontools/releases/tag/RELEASE_7_5>; the Windows installer script
  `smartmontools/os_win32/installer.nsi` at that tag (usage line `[/S] [/SO component,...] [/D=INSTDIR]`; sections `smartctl`,
  `smartd`, `drivedb.h`, `Uninstaller`, Start-menu shortcuts). <https://www.smartmontools.org/> (7.5 released 2025-04-30; the
  download page refused automated reads).
- Not verified, left to WP-0 in a VM: `sfltool` as root; `smartctl` without root on macOS; Windows firewall reads as a standard
  user (secondary sources only: a PowerShell cheat sheet and forum posts say reads work); `pfctl` and `nft` needing root (common
  knowledge, not re-read); PackageKit's `InstallPackages` polkit flow on the Ubuntu VM.
