# 4. App ↔ service IPC: framed JSON over a local stream, ACL'd, with pushed events

- Status: accepted (2026-10-22)
- Amended by [ADR 0022](0022-host-environment-and-shared-folders.md) (accepted 2026-10-29): environment, secret, clean and reachability requests are authorised from the caller's token (Windows) or peer uid, not by the ACL alone.
- Related: ADR 0001 (architecture), ADR 0003 (service model); code: `pitcrew_core::protocol`, `pitcrew_service::ipc`,
  `app/src-tauri/src/{service_client,events}.rs`

## Context

ADR 0001 fixed a local stream with length-prefixed JSON frames and left two things open: how status reaches the app
without polling, and who may talk to the privileged Windows service. The user wants live status ("push events, not
polling") and the pipe "ACL'd to the interactive user and SYSTEM".

## Decision

- **Transport.** Windows: `\\.\pipe\pitcrew`, created by the service as the *first instance*
  (`FILE_FLAG_FIRST_PIPE_INSTANCE`: if anyone squatted the name, the service refuses to start and says so), remote
  clients rejected (`PIPE_REJECT_REMOTE_CLIENTS`), at most 32 connections. Security descriptor
  `D:P(A;;GA;;;SY)(A;;GA;;;BA)(A;;GA;;;OW)(A;;GRGW;;;IU)`: SYSTEM, Administrators and the owner full access,
  **interactive users read/write**, nobody else (no network, service or batch logons). macOS/Linux: a Unix socket
  `pitcrew.sock` in the data folder (`~/Library/Application Support/Pitcrew`, mode 0700), the socket itself 0600 —
  only the agent's own user.
- **Authorisation is the ACL.** ADR 0001 planned "changes require an administrator"; that would make every start,
  stop or drain from the tray an elevation prompt for the person who owns the build machine. Instead everybody the
  ACL admits (interactive users of the machine) may control the runners. `Request::mutates()` marks the requests that
  change something, so a stricter rule can be added without a protocol change. On a shared multi-user build machine,
  interactive users can therefore stop each other's runners — a known limitation.
- **Framing.** 4-byte little-endian length + JSON, at most 1 MiB (`MAX_FRAME`); malformed or oversized frames close
  the connection. The first frame must be `Hello { protocol }`; `PROTOCOL_VERSION` is **2**.
- **Push.** `Request::Subscribe` turns a connection into an event stream: the service answers `Ok`, then sends
  `Response::Event` frames — the full status at once and whenever it changes (the comparison ignores the service's
  own CPU figure), and notifications. A subscriber that falls 64 events behind is dropped and reconnects. The app keeps
  one subscription (`events.rs`) that feeds the UI (`pitcrew://event`), the tray and native notifications, and
  reconnects with backoff; other requests use one short connection each, with a timeout.
- **Validation.** Every field is parsed into typed values on arrival (`RunnerName`, `RunnerVersion`, `Secret`, and a
  `Config` validated exactly like a file on disk); the app validates first, the service again.

## Consequences

- The UI never polls status. Logs are paged on request (the follow mode asks for lines after the last sequence
  number every 2 s while visible).
- One protocol for both platforms; the transport is small per platform, with the unsafe code limited to building the
  pipe's security descriptor.
- Tested: framing and hello rules (`pitcrew_core::protocol`), the connection state machine over an in-memory duplex
  (`ipc.rs`), and the real pipe or socket end to end (`tests/transport.rs`, which also checks the socket's mode).

## Amendment 2026-11-11: changes need an elevated administrator (Windows)

The decision "authorisation is the ACL" above is reversed on Windows. The pipe still admits interactive users, but the
service (LocalSystem) runs a program that a request names (`binary_path`, `dir`), so a request that changes something let any
program of any logged-on user run code as SYSTEM (release blocker, 2026-11-11).

- **Server.** A request with `Request::requires_admin()` is accepted only when the pipe client's own
  token has Administrators enabled (`paddock_ipc::endpoint::raw_peer_of_pipe`, `ImpersonateNamedPipeClient`; a UAC-filtered
  token or an unidentifiable caller is refused with `PermissionDenied`). The first such request on a connection asks, so
  reads never do. macOS and Linux: the socket is the user's own and the service runs as that user; nothing is asked.
- **App.** The app tries as itself; on `PermissionDenied` it runs itself elevated (`runas`) once to forward that single
  request (`--forward-request <file> <sha-256> <file>`; the digest on the command line stops a swap of the request file).
  One Windows prompt per change.
- **Which requests (rule 77).** `requires_admin` is true for what changes **what runs or where**: `SaveSettings`, `AddRunner`,
  `RemoveRunner`, `Register`, `SaveRunnerConfig` (the runner's own `config.yaml`), `PinVersion` and `SkipVersion` (a setting the
  elevated side acts on). Operating an existing runner (start, stop,
  restart, drain, resume, check, apply or cancel an update of the managed binary from its checked release source, disk and
  process housekeeping) and every read stay open to the pipe's users, like `sc stop`. The match names every request, so a new one must decide.
- **Folders.** The data folder is protected (shared trust rule); a runner that starts with the service's token needs a
  protected program and folder, else it is refused rather than copied (a copy of a runner is large, carries its
  registration and would need repair on every update; moving it is one visible step for the administrator).
- **Pipe.** The pipe's ACL gives interactive users exactly `0x12019b` (read and write data, no `FILE_CREATE_PIPE_INSTANCE`;
  `GRGW` includes it), so a standard user cannot host `\\.\pipe\pitcrew` and receive an elevated caller's connection. Clients
  open with that mask and at identification level (`pitcrew_core::service_paths::open_pipe_options`); the elevated forwarder also
  checks that the server is the SYSTEM service (a held handle to it, its token's user, its pid read twice) before it sends.
- **Callers.** Every request that changes anything is attributed (an unidentifiable caller changes nothing, not even a Stop) and
  logged with the account's SID. "Elevated" means the Administrators group is enabled and not deny-only.

## Second amendment 2026-11-12: the service touches only folders only administrators can change

A security review of the first amendment found, one after another, places where the SYSTEM service did work inside a folder a
user can change (the update swap, the logs, cleanup, the cache trim whose folder comes from the runner's own `config.yaml`, an
`icacls` grant on a folder holding a user's hard link). Each fix closed one door and the next review found another. The user
decided on 2026-11-12 to remove the class, following common practice.

**Decision.** The SYSTEM service manages **only service runners whose folders only administrators can change** (SYSTEM,
Administrators, TrustedInstaller own them and nobody else may write, delete or take ownership of them or of anything above
them). A runner in a folder a user can change is run by the user, in the user's own session, and managed with the user's own
rights. The service never reads, writes, deletes or runs anything in a folder a user can change.

**Common practice (ER-37).** The GitHub Actions runner has the same shape: the application is installed in a folder owned by the
account that runs it, `svc.sh` or `config.cmd --runasservice` installs a service that runs it as that account (on Windows the
default service account is NETWORK SERVICE, not SYSTEM), and GitHub's security guidance tells the administrator that workflow
code runs with that account's rights and that the runner folder must not be writable by others; a runner that needs a desktop
(UI tests) is started with `run.cmd` in a logged-on session instead of as a service. Pitcrew's service mode is that service; its
interactive mode is that `run.cmd`. (Quoted from the GitHub documentation as the author knows it; confirm the wording when the
user guide is written.) A service that does file work with its own rights inside a folder users write to, as Pitcrew did, is the
shape OS vendors' guidance calls a confused deputy, and the one a link, a junction or a hard link attacks.

**What the service does.**
- The guard is `protected::check_runner_folder` (the shared strict trust rule over the folder and its ancestors, then every
  direct child of the folder). Every request that makes the service read, write or delete in a runner's folder
  (`Request::folder_runner`: logs, disk, cleanup, cache, configuration file, update, start, registration) is refused with
  `PermissionDenied` and a fix-it message, and logged, when the guard says no; a runner whose folder fails is **blocked**: the
  service does not start it, probe it, update it, scan or clean it, and keeps its log in its own folder
  (`data\logs\blocked\<name>`), never in the runner's. The guard is asked again on every request and every probe tick, so an
  administrator's fix, or a user's change, is seen within seconds. Deeper entries are not walked on every request (a runner's
  `work` can hold millions of files, made by the runner as SYSTEM inside a folder that forbids users to add to it, so they inherit
  that); the folders an older Pitcrew made directly inside a service runner's folder are locked at start with the folder itself
  (`settle_runner_dir`, one folder at a time, by handle, never propagated).
- The cache trim takes its folder **only below the runner's own folder**, never from `config.yaml`: an absolute path or a `..` there
  is refused (`hygiene::cache_dir_of`).
- Deletion never follows a link (`safe_delete::remove_below`: no `canonicalize`, every directory on the way is a real folder).
- Removed as no longer needed: the `icacls` grant of interactive folders, the impersonation of the logged-on user for deleting,
  and the settle of interactive runners. Kept: `lock_dir`, the pure decision split, fail closed.
- The app checks the pipe's **owner** (SYSTEM or Administrators) through its own handle before it writes anything on any Windows
  connection, reads and the event subscription included (`pipe_server`, tested with a fake identity that shows nothing is written
  first). The forwarder's hand-off file and folder are opened without following a link.

**What this costs, and what is not built.** A runner in a user's folder is no longer supervised by the SYSTEM service on Windows:
it is shown blocked, with the reason. Running, updating, cleaning and logging such a runner with the user's own rights is the
app's side of this decision. The app has no runner supervisor today; the cheapest reuse is the same engine (`pitcrew-service run`)
started in the user's session at log-on with its own data folder, exactly as on macOS and Linux (where the service already is the
user's own), and the app talking to both. That is a work package of its own (a design decision for the user: one engine per
user, or the app hosting it) and is **not** part of this change. Until it lands, an interactive runner must live in an
administrator-only folder to be supervised by the service at all (and cannot write there), so on Windows the supported supervised
mode is the service runner.

**New limits.** (1) A service runner in a folder users can change (such as `C:\actions-runner`) is blocked; move it under
`C:\Program Files` or `C:\ProgramData\Pitcrew\runners`, which the service locks at start. (2) A user's own runner is not
supervised by the service (above). (3) Only the folder, its ancestors and its direct children are checked, not every file below:
an older entry deeper down with a weak ACL would not be detected, and a job that runs as SYSTEM can create what it likes in its own
folder (that is the nature of a service runner; it is why the folder must be administrator-only). (4) `EnginePrune` and
`CacheClean` by a standard user are an availability point, not an elevation: they delete what the policy allows, in the
administrator-only cache folder. (5) UAC on the same desktop is not a boundary against a program of the same user; the service's
check on who asks is. (6) The container engine (`docker`, `podman`) is found on the service's `PATH` by SYSTEM for the engine
report and prune: a `PATH` entry a user can write could name a program SYSTEM would run. Out of scope here; the engine requests
should resolve the program from a protected folder (follow-up).
