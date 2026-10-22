# 2. Runner auto-update: verified downloads, updates between jobs, automatic roll-back

- Status: accepted (2026-10-22)
- Related: ADR 0001 (architecture), ADR 0003 (service model); code: `pitcrew_core::{distribution, checksum, version, update}`

## Context

The user decided that Pitcrew updates the runners itself. The PowerShell script it replaces checked the gitea.com
releases API daily, downloaded `*-windows-amd64.exe`, checked the `.sha256` when the release had one (and went on
without when it had none), and swapped the binary when no job ran. It kept no previous binary, could not roll back,
and a busy runner simply waited for the next day.

### Where the runners are published (checked 2026-10-22)

- **Gitea runner.** The project moved from `gitea/act_runner` to `gitea/runner` and the binary from `act_runner` to
  `gitea-runner` (current release v5.2.0).
  - `https://dl.gitea.com/gitea-runner/version.json` → `{"latest":{"version":"5.2.0"}}`.
  - `https://dl.gitea.com/gitea-runner/<v>/gitea-runner-<v>-<os>-<arch>[.exe]` for `windows-amd64`, `darwin-amd64`,
    `darwin-arm64`, `linux-amd64`, `linux-arm64` and more, each with `<file>.sha256` (`<hex>  <file name>`), an
    `.xz` variant with its own `.sha256`, and `checksums.txt` for the whole release.
  - The same files are attached to the gitea.com release `v<v>`
    (`https://gitea.com/api/v1/repos/gitea/runner/releases/latest`).
  - `https://dl.gitea.com/act_runner/` answers 301 to `gitea-runner/`; versions before 1.0 used
    `act_runner-<v>-<os>-<arch>` names under `act_runner/` (assumed for pinning old versions; verify before relying
    on it).
  - **No signatures** (no `.asc`/`.sig`) are published for the runner, unlike Gitea itself.
- **Forgejo runner** (`code.forgejo.org/forgejo/runner`, v13.2.0): `forgejo-runner-<v>-linux-<arch>` with `.sha256`
  and an OpenPGP `.asc` signature. **No Windows or macOS binaries are published.**

## Decision

### 1. Discovery and download

- The service checks on a schedule (`check_every_hours`, default 24, jittered so many machines don't hit the
  server at once) unless the policy is manual, and whenever the user asks. It reads the kind's latest-version URL
  (`RunnerKind::latest_url`), bounded to 256 KB, with a timeout.
- Downloads go to `<runner dir>/bin/<binary>.download` over HTTPS only, with a timeout, a size cap (256 MB) and the
  OS certificate store; the uncompressed binary is downloaded (no decompressor to trust; the `.xz` can be added to
  save bandwidth later).
- Release locations are data per kind (`RunnerKind::release_files`); a new kind (Forgejo, GitHub's runner) adds its
  description and, where needed, its own discovery parser — nothing else changes.

### 2. Verification

- The SHA-256 is computed while streaming the download and compared with the published checksum
  (`checksum::verify`); a mismatch deletes the download and reports the expected and actual digest.
- Because Gitea publishes no signature, a checksum is only as trustworthy as its host. Pitcrew therefore reads the
  checksum from **both** `dl.gitea.com` (`<file>.sha256`) and the gitea.com release (`checksums.txt`) and requires them
  to agree (`checksum::agreed`). If the second source cannot be reached the first is used and the result says so; if
  they disagree the update fails.
- When a kind publishes a signature (Forgejo's `.asc`), it must verify against the project's pinned public key
  before the binary is used (OpenPGP verification with a Rust library such as `pgp`/rPGP, MIT/Apache; to be added
  with the Forgejo adapter). A download without a checksum is never installed.
- After verification the new binary is run with `--version` (with a timeout) and must report the expected version.

### 3. Policy (`update::policy`)

- **Automatic** (default): install when found, inside the optional update window (days + local time span, may
  cross midnight).
- **Notify only**: check on the schedule and notify; the user installs from the window or tray.
- **Manual**: no scheduled checks.
- **Pinned version**: stay on that version; pinning an older version is how a user goes back. Without a pin only
  newer, non-pre-release versions are offered; Pitcrew never downgrades on its own.
- Defaults apply to every runner; a runner may override them.

### 4. Installing between jobs (`update::machine`)

The state machine is pure; the service feeds it events and carries out its commands:

1. **Downloading → Verifying** (as above). Failures end the update with the installed version untouched.
2. **Draining**: wait until the runner has no job (no child processes). The runner keeps working meanwhile. If it
   stays busy past the drain timeout (default 3 h), the update is **postponed** to the next check — a running job is
   never killed for an update.
3. **Stopping**: ask the idle runner to stop gracefully (Ctrl+Break to its process group on Windows, SIGTERM on
   macOS); it stops polling and would finish a job it took at that very moment. If it has not exited within the stop
   timeout (default 2 min) the update is postponed and the supervisor restarts the installed version once it exits.
4. **Swapping**: the installed binary is renamed to `<binary>.previous` and the verified download renamed into
   place (same folder, so both renames are atomic). A failure puts the old binary back and restarts it.
5. **Confirming**: start the runner and wait for the server to accept it ("declare successfully" in its log) within
   the confirm timeout (default 5 min). Accepted = **updated**.
6. **Rolling back**: if the new runner exits before it is accepted, or is not accepted in time, Pitcrew stops it,
   puts `<binary>.previous` back, starts it, and records **rolled back** with the reason. If there is no previous
   binary (first install) or the roll-back fails, the result is **failed** and the runner needs the user (tray badge
   and notification).

"Picks up a job" is recorded as a further confirmation when the next job starts, but is not required: a runner can
be idle for hours. If the updated runner crashes during its first job within 24 hours, Pitcrew offers to roll back
(planned: automatic roll-back on the first job's runner crash).

The previous binary is kept until the next successful update. The user can cancel an update until the swap.

### 5. What the user sees

Per runner: installed version, latest version found and when it was checked, the update in progress (phase), and
the last update's result (updated / rolled back / postponed / cancelled / failed) with time and reason. Failures and
roll-backs notify; "notify only" notifies about available versions.

## Consequences

- An update can never leave a runner without a working binary unless both the new and the previous one fail, and
  then the user is told.
- Integrity rests on HTTPS plus two independent checksum hosts for Gitea's runner, and on signatures where a project
  publishes them. If Gitea starts signing runner releases, verification is added to the kind's description.
- Forgejo's runner on Windows and macOS can only be a user-provided binary (`binary_path`), which Pitcrew supervises
  but never updates.
- The core's state machine and policy are fully unit-tested (including every roll-back path); the service's
  download, swap and process control need integration tests with a fake runner binary.
