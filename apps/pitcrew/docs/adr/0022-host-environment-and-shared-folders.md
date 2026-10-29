# 22. The host environment: shared folders, variables and PATH for every runner, with their sources shown

- Status: Accepted (2026-10-29), by the user, after independent review (rounds 1–2). The owner decided the four open
  points on 2026-10-29 ("Decided by the user" at the end); the reviews are answered in "Reviews: what changed".
- Date: 2026-10-29
- Amends: ADR 0004 (the environment, secret, clean and reachability requests are authorised from the caller's identity,
  not by the pipe's ACL alone), ADR 0005 (environment secrets are created, stored and read by the **service**), ADR 0009
  (the runner environment gains a host layer and a longer reserved list), ADR 0013 (the low-disk guard watches every
  volume a runner writes to; two new hold reasons, `SharedFolderUnavailable` and `SecretUnavailable`; `SpaceCache` is keyed
  by volume), ADR 0016 (cache trimming adopts this ADR's "clean when idle" lock instead of refusing while a job runs). Each
  of these ADRs carries a back-reference to this one.
- Related: ADR 0006 (housekeeping), 0011 (config.yaml editing, restart after the job), 0021 (auto-scale, proposed);
  family rules 10-13 (jobs, progress, no UI timeouts, bounded parallelism), 17 (dialogs are windows), 18 (container
  queries, middle truncation), 21 (knowledge lists are TOML packs), 42 (share first), 44 (a settings section has a
  setting), 47 (Material first), 48 (recorded benchmarks), 51 (parallelism and cost); family ADRs 0008/0024 (jobs,
  background work), 0009/0026 (dialogs), 0020 (shared executor), 0025 (CI job isolation, proposed, on `ci/job-isolation`);
  the principle "local first, opt-in online".
- Code today (read on 2026-10-29 at `a24fb2be6`): `pitcrew_core::{runner_env, config, disk_guard, hold, hygiene,
  runner_settings, runner_config, state, history, notify, redact, protocol}`, `pitcrew_service::{login_env, supervisor
  (launch), launch, launcher, probe, platform::win_session}`; the shell branch `pitcrew/shell-1.1` (not merged) for the UI
  and jobs. Upstream: gitea/act `pkg/runner/run_context.go`, `startHostEnvironment`, read 2026-10-29.
- Mock-up: [design/environment.html](../design/environment.html), rendered by
  [design/render.mjs](../design/render.mjs) (compiles the current tokens, then shoots every frame at 2x in both
  schemes into `design/renders/`). The mock-up is reviewed with this ADR (rule 22).

## Context

The owner asked for "an environment setting screen to set PATH etc. for shared resources for runners". On a build host,
several runners need the same things: a compile cache (sccache), Cargo's home, the npm cache, Playwright's browsers, a
build or target root on a big disk, tool caches, and the Actions cache server. Today each is configured by hand per
runner, or in each workflow.

**What exists (verified in the code, 2026-10-29):**

- **Login-shell PATH** (macOS, Linux): the service asks `$SHELL -l -i -c` for `PATH` (5 s timeout, fallback list) and
  passes it to every runner start (`login_env.rs`, `runner_env::runner_environment`). Only `PATH` is taken from the
  shell; every other variable of the runner process is what the service inherited from launchd or systemd.
- **`[runners.env]`** in `pitcrew.toml`: per runner, validated by `runner_env::validate_env` (at most 32 variables, names
  `[A-Za-z_][A-Za-z0-9_]{0,63}`, values up to 4096 characters without control characters, `PITCREW_*` and
  `GITEA_RUNNER_*` reserved). An invalid table skips the whole runner (`config.rs` line 515). No UI edits it.
- **Windows, corrected:** the roadmap says the table is "applied on macOS and Linux only". In the code it is applied to
  runners in `service` session mode (`launcher.rs` falls back to `spawn_child`, `std::process::Command::envs`, over
  LocalSystem's environment) and ignored for `interactive` runners: `win_session::start_in_session` builds the user's block
  with `CreateEnvironmentBlock` and never merges `spec.env`.
- **IPC authorisation** (ADR 0004): on Windows the pipe's DACL gives SYSTEM, Administrators and the owner full access and
  **interactive users read/write**; "authorisation is the ACL", so any interactive user may send any request. On macOS
  and Linux the socket is 0600 in a 0700 folder: only the agent's own user.
- **config.yaml** (ADR 0011): the form edits `runner.envs`, the cache (`cache.enabled`, `cache.dir`,
  `cache.external_server`) and container options and volumes, one runner at a time.
- **Disk:** the guard drains a runner below `drain_below_mb` on the disk of *its folder* (ADR 0013). `Probe::disk_space`
  (`probe.rs` line 179) keys `SpaceCache` by the *path* and answers with the **longest mount point that is a prefix** of
  it; for a folder on an unmounted disk that is the boot disk. `SpaceCache` clears all entries when it reaches
  `MAX_DISKS` (64) keys (`disk_guard.rs` line 114). ADR 0016 measures and trims the Actions cache and refuses while a job
  runs.
- **Background priority:** ADR 0024 makes `Class::Background` binding for background work; `priority::apply_current` is
  **not built yet** (ADR 0024, Context; it is to live in `paddock-proc`).
- **Secrets:** ADR 0005: the OS credential store, owned by the *app*; the service never reads it.
- **Shared code that fits** (rule 42): `paddock-storage` (bounded walk, preview, guarded clear, `also_within`), `paddock-jobs`
  (jobs, keys, `par_walk`, Cancel), `paddock-secrets` (`Secret`, `Secrets` port, keyring and owner-only file adapters),
  `paddock-packs` (TOML packs), `paddock-net` (egress policy, Offline mode, audit).

**What a host job sees.** In gitea/act's `startHostEnvironment` (read 2026-10-29) a host job's environment is built from
the workflow's own variables, then `RUNNER_<KEY>` is set **unconditionally** for each key of the runner context, then the
runner process's `os.Environ()` is copied in **without overriding** keys already set ("don't override"). On the host the
runner context is `HostEnvironment.GetRunnerContext` (`pkg/container/host_environment.go`, read 2026-10-29): exactly
`os`, `arch`, `temp` and `tool_cache`. So the runner process's environment reaches every host job step, below the
workflow's `env:`; only `RUNNER_OS`, `RUNNER_ARCH`, `RUNNER_TEMP` and `RUNNER_TOOL_CACHE` from the process are always
overwritten, while other `RUNNER_*` names (for example `RUNNER_DEBUG`) do reach the step. A job in a container does not get `os.Environ()` at all: it sees `runner.envs` and the container
options and volumes of config.yaml (`startJobContainer` sets its own `RUNNER_TOOL_CACHE=/opt/hostedtoolcache`,
`RUNNER_TEMP=/tmp`).

**What the CI kit decided** (family ADR 0025, proposed): host jobs keep folders under one CI home (`PADDOCK_CI_HOME`), one
per host, with per-slot folders under an OS file lock (`flock`/`LockFileEx`); the kept-folder list puts **sccache's disk
cache and npm's cache per slot**, and declares one sccache port block per slot (`ports.txt`). sccache's local disk cache
"only supports a single sccache server at a time" (sccache `docs/Local.md`, read 2026-10-28), and two sccache servers
cannot share a port. Pitcrew is a general tool, but its defaults must not contradict this.

## Decision

### 1. One host layer, five kinds, one composition

A new optional section `[environment]` in `pitcrew.toml` holds the host-wide definitions. Every entry applies to all
runners unless it names some (`runners = [...]`; a future runner gets it by default, so ADR 0021's pool slots do too):

| Kind | What it is | Example |
|---|---|---|
| **Shared folder** | a variable whose value is a folder; validated as a folder, measured, cleanable by its preset's rule | `SCCACHE_DIR = /Volumes/build/sccache/{runner}` |
| **Variable** | a plain value | `SCCACHE_CACHE_SIZE = 20G` |
| **Port** | a number per runner from a base: `base + the runner's number` | `SCCACHE_SERVER_PORT`, base 4260 |
| **Secret** | a value stored by the service, never in `pitcrew.toml`, never shown, never logged | `NPM_TOKEN` |
| **PATH addition** | a folder put before or after the login shell's `PATH` | `{home}/.cargo/bin` (before) |

and one setting projected into each runner's config.yaml (section 6): the **Actions cache server**.

**Runner numbers.** Each runner gets a small stable number (`env_number`, 1-16, the lowest free one, assigned when the
runner is added or on first load and written back to `pitcrew.toml`, never reused while the runner exists). A Port value
is `base + env_number`; the block `base+1 .. base+MAX_RUNNERS` is the variable's. This is the same idea as ADR 0025's
per-slot port blocks, for runners instead of slots. **The sccache preset's base is 4260** (ports 4261-4276): clear of the
CI kit's blocks in `conformance/ci/ports.txt` on `ci/job-isolation` (read 2026-10-29: sccache 4227-4230 for its default
4 slots and 4227-4242 for 16; WebKit 4320-4322, 4420-4422, … per slot) and of the family's end-to-end ports (each app's
`paddock.ci.json`: 4359, 4369 and 4390 plus their shards) and every other fixed port in the tree's apps, `ui`, scripts
and workflows (none between 4261 and 4276, checked 2026-10-29). Not 4227, which is the CI kit's first slot.

**Placeholders.** Values may use `{home}` (the runner user's home folder), `{runner}` (the runner's name) and `{n}` (its
number), and `{{`/`}}` for literal braces; anything else in braces is an error. They are expanded per runner before
validation and before each start. For a Windows runner in `service` mode `{home}` would be LocalSystem's profile
(`C:\Windows\System32\config\systemprofile`), a system folder: it is an **error** there ("This runner runs as LocalSystem;
give an absolute folder").

**Presets are a knowledge pack** (rule 21): `packs/env-presets.toml` (built in, plus the user's `packs/` folder, read with
`paddock-packs`, with limits, never executing anything). A preset names a resource, the variables it sets (with their
kinds and suggestions), its **sharing mode**, its **clean rule** and the source each fact was checked against, with the
date. v1's built-in presets (each source re-checked in WP-3):

| Preset | Variables | Sharing | Clean rule | Why |
|---|---|---|---|---|
| sccache (local folder) | `SCCACHE_DIR` (folder), `SCCACHE_SERVER_PORT` (port, base 4260: 4261-4276), `SCCACHE_CACHE_SIZE`, `RUSTC_WRAPPER` | per-runner | `empty` after stopping that runner's sccache server | one server per cache and per port (`docs/Local.md`, ADR 0025) |
| Cargo home | `CARGO_HOME` (folder) | shared | subfolders `registry/cache`, `registry/src`, `git/checkouts` | Cargo locks its package cache; `bin/`, `config.toml` and `credentials.toml` are the user's and never cleaned |
| Cargo target root | `CARGO_TARGET_DIR` (folder) | per-runner | `empty` | absolute paths and Cargo's build lock (ADR 0025) |
| npm cache | `npm_config_cache` (folder) | per-runner | `empty` | ADR 0025 keeps it per slot; Pitcrew follows that rather than relying on cacache's concurrent-writer design |
| Playwright browsers | `PLAYWRIGHT_BROWSERS_PATH` (folder) | shared | `empty` (jobs reinstall) | installs are renamed into place (ADR 0025's `shared` criterion) |
| Gradle home | `GRADLE_USER_HOME` (folder) | shared | subfolders `caches`, `wrapper/dists` | `gradle.properties` may hold credentials; never emptied |
| Paddock CI home | `PADDOCK_CI_HOME` (folder) | shared | `none` (the kit cleans its own slots under its lock) | one per host; the kit takes per-slot folders with `flock` (ADR 0025) |

A remote sccache backend is future work (decision 2).

**Sharing modes** are the domain rule that keeps the owner out of trouble:

- A `per-runner` folder applied to more than one runner must contain `{runner}` or `{n}`: otherwise an **error** with the
  fix offered ("Add {runner} to the path").
- A Port applied to more than one runner is always per runner (that is what the kind is); a Variable that *looks* like a
  port (`*_PORT`) with one value on several runners is an **error** ("Use the Port kind").
- A `per-runner` target root on a runner with `runner.capacity` > 1 is a **warning** (its jobs wait on each other's build
  lock).
- The Paddock CI home together with Pitcrew's sccache preset on one runner is a **warning** ("The CI kit runs its own
  sccache per slot; keep one of them").
- A folder without a preset is `per-runner` unless the owner marks it `shared`.

**One composition, used by the start and by the view.** `runner_env::runner_environment` becomes
`environment::compose(layers) -> Composed`, a pure function returning every variable with its value (or a masked marker),
its source and the values it shadows. The supervisor starts the runner with `Composed::to_launch()`; the effective view
shows `Composed` itself. A property test checks that the two never disagree.

### 2. Precedence (lowest first; a later layer wins per name)

| # | Source (shown as) | Reaches | Who sets it |
|---|---|---|---|
| 1 | **Service** ("Inherited from the service") | runner process | launchd / systemd / the SCM; for Windows interactive runners, the user's `CreateEnvironmentBlock` |
| 2 | **Login shell** ("Login shell") | `PATH` only, macOS and Linux | the user's shell profile |
| 3 | **Host** ("Host: <label>") | runner process | this page; PATH additions go before or after layer 2's `PATH` |
| 4 | **Runner override** ("Runner override") | runner process | the runner's Environment tab (`[runners.env]`, `env_secrets`) |
| 5 | **config.yaml** ("config.yaml, jobs only") | job steps | ADR 0011's form (`runner.envs`, the names in `runner.env_file`) |
| 6 | the workflow's own `env:`, and `RUNNER_OS`, `RUNNER_ARCH`, `RUNNER_TEMP`, `RUNNER_TOOL_CACHE` from the runner context | job steps | outside Pitcrew (act, see Context); the view's footer says so |

Names compare case-sensitively on macOS and Linux and **case-insensitively on Windows** (`Path` is `PATH`; the reserved
list is matched the same way). A runner override of `PATH` replaces layers 1-3 (today's behaviour, kept) and is shown
with a warning. A runner opts out of a host entry by being left out of its `runners` list (one mechanism, edited from
either side). PATH: host "before" entries, the login shell's `PATH`, host "after" entries, then `sanitize_path`.

### 3. Who may change it (authorisation)

On Windows a runner in `service` mode runs as **LocalSystem**, and ADR 0004 lets every interactive user write to the
pipe. Without a check, any interactive user could set `RUSTC_WRAPPER`, `NODE_OPTIONS`, `BASH_ENV` or `PATH` and have a
job run their code as SYSTEM, or have SYSTEM delete another user's files through Clean. So:

- **`Request::needs_owner()`** marks `SaveEnvironment`, `SetEnvSecret`, `ClearEnvSecret`, `MeasureSharedFolders`,
  `CleanSharedFolder` and `CheckEndpoint`. The service authorises them from the **caller's identity**, never from a
  field of the request:
  - Windows: the service reads the client's token (`ImpersonateNamedPipeClient`, `OpenThreadToken`, `RevertToSelf`) and
    admits it when it is **elevated and in BUILTIN\Administrators**, or when its user SID is the **owner SID**. The owner
    SID is recorded by the elevated install step (ADR 0003) in the service's own state; it is never taken from the app.
    Under over-the-shoulder UAC (a standard user's session, an administrator's credentials typed into the prompt) the
    elevated installer runs as the administrator, so its own token is the wrong user: the installer records the
    **console session's user** instead (`WTSGetActiveConsoleSessionId` then `WTSQueryUserToken`, the user SID of that
    token), which is the person sitting at the machine, and shows it in the install step ("Pitcrew will be managed by
    DESKTOP\ana") so a wrong owner is visible at once.
    A change that reaches a `service`-mode runner needs an **Administrator**; the owner alone may change only entries
    that reach interactive runners (which run as that owner). Anything else: `PermissionDenied` with "Only an
    administrator can change what runners running as LocalSystem start with". An "All runners" entry saved by the owner
    does **not** reach a runner that is later switched to `service` mode: that runner starts without the owner-saved
    entries and shows the finding "N environment entries were saved without an administrator; an administrator must save
    the environment once before this runner gets them", until an administrator saves. **Measure and Clean of a folder
    that reaches a `service`-mode runner need an Administrator** too.
  - macOS and Linux: the socket already admits only the agent's user; the service also checks the peer's uid
    (`LOCAL_PEERCRED`, `SO_PEERCRED`) equals its own, so a mode change of the socket cannot widen access.
- **Measure and Clean on Windows run under the caller's token** (each worker thread gets the impersonation token with
  `SetThreadToken`, through a per-thread setup hook that `paddock-jobs`' `par_walk` gains for this, WP-6), so the file system checks the caller's rights: SYSTEM never deletes or lists what the caller could
  not.
- **The decision is pure:** `environment::auth::decide(caller: &CallerIdentity, request, reach)` in the core, over a
  `CallerIdentity` port (`is_elevated_admin`, `user_sid` or `uid`) that each OS adapter fills; the core tests cover the
  table, the Windows adapter and its tests belong to WP-8.
- The **reserved list is a correctness list, not a security boundary**: it stops names that the runner overwrites, that
  break the runner, or that nobody should set by accident. Security comes from the authorisation above.

### 4. Validation in Rust

All checks are pure functions over *facts* the service collects (`FolderFacts`: exists, is a folder, nearest existing
ancestor, its **volume id**, the expected volume id, writable by the runner's user, free bytes, volume kind, world-writable,
is a link and its target). The app may show a hint early; the service repeats every check on save and at each start.
Findings are `error` (save refused) or `warning` (saved, shown), each with the fix in words.

- **Names:** today's syntax and limits. **Reserved** (matched case-insensitively on Windows):
  - `PITCREW_*`, `GITEA_RUNNER_*`, `FORGEJO_RUNNER_*` (the runner's own);
  - `GITHUB_*` and `ACTIONS_*` (act sets these per job);
  - exactly `RUNNER_OS`, `RUNNER_ARCH`, `RUNNER_TEMP` and `RUNNER_TOOL_CACHE`: act overwrites them from the runner context
    (`GetRunnerContext` in act's `host_environment.go`, called from `startHostEnvironment` in `run_context.go`, both read
    2026-10-29), so setting them would silently do nothing. **Other `RUNNER_*` names are allowed with a warning**
    ("RUNNER_DEBUG is read by actions; set it only if you mean every job on this runner");
  - `HOME`, `USER`, `LOGNAME`, `SHELL` (the runner's identity);
  - `DYLD_*`, `LD_PRELOAD`, `LD_AUDIT`, `LD_LIBRARY_PATH` (loader settings that break the runner binary itself);
  - `PATH` in the host layer (use PATH additions).
- **Entries that exist today with a newly reserved name** (only possible in a hand-edited `[runners.env]`) **keep applying,
  with a warning** ("HOME is reserved from 2026-10; it still applies because it was set before. Remove it, or keep it
  knowingly."). The service refuses a save that **adds or changes** such an entry; an unchanged one passes. A runner never
  starts with part of its environment silently dropped. Entries that are invalid today keep today's behaviour.
- **Folders:** absolute after expansion; no `..`; at most 1024 characters; not a root, not the home folder itself, not a
  system folder (`/System`, `/usr`, `/bin`, `/sbin`, `/etc`, `/Library`, `C:\Windows`, `C:\Program Files`, LocalSystem's
  profile); not inside any runner's folder (housekeeping cleans its `work/` and `cache/`, updates swap its binary) and
  not a parent of one; not inside Pitcrew's own folders. Links are resolved for these checks. It must exist, or be
  creatable (`create = true` and the nearest existing ancestor writable **and on the expected volume**). Writable means
  the service created and removed a probe file `.pitcrew-write-check-<random>` in it as the runner's user (on Windows
  with the runner user's token, WP-8). One shared folder inside another: warning, and Clean refuses the outer one.
- **Volume identity.** When a folder is added the service records its volume's identity (`volume_id`): macOS the volume
  UUID (`getattrlist` `ATTR_VOL_UUID`), Linux the file system UUID of its device (`/dev/disk/by-uuid`, else
  `st_dev` with a note that it may change across boots), Windows the volume GUID path (`GetVolumeNameForVolumeMountPoint`).
  At every check and start the nearest existing ancestor's volume must be the expected one; if it is not (an empty
  mount-point folder on the boot disk, a different disk mounted at the same path) the folder is **unavailable**
  (section 5). Nothing is ever created on a volume other than the expected one.
- **Free disk:** read per volume id (section 5); below the guard's `resume_above_mb` is a warning ("runners using it would
  be held").
- **PATH additions:** absolute, no separator inside (`:` or `;`), a folder; missing is a warning (kept, so a tool installed
  later works); writable by other users without the sticky bit is an **error** (another user could plant a `cargo`);
  duplicates of the login `PATH` are a note.
- **Ports:** the block `base+1 .. base+16` within 1024-65535; blocks of two Port variables must not overlap (error).
  **When the Paddock CI home preset applies to any runner of the host**, a Port block that overlaps a port the CI kit
  declares is an **error** ("4227-4230 belong to the CI kit's sccache slots; use a base clear of them, such as 4260"). The
  kit's blocks are knowledge in the preset pack (the CI home preset's `kit_ports`, copied from ADR 0025's
  `conformance/ci/ports.txt` with its date: `sccache 4227 1 1`, `webkit 4320 3 100`, expanded for 16 slots), and a
  conformance test in the repository fails when the pack and `ports.txt` disagree.
- **Ports at start** (a finding, not a refusal, since another process may hold a port only for a while): before starting a
  runner the service reads the OS socket table (macOS `proc_pidfdinfo` over the listening sockets, Linux `/proc/net/tcp*`
  with inode to pid, Windows `GetExtendedTcpTable`; no bind attempt, so no race with the owner) for each of the runner's
  ports. A port held by a process other than this runner's own `sccache` (same user, the runner's process tree or an
  `sccache` started with this port) gives a warning on the runner and a notification: "Port 4261 (SCCACHE_SERVER_PORT) is
  in use by node (pid 4120); sccache on mac-a cannot start its server. Free the port or change the base." One table read
  per start, only for runners with Port variables.
- **Secrets:** a name that looks secret (`*TOKEN*`, `*SECRET*`, `*PASSWORD*`, `*PASSWD*`, `*_KEY`, `*CREDENTIAL*`)
  defaults to the Secret kind; keeping it plain is a warning.
- **Limits** (constants explained by their messages, family ADR 0019): host entries of all kinds 64, PATH additions 32,
  runner overrides 32 (today), runner secrets 16, value 4096 characters, labels 64 characters, the composed block 64 KiB
  (a larger one holds the runner with "Fix the environment" instead of failing in `exec`), presets per pack 128.

### 5. Secrets, applying changes, holds and the disk guard

**Secrets: one mechanism on every OS — the app hands the value to the service, the service stores and reads it.**

- The app never stores environment secrets. `SetEnvSecret { scope, name, value: Secret }` carries the value once; the
  service stores it through `paddock-secrets`' `Secrets` port with a backend that a background service can read with
  nobody at the screen:
  - **macOS:** an owner-only file store (`FileSecrets`, mode 0600 in the service's 0700 data folder). The Keychain is not
    used for now: an item's access list is bound to the creating program's code signature, Pitcrew's builds are ad-hoc
    sealed (rule 23), so every update would change the signature and the next read would raise a prompt that nobody
    answers. This store protects against other users and against files, logs and screens; it is the same boundary as the
    runner's environment itself (below). When Developer ID signing lands (roadmap Phase 4) the backend becomes a Keychain
    item **created by the service**.
  - **Linux:** the same file store (a systemd user service often has no unlocked Secret Service).
  - The file store is **excluded from backups** (macOS: `tmutil addexclusion` on the file, which sets the
    `com.apple.metadata:com_apple_backup_excludeItem` attribute; Linux: a `CACHEDIR.TAG`-style exclusion is not
    universal, so the docs name the file for the owner's backup tool) and **never goes into the diagnostics bundle**
    (the bundle lists the store's existence and entry count only). Caveat, in the docs and as a warning finding when the
    service sees it: a data folder on NFS or SMB, or a home folder synced by a cloud or roaming-profile service, copies the
    file elsewhere and file modes may not hold there; keep Pitcrew's data folder local.
  - **Windows:** LocalSystem's own Credential Manager (DPAPI), written and read by the service.
- The wire never carries a value back: replies say `present: bool`. `Debug` is redacted; logs, history, notifications
  and the diagnostics bundle name variables, never values (`redact.rs` gains the environment's secret values as exact
  strings to remove, so a job log line that echoes one is scrubbed in Pitcrew's own views too).
- A secret that cannot be read **holds** the runner (`HoldReason::SecretUnavailable`, "Secret NPM_TOKEN is not
  available; set it again") instead of starting it without.
- Honest limit, in the dialog and the docs: every value in a runner's environment is readable by every job it runs and
  by processes of the same user (`ps eww`, `/proc/<pid>/environ`). The Secret kind protects files, logs and screens, not
  jobs.

**Applying.** A save computes, per runner, a fingerprint of its composed environment: SHA-256 over names and plain values,
and for each secret its name and a **revision counter** (incremented by every `SetEnvSecret`), never a hash of the value.
It is compared with the fingerprint the running process started with (`state.json`, optional `env_started`). Changed
runners restart **after their current job** through the controller's `Input::Restart`, as ADR 0011 (`RestartPlan`:
`notNeeded`, `notRunning`, `afterJob`). The dialog states the consequence before Save: "Saving restarts 3 runners after
their current job."

**Holds** (ADR 0013's tracker, history event, notification):

- `SharedFolderUnavailable`: a folder the runner uses is unavailable (wrong or missing volume). Released when the expected
  volume is back. Without this, sccache would recreate the path on the boot disk and fill it.
- `SecretUnavailable`: as above.

**The disk guard watches every volume a runner writes to** (its folder's and its shared folders'): held when any is below
`drain_below_mb`, resumed when all are above `resume_above_mb`. `SpaceCache` is keyed by **volume id**, not by path, so
more folders on one disk cost nothing and an unmounted folder never reads the boot disk's free space through the
longest-prefix fallback (`probe.rs` line 179); the 64-entry clear (`disk_guard.rs` line 114) then counts volumes.

### 6. The Actions cache server

The runner reads the Actions cache server from config.yaml (`cache.external_server`), not from the environment (act sets
`ACTIONS_CACHE_URL` per job). The page offers one host value in `[environment.actions_cache]`; "Apply to runners…" opens a
**multi-runner** version of the config-diff dialog (ADR 0011), one diff per runner, and saves through the same patcher,
backups and restart plan. A runner whose config.yaml differs shows "Differs on mac-c". `cache.external_secret` stays a
config.yaml secret that ADR 0011 never displays.

**Reachability check, opt-in only** (decision 3): a "Check connection" button, never automatic. It goes through
`paddock-net`, which gains a **connect-only probe** (`Egress::probe_connect(host, port, timeout)`: the same reach policy,
Offline mode and audit entry as an HTTP request, but only a TCP connect, nothing sent). Results: "Port open", "Refused",
"Timed out", "Name not found", "Blocked: offline". It says that an open port is not proof the cache works. In Offline mode
the button is disabled with the reason.

### 7. Size and clean-up per shared folder

- **Measure** is a `paddock-jobs` job per folder (key `measure:<entry>`), phase "Measuring <label>" with a growing count of
  files and bytes (total unknown), over `paddock-storage`'s bounded walk (entry and time budget, no links, one volume; a
  stopped walk says the size is a lower bound). It runs only when the owner asks (Measure sizes, or Measure in a row's
  menu). The last result is kept in `state.json` (optional `shared_sizes`: bytes, files, measured at, partial,
  estimated), so the page opens with sizes and their age; a new folder shows "Not measured".
- **Clean** follows the preset's clean rule: `empty` (all top-level items), `subfolders` (only the named subfolders'
  contents) or `none`. A folder without a preset has **no Clean** until the owner names its cleanable subfolders
  (`clean_subfolders`). CARGO_HOME and GRADLE_USER_HOME are never emptied wholesale.
- **One policy with ADR 0016: clean when idle, under a lock.** Flow: preview (what goes, size, count) in a decision
  window → **Clean when idle**:
  - The clean job takes a **maintenance lock** on the runners using the folder (for a per-runner folder: **one runner at
    a time**, so the others keep working). The controller drains a locked runner and **refuses Resume, Start and
    Restart** for it with "Cancel the clean first" (the UI shows that text and the Cancel button). The lock is held in
    the service's memory and in `state.json` (`maintenance`), so a service restart releases it with a history event
    instead of stranding the runner. The lock also **blocks Pitcrew's own automatic actions** on a locked runner: a hold
    tracker's auto-resume (ADR 0013), the runner's automatic update (ADR 0002: postponed, as while a job runs) and the
    service's self-update restart (ADR 0009's `self_update::decide` treats a held lock like a running job).
  - Before **each item** the job re-checks that every locked runner is drained and has no job; if not, it stops and
    reports what it removed (belt and braces against a lock bug).
  - For sccache the job stops that runner's sccache server first (`sccache --stop-server` with its
    `SCCACHE_SERVER_PORT`, as the runner's user, with a timeout).
  - Phases, each on its own scale, counting up: "Waiting for jobs to finish" (N of M finished), "Stopping sccache"
    (where it applies), "Checking" (items re-checked against the preview, N of M), "Removing" (bytes of the previewed
    total), "Releasing runners". **Cancel** works in every phase: the lock is released, removal stops after the current
    item, the result says what was removed and what was left. The UI never times out; a long wait shows "Still working".
  - ADR 0016's Actions cache trim adopts the same lock and "when idle" flow instead of refusing while a job runs.

### 7a. Parallelism and cost (rule 51)

Measuring and cleaning are the bulk operations: I/O-bound walks over folders that can hold hundreds of thousands of files
on the disk the runners build on, so they must cost the jobs as little as possible.

**(a) Workers.**

- **Jobs vs threads.** One job per folder. Jobs on the same volume run **one after another** (a FIFO per volume id), each
  using that volume's whole thread budget; jobs on different volumes run side by side. So there is no "N jobs per
  volume"; there is a **thread budget per volume**.
- **Thread budget: Settings with Auto** (rule 51), `[defaults.storage_io]`, validated in the engine:
  `walkers = "auto" | 1..=16`, `removers = "auto" | 1..=8`. Auto is the table below, by the volume kind the service
  probes once per volume (`Probe::volume_kind`: SSD, HDD, network, unknown). The total over all volumes never exceeds
  cores minus one.

  | Volume kind | Walkers (Auto) | Removers (Auto) | Why |
  |---|---|---|---|
  | SSD | 4 | 2 | metadata reads parallelise on flash; removal serialises on directory locks |
  | HDD, network, unknown | 1 | 1 | seeks and round trips; parallel walkers make it slower |

- **Priority:** every walker and remover runs in **`Class::Background`** of ADR 0024 (QoS background on macOS, nice 10
  and the idle I/O class on Linux, `THREAD_MODE_BACKGROUND_BEGIN` on Windows, which also lowers I/O priority), so a
  running build's I/O comes first. This depends on `priority::apply_current`, which ADR 0024 says is **not built yet**;
  WP-5 uses it once it exists and until then runs the walk on one thread per volume. There is no "yield while busy":
  Pitcrew has no I/O-busy signal (ADR 0013's `when-idle` is CPU), and building one (per-disk I/O counters per OS) is not
  worth it next to background I/O priority.
- The walk is `paddock-jobs`' `par_walk` under `paddock-storage`'s budget; Cancel is honoured within
  `CANCEL_CHECK_ITEMS`.

**(b) Cheaper operations, no repeated work.**

- **One job per folder.** A second request with the same key joins the running job. A Clean cancels that folder's pending
  Measure.
- **A size index keyed on ctime and file id.** Each measured folder keeps an index in Pitcrew's cache folder
  (`cache/env-sizes/<hash of the path>.bin`, versioned, missing or damaged means rebuild): per directory its file id
  (inode / file index), its **ctime** (status change, which `touch` cannot set back and which changes on every entry
  added, removed or renamed), and the summed size and count of its direct files. A re-measure stats each directory and
  re-lists only those whose ctime or id changed. Caches write by rename (sccache, npm's cacache, Playwright's installs),
  which changes the directory's ctime, so an unchanged cache costs one `stat` per directory instead of one per file. A
  file rewritten in place is missed: such a result is labelled "estimated", and "Measure again from scratch" (row menu)
  ignores the index. There is no timer.
- **Budgets of the index:** about 64 bytes per directory plus its name; at most **500,000 directories per folder** (about
  40 MB on disk; beyond it the folder is measured without an index and the page says so) and **256 MB for all indexes**
  (the least recently used index is dropped). In memory only during a measure: one folder's index at a time per volume.
- **Nothing on a timer.** Free space comes from `SpaceCache` (one read per volume per 60 s, shared with the guard).
- **Clean reuses the preview.** The preview lists top-level items (or the named subfolders) from a fresh index without
  a second walk; removal re-checks each item with one `stat` (`paddock-storage`'s guard); afterwards the index drops the
  removed items and the size is the preview minus what was removed, not a re-measure.
- **Validation is cheap.** Folder facts are probed once per folder per preview; the writability probe at most once per
  folder per 60 s; a runner start composes its environment once and reads each secret once.
- **Measured, not assumed:** WP-5 records walk, incremental re-measure and clean rates on synthetic trees (100 k and 1 M
  files, SSD and a USB HDD) in `docs/performance.md` (rule 48) before any number is quoted.

### 8. Windows joins this work

The host layer applies on every OS. WP-8 merges the composed variables into the block `CreateEnvironmentBlock` builds for
interactive runners (a pure `env_block::merge` over UTF-16 `NAME=value\0...\0\0` with case-insensitive names and `;` PATH
lists, in the core; the FFI stays in `win_session.rs`), checks writability with the runner user's token, and uses the
service's Credential Manager for secrets. The login-shell layer does not exist on Windows. Until WP-8 lands, the page and
the runner tab show "Not applied to interactive runners on Windows yet" on each affected runner, never a silent no-op. The
roadmap's Phase 3 item "Per-runner env on Windows" is closed by WP-8.

### 9. UI (main window)

- **A page "Environment"** in the sidebar (order 550, after Storage): a working view with sizes, jobs and a per-runner
  view, so a page, not a Settings section (rule 44). Four sections, each a `mat-table` (rule 47): Shared folders,
  Variables (plain, port and secret), PATH additions (ordered, `cdkDropList` with Move up/down buttons for the keyboard),
  Actions cache server (one form row, two status badges, a help line). Toolbar: Measure sizes (text button), Add (the one
  primary, a menu: Shared folder, Variable, Secret, PATH addition).
- **A runner tab "Environment"**: the effective environment (`mat-table`: Name, Value, Source, Note, row actions), a
  filter field and a source filter (`mat-button-toggle-group`: All, Set by Pitcrew, Inherited); a row expands into the
  values it shadows; `PATH` expands into its entries with their sources. Row actions: Override for this runner, Stop using
  on this runner (host entries), Edit or Remove (runner overrides), Copy name.
- **Dialogs are windows** (rule 17, ADR 0026): Add/Edit (form, modal, 560, fitted height; it lists the preset's other
  variables with their per-runner values), Clean (decision, 560, then the job's phases), Apply cache server
  (multi-runner config diff, 720).
- **Calm by default:** a row without a problem shows no badge; status appears only for exceptions (word + icon + colour).
- **States:** skeleton rows in the tables' geometry, drawn with the family's `--pd-skeleton` token as it is (decorative,
  not a control or a graphic that carries meaning, so WCAG 1.4.11 does not apply), with `aria-busy="true"` on the section
  and a `role="status"` line "Loading the environment…" as its text equivalent; empty ("Give every runner the same caches and tools", one button);
  error with what failed, Retry and Open logs; "service not running" with Repair (rule 19); measuring (a row's size
  counts up); estimated size; read-only (a configuration written by a newer Pitcrew: banner, Add disabled with the
  reason); secret store unavailable; a validation error in the dialog; Windows not applied; container note; Check
  connection disabled in Offline mode; restart pending.
- **Layout:** paths are not monospace (only variable names are); they truncate in the middle, keeping the last segments,
  so `{runner}` stays visible, with the full path in a tooltip (rule 18). Under container queries at the 900 px window
  width the Variable, Applies to and Free columns fold into a second line of the row (not hidden); nothing scrolls
  sideways at 320 px and up.

Wireframe (1280 px window, sidebar 216; the mock-up has every frame at 900 and 1280):

```
┌ Environment ─────────────────────────────────────────────── Measure sizes  [+ Add ▾] ┐
│ Shared folders, variables and PATH that every runner on this Mac starts with.         │
│ ⓘ 2 runners restart after their current job to start with your changes: mac-a, mac-b. │
│ Shared folders                                         30.5 GB · measured 12 min ago   │
│ Name               Variable          Folder                    Applies to Size  Free   │
│ sccache            SCCACHE_DIR       /Volumes/build/…/{runner} All (3)   18.2 GB 412 GB│
│ Cargo home         CARGO_HOME        /Volumes/build/cargo      All (3)    6.3 GB  ⟳ pending
│ Cargo target root  CARGO_TARGET_DIR  /Volumes/scratch/…/{runner} All (3)   —  ⚠ Disk not mounted
│ Variables                                                                             │
│ SCCACHE_SERVER_PORT  4260 + runner number (4261-4263)   All (3)                       │
│ NPM_TOKEN            Hidden, stored by Pitcrew          All (3)                       │
│ PATH additions  ⠿ {home}/.cargo/bin  Before  ·  ⠿ /opt/homebrew/opt/llvm/bin After ⚠  │
│ Actions cache server [http://cache.lan:8089] [Check connection] [Apply to runners…]   │
│   ✓ Port open   ⚠ Differs on mac-c                                                    │
└───────────────────────────────────────────────────────────────────────────────────────┘
```

### 10. Config schema

```toml
schema_version = 2              # bumped: an older Pitcrew must not save over [environment] and lose it

[defaults.storage_io]           # optional; rule 51
walkers = "auto"                # "auto" or 1..=16 threads per volume
removers = "auto"               # "auto" or 1..=8

[[environment.vars]]            # kind = "folder" | "value" | "port" | "secret"
name = "SCCACHE_DIR"
kind = "folder"
label = "sccache"               # optional; shown instead of the preset title
value = "/Volumes/build/sccache/{runner}"
create = true                   # folder only; default false
sharing = "per-runner"          # folder only; "per-runner" (default) | "shared"
volume_id = "6F1C2D3E-…"        # folder only; recorded by the service when the folder is added
preset = "sccache.local"        # optional; grouping, sharing default and clean rule
clean_subfolders = []           # folder without a preset: what Clean may empty; empty list = no Clean
runners = ["mac-a", "mac-b"]    # optional; absent = every runner, present and future

[[environment.vars]]
name = "SCCACHE_SERVER_PORT"
kind = "port"
base = 4260                     # value = base + the runner's env_number (4261-4276)

[[environment.vars]]
name = "NPM_TOKEN"
kind = "secret"                 # no value: the service's secret store holds it

[[environment.path]]
dir = "{home}/.cargo/bin"
position = "before"             # "before" | "after" the login shell's PATH; default "after"

[environment.actions_cache]
server = "http://cache.lan:8089"   # optional; projected into each runner's config.yaml (section 6)

[[runners]]
name = "mac-a"
kind = "gitea-runner"
dir = "/Users/ci/runners/mac-a"
env_number = 1                  # new, assigned by Pitcrew, stable while the runner exists
env_secrets = ["NPM_TOKEN"]     # new, optional: names whose values the service stores; a key of [[runners]],
                                # so it comes before the [runners.env] sub-table
[runners.env]                   # unchanged: plain per-runner overrides
SCCACHE_CACHE_SIZE = "10G"
```

- Every new key is optional with a default; the section is merged over an empty default; one bad entry is skipped with a
  problem, the rest loads.
- `SCHEMA_VERSION` 1 → 2 with an identity migration (plus assigning `env_number`) and its test. The bump exists for
  downgrade safety: Pitcrew never overwrites a file written by a newer Pitcrew, so a downgrade keeps `[environment]`.
- A round-trip test writes the example above and reads it back equal, with `env_secrets` before `[runners.env]` in the
  output (TOML requires a table's keys before its sub-tables).
- `state.json` gains optional `env_started`, `shared_sizes`, `secret_revisions` and `maintenance`; all are caches or
  self-healing, so `STATE_VERSION` stays 1 and a missing or damaged value means "unknown" (and, for `maintenance`, "no
  lock", with a history event).

### 11. Wire contract

App ↔ service, `pitcrew_core::protocol`, the next version after the shell's (6 if `pitcrew/shell-1.1` lands with 5).
Queries have no side effects; `needs_owner` is section 3.

| Request | Kind | Reply |
|---|---|---|
| `GetEnvironment` | query | `Environment { host: HostEnv, presets, runners: Vec<RunnerEnvSummary>, findings, revision, read_only: Option<String> }` |
| `PreviewEnvironment { edit: EnvEdit }` | query | `EnvPreview { findings: Vec<EnvFinding>, facts: Vec<FolderFacts>, plans: Vec<(RunnerName, RestartPlan)> }` |
| `SaveEnvironment { edit, revision }` | owner | `EnvSaved { plans }`; refused with "reload and make your change again" when `revision` is stale |
| `EffectiveEnvironment { runner }` | query | `EffectiveEnv { vars: Vec<EffectiveVar>, notes, pending: bool }` |
| `SetEnvSecret { scope, name, value: Secret }` / `ClearEnvSecret { scope, name }` | owner | `Ok` (`present` only) |
| `MeasureSharedFolders { entries, from_scratch }` | owner (job) | `JobsStarted { ids }` |
| `CleanPreview { entry }` | query | `CleanPreview { token, items, bytes, runners }` |
| `CleanSharedFolder { entry, token }` | owner (job) | `JobStarted { id }` |
| `CheckEndpoint { target }` | owner | `Reachability` |

Types (core, serde camelCase): `EnvKind` (`folder | value | port | secret`), `EnvValue` (`Plain(String) | Port { base } |
Secret { present }`), `RunnerScope` (`All | Only(Vec<RunnerName>)`), `Sharing` (`shared | perRunner`), `CleanRule`
(`empty | subfolders(Vec) | none`), `PathPosition`, `Source` (`service | loginShell | host { entry } | runner | configYaml |
envFile`), `Shown` (`Plain(String) | Masked`), `VolumeId`, `EnvFinding { entry, runner?, severity, code, message, fix? }`.
`VarName` is a parsed newtype. Events: `ServiceEvent::Job` for measure and clean, `ServiceEvent::EnvironmentChanged
{ revision }`. A schema test asserts that no reply type can carry a secret value.

App ↔ UI: Tauri commands `environment_get`, `environment_preview`, `environment_save`, `environment_effective`,
`environment_secret_set`, `environment_secret_clear`, `environment_measure`, `environment_clean_preview`,
`environment_clean`, `environment_check`; `api.types.ts` mirrored by `dto.rs` in the same commit, the mock backend, and
each dialog's command list in `dialog-contracts.json`.

## Consequences

- One place for build caches and tools on a host; each runner shows exactly what it starts with and why.
- Environment changes on Windows need an administrator when they reach LocalSystem runners; a non-admin owner can still
  manage interactive runners.
- The service now stores and reads secrets itself (amends ADR 0005), in an owner-only file on macOS and Linux until
  signed builds allow a service-created Keychain item.
- More `pitcrew.toml` (schema 2) and four optional keys in `state.json`.
- Container jobs are not covered (decision 1); runners with container labels say so.

## Risks

| Risk | Mitigation |
|---|---|
| An interactive Windows user runs code as SYSTEM through the environment | `needs_owner` from the caller's token; admin for LocalSystem runners; Measure/Clean impersonate the caller (section 3) |
| Secrets in a process environment are readable by every job and the same user's processes | Stated in the UI and docs; the Secret kind protects files, logs and screens only |
| A file-based secret store on macOS/Linux | 0600 in a 0700 folder, the same boundary as the runner's environment; Keychain once builds are signed |
| A shared folder on an unmounted disk is recreated on the boot disk | Expected `volume_id`; nearest-ancestor check; `SharedFolderUnavailable`; free space keyed by volume |
| Two runners share a single-server cache or a port | `per-runner` presets need `{runner}`/`{n}`; the Port kind; `*_PORT` on several runners is an error |
| Clean deletes something it should not | Clean rules per preset (never `CARGO_HOME`/`GRADLE_USER_HOME` wholesale), no Clean without named subfolders, `paddock-storage` boundaries, preview token, per-item re-check, maintenance lock |
| A runner resumed by hand during a clean | The lock refuses Resume/Start/Restart; the per-item re-check stops the clean |
| A widened reserved list breaks an existing config | Existing entries keep applying with a warning; only new edits are refused |
| Downgrade loses `[environment]` | Schema 2 |
| Background priority not built yet | One thread per volume until `apply_current` exists (section 7a) |
| Windows verified on CI only | The pure merge is tested everywhere, the FFI on `windows-latest`; unapplied runners are named in the UI until WP-8 |
| ADR 0025 and 0021 change underneath | The CI home is a preset with `none` clean; `RunnerScope` gains `Pool(name)` when ADR 0021 lands |

## Work packages

Starts after `pitcrew/shell-1.1` is merged. Estimates are agent days (about 27 in total). The shared integration points
(`protocol.rs`, `lib.rs` files, `api.types.ts`, `dto.rs`, `pitcrew.contributions.ts`, `dialog-contracts.json`) belong to
the integrating agent; other packages add declared lines only. UI files follow the flat `*-panel.ts` convention of
`features/runners/`.

| WP | What | Files owned | Role | Est. |
|---|---|---|---|---|
| WP-1 | Core model: kinds, placeholders, runner numbers, sharing and clean rules, validation over facts, reserved list (Windows case-insensitive), keep-with-warning for old entries, `compose` with sources, fingerprints with secret revisions, schema 2 and migration, `[defaults.storage_io]` | `pitcrew-core/src/environment/{mod,model,validate,compose,placeholders,fingerprint}.rs`, `runner_env.rs`, `config.rs` | implementer | 3 d |
| WP-2 | Protocol and wire types, right after WP-1: requests, `needs_owner`, replies, events; `dto.rs`, `api.types.ts`, the mock backend, the schema test that no reply carries a secret | `pitcrew-core/src/protocol.rs` (environment part), `app/src-tauri/src/dto.rs`, `app/src/app/core/api.types.ts`, `app/src/app/core/mock/environment.mock.ts` | integrating agent | 1.5 d |
| WP-3 | Presets pack and reader (`paddock-packs`), each fact with source and date (sccache, Cargo, npm, Playwright, Gradle, act) | `pitcrew-core/packs/env-presets.toml`, `pitcrew-core/src/environment/presets.rs` | researcher + implementer | 1 d |
| WP-4 | Service core: facts probe with `volume_id` and `volume_kind`, the pure `auth::decide` over the `CallerIdentity` port and the macOS/Linux peer-uid adapter, get/preview/save, restart plan, launch through `compose`, the start-time port check, service-owned secret store (backup exclusion, kept out of the diagnostics bundle), holds `SharedFolderUnavailable` and `SecretUnavailable`, guard over all volumes, `SpaceCache` by volume id, **all four** `state.rs` keys (`env_started`, `secret_revisions`, `shared_sizes`, `maintenance`), **all** history and notification texts of this ADR (holds, lock, port in use, owner-saved entries), diagnostics redaction | `pitcrew-core/src/environment/auth.rs`, `pitcrew-service/src/environment/{facts,peer,ports,requests,secrets}.rs`, `supervisor.rs` (launch, holds), `probe.rs`, `pitcrew-core/src/{hold,disk_guard,state,history,notify,redact}.rs` | implementer | 5 d |
| WP-5 | Measure and clean jobs: per-volume FIFO and thread budget from Settings, background class, size index, clean rules, maintenance lock with refusals, per-item re-check and the block on automatic resume, runner update and self-update restart, sccache stop, one runner at a time, ADR 0016's trim on the same lock; rates in `performance.md` (it uses WP-4's state keys and texts) | `pitcrew-core/src/environment/{size_index,clean}.rs`, `pitcrew-core/src/controller.rs` (lock input), `pitcrew-core/src/self_update.rs` and `pitcrew-core/src/update/` (lock = busy), `pitcrew-service/src/shared_folders.rs`, `pitcrew-service/src/hygiene.rs` (trim), `docs/performance.md` | implementer | 4 d |
| WP-6 | Shared crates: `paddock-net`'s `Egress::probe_connect` (reach, Offline, audit) and `paddock-jobs`' per-thread setup hook for `par_walk` (used for impersonation on Windows), each tested in its crate; Pitcrew's `CheckEndpoint` on the probe | `crates/paddock-net/src/probe.rs`, `crates/paddock-jobs/src/walk.rs`, `pitcrew-service/src/reach.rs` | implementer | 1.5 d |
| WP-7 | Actions cache projection: `cache.external_server` across runners through `runner_config`, the multi-runner config-diff dialog | `pitcrew-core/src/runner_config.rs` (multi-runner preview), `app/src/app/features/runners/config/config-diff-dialog.ts` | implementer | 2 d |
| WP-8 | Windows: `env_block::merge`, interactive runners get the composition, writability with the runner user's token, Credential Manager backend, the `CallerIdentity` adapter (client token, elevation, owner SID) and **all Windows authorisation tests**, the elevated install step that records the owner SID (console user under over-the-shoulder UAC), impersonated Measure/Clean through the `par_walk` hook | `pitcrew-core/src/env_block.rs`, `pitcrew-service/src/platform/{win_session,win_auth}.rs`, `pitcrew-service/src/platform/windows.rs` (install: owner SID), `app/src-tauri/src/service_install.rs` (shows the owner) | implementer (Windows CI) | 2.5 d |
| WP-9 | UI: page and panels, store, Add/Edit and Clean dialogs, runner tab, every state of section 9, `msg()` keys, and the **Settings UI for `[defaults.storage_io]`** (walkers and removers, Auto or a number, in the Defaults section; rule 44) | `app/src/app/features/environment/{environment-page,shared-folders-panel,variables-panel,path-panel,cache-server-panel,env-edit-dialog,clean-dialog}.ts` (+ `.html`, `.scss`, `.spec.ts`), `environment.store.ts`; `app/src/app/features/runners/{environment-panel,runner-environment.store}.ts`; `app/src/app/features/settings/defaults-section.ts` (storage I/O fields); declared lines in `runner-detail-page.ts` (tab list) and `runner-detail-page.html` (the Environment `@case`), `pitcrew.contributions.ts`, `dialog-contracts.json` | designer pass + implementer | 5.5 d |
| WP-10 | Docs and images: features, user guide, roadmap (Phase 3 item closed, the Windows correction), architecture, ADR 0016's text, CHANGELOG, screenshots with an independent review (rule 22) | `docs/{features,user-guide,roadmap,architecture}.md`, `docs/adr/0016-*.md`, `CHANGELOG.md`, `docs/screenshots/**` | implementer + reviewer | 1 d |

Order: WP-1 → WP-2 → (WP-3, WP-4, WP-6, WP-8 in parallel) → WP-5, WP-7 → WP-9 → WP-10.

## Tests (each written first and seen failing)

- **Core (WP-1, ≥ 99 % lines):** the precedence table row by row (PATH before/after, a runner `PATH` replaces); Windows
  names and reserved names match case-insensitively; placeholders (`{runner}`, `{n}`, `{home}`, `{{`, an unknown one is an
  error, `{home}` on a LocalSystem runner is an error); runner numbers are stable, lowest free, never reused while the
  runner exists; ports are `base + n`, overlapping blocks and a `*_PORT` Variable on two runners are errors; every
  reserved name; an existing `HOME` in `[runners.env]` keeps applying with a warning, a changed or added one is refused;
  folder rules (relative, `..`, root, home, system folder, LocalSystem's profile, inside and parent of a runner folder,
  a link into one); volume rules with fake facts: **an empty mount-point folder** whose nearest ancestor is on another
  volume is unavailable and is never created; a `per-runner` folder without `{runner}`/`{n}` on two runners is an error;
  a world-writable PATH entry is an error; limits at N and N+1; the 64 KiB block; schema 1 loads as 2 with numbers
  assigned; **the schema example round-trips** with `env_secrets` before `[runners.env]`; a schema 3 file is never written;
  the fingerprint changes on a secret's revision and never depends on its value; proptest: `compose` never panics,
  `to_launch()` equals the view's values, no secret value appears in `Debug` or a finding.
- **Authorisation (WP-4, WP-8):** on `windows-latest`, a non-admin, non-owner client gets `PermissionDenied` for every
  `needs_owner` request; the owner is refused for an entry that reaches a `service`-mode runner and admitted for an
  interactive-only one; an elevated admin is admitted; Clean under a non-admin token cannot remove a file only SYSTEM can
  write. On macOS and Linux a peer with another uid is refused (socketpair with a forged uid check in a test double).
- **Secrets (WP-4):** a value set through `SetEnvSecret` is read back by the service **with no UI**: the macOS CI job
  starts the service binary detached from any terminal (`stdin` closed, no session) against a temporary data folder, sets
  a secret, restarts the service, and a fake runner sees it; a missing secret holds the runner with the name; a log capture
  during save, start and a job echoing the value contains no secret value.
- **Service (WP-4, WP-5):** facts against temporary folders only (rule 7); save restarts only runners whose fingerprint
  changed, after the job (fake runner); a stale `revision` is refused; a volume that disappears holds and its return
  releases; low disk on a shared folder's volume holds; `SpaceCache` by volume id never reads the boot disk for an
  unmounted folder; measure counts up, stops at its budget as a lower bound, a second request joins the job; the size
  index re-lists only directories whose ctime or id changed (counting fake file system), a damaged index is rebuilt,
  "from scratch" ignores it, the 500,000-directory cap falls back to an unindexed walk; thread budgets follow the
  Settings and the volume kind; **the maintenance lock** refuses Resume, Start and Restart with "Cancel the clean
  first"; a runner that becomes busy despite the lock stops the clean before the next item; per-runner folders are
  cleaned one runner at a time while the others keep their jobs; sccache is stopped before its folder is emptied; a
  `CARGO_HOME` clean touches only its three subfolders; a folder without a preset has no Clean; Cancel in each phase
  releases the lock and reports what was left; a service restart during a clean releases the lock with a history event.
- **Ports (WP-1, WP-4):** with the CI home preset on any runner a block overlapping the pack's `kit_ports` is an error,
  without it not; the default base 4260 overlaps nothing; a conformance test compares the pack's `kit_ports` with
  `conformance/ci/ports.txt`; the start-time check reports a port held by a foreign process (fake socket table) and not one
  held by this runner's own sccache.
- **Reserved names (WP-1):** the four act-forced `RUNNER_*` names are refused, `RUNNER_DEBUG` passes with a warning.
- **Lock (WP-5):** while locked, hold auto-resume, a due runner update and a self-update restart all wait.
- **Windows (WP-8):** under over-the-shoulder UAC the recorded owner is the console user; an owner-saved "All runners"
  entry does not reach a runner switched to `service` mode, which shows the finding until an administrator saves; Clean
  of a folder that reaches a `service`-mode runner is refused for the owner.
- **paddock-net and paddock-jobs (WP-6):** the setup hook runs once per walker thread before its first item; the connect probe obeys reach, Offline mode and the allow-list, writes the audit entry and sends
  no bytes.
- **Contract (WP-2):** `dto.rs` and `api.types.ts` round-trip the same JSON fixtures.
- **UI (WP-9, stores near 100 %):** every state of section 9; findings shown from the engine, not re-implemented; keyboard
  reorder of PATH additions; Playwright at 900 and 1280 px, both themes, axe clean, no horizontal scroll at 320 px.
- **Windows (WP-8):** `env_block::merge` on synthetic blocks (empty, duplicates in other case, long PATH); on
  `windows-latest`, an interactive-mode fake runner sees a host variable.

## Decided by the user (2026-10-29)

1. **Host jobs only in v1.** The host layer sets the runner process's environment. Runners with container labels show a
   note on their Environment tab and on the page ("Jobs in containers do not see these; use config.yaml envs and
   volumes"). Pushing host entries into config.yaml for container runners is not part of this ADR.
2. **sccache uses local folders only for now.** The built-in pack has the local preset (per runner, with its own port);
   no remote backend preset ships in v1. Future work, decided later: a remote backend (WebDAV, Redis or S3/MinIO, whose
   endpoint and token would use the Variable and Secret kinds as they are), and sccache's GitHub Actions cache backend
   against act_runner's cache server, which is **unverified** (sccache's GHA backend speaks GitHub's cache service;
   whether act_runner's server answers it is not known) and needs a check before it is offered.
3. **The cache server check is an opt-in TCP connect only** (section 6): user-started, nothing sent, through
   `paddock-net`.
4. **Cleaning in v1 is "empty when idle"** (section 7), within each preset's clean rule. Trimming the oldest entries to a
   size is not in v1.

## Reviews: what changed

### Review 1 (2026-10-29, at `fdd830f61`)

P1: 1 authorisation from the caller's token, impersonated Measure/Clean, reserved list as correctness (section 3);
2 service-owned secrets on every OS, the Keychain signature problem, a no-UI CI test (section 5); 3 the Port kind and
runner numbers, CI home `shared`, npm per-runner after ADR 0025 (section 1); 4 `volume_id` and the nearest-ancestor
rule, `SpaceCache` by volume (sections 4, 5); 5 the maintenance lock plus the per-item re-check (section 7); 6 clean rules
per preset, no Clean without named subfolders, no `CARGO_HOME` example (sections 1, 7); 7 the exact schema and the
round-trip test (section 10); 8 old reserved entries keep applying with a warning (section 4); 9 thread budgets as
Settings with Auto, jobs vs threads (section 7a); 10 work packages re-cut and re-estimated (about 25 days). P2: ctime and
file id, index budgets, no timer, no I/O-busy claim, `Class::Background` and `apply_current` not built, `SpaceCache` by
volume, one runner at a time, sccache stopped first, one policy with ADR 0016, secret revisions instead of value hashes,
`paddock-net` probe, `label`/`sharing`/`volume_id` keys, both hold reasons and back-references, Windows case, `{home}`
for LocalSystem, act's `RUNNER_*` and `startHostEnvironment` cited.

### Review 2 (2026-10-29, at `876caae4f`)

P1: the sccache base moved from 4227 (the CI kit's first slot) to 4260 (ports 4261-4276), clear of `ports.txt` and the
end-to-end ports; a Port block overlapping the CI kit's declared ports is an error whenever the CI home preset is used on
the host; a start-time finding names a foreign process that holds a runner's port (section 1, section 4). P2: exactly
the four act-forced `RUNNER_*` names are reserved, others warn (Context, section 4, the mock-up's footer); the secret file
store is excluded from backups and the diagnostics bundle, with the NFS and synced-home caveat (section 5); the
maintenance lock blocks auto-resume, runner updates and self-update restarts (section 7); WP-9 owns the storage I/O
Settings UI (rule 44); WP-4 owns all four state keys and every text; `auth` is a pure decision over `CallerIdentity`,
WP-8 owns the Windows adapter, its tests and the owner-SID install step; WP-6 owns the `par_walk` setup hook;
`runner-detail-page.html` is assigned; the owner SID under over-the-shoulder UAC, owner-saved entries and runners later
switched to `service` mode, and Clean of folders reaching `service`-mode runners (section 3); the mock-up's second lines
use one short form and the skeleton uses the family token with `aria-busy` and a status line.
