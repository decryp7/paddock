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
- **Programs the service starts.** A runner that starts with the service's token needs a protected program and folder; so does
  every program the service runs for a runner (the version probe, `generate-config`, `register`, an update's check). For an
  interactive runner in a folder users can change those are refused with the reason (register it with the runner's own command
  as the user; Pitcrew adopts it). Deleting in a runner's folder (cleanup, cache trim) never follows a link, and where users can
  change the folder it runs as the logged-on user.
- **Data folder and runner folders.** `C:\ProgramData` lets users create folders, so a folder made by an older Pitcrew keeps what it
  inherited. At start the data folder is locked, and each runner folder inside it is settled: a `service` runner's folder (and the
  folders above it) is locked (one folder at a time, by handle, never propagated), a **new** `interactive` runner's folder gets
  SYSTEM and Administrators full control and interactive users modify (an empty folder only: `icacls` rewrites what is already
  in a folder, and a hard link a user planted could make that rewrite a file they cannot write). An existing interactive folder is
  left as it is. The VM test `settled_runner_folders_protect_service_runners_and_leave_interactive_ones_writable` measures both the upgrade and the fresh layout (its first VM run is still to be recorded).
- **Known limits.** (1) A service-mode runner in a user-writable folder such as `C:\actions-runner` is refused at start, with the
  reason and next step shown; runners under `%ProgramData%\Pitcrew\runners` pass after the settle. (2) An interactive runner in a
  folder users can change cannot be version-probed, configured or registered by the service (see above). (3) Only the runner's
  program and folder are checked, not its `config.yaml` or `logs` folder inside it: an older file with a weak ACL would not be
  detected, and the service writes its runner log under the folder. (4) `EnginePrune` and `CacheClean` by a standard user are an
  availability point (they delete what the policy allows, never more), not an elevation. (5) The prompt itself: UAC on the same
  desktop is not a boundary against a program of the same user.
