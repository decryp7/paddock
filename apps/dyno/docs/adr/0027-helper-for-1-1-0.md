# 27. Dyno 1.1.0: the privileged helper, switched on by the user, started with the OS

- Status: Accepted (2026-11-03; user decisions 2026-11-03; independent review approved, round 1 and a delta review).
  Decisions: D1 = L; D1a = the full plan ships in Dyno 1.1.0, and the slice list is the landing order only; D2 to D8 as
  recommended; D9 yes.
- Revision 2 (2026-11-03): answers the independent review (P1: caller identity never from a pid; callers restricted per
  request; `SeDebugPrivilege` measured or dropped. P2: family ADR links, ADR 0042 landing plan, release order, R1 wording, tool
  pins, R3, systemd hardening, log levels, the DMG ownership argument, ADR 0041, the smallest first slice).
- Revision 3 (2026-11-03): the delta review's P2 (pipe squatting on Windows) and the user's decisions; accepted.
- Revision 4 (2026-11-03): WP-0 measured in VMs (section "WP-0 results"): `SeDebugPrivilege` dropped, the pipe SDDL names
  its owner, the NSIS components fixed, `sfltool` works as root, Windows firewall and NVMe SMART need no helper.
- Date: 2026-11-03
- Related:
  - Amends ADR [0016](0016-privileged-helper-install-and-ipc.md) (install on a click and the closed request list stay; the
    single authorised user, the per-user token and the private `dyno-ipc` are replaced) and ADR
    [0022](0022-helper-installed-with-the-app.md) (Proposed, revision 3). This record is the 1.1.0 plan; it adopts 0022's
    read set, scope and trust rules, and defers 0022's packaging and helper-run updates (D1).
  - Keeps ADR [0006](0006-privileged-helper-and-observe-only.md) point 3 (the app and the agent never run elevated) and ADR
    [0021](0021-monitoring-only.md) (monitoring only: the helper changes nothing except installing or removing itself and the
    one tool in D3).
  - Family: ADR 0001 and `docs/known-copies.toml` (the `dyno-ipc` copies expire at `dyno-v1.1.0`), the family ADR
    [`docs/adr/0022-rust-developer-and-ci-tool.md`](../../../../docs/adr/0022-rust-developer-and-ci-tool.md) §G.7 (no `crates/`
    change inside a release window; commit `47f59e133`), ADR 0024 (no polling), ADR 0034 / rule 55 (one implementation per mechanism), the accepted
    redesign ADRs 0037 (composition contract), 0039 (release gates inspect the artifacts), 0040 (Windows GUI test session),
    0041 (deterministic timing) and 0042 (one owner per harness, small batches), each read on its branch (`cad10f67b`,
    `8bf509f99`) on 2026-11-03.
  - Rules 1 (ER-1), 7 (ER-14), 19, 23, 24, 26, 53, 55; ER-27 (logs).
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
| Registration | `paddock-service` `LaunchdFile`: `/Library/LaunchDaemons/net.decryptology.dyno.helper.plist`, `launchctl bootstrap system`, `RunAtLoad`, `KeepAlive` on failure | `paddock-service` `WindowsService`: `DynoHelper`, LocalSystem, auto start, restart on failure, quoted `binPath`, **required privileges** (`SERVICE_CONFIG_REQUIRED_PRIVILEGES_INFO`: only what WP-0 measures as needed, see below; the SCM removes the rest from the token), a service DACL that lets interactive users *start* it | unit `/usr/lib/systemd/system/dyno-helper.service` shipped **disabled** in the package; `install` runs `systemctl enable --now` |
| Data | `/Library/Application Support/Dyno/Helper/` (0700) | `%ProgramData%\Dyno\Helper\`, created with create-new and an explicit descriptor; an existing folder that fails the protected-location rule is renamed aside (0022 §1 and its P2 list) | `StateDirectory=dyno-helper` (0700) |
| Endpoint | `/var/run/dyno-helper/helper.sock` 0666 in a root-owned 0755 folder | `\\.\pipe\net.decryptology.dyno.helper`, SDDL `O:SYD:P(A;;GA;;;SY)(A;;GA;;;BA)(A;;GRGW;;;IU)(D;;GA;;;NU)` (owner named: WP-0), `SY`, `BA` full, `IU` read/write, `NU` denied, `PIPE_REJECT_REMOTE_CLIENTS` | `RuntimeDirectory=dyno-helper`, socket 0666 |
| Start at boot | launchd at boot | SCM auto start | systemd `WantedBy=multi-user.target` |
| Turn off | Settings > Turn off: the authenticated `Uninstall` request (owner or administrator only); `dyno-helper uninstall` elevated when the helper does not answer | the same; the per-user NSIS uninstaller's `PREUNINSTALL` hook asks for one UAC prompt to run the protected copy's `uninstall` | the same through `systemctl disable --now`; `prerm remove` stops and disables; `postrm purge` removes the state folder |
| Orphan | the orphan watch of 0022 §3 over every recorded app location (one-shot one-hour deadline, `paddock-watch` events, no loop) | same | dpkg owns it |

**Windows privileges (settled by WP-0).** The required-privileges list is **`SeChangeNotifyPrivilege` only**;
`SeDebugPrivilege` is not in it. Measured as LocalSystem with only that privilege: C1 reads 125 of 132 and 132 of 143
processes on two boots (owner, command line, image), EStats switch-on (C2), `powercfg /requests`, `wbadmin get versions`, `smartctl`, the
silent smartmontools install and the pipe server with identification-level impersonation all work. What `SeDebugPrivilege`
adds is a handful of OS processes (`csrss`, `WmiPrvSE`, `SppExtComObj`) and the interactive user's own brokers, which the
agent already reads unprivileged; those few show name, pid, memory and CPU from the system process list and "Windows
protects this process" for the rest. The SCM removes every other privilege (backup, restore, take ownership, load driver,
debug, impersonate, TCB) from the token. This is still not a sandbox: the helper stays LocalSystem.

**Linux hardening** (systemd.exec(5), freedesktop.org, read 2026-11-03): `ProtectSystem=strict`, `ProtectHome=read-only`,
`PrivateTmp=yes`, `NoNewPrivileges=yes`, `RestrictAddressFamilies=AF_UNIX AF_NETLINK`, `IPAddressDeny=any`,
`CapabilityBoundingSet=CAP_SYS_PTRACE CAP_DAC_READ_SEARCH CAP_SYS_RAWIO CAP_SYS_ADMIN CAP_NET_ADMIN` (ptrace-class `/proc`
reads, root-only files, SMART pass-through, `nft`), `SystemCallFilter=@system-service` and `~@mount`, `ProtectKernelModules=yes`,
`ProtectClock=yes`, `ProtectHostname=yes`, `RestrictNamespaces=yes`, `RestrictSUIDSGID=yes`, `LockPersonality=yes`,
`MemoryDenyWriteExecute=yes`, `UMask=0077` (the manual pairs
`ProtectSystem` with denying mounts, because `CAP_SYS_ADMIN` is needed for NVMe admin commands). The polkit rule of 0022 §1
(start only, local active session) ships as a package file. **AppImage: no helper** (`NotIncluded`): root cannot read another
user's FUSE mount, and its path changes every run.

**macOS: why not `SMAppService` here (D2).** Dyno is ad-hoc signed (rule 23). Apple DTS answers on the developer forums say a
daemon registered through `SMAppService` should be signed with an Apple-issued identity shared with the app, and ad-hoc signing
"will see problems like this" (threads 733046, 761249, 757463, read 2026-11-03; forum answers, not documentation). The daemon
would also run from inside `/Applications/Dyno.app`. A bundle dragged from the DMG belongs to the user who dragged it, so any
program of that account can change it (and `/Applications` itself is `root:admin 0775`, measured on the build Mac on
2026-10-28), and every update's new signature hits the launch-constraint loop of lesson 2026-10-23
(rule 24). A plist file plus a root-owned copy avoids all three. Like any LaunchDaemon, it appears in System Settings > Login
Items & Extensions; switching it off there is the `NotRunning` state with its own text.

### 2. Who may call, and what each caller sees

- **Caller identity never comes from a pid** (rule 53: a pid read earlier may name another process by the time it is used).
  The facts come from the connection itself (in `paddock-ipc`, WP-1a):
  - macOS: the `LOCAL_PEERTOKEN` audit token (uid, pid and pid version together, so no reuse race), and
    `proc_pidpath_audittoken` for the audit line.
  - Linux: the scope comes from the `SO_PEERCRED` **uid only**, groups from `SO_PEERGROUPS`. The program for the audit line is
    read through `SO_PEERPIDFD` (Linux 6.5 and later) and the pidfd's `/proc` entry; on older kernels the audit line has no
    program rather than one read by pid.
  - Windows: `ImpersonateNamedPipeClient`, then `OpenThreadToken` on the impersonating thread, then `RevertToSelf`: the
    account SID and Administrators membership come from the caller's own token, never from `GetNamedPipeClientProcessId`. The
    client pid is logged as a hint only, never used for a decision.
  - If a fact cannot be read the connection is refused (fail closed).
- **The agent checks the helper too (Windows pipe squatting).** While the helper is not running, any local process can create
  `\\.\pipe\net.decryptology.dyno.helper` first and answer in its place. The agent therefore opens the pipe with
  `SECURITY_SQOS_PRESENT | SECURITY_IDENTIFICATION` (a squatter can at most identify the agent's account, never impersonate it)
  and, before trusting any answer, reads the pipe's owner with `GetSecurityInfo` on its own handle: it must be SYSTEM
  (`S-1-5-18`). Anything else is `Missing` with "Something other than Dyno's helper answered on its pipe" and an audit-style log
  line naming the owner SID. On Unix the socket's folder is root-owned, so the same check is `fstat` of the folder and an
  owner of root.
- **No token, any local user.** ADR 0016's per-user token is dropped (0022 §2.2).
- **Scope (0022 §6, U3) plus the owner.** Membership of Administrators counts even when UAC filtered the token (R3). Full detail for administrators and for the **owner**, the account that turned the
  helper on (recorded by `install --for <uid|SID>`, which the agent passes; a different account turning it on again becomes the
  owner, guarded by the OS prompt). Every other account: its own processes and system/service accounts in detail, other people
  as one "Other users" total. The administrator-edited policy file (`others = "total" | "detail" | "hidden"`) stays (D4).
- **The caller's program** is recorded in the audit log. It is a boundary only where its path is protected (`.deb`:
  `/usr/bin/dyno-agent`); with L's per-user and `/Applications` installs a same-account program can call the helper, and it gets
  exactly what that account may see (R2). That is why the answers are read-only and scoped by the account, never by the program.
- **Per request:** `InstallTool`, `Audit`, `LogTail` and `Uninstall` are for the owner and administrators only. `Firewall` and
  `Extras` give full detail to the owner and administrators, and only the state and counts (firewall on or off per profile,
  number of rules; number of wake requests and backup versions) to everyone else.
- **Concurrency per caller:** at most one running request per account for the slow kinds (`Smart`, `LoginItems`, `Firewall`,
  `Extras`, `InstallTool`) and at most four requests in all per account; more answer "busy" at once. `Smart` reads disks one at a
  time inside that one slot, so 32 disks at 20 s each hold one worker of that account, never the helper.

### 3. Trust and residual risks

- **Nothing elevated runs from, or is read in, a place a non-administrator can change, except the one install run.** The
  service runs the protected copy only; `serve` and every tool run check 0022 §2.1's protected-location rule (pure, over a
  `FileFacts` port) on the program, the config, the policy file and the tools folder, by handle.
- **R1 (accepted, as ADR 0016 §3 did):** the install and "Update the helper" runs elevate the program in the app folder, which
  the same account can replace before the prompt. A program already running as that account can raise any prompt it likes, so
  this does not widen what it can do. The prompt does not always name Dyno: on Windows UAC shows `dyno-helper.exe` (unsigned,
  "Unknown publisher"); on macOS the password dialog is `osascript`'s and names `osascript`, not Dyno; on Linux `pkexec`
  shows the command line. So the user's protection is that the prompt appears only right after their own click in Dyno's
  Settings, and the dialog before it says so ("macOS will ask for your password for osascript, on behalf of Dyno"). The helper
  copies itself by handle and records the copy's SHA-256 in the audit log. Prompts are rare (section 4), so they do not train the user to click through.
- **R3 (accepted):** a UAC-filtered token of an Administrators member counts as an administrator, so a non-elevated
  program of an administrator account gets the administrator scope (every account's processes in detail) without an elevation
  prompt. Windows treats that account as able to elevate anyway; the scope is still read-only and field-limited. The policy file
  can set `others = "total"` for filtered tokens if the user prefers (D9).
- **R2:** same-account injection or impersonation gets that account's scope. Bounded by the field allow-list (never command
  lines, environment, window titles, open files or memory; 0022 §5) and by redaction of other accounts' profile paths.
- **The helper never opens a path a caller chose.** A tool package comes as bytes streamed over the connection (section 7),
  never as a path, a descriptor or a handle duplicated from a pid; devices for
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
| `InstallTool(Smartmontools, bytes)` | C6: puts the downloaded upstream package into the helper's tools folder; owner and administrators only | Windows, macOS | section 7 |
| `Firewall` | C10: typed, bounded view (state per profile or table, default policies, at most 4,096 rules with name, direction, action, program, ports, addresses; rules to the owner and administrators only, state and counts to others) | macOS `pf`, Linux | `pfctl -s info`, `pfctl -s rules`; `nft -j list ruleset` (JSON, capped at 4 MB), firewalld/ufw state derived from it. Windows profiles and rules (`INetFwPolicy2` or `Get-NetFirewall*` in the interactive session), and the macOS Application Firewall (`socketfilterfw --get*`/`--listapps`), are read by the agent without the helper (WP-0 measured) |
| `Extras(WakeRequests \| BackupVersions)` | C9; detail to the owner and administrators, counts to others | Windows | `powercfg /requests`, `wbadmin get versions`; fixed arguments, 15 s |
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
  phases) and **streams the bytes** to `InstallTool` over the connection (about 1.5 MB, under the 4 MB frame; the helper refuses
  more than the pinned size). The helper writes them create-new into `tools/.staging/`, hashing while it writes (rule 15), and
  checks size and SHA-256, then:
  - Windows: runs the copy as `/S /SO <components> /D=%ProgramFiles%\Dyno Helper\tools\smartmontools-7.5` with `TEMP`/`TMP`
    pointing into the staging folder (0022's P1-3). The installer's own options are
    `[/S] [/SO component,...] [/D=INSTDIR]` (`os_win32/installer.nsi`, tag `RELEASE_7_5`, read 2026-11-03); components chosen so
    no `smartd` service, no Start-menu entries and no uninstaller registry key are written: `/SO x64,smartctl,drivedb` (WP-0,
    measured: writes only `bin\smartctl.exe`, `bin\drivedb.h`, `bin\update-smart-drivedb.ps1`; no registry, service, menu or
    PATH change). WP-0 also found NVMe SMART readable on Windows **without** administrator rights, so for NVMe disks the agent
    could run `smartctl` itself; the helper stays the reader for SATA (ATA pass-through) and for one code path.
  - macOS: `hdiutil attach -readonly -nobrowse -mountrandom <staging>`, `pkgutil --expand-full` of the package inside into
    staging, copy `smartctl` and `drivedb.h` into `tools/smartmontools-7.5/`, detach. Nothing is installed system-wide. If WP-0
    shows `smartctl` reads SMART on macOS without root, the agent instead runs it from its own `paddock-runtime` store and the
    helper is not used for SMART on macOS.
- **Linux.** The agent asks PackageKit to install the distribution's `smartmontools` package (`InstallPackages` over D-Bus; the
  user authenticates through PackageKit's own polkit action). The helper then runs `/usr/sbin/smartctl` (protected path). Without
  PackageKit, the text gives the distribution's command.
- **Tool pins ride on helper versions.** The pinned table is compiled into the helper, so a new smartmontools version reaches
  users only with a new helper (D5: a `MIN_HELPER_VERSION` raise, or the offered update), then shows "Update smartmontools".
- **The pinning record is committed** next to the catalogue entry
  (`apps/dyno/crates/dyno-helper/tools/smartmontools-<version>.pin.md`): the signing key's fingerprint and where it was taken
  from, the date, the `gpg --verify` output for each artefact, and the SHA-256 and size.

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
| `sfltool` asks for authorization as root on a later macOS (WP-0 measured it does not on 26.6.2) | `LoginItems` answers `Unavailable` ("macOS would not list login items even for the helper") and C5 stays as in 1.0 |
| Download refused, interrupted or hash mismatch | the job fails with the phase and the reason; nothing reaches the helper; Retry |
| Tool install fails | staging is deleted; the state stays "not installed"; the audit log has the step and exit code |
| Policy file invalid | defaults (U3) and an audit line naming the line and field |
| Reply over a cap | truncated with `truncated: true` and the count; the UI says "showing N of M" |
| App removed, helper left | orphan watch removes the helper after the one-hour confirmation (0022 §3) |

### 9. Logs (ER-27)

- The helper logs through `paddock-log` to its data folder (0600, rotated, administrators only), and `LogTail` puts the last
  entries into the agent's diagnostics bundle.
- Info level: start (version, API, OS, policy, protected-location verdicts), every connection (correlation id, account,
  administrator yes/no, scope, program where known), every tool run (the exact command line, exit code, last 2 KB of stderr,
  duration, timeout), install and uninstall phases with durations, **every refusal** with its reason, and a **per-minute summary**
  per account (requests by kind, failures, slowest duration), written only in minutes that had requests.
- Debug level (off by default, switched on for a diagnostics session): one line per request (kind, class, duration, item count,
  truncation, outcome). A 2 s sampler therefore costs one summary line a minute, not thirty lines.
- Never in a log: process command lines (never read), other accounts' paths (redacted as in the answers), BTM item names (counts
  only), firewall rule addresses (counts only), disk serials (never read into the reply).
- The agent logs every `HelperState` change with its cause, each call's correlation id and duration, and the tool jobs' phases.

### 10. Tests and release gates

**CI (nothing installed, no real service; rule 7 / ER-14; fake clocks only, ADR 0041):**

- `paddock-service` goldens for the daemon plist, the hardened unit, the `sc.exe` calls including required privileges and the
  start DACL; the elevated `install` and `uninstall` plans over the fake `System`, including taking over a 0.x registration.
- The protected-location rule over fake `FileFacts` (every case of 0022 §2.1) and on real temporary folders per runner.
- Scope: owner, administrator, other account, system account; policy `total`/`detail`/`hidden`; refused peers; Windows filtered
  administrator tokens through a fake token port; per-request restrictions (`InstallTool`, `Firewall`, `Extras`) and the
  per-account concurrency caps; identity taken from the impersonated token / `SO_PEERCRED` / audit token, with a fake that
  reports a different pid owner to prove the pid is never used.
- Request caps and decoding: every limit and one over it, unknown variants, the 4 MB frame at the caps.
- Parsers on invented sample bytes with fuzz targets: `parse/btm.rs` (several uids; only the caller's survive), `smartctl -j`
  (serials stripped), `nft -j`, `pfctl`, `powermetrics`, DMI, `powercfg`, `wbadmin`.
- Tool runs through a fake runner: timeouts fire on a `FakeClock`, exit codes and stderr reach the reply and the log.
- `InstallTool`: wrong hash, one byte over the pinned size, a non-owner caller, a second concurrent install, `TEMP` inside staging.
- `paddock-ipc`: peer facts with fakes per OS; a pipe created by the test's own (non-SYSTEM) account under the helper's name is
  refused by the client with the owner SID in the error, and the client's open flags carry `SECURITY_IDENTIFICATION`, on the
  Windows runner; the Windows pipe refuses a remote client on the
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
| 0 | VM spikes: whether C1 needs `SeDebugPrivilege` on Windows (and what `wbadmin` and the SMART installer need); `sfltool dumpbtm` as root from a LaunchDaemon (no SecurityAgent?); `smartctl` rights on macOS; the NSIS `/SO` component names; firewall reads without admin on Windows and the macOS Application Firewall; how macOS 15 and 27 show a plist-file daemon in Login Items | – | | 2 |
| 1a | `paddock-ipc`: a fixed system endpoint (Unix path and mode; pipe name, SDDL, remote clients refused), peer facts per OS without pids (audit token; `SO_PEERCRED`, `SO_PEERGROUPS`, `SO_PEERPIDFD`; `ImpersonateNamedPipeClient` and `OpenThreadToken`), a bounded byte stream for one call; on the client side, `SECURITY_SQOS_PRESENT \| SECURITY_IDENTIFICATION` and the SYSTEM-owner check of the server pipe (root-owned folder on Unix) | – | yes | 2.5–3.5 |
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
| 14 | Test tooling (under the `vm-smoke` lease on `tools/vm-smoke`, ADR 0042): VM matrix in vm-smoke (macOS GUI, Windows GUI per ADR 0040, Ubuntu), `.deb` container tests, testing-manual section | 13 | | 3–4 |
| 15 | Docs: ADR 0016/0022 status lines, user guide, `docs/{macos,linux}.md`, About and notices text for the downloaded tool, privacy text | all | | 1 |
| – | VM runs before the tag (a first pass, then a re-run after fixes) | 14 | | 4–6 |

**Total: 39 to 59 agent-days of implementation, plus 4 to 6 days of VM runs: 43 to 65.** The crates/ packages (1a, 2,
maybe 8) total 4 to 6 (plus 8 if it applies) and go first, in one window between the Spotter 1.0 and Pitwall 1.1 tags; 1b is required
by `known-copies.toml` whatever this record decides. With up to five agents (the Mac's limit) the critical path is
1a → 1b → 11 → 12 → 14 → VM runs, about 15 to 22 working days.

### WP-0 results (measured 2026-11-03)

Measured in local VMs only (ER-14), never on the build Mac or a user's machine. **Windows:** `paddock-win11-test` (Windows 11
Pro Arm64, `DisplayVersion` 26H2, build 26300, VMware Fusion), reverted to its `gui-session` snapshot afterwards. Callers:
`paddock-test` (standard user) over SSH (a `NETWORK` logon, session 0) and in its console session 1 through a scheduled task
(`LogonType Interactive`, an `INTERACTIVE` token); `paddock-admin` over SSH (full administrator token); LocalSystem services
with `sc.exe privs` lists. Probe: a C# program compiled in the VM (`Add-Type -OutputAssembly`), per process
`OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION)`, `OpenProcessToken`+`TokenUser` (owner), `NtQueryInformationProcess`
class 60 (command line), `QueryFullProcessImageNameW`, class 61 (protection). **macOS:** a Tart clone of
`vmgui-base-tahoe-4aaa751f` (macOS 26.6.2, build 25G83, arm64), deleted afterwards. macOS 15 and 27 were not run. Process
counts move by a few between runs (processes come and go); each comparison below is from one boot.

**1. `sfltool dumpbtm` as root: works, no dialog.** Run as root three ways, each with a 25 s watchdog and a `pgrep -x
SecurityAgent` before and after:

| Context | `launchctl managername` | exit | time | stdout | stderr | SecurityAgent |
| --- | --- | --- | --- | --- | --- | --- |
| `sudo` from the console user's Aqua context | Aqua | 0 | < 1 s | 146 lines | 0 bytes | none |
| `sudo` in an SSH session (no GUI) | Background | 0 | < 1 s | 159 lines | 0 bytes | none |
| a root LaunchDaemon (`/Library/LaunchDaemons/*.plist`, `launchctl bootstrap system`, `RunAtLoad`) | System, uid 0 | 0 | < 1 s | 146 lines | 0 bytes | none |

Unprivileged it hangs with a SecurityAgent dialog (measured in `17bae21b1`). Format: blocks headed by a line of `=`, then
` Records for UID <uid> : <uuid>` (seen: `-2`, `0`, `501`), then ` ServiceManagement migrated:`, ` LaunchServices registered:`,
` Items:` and numbered records ` #n:` with right-aligned `Key: value` lines: `UUID`, `Name`, `Developer Name`, optional `Team
Identifier`, `Type` (seen `developer (0x20)`, `legacy daemon (0x10010)`), `Flags`, `Disposition` (for example `[enabled,
allowed, notified] (0xb)`), `Identifier`, `URL`, optional `Executable Path`, `Generation`, optional `Parent Identifier` and
`Embedded Item Identifiers` (a nested numbered list). A plist-file LaunchDaemon with an unsigned program appears as `Type: legacy
daemon`, `Identifier: 16.<label>`, `Parent Identifier: Unknown Developer`, disposition `notified` (macOS 26.6.2; how 15 and 27
show it stays open). **Settles:** WP-7 goes ahead as designed; the "still asks as root" failure row becomes a guard for later
macOS versions. **New for WP-7:** `parse/btm.rs` knows only `login item` and `agent`; system-wide items arrive as `legacy daemon`
(and presumably `daemon`) records under UID 0 and -2, so the parser needs those kinds or it under-reports system items.

**2. `smartctl` on macOS: not settled for real hardware.** The upstream universal `smartctl` 7.5 (from the DMG, run in place
with `-B drivedb.h`) in the VM: `--scan-open -j` exits 0 with no devices, as user and as root; `-a -j disk0` exits 2 with
`Smartctl open device: disk0 failed: Operation not supported by device`, as user and as root. The VM's disk is
`AppleVirtIODiskStorageDevice`, which has no SMART at all, so a VM cannot answer whether an Apple silicon internal SSD needs
root. Upstream's `os_darwin.cpp` (tag `RELEASE_7_5`) reads NVMe through the IOKit `IONVMeSMARTUserClient` plug-in
(`GetIdentifyData`, `GetLogPage`) with no uid check of its own (read, not measured). **Plan:** keep section 7 as written (helper
copy on macOS); the one remaining measurement is a single read-only `smartctl -a -j disk0` without `sudo` on a real Apple
silicon Mac, which only the user can allow (open item O1 below). If it works, macOS SMART moves to the agent and the helper's
macOS `InstallTool` drops out.

**3. smartmontools 7.5 artefacts.** Downloaded from the GitHub release `RELEASE_7_5` inside the VMs (the Windows installer in
both VMs; hashes agree):

| File | Bytes | SHA-256 | Upstream `.md5` |
| --- | --- | --- | --- |
| `smartmontools-7.5.win32-setup.exe` | 1,508,408 | `896337fcc253220614cf8cdbd5cf2321c5aa326a37a04160a672a281e6104c70` | `bb1e199ad6a3db3e1c27ae54b835cbd5` (matches) |
| `smartmontools-7.5.dmg` | 1,619,514 | `4aa176c161612488f3db053fdf3eee47bbc5178c8fe46afb2002d8dbdf563cc5` | `414d905b7a6c6ea8861d4e6f7fa14c1c` |
| `smartmontools-7.5.tar.gz` | 1,122,317 | `690b83ca331378da9ea0d9d61008c4b22dde391387b9bbad7f29387f2595f76e` | |

Signatures: `gpg --verify` (GnuPG 2.5.24 in the macOS VM) says `Good signature from "Smartmontools Signing Key (through 2025)
<smartmontools-support@listi.jpberlin.de>" [expired]` for all three, made 2025-04-30 08:29 UTC with RSA key
**`0C95 77FD 2C4C FCB4 B9A5 9964 0A30 812E FF3A EFF5`** (rsa4096, created 2020-12-21, **expired 2025-12-31**, after the
signatures). The key came from `keyserver.ubuntu.com` (keys.openpgp.org has no copy); its short id matches `KEYID=0xFF3AEFF5`
"Smartmontools Signing Key (through 2025)" in upstream `do_release` at the tag. smartmontools.org (which lists the keys) refused
automated reads (an Anubis challenge), so the full fingerprint has one independent source plus the short id: the pinning record
of section 7 should add the fingerprint from smartmontools.org read by a person. The Windows installer also carries a valid
Authenticode signature (`CN=SignPath Foundation`). The next smartmontools release will be signed with a new key, so the
pinning step must expect a key change.

**Windows installer.** `os_win32/installer.nsi` at the tag: `/SO` takes a comma list of `x32`, `x64`, `smartctl`, `smartd`,
`smartctlnc`, `drivedb`, `doc`, `uninst`, `menu`, `path`, `driveremove`, `drive0` to `drive5`; any section not named is
switched off; `/D=` must come last. Measured as LocalSystem with only `SeChangeNotifyPrivilege`: `/S /SO x64,smartctl,drivedb
/D=C:\ProgramData\DynoHelperWp0\tools\smartmontools-7.5` exits 0 and writes exactly `bin\smartctl.exe` (1,165,312 bytes,
x86-64, runs under emulation on Arm64), `bin\drivedb.h` and `bin\update-smart-drivedb.ps1`; before and after: no
`Uninstall\smartmontools` key (native or WOW6432Node), no `smartd` service, no Start-menu folder, no machine `PATH` change, no
`Drive\shell\smartctl*` verbs, nothing under `Program Files\smartmontools`. There is no native Arm64 build.

**macOS DMG.** One HFS volume with `README.html` and `smartmontools-7.5-r5714.pkg` (unsigned: `pkgutil --check-signature` says
"no signature"). `pkgutil --expand-full` gives `Payload/usr/local/sbin/{smartctl,smartd,update-smart-drivedb,smart-pkg-uninstall}`,
`Payload/usr/local/share/smartmontools/drivedb.h`, docs, man pages and `Payload/private/etc/smartd*`; no install scripts.
`smartctl` is a universal (x86_64, arm64) binary, ad-hoc and linker-signed (`flags=0x20002(adhoc,linker-signed)`).
`hdiutil attach -readonly -nobrowse -mountrandom <dir>` needs `<dir>` to exist.

**SMART on Windows without the helper.** As the standard user, `smartctl --scan-open -j` finds `/dev/sda` (type `nvme`) and
`smartctl -a -j /dev/sda` exits 0 with the full `nvme_smart_health_information_log` (percentage used, media errors, power-on
hours, unsafe shutdowns, temperature) on the VM's `VMware Virtual NVMe Disk`. NVMe health goes through the in-box `stornvme`
storage query, which Windows allows without administrator rights; SATA SMART (ATA pass-through) needs administrator rights according to
upstream's Windows notes (not re-read, not measured: the VM has no SATA disk). **Plan change for WP-8:** on Windows, NVMe SMART can be
read by the agent; the helper's `Smart` request is needed for SATA disks. Whether `InstallTool` stays in the helper on
Windows is open item O2 below.

**4. `SeDebugPrivilege` and C1: drop it.** Readable processes (`OpenProcess` limited, then owner and command line; the three
pseudo-processes `System`, `Registry`, `Memory Compression` have no command line for anyone):

| Caller | Token privileges | Processes | Opened | Owner | Command line | Not opened |
| --- | --- | --- | --- | --- | --- | --- |
| standard user, SSH (session 0) | 5, no debug | 131 | 34 | 34 | 34 | 97 (all services, other sessions) |
| standard user, interactive (session 1) | 5, no debug | 146 | 35 | 35 | 35 | 111 (services, session 0, `winlogon`, `dwm`, `csrss`) |
| administrator, SSH | 24, debug on | 131 | 131 | 130 | 128 | none |
| administrator, debug removed | 23 | 131 | 91 | 90 | 88 | 40, mostly the session-1 user's processes |
| LocalSystem, SCM default list | 28, debug on | 127 | 127 | 127 | 124 | none |
| LocalSystem, `SeChangeNotifyPrivilege` only | 1 | 132 / 143 | 125 / 132 | 125 / 132 | 122 / 129 | 7 / 11: `csrss` (s0, s1), `WmiPrvSE`, `SppExtComObj`, `slui`, `smartscreen`, `RuntimeBroker`, `dllhost`, `UserOOBEBroker` |
| LocalSystem, change-notify and debug | 2 | 131 | 131 | 131 | 128 | none |

The session-1 processes the stripped helper cannot open (`RuntimeBroker`, `dllhost`, `UserOOBEBroker`, `smartscreen`, `slui`)
belong to the interactive user, whose agent reads them itself (they are not in its "not opened" list). What remains is OS
plumbing (`csrss`, `WmiPrvSE`, `SppExtComObj`), which keeps name, pid, memory and CPU from the system process list.
`SeDebugPrivilege` would let the helper open nearly every process (all but protected ones) for those few rows. **Decision:** not
in the required-privileges list (section 1). Same measurement, other C-rows: `SetPerTcpConnectionEStats` (C2) returns 0 as
LocalSystem with only `SeChangeNotifyPrivilege` and 5 (access denied) as the standard user; `powercfg /requests` (C9) exits 0
as stripped LocalSystem and 1 ("You do not have the permissions required") as the standard user; `wbadmin get versions`
reaches "No backup was found" as stripped LocalSystem and "Access denied. You must be a member of the Administrators group or
Backup Operators group" as the standard user. None needs a privilege beyond `SeChangeNotifyPrivilege`.

**Firewall (C10) without administrator rights: yes, in the interactive session.** As the standard user in its console session:
`Get-NetFirewallProfile` (3 profiles), `Get-NetFirewallRule` (494 rules), `HNetCfg.FwPolicy2` (`INetFwPolicy2`: per-profile
state, current profile, 494 rules) and `netsh advfirewall firewall show rule name=all` (exit 0) all work. Over SSH (a
`NETWORK` logon) the CIM cmdlets fail with "Cannot connect to CIM server. Access denied" while COM and `netsh` still work, so
**WP-9 reads through `INetFwPolicy2`** (works in every logon type, no WMI), and tests that run over SSH must not use the CIM
cmdlets as their oracle. macOS: `socketfilterfw --getglobalstate`, `--getblockall`, `--getstealthmode`, `--getallowsigned` and
`--listapps` exit 0 as the user; `defaults read /Library/Preferences/com.apple.alf` has no `globalstate` on 26.6.2 (do not read
the plist); `/sbin/pfctl -s info` and `-s rules` fail as the user (`/dev/pf: Permission denied`) and work as root.

**5. Named-pipe owner check and identification-level connect: confirmed, with one fix.** Server: LocalSystem with only
`SeChangeNotifyPrivilege`, `CreateNamedPipeW` with `FILE_FLAG_FIRST_PIPE_INSTANCE` and `PIPE_REJECT_REMOTE_CLIENTS`. Client:
`CreateFileW(..., SECURITY_SQOS_PRESENT | SECURITY_IDENTIFICATION)`, then `GetSecurityInfo(SE_KERNEL_OBJECT,
OWNER_SECURITY_INFORMATION)` and a refusal unless the owner is `S-1-5-18`, before sending anything.

| Case | Result |
| --- | --- |
| SDDL `D:P(A;;GA;;;SY)(A;;GA;;;BA)(A;;GRGW;;;IU)(D;;GA;;;NU)` (no owner part), interactive client | owner is **`S-1-5-32-544` (Administrators)**, not SYSTEM: the client refuses the real helper |
| same with `O:SY` in front, interactive client | owner `S-1-5-18`, client trusts it, round trip works |
| server side, both cases | `ImpersonateNamedPipeClient` works without `SeImpersonatePrivilege`; the thread token has the client's SID and level 1 (`SecurityIdentification`); opening a file while impersonating fails with 1346 (`ERROR_BAD_IMPERSONATION_LEVEL`) |
| client over SSH (`NETWORK` logon) | `CreateFile` error 5: the `NU` deny works |
| standard user creates the pipe name with `O:SY` | `CreateNamedPipe` error 1307 (`ERROR_INVALID_OWNER`): a user cannot fake the owner |
| standard user squats the name first (default owner), then the helper starts | the helper's create fails with 231 (`ERROR_PIPE_BUSY`, first instance taken); the client sees owner = the user's SID and refuses before sending |

**Settles:** WP-1a's client check (owner == SYSTEM) and identification-level connect are right, and the helper needs no
impersonation privilege. **Fix:** a LocalSystem service's objects default to the Administrators group as owner, so the
helper's SDDL must start with `O:SY` (section 1's endpoint row now does). `paddock-ipc`'s existing service SDDL
(`crates/paddock-ipc/src/endpoint/mod.rs:49`, `D:P(A;;GA;;;SY)(A;;GA;;;BA)(A;;GA;;;OW)(A;;GRGW;;;IU)`) has no owner part,
so WP-1a adds `O:SY` there (a Pitcrew client that adds the same owner check would otherwise refuse its own service) and a test
that the created pipe's owner is SYSTEM. The helper should log the create error by name (231 means the name is taken) as part
of the squatting refusal of revision 3.

**What changes in the plan.** WP-2: the Windows required-privileges list is `SeChangeNotifyPrivilege` alone. WP-1a: `O:SY`
in the system endpoint's SDDL and an owner test. WP-7: parser learns daemon kinds. WP-8: Windows `/SO x64,smartctl,drivedb`;
NVMe SMART on Windows needs no helper; macOS open until one real-hardware read; the pinning record expects an expired key and a
new key for the next release. WP-9: `INetFwPolicy2` for Windows, `socketfilterfw` for macOS, both in the agent. WP-0's two
days are spent; the estimates of the other packages do not change by more than half a day each (WP-8 may shrink).

**Open after WP-0 (for the user).** O1: allow one read-only `smartctl -a -j disk0` without `sudo` on a real Apple silicon Mac
(the upstream binary run from a scratch folder, nothing installed), or accept the helper path on macOS untested on hardware
until the VM runs. O2: on Windows, keep `InstallTool` in the helper (one tool for NVMe and SATA, installed under Program
Files) or let the agent fetch `smartctl` into its own `paddock-runtime` store for NVMe and use the helper only for SATA.
Recommended: O2 keeps the helper path (one mechanism, rule 55), with the agent reading NVMe itself when the helper is off.

### The smallest first slice (D1a): the landing order

Decided 2026-11-03: the full plan ships in Dyno 1.1.0. The slice below is only the order in which the work lands:

1. **First:** WP-0, 1a, 1b, 2, 3, 4, 5, 7, 11, 12, 13, 14, 15 and the VM runs: about **33 to 49 agent-days**. This is "run as
   administrator, also at start-up" (every process in detail from login on) plus macOS login items.
2. **Then:** WP-8 (SMART through smartmontools) and the agent's unprivileged firewall readers of WP-9 (Windows, the macOS
   Application Firewall).
3. **Last:** WP-6 (counters and memory slots), WP-10 (socket owners, file holders, Windows extras) and the helper's `pf` and
   `nft` reads. All three are in 1.1.0.

The wire (`HELPER_API` 2) is declared whole in WP-3; requests of a later step answer `NotSupported` until they land, so the
slices need no API change between them.

### Release order and landing (ER-1, ADR 0042)

- **Order.** On 2026-11-03 the user put the next Dyno release (all nine 1.0.0 bugs and every 1.1 feature, numbered 1.1.0)
  first, then the Spotter 1.0 tag (waiting for ADR 0039's first gates), then Pitwall 1.1. The helper is the longest part of
  Dyno 1.1.0, so it sets that release's date; the slice above shortens it.
- **Shared crates.** WP-1a changes `paddock-ipc`, which Pitwall uses: it must land before Pitwall 1.1's release window opens,
  or wait until after that tag (family ADR 0022 §G.7). WP-2 (`paddock-service`) is used by Pitwall and Pitcrew and has the same
  constraint. Both go first and land together with their adoption by every app that uses them (rule 43).
- **Batches.** Each landing to main stays within ADR 0042's caps: at most 60 commits, 200 files and 72 hours of age. Planned
  batches: (1) WP-1a and WP-2 with their app adoptions; (2) WP-1b, WP-3; (3) WP-4, WP-5, WP-7; (4) WP-11, WP-12, WP-13;
  (5) WP-8 and WP-9; (6) WP-6 and WP-10. WP-14 lands on `tools/vm-smoke` by the agent holding that lease, and
  reaches main with that home's own landing, never from a Dyno branch.

## Consequences

- Dyno 1.1.0 shows every process and the administrator-only readings, from login on, after one prompt; updates rarely ask again.
- Dyno's packaging, the Tauri updater and the family key are unchanged; ADR 0022's per-machine install and helper-run updates
  wait for a signing identity (D1, D7).
- A resident, idle root/SYSTEM process exists once the user turns it on; Settings shows it and removes it.
- R1 and R2 (section 3) are the accepted residual risks and are stated in the user guide.
- `dyno-ipc` is gone and the helper's registration is `paddock-service`'s, so the family keeps one implementation each.

## Decisions (the user, 2026-11-03)

| # | Question | Recommendation, and the user's decision |
| --- | --- | --- |
| D1 | How the helper is installed in 1.1.0: L (switched on in Settings, packaging unchanged) or P (ADR 0022: per-machine, `.pkg`, helper-run updates) | **L.** No forced reinstall from 1.0, no new key, no Gatekeeper `.pkg` flow; P later with a signing identity Decided: as recommended. |
| D1a | Ship the smallest first slice (section above) first, SMART and firewall after it | **Yes**, if 1.1.0's date matters more than shipping every reading at once Decided: the full plan ships in 1.1.0; the slice is the landing order only. |
| D2 | macOS mechanism: a LaunchDaemon plist file with a root-owned copy, or `SMAppService.daemon` | **Plist file.** `SMAppService` daemons want an Apple-issued identity and would run from the admin-writable bundle; revisit with a Developer ID Decided: as recommended. |
| D3 | smartmontools (GPL-2.0-or-later): downloaded from upstream on a click on Windows and macOS, the distribution package through PackageKit on Linux, never bundled or mirrored, as a named licence exception | **Yes** Decided: as recommended. |
| D4 | Scope: 0022's U3 plus the owner (the account that turned it on) in full detail, with the administrator policy file | **Yes** Decided: as recommended. |
| D5 | Helper updates: a prompt only when `HELPER_API` changes or a fix raises `MIN_HELPER_VERSION`; otherwise offered, not forced. A new smartmontools pin is a helper update too, so it arrives through this same offer | **Yes** Decided: as recommended. |
| D6 | Leave Defender exclusions, event logs and the journal, the disk map of unreadable folders and per-app energy out of the helper | **Yes**, reasons in section 6 Decided: as recommended. |
| D7 | ADR 0022's helper-run updates and the Dyno-only key: deferred with P | **Defer** Decided: as recommended. |
| D8 | Linux: helper for the `.deb` only; the AppImage shows `NotIncluded` | **Yes** Decided: as recommended. |
| D9 | Windows: a UAC-filtered administrator token counts as an administrator for the scope (R3) | **Yes** (the account can elevate anyway); the policy file can narrow it Decided: yes. |

## Sources (read 2026-11-03 unless stated)

- Code: `origin/main` `9da2ccf59` (`dyno-v1.0.0`) and `fix/dyno-disk-startup` `17bae21b1` (its commit message records the
  `sfltool` measurement in a macOS 26.6.2 Tart VM); `fix/dyno-agent-via-launchd` `b3f611269` (macOS 27.2 measurements);
  `docs/known-copies.toml`; ADR 0022 revision 3 (`e401a6967`).
- freedesktop.org, systemd.exec(5): <https://www.freedesktop.org/software/systemd/man/latest/systemd.exec.html>
  (`ProtectSystem`, `ProtectHome`, `CapabilityBoundingSet`, `NoNewPrivileges`, `PrivateTmp`, `StateDirectory`,
  `RuntimeDirectory`, and the advice to deny mounts with `ProtectSystem`).
- Family ADR 0022 (`docs/adr/0022-rust-developer-and-ci-tool.md`, `47f59e133`) §G.7; ADR 0042 (`8bf509f99`): homes, leases,
  batch caps.
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
- Measured in WP-0 (section "WP-0 results"): `sfltool` as root; Windows firewall reads as a standard user; `pfctl` needing
  root. Still not verified: `smartctl` on a real Apple silicon internal SSD (a VM has none); `nft` needing
  `CAP_NET_ADMIN` (common knowledge, not re-read); PackageKit's `InstallPackages` polkit flow on the Ubuntu VM.
