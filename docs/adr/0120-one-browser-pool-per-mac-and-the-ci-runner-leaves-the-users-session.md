# 0120. One browser pool per Mac, and the CI runner leaves the user's login session

- Status: Accepted 2026-11-25 by the user: D1 = option D, D3 = a pool of 6, D5 = (a), amended, D7 = the sampler and log
  collector for 14 days, D8 = 4 lanes. Changed the same day: one daemon runner of capacity 4 and no GUI lane, GUI jobs off the
  Mac (D1 and D8; see "Decisions of 2026-11-25"). Decisions 1 and 2 of the brief were the user's on 2026-11-24 (D2 = (a));
  D4 = (a) landed as WP-0. Proposed 2026-11-24.
- Date: 2026-11-24
- Related: ER-12 (no limit without a measured cause), ER-14 (agents never reconfigure shared infrastructure), ER-22 (share
  first), ER-24 and ER-48 (load and concurrency budget), ER-27 (logs explain failures), ER-28 (intermittent causes), ER-33
  (positive controls), ER-37 (common practice and cost), ER-41 (new checks start report-only), ER-43 (a waiter holds no
  capacity), ER-51 (start, hold, end); rule 7 (tests never touch the user's machine), rule 80 (ER-43), rule 88 (removing an OS
  from a test needs the user's approval); ADR 0025 (job isolation and slots), ADR 0102 (the heavy slot).
- Number: 0120. On 2026-11-24 the highest ADR on any local or `origin` branch was 0119 (`docs/adr/0119-every-process-is-held-by-its-handle-and-ends-with-its-scope.md`,
  `git ls-tree docs/adr/` over every ref), and no worktree under the worktree root held an uncommitted 012x file. No earlier draft of
  this ADR existed (no branch `docs/adr-mac-browser-cap-headless-runner`, no matching worktree).

**Evidence heads** (read 2026-11-24, re-read before the commit):

| Ref | Sha |
|---|---|
| `origin/main` | `d04af7b46a8a4374b4b56d3d08b02193ad6755a4` |
| `origin/train/2026-11-23-graph` | `47a27bf1f57db563f420822b9504d27fad0dabf9` |

Everything cited as code was read on the train with `git show origin/train/2026-11-23-graph:<path>`. Where a mechanism is on
`origin/main` too, it says so. This ADR claims nothing about released code: no tag is cited.

## Context

### The incident (2026-11-24)

At 10:33:48 the Mac host's WindowServer (pid 422) ended abruptly. loginwindow then logged the user out; the machine did not
reboot. There was no crash report and no jetsam record, and kernel lines are not persisted. Of 434 app registrations since
10:25, 391 were `chrome-headless-shell` and 42 Playwright WebKit (about 48 a minute on average over those 9 minutes; the
coordinator read peaks of about 130 a minute). The load average was 100 to 150 on 14 cores. launchservicesd and pboard were
busy. The launches came from the Gitea runner, which runs as the user in the user's GUI session (`gui/501`), and from agents'
local test runs. Other workloads also ran browsers. They are out of scope here.

The host runs macOS 27.2.1 (`sw_vers`, read 2026-11-24).

### Causes, and what would rule each out (ER-28)

None is confirmed. The incident happened once, and a cause counts as confirmed only after a reproduction (ER-28).

| # | Plausible cause | Observation that rules it out | Can be checked now? |
|---|---|---|---|
| H1 | **Launch storm.** Many short-lived browser processes, each registering with LaunchServices and the window server, overloaded WindowServer, launchservicesd and pboard at a load of 100 to 150. This was the coordinator's first reading (medium-low confidence). | The sampler (WP-3) shows WindowServer's memory and port count flat at the same launch rate, or the abort repeats with no launches. | No. Needs the sampler. |
| H2 | **WebKit trace screenshots while the display is asleep.** This is Playwright issue [#43066](https://github.com/microsoft/playwright/issues/43066) (closed 2026-10-08, read 2026-11-24). On macOS 26.6 to 27.0.1, WebKit's screencast calls `CGWindowListCreateImage` about 25 times a second. While the display is asleep, WindowServer keeps every captured surface. The reporter's WindowServer reached its surface limit after about 700 WebKit tests in 4 h 18 min, and the abort ended the login session "(every app and terminal with it)". The maintainers put the root cause in WindowServer (Apple FB25101947); the workaround is `caffeinate -d` or `screenshots: false` for WebKit traces. Every Paddock config uses `trace: 'retain-on-failure'`, which records screenshots for every test (`ui/projects/paddock-e2e/src/config.ts:93`, and the five apps' `playwright.config.ts`). | `pmset -g log` shows the display awake from 10:25 to 10:33, or no WebKit run was in progress in the hours before 10:33. | **Yes:** the user can read `pmset -g log` today (step U0 below). |
| H3 | Memory pressure | A jetsam or memory-pressure record before 10:33. None was found, so H3 is weak. | Already checked by the coordinator. |
| H4 | A GPU, driver or kernel fault | A panic or reboot. The machine did not reboot, so H4 is unlikely. | Already checked. |

H1 and H2 do not exclude each other. The decisions below hold under both. The browser cap bounds H1. WP-0 removes H2's
trigger at a cost of a quarter of a day. Which cause it was changes only how much the runner move matters (D1).

### What Apple and the CI vendors say

All sources were read on 2026-11-24.

- **Apple: one WindowServer for every GUI session.**
  - TN2083 "Daemons and Agents" (2007, still current) says: "There is a single instance of the WindowServer process that
    resides in the global security context. However, it knows about all GUI login sessions." Also: "when the window server
    quits, any process that's connected to it will terminate." And: "A daemon cannot display any GUI; more specifically, it is
    not allowed to connect to the window server."
    [developer.apple.com/library/archive/technotes/tn2083](https://developer.apple.com/library/archive/technotes/tn2083/_index.html)
  - Quinn (Apple DTS), September 2024, for macOS 15: "There is but one instance of the window server; it manages all the GUI
    login sessions." [forums thread 763453](https://developer.apple.com/forums/thread/763453)
  - **So option C does not isolate the user's session from a WindowServer abort.** A crash caused from the CI user's session
    ends every GUI session, the user's included. This is an inference from the single shared process plus TN2083's
    "processes connected to it terminate", held with medium-high confidence. No source covers macOS 26 or 27.
- **Apple on code signing from daemons.** Quinn, February 2025: `UserName` in a daemon plist gives "a mixed execution
  context". He advises signing code "as a logged in user". [thread 774832](https://developer.apple.com/forums/thread/774832)
  Paddock signs ad hoc (`-`, no keychain identity), which this advice does not directly cover.
- **GitHub actions/runner.** `svc.sh install` writes a LaunchAgent to `~/Library/LaunchAgents` and refuses to run as root
  ("launchctl should not run as sudo for launch runners"). The plist has `SessionCreate`, `ProcessType Interactive` and
  `RunAtLoad`.
  [darwin.svc.sh.template](https://raw.githubusercontent.com/actions/runner/main/src/Misc/layoutbin/darwin.svc.sh.template),
  [actions.runner.plist.template](https://raw.githubusercontent.com/actions/runner/main/src/Misc/layoutbin/actions.runner.plist.template).
  GitHub's docs do not say why
  ([configuring the runner as a service](https://docs.github.com/en/actions/hosting-your-own-runners/managing-self-hosted-runners/configuring-the-self-hosted-runner-application-as-a-service)).
- **GitLab Runner.** "On macOS, GitLab Runner runs as a user-mode `LaunchAgent`, not as a system-level `LaunchDaemon`." "This
  is the only supported mode." "The `LaunchAgent` requires a graphical login session to bootstrap." "To keep the runner
  available after a reboot, turn on automatic login."
  [docs.gitlab.com/runner/install/osx](https://docs.gitlab.com/runner/install/osx/)
- **Buildkite.** The agent's plist goes in `~/Library/LaunchAgents` (template `launchd_local_with_gui.plist`). Troubleshooting
  says: "Ensure that you have a user logged in to the macOS host."
  [buildkite.com/docs/agent/v3/macos](https://buildkite.com/docs/agent/v3/macos)
- **Gitea act_runner.** The 1.23 docs have a section "Start the runner with LaunchDaemon (macOS)": a plist with `UserName`
  `_act_runner`, loaded with `sudo launchctl load`.
  [docs.gitea.com/1.23/usage/actions/act-runner](https://docs.gitea.com/1.23/usage/actions/act-runner) (read from the search
  summary only).
- **Cirrus Orchard (Tart).** The worker is installed as a LaunchDaemon
  ([tart.run/orchard/deploying-workers](https://tart.run/orchard/deploying-workers/)). `tart run --no-graphics` crashed in
  AppKit start-up under a LaunchDaemon (orchard issue #308, macOS 15.4). `SessionCreate=true` worked around it, and Tart commit
  ad539a6 ("Use Foundation run loop in headless mode", 2025-05-12) fixed it. Which Tart release first contains that commit was
  not checked.
- **Auto-login.** Apple: "When FileVault is turned on, automatic login is disabled", and "Automatic login allows anyone to
  access your Mac simply by restarting it."
  [support.apple.com mchlp1158](https://support.apple.com/en-tm/guide/mac-help/mchlp1158/15.0/mac/15.0). The password is kept
  in `/etc/kcpassword`, XORed with a fixed public key, so it can be recovered.

**Common practice:** a dedicated user with its own GUI session runs the runner as a LaunchAgent, kept alive across reboots
by auto-login (GitLab, Buildkite, GitHub's `svc.sh`, and Apple's signing advice). Machines are usually dedicated to CI or
are ephemeral VMs (GitHub-hosted runners, Cirrus/Tart). A LaunchDaemon is the practice only for work that needs no GUI
(Orchard's VM host, Gitea's example). None of these vendors caps browsers across a whole machine: they run one job per machine
or VM and size each job's workers.

### The Mac's runners today

These were read from the plists and config files on 2026-11-24, with no `launchctl` capture (ER-14).

- Three LaunchAgent plists in `~/Library/LaunchAgents`: `net.decryptology.gitea-runner.d7-mac-mini` (config `capacity: 3`),
  `-d7-mac-mini-2` and `-d7-mac-mini-3` (`capacity: 1` each).
- All three set `PADDOCK_CI_JOBS_PER_HOST=3`, `PADDOCK_CI_RESERVED_CORES=2`, `HOME=/Users/decryp7`, and a PATH through the
  user's nvm, cargo and Homebrew.
- Work folders are under `/Volumes/vault-dev/gitea-runners/<name>/work` (an external volume).
- Which plists are loaded was not checked. If all three are, the host can run 5 jobs while the build-jobs layout divides its
  cores by 3. This is a separate finding (D8).

### What exists to build the cap on (ER-22)

| Mechanism | What it is | Fits the cap? |
|---|---|---|
| `paddock-slots` (`crates/paddock-slots`, on main) | N numbered slots, each an exclusive OS file lock (`flock`, `LockFileEx`) on `<root>/slots/sN.lock`; holder tokens (`fresh_token`, `release` honours only its own token); the kernel drops the lock when the holder dies, "so there is no stale lock to reclaim"; `hold_with` waits its own `grace` for a free slot by repeated try-locks, holding nothing meanwhile | **Yes.** A counted semaphore with tokens that frees itself on crash. |
| `./dev ci slot acquire/hold/release/status` (`tools/paddock-dev/src/slots.rs`, on main) | The CLI over it. A detached holder keeps the slot until the job folder (`--owner`) disappears, a release asks, or `DEFAULT_MAX_HOLD_SECS` (3 h 30 min) passes. | It is the right CLI, but its end condition does not fit: a crashed Playwright worker leaves its folder behind, so its slot would stay held for up to 3.5 h. It needs an end tied to the caller's own handle (WP-1). |
| `heavy` (`tools/heavy`, on main) | A FIFO queue of heavy steps (cargo, npm, ng, playwright) with a load and memory gate. Its lock is a directory plus an owner record (pid and start time), with breaking rules. | No. It admits whole commands, not browsers. Agents' Playwright runs already go through it; the cap sits inside such a run. |
| `plock` | Retired. `heavy` keeps its file layout; `./dev wait --plock` reads its log. | No. |
| `e2eWorkers` (`ui/projects/paddock-e2e/src/env.ts`, on the train only, commit a080f848a) | Playwright workers = min(the config's count, `PADDOCK_E2E_WORKERS`), the job's core share (ER-48). | Yes: the worker count derives from it (WP-2). |
| The kit's `test` (`ui/projects/paddock-e2e/src/fixtures.ts`, on main) | Already has a worker-scoped auto fixture (the rule 7 browser guard) | It is the one place every kit spec passes through, so the slot goes there. 73 spec and e2e files import the kit; 30 import `@playwright/test` directly. |

## Inventory: macOS CI work that needs a GUI or login-session service

Sources: `.gitea/workflows/*.yml` and `.gitea/actions/` on the train, plus the scripts they call. "B" is a LaunchDaemon under
its own user with no GUI session. "C" is a LaunchAgent in that user's own GUI session.

| # | Work (where) | What it needs | Under B | Under C |
|---|---|---|---|---|
| 1 | Tauri DMG: `bundle_dmg.sh`'s Finder AppleScript (`app-release.yml`, `heavy-release.yml`, the demo release workflows, through `conformance/ci/tauri-bundle.sh`) | `tauri-bundle.sh:51` sets `CI="${CI:-true}"` (on main too). tauri-bundler then passes `--skip-jenkins` only when `CI` is exactly `true` (`dmg/mod.rs`, 2.10.6, [docs.rs](https://docs.rs/crate/tauri-bundler/latest/source/src/bundle/macos/dmg/mod.rs)). create-dmg: "skip Finder-prettifying AppleScript, useful in Sandbox and non-GUI environments". So no Finder is involved; `hdiutil` is. | Finder: not used. `hdiutil create/attach` from a daemon: **not verified.** The only reports of failure are old (2004 to 2009). Trial T2. | Works, as today. |
| 2 | Code signing (`app-release.yml`, `macos-verify`, `tauri-bundle.sh`) | Ad-hoc signing (`-`, rule 23) and `codesign --verify --deep --strict`. No keychain identity. | Expected to work: ad-hoc signing reads no keychain. Apple advises against signing from daemons, but that advice is about keychain identities. Trial T2. | Works. |
| 3 | Keychain | None. The `keychain` action is conformance check C99 ("no shipped process uses the macOS Keychain"), not keychain use. The updater key comes through an environment variable (ADR 0085). | Not needed. | Not needed. |
| 4 | TCC: Removable Volumes and Full Disk Access (the work folders on `/Volumes/vault-dev`; `conformance/ci/job-tmpdir.sh` and `boot-volume.sh` put each job's `TMPDIR` on the internal disk) | TCC denies a process that is its own responsible process (disclaimed children, `launchctl submit` jobs, a browser that lives on the volume) unless a prompt is answered. The runner binaries have Full Disk Access in the user's setup. Lessons of 2026-11-04 and 2026-11-06. | **No prompt can be shown, so such a process is denied silently.** The 2026-11-09 research recorded "LaunchDaemon gets silent deny"; this was not re-verified. The test browsers must move to the internal disk (the open branch `fix/test-browser-internal-disk`), and `TMPDIR` stays internal. Full Disk Access must be granted to the new runner path. Whether a grant applies to every user was not verified (TCC's storage is "an implementation detail", [thread 740055](https://developer.apple.com/forums/thread/740055)). | Prompts appear in the CI session. The user answers them once, by switching to that session (steps C5 and C7). Same internal-disk rule. |
| 5 | `launchctl submit` (Dyno's `apps/dyno/scripts/check-bundle.sh`, from `dyno-release.yml`, `nightly.yml` and `tag-rehearsal.yml`) | A launchd domain for the user. The script reads `gui/<uid>`, then `user/<uid>`. | A daemon process running under a `UserName`: **whether `submit` lands in `user/<uid>` is not verified.** Trial T2. | `gui/<uid>` of the CI user. Works. |
| 6 | Debrief's real-app macOS smoke (`apps/debrief/scripts/ci/macos-smoke.sh`, `debrief.yml` installers, full runs only) | Starts the built `Debrief.app` (a WKWebView window) and quits it with an Apple event. This needs WindowServer and Automation consent. | **Fails.** "A daemon ... is not allowed to connect to the window server" (TN2083). | Works. Automation consent is given once in the CI session. |
| 7 | Nightly `dialog-window-probe`, macOS leg (`nightly.yml`, manual or `PADDOCK_REAL_WINDOW_PROBE`) | Real windows | **Fails.** | Works (the shared displays may sleep, as the job's comment says). |
| 8 | `ci.yml` `e2e-webkit` (reference apps and the kit's self-test in Playwright WebKit on macOS) | `Playwright.app` registers with WindowServer. #43066 shows `_RegisterApplication` aborting when there is no session. | **Fails.** Moving it to Linux WebKit only drops macOS from a test: rule 88, the user's approval. | Works. H2 applies in any session (one WindowServer), so WP-0 is needed. |
| 9 | Playwright Chromium (`chrome-headless-shell`): `dyno-screenshots.yml` on macOS, agents' local runs. The apps' e2e legs run on Linux (`plan_ci.rs`, derived legs). | Headless. Its LaunchServices registration was seen in the incident. | **Not verified**; no source either way. Trial T2. | Works. |
| 10 | Rust real-browser tests: `crates/paddock-chromium/tests/it/real_browser.rs` (10 tests, `--headless=new`) and Debrief's `chromium-driver/tests/it/real_browser.rs` (2 tests, **windowed**: `launch.rs` has "no headless mode"). Both use `CHROMIUM_TEST_PATH`, Chrome for Testing. | The windowed Debrief tests open real windows when the macOS leg selects the package. | Windowed: **fails**. `--headless=new`: not verified (T2). | Works. The windows open on the CI session, not the user's desktop: a rule 7 gain. |
| 11 | Tart VMs (`service-smoke.yml`, `tools/macos-vm-smoke.sh`) | Virtualization.framework. `TART_HOME` is per user. | Works with a Tart release that contains ad539a6, or with `SessionCreate=true` (orchard #308). Trial T2. VM images are downloaded again for the new user, unless `TART_HOME` is shared. | Works. |
| 12 | `macos-verify` sidecar `--self-test`, Dyno's `dyno-agent once` from the built app | Command-line programs. Dyno's agent has code that reads `CGWindowList` (`tray_platform.rs`, `power_actions.rs`). | Sidecars: expected to work. `dyno-agent once`: **not verified** (it must degrade without a window server). Trial T2. | Works. |
| 13 | Pitcrew's disclaim and supervision tests (`responsibility_spawnattrs_setdisclaim`) | No GUI. A `TMPDIR` on the internal disk. | Works (row 4's rule). | Works. |
| 14 | cargo, npm, nextest, conformance, `./dev` | Nothing from the session | Works. The CI user needs its own rustup, nvm and Node (`PATH` reads the user's home today). | Same. |

**Summary.**
- Under B, rows 6, 7, 8 and 10 (windowed) fail.
- Rows 1, 2, 5, 9, 11 and 12 are not verified under B and need trial T2.
- Row 4 turns prompts into silent denials under B.
- Under C everything works as it does today, but the WindowServer is still the user's (H1 and H2 still reach it).

## Options for the runner (decision 2 of the brief)

| | A: today's setup plus the cap | B: a LaunchDaemon under its own user, no GUI | C: its own user and GUI session, LaunchAgent, kept logged in | **D: B for most jobs, plus one C lane (`macos-gui`, capacity 1) for rows 6 to 8 and 10** |
|---|---|---|---|---|
| Keeps CI away from the user's WindowServer | No | **Yes** for everything that runs. The GUI rows do not run at all. | **No.** One WindowServer for every session (TN2083, Quinn 2024). C only moves the windows and launches out of the user's session. | Yes for every job except the `macos-gui` lane: one job at a time, capped, with WP-0 |
| Keeps CI away from the user's LaunchServices, pboard and desktop | No | Yes | Yes (these are per-session services) | Yes |
| Coverage lost | None | Rows 6, 7, 8 and 10, unless they move to Tart guests (about 2 to 3 more days) or are dropped (rule 88) | None | None |
| Survives a reboot | Yes, after the user logs in | **Yes**: daemons start at boot, before any login | Only with auto-login (FileVault must be off; `kcpassword` can be recovered), or after the user logs in to the CI account by hand | The daemon lane: yes. The GUI lane: after a manual login (FileVault stays on); its jobs wait, bounded, with an error naming the step. |
| Memory | Nothing extra | About 0.1 GB (the daemon) | A second GUI session: Dock, Finder and others, **not measured** (T1 measures it on the 32 GB host) | As C, for one session |
| TCC and Full Disk Access | As today | Grants for the new runner path; prompts denied silently | Grants and prompts in the CI session, answered once | Both |
| Security | CI runs as the user, with the user's files and keychain | **Best**: a non-admin user with no login session, no access to the user's home | Good with a manual login. Weaker with auto-login (anyone who restarts the Mac gets the session; the password is in `kcpassword`). | As B for most work, as C (manual login) for the lane |
| Common practice | Small shops | Gitea's docs, Orchard's VM host: only for work with no GUI | **GitHub, GitLab (the only supported mode), Buildkite, Apple DTS** | Not a vendor default. It is GitLab's mode for the GUI lane plus Gitea's daemon for the rest. |
| User's time (steps below) | 0 | About 1.5 h | About 1.5 h | About 2 h |
| Agent-days (runner work only) | 0 | 0.5, plus 2 to 3 if rows 6 to 8 move into Tart guests | 0.5 | 0.75 |

**Recommendation: D.** It gives what the user chose B for, CI off the user's desktop and out of its WindowServer for nearly
every job. It keeps the macOS-only checks without dropping an OS (rule 88) and without a Tart rebuild of the GUI tests.

C is the cheaper common-practice option. It is fine if the user accepts that a WindowServer abort still ends both sessions.
After WP-0 and the cap, the remaining risk is H1 at a bounded rate.

B alone is the right call only if the user agrees to drop or move rows 6 to 8 and 10 (D6).

## Decision (proposed)

1. **One browser pool per host, shared by every CI job and every agent test run, on every user of the host.**
   - **Mechanism.** It is a `paddock-slots` pool (ER-22). Its root is `/Users/Shared/paddock/slots/browsers` on macOS, a
     group-shared folder on the internal disk, owned by group `paddock-ci` with mode `2770`. Lock files are `0660`.
   - **Sharing across users.** The CI user and the user are both in the group, so a lock taken by one user's process is seen by
     the other's.
   - **Holders.** A holder never deletes or replaces a lock file (`paddock-slots` already refuses a replaced one). A world-writable
     folder is refused.
   - **Size.** `PADDOCK_BROWSER_SLOTS` (default below) sets the pool's size, and one function decides it
     (`paddock_slots::pool::browsers()`): it reads the variable, else the OS layout. Linux and Windows have no pool until a
     layout names one; there, taking a slot is a no-op and the log says so.
2. **A browser is held by the process that uses it (ER-51).**
   - **The command.** `./dev ci slot hold --pool browsers --stdin --wait <bound>` takes one slot, prints `sN <token>`, and keeps
     it while its standard input is open. When the caller ends, however it ends, the pipe closes. The holder then ends and the
     kernel drops the lock.
   - **No folder watching.** There is no owner folder and no 3.5 h cap, so a crashed Playwright worker frees its slot at once.
   - **The wait holds nothing (ER-43).** While it waits, the holder only tries locks (`hold_with`'s repeated try-lock) and holds
     no slot.
   - **When the wait ends.** It ends at `--wait` with `::error::`, the holders' list (`status`) and the next step. The holder
     exits with code 79, as `heavy` does for "no slot in time".
   - **No new limit (ER-12).** `--wait` is the caller's own remaining bound: on CI the step's `timeout-minutes`, which the action
     passes as `PADDOCK_STEP_DEADLINE`; locally `heavy`'s `--run-max`.
3. **Playwright takes a slot per browser.**
   - **Where.** The kit's `test` overrides the worker-scoped `browser` fixture: it takes a slot through decision 2, then launches.
     A worker that never starts a browser takes none.
   - **Fixture timeout.** The fixture's own `timeout` is the same bound, so a worker waiting for a slot is not killed by a test
     timeout. Playwright's fixtures take a separate `timeout` for slow worker fixtures.
   - **Worker count (ER-48).** `e2eWorkers` takes the smaller of the config's count, `PADDOCK_E2E_WORKERS` and the pool's size.
     Workers beyond the pool would only wait.
   - **Specs that bypass the kit.** The 30 files that import `@playwright/test` directly, and the four scripts that call
     `chromium.launch()` (`icons/tools/shoot.mjs`, `docs/design/*/render.mjs`, `apps/pitcrew/docs/design/render.mjs`), are
     listed by a report-only check (ER-41; its id comes from `./dev check new-id`).
4. **Rust real-browser tests take a slot.** `paddock_testkit::browser_slot()` takes one in-process through `FsLocks` and
   holds it for the test (nextest runs one test per process). The Chromium driver's `launch` does not change.
5. **The pool's default size is 6 on macOS, provisional.**
   - **Why 6 (ER-12).** It is one browser per two of the 12 cores the layout leaves to work (14 minus 2 kept), which is the same
     ratio `heavy` uses per step. The cause for having a cap at all is this incident.
   - **Measurement plan (T3).** Run the reference apps' Chromium and WebKit suites as synthetic load at pool sizes 2, 4, 6, 8
     and 12, three runs each, with the sampler on. For each run record:
     - WindowServer RSS and port count;
     - launchservicesd and pboard CPU;
     - registrations per minute;
     - the 15-minute load;
     - wall time.
   - **Choosing the value.** The default becomes the largest size at which WindowServer's RSS and ports return to the
     baseline after each run and wall time still improves by 10% or more over the next smaller size. The results go into
     `docs/benchmarks.md`.
   - **Measurement is report-only.** It changes the default only through a reviewed change citing that block.
6. **WebKit traces carry no screenshots on macOS, and macOS GUI jobs keep the display awake (WP-0, H2).** A shared
   `paddockTrace(browserName, platform)` in the kit returns `{ mode: 'retain-on-failure', screenshots: false }` for WebKit on
   macOS. The five apps' configs use it instead of their own `trace:` lines. CI's macOS WebKit step runs under `caffeinate -d -i`.
7. **The runner moves (D1). The user does it (ER-14).** Steps are below. Workflows give rows 6 to 8 and 10 `runs-on: macos-gui`
   (under D). Everything else keeps `macos-latest`.
8. **Logs for a repeat (ER-27).** These are a sampler and a log collector that the user installs as root LaunchDaemons (WP-3,
   steps U1 to U3). Their output stays on the machine. Agents read it only with the user's OK (ER-14).
9. **Rule (draft, to be numbered):** *Every browser a test or CI step starts on a host takes a slot of that host's browser pool
   first, through the kit or the testkit, and holds it by its own handle.*

### Positive controls (ER-33)

- With a pool of 1:
  - a second holder waits and takes the slot within a poll of the first one's stdin closing;
  - a `SIGKILL` of the first holder frees the slot;
  - a holder whose caller is killed frees the slot.
- A spec run with a pool of 1 and 3 workers never has 2 browsers at once (a process count during the run). The same run with
  the fixture removed has 3, so the check is shown to go red.
- The report-only check finds an inserted `chromium.launch()`.

## Work packages

The estimates are design-level ranges, not firm.

| WP | What | Agent-days |
|---|---|---|
| WP-0 | `paddockTrace` (no WebKit screenshots on macOS), adopted by the kit preset and the five app configs; `caffeinate -d -i` around the macOS WebKit step. A failing test first: the config test reads the WebKit project's trace on `darwin`. | 0.25 |
| WP-1 | `paddock_slots::pool` (root and size from one function, a check of folder mode and group, the no-op without a layout); `./dev ci slot hold --pool --stdin --wait` (end on EOF, exit 79 with the holders' list); positive controls. | 1.0 |
| WP-2 | The kit's `browser` fixture with its timeout; `e2eWorkers` takes the pool's size; `paddock_testkit::browser_slot()` in the two Rust real-browser suites; the report-only check over raw imports and launches; logs of each wait and hold (`browser-slot wait=<ms> slot=sN`). | 1.0 |
| WP-3 | The sampler (`tools/host-sampler/`: a script and two LaunchDaemon plist templates) and its doc. | 0.5 |
| WP-4 | The runner move, the agents' part: the `macos-gui` label on rows 6 to 8 and 10, `PADDOCK_STEP_DEADLINE` in the actions, `PADDOCK_CI_JOBS_PER_HOST` matched to the real lanes (D8), the user's runbook in `docs/dev-setup.md`, trial dispatches T2 after the user's steps | 0.75 |
| | **Total (D, recommended)** | **3.5** |
| | B alone, with rows 6 to 8 and 10 moved into Tart guests | add 2 to 3 (total 5.5 to 6.5: **over the 2 to 4 the user expects**) |
| | C alone | 3.25 |
| | **Common-practice option:** WP-0, a static per-job budget (`PADDOCK_E2E_WORKERS` from the layout so that lanes times workers is at most 6), no machine-wide pool, C | 1.25 (WP-0 0.25, budget 0.25, WP-3 0.5, runner 0.25) |

**The machine-wide pool costs about 1.75 agent-days more than the static per-job budget**, which is common practice (no vendor
caps across a machine). The user decided on the machine-wide pool (decision 1). The static budget cannot see agents' local
runs, which were part of the incident's load. This is shown here so that ER-37 can be applied with both costs in view.

**Trials (no agent-days of their own):**
- **T1** measures the second session's memory.
- **T2** dispatches `ci.yml`, `service-smoke.yml`, `dyno-release.yml` (no tag) and `debrief.yml` (full) once on the new
  runners, and records each row's result in this ADR.
- **T3** is the pool-size sweep from decision 5.

## Steps for the user

Agents never run these (ER-14). Commands assume the CI user `paddockci` and the group `paddock-ci`.

**U0: check H2 for this incident (5 minutes, nothing installed).**
- Run `pmset -g log | grep -E '2026-11-24 (09|10):' | grep -iE 'display|sleep|wake'`.
- If the display was off before 10:33, and `ls -l /Library/Logs/DiagnosticReports | grep -iE 'WindowServer|watchdog'` shows
  nothing, H2 is likely.
- Tell the coordinator the times only.

**U1: persist the logs for next time.**
- Run `sudo log config --status` and note what it prints.
- Then run
  `sudo log config --subsystem com.apple.WindowServer --mode "level:info,persist:info"`.
- Install WP-3's collector, a root LaunchDaemon that runs
  `log stream --style compact --level info --predicate 'process == "kernel" OR process == "WindowServer" OR process == "loginwindow" OR process == "launchservicesd" OR process == "watchdogd"'`.
  It writes into `/Library/Logs/paddock-host/` with a 1 GiB size cap, oldest file first. This is the reliable path for kernel
  lines, because it does not depend on what the log store keeps.
- Remove both after 14 quiet days.

**U2: install the sampler.**
- It is WP-3's root LaunchDaemon. Every 15 s it writes one CSV line with these fields:
  - WindowServer RSS and port count (`top -l 1 -pid <pid> -stats pid,rsize,ports`);
  - launchservicesd and pboard CPU;
  - the count of `chrome-headless-shell`, `Playwright` and Chrome for Testing processes (`pgrep -x -c <name>`: an exact
    process name, never `-f`);
  - the 1- and 15-minute load;
  - the memory pressure level.
- It keeps 14 days.
- Install it with `sudo cp <plist> /Library/LaunchDaemons/` and `sudo launchctl bootstrap system /Library/LaunchDaemons/<plist>`.

**U3: right after any repeat.**
- Run `sudo log collect --last 1h --output /Users/Shared/paddock-incident.logarchive`.
- Save the sampler's file for that day.

**Runner move (D: daemon lane plus GUI lane).** Do this between trains, with no job running.

1. **Create the CI user and group.**
   - `sudo sysadminctl -addUser paddockci -fullName "Paddock CI" -password -` (a standard user, not an administrator; it asks
     for the password).
   - `sudo dseditgroup -o create paddock-ci`
   - `sudo dseditgroup -o edit -a decryp7 -t user paddock-ci`
   - `sudo dseditgroup -o edit -a paddockci -t user paddock-ci`
2. **Create the shared folders.**
   - `sudo mkdir -p /Users/Shared/paddock/slots/browsers`
   - `sudo chgrp -R paddock-ci /Users/Shared/paddock`
   - `sudo chmod -R 2770 /Users/Shared/paddock`
3. **Stop the old runners.** For each of the three plists:
   - `launchctl bootout gui/501 ~/Library/LaunchAgents/<plist>`
   - `mv ~/Library/LaunchAgents/<plist> ~/Library/LaunchAgents/<plist>.before-adr0120`
4. **Move the runner folders and work folders to the CI user.**
   - `sudo mv /Users/decryp7/gitea-runner /Users/paddockci/gitea-runner`
   - `sudo mv /Users/decryp7/gitea-runners /Users/paddockci/gitea-runners`
   - `sudo chown -R paddockci:paddock-ci /Users/paddockci/gitea-runner* /Volumes/vault-dev/gitea-runners`
   - `diskutil info /Volumes/vault-dev | grep -i owners` must say ownership is enabled.
   - Edit `config.yaml` where it names `/Users/decryp7`.
   - Keep each `.runner` with its folder: each registration stays one runner.
   - Set capacities: `d7-mac-mini` (daemon lane) `capacity: 3`, `d7-mac-mini-2` (GUI lane) `capacity: 1`, labels
     `macos-gui:host`. Retire `d7-mac-mini-3`.
5. **Install the CI user's tools.** As the CI user (`sudo -iu paddockci`), install rustup and nvm, and the Node version the
   PATH names. Optionally set `TART_HOME=/Users/Shared/paddock/tart` (group-shared) so the VM images are not downloaded twice.
6. **Install the daemon lane.**
   - Write `/Library/LaunchDaemons/net.decryptology.gitea-runner.d7-mac-mini.plist` with:
     - `UserName` `paddockci`, `GroupName` `paddock-ci`;
     - the same `ProgramArguments` with the new paths, and `WorkingDirectory`;
     - `EnvironmentVariables`: `HOME=/Users/paddockci`, the new PATH, `PADDOCK_CI_JOBS_PER_HOST=4`,
       `PADDOCK_CI_RESERVED_CORES=2`;
     - `SessionCreate` `true`, `RunAtLoad` `true`, `KeepAlive` `true`;
     - `StandardOutPath` and `StandardErrorPath` under `/Library/Logs/gitea-runner/`.
   - `sudo chown root:wheel <plist>`
   - `sudo chmod 644 <plist>`
   - `sudo launchctl bootstrap system <plist>`
7. **Grant Full Disk Access.** In System Settings, Privacy and Security, Full Disk Access, add
   `/Users/paddockci/gitea-runner/gitea-runner` and `/Users/paddockci/gitea-runners/d7-mac-mini-2/gitea-runner`.
8. **Install the GUI lane.**
   - Log in as `paddockci` with Fast User Switching.
   - Put `~/Library/LaunchAgents/net.decryptology.gitea-runner.d7-mac-mini-2.plist` there (the same keys as today, the new
     paths, `LimitLoadToSessionType` `Aqua`).
   - `launchctl bootstrap gui/$(id -u) <plist>`
   - In System Settings for this user, turn off display sleep. Leave FileVault on and auto-login off (D5).
   - Switch back to your own user. The session stays logged in.
   - After a reboot, log in to `paddockci` once and switch back. Until then, `macos-gui` jobs wait, and the dispatch tool's
     bounded wait names the reason.
9. **Tell the coordinator.** It dispatches trial T2 and answers the first TCC prompts with you: switch to the CI session when a
   GUI-lane job reports one.

**Rollback:**
- `sudo launchctl bootout system/net.decryptology.gitea-runner.d7-mac-mini`
- Move the folders back and restore the `.before-adr0120` plists.
- `launchctl bootstrap gui/501 <plist>`

## Consequences

- CI's browsers, launches and windows leave the user's session. Under D, only the one GUI lane still uses the user's
  WindowServer.
- Agents' local runs stay in the user's session, bounded by the pool.
- A Playwright run on a busy host may wait for browsers. Each wait is logged with its duration (ER-27), and a run never fails
  for waiting unless its own bound runs out.
- The CI user needs its own toolchains. Rust and Node upgrades become two installs on the Mac until the CI image's pins
  (ER-26) cover the host.
- **Not measured:**
  - the second session's memory (T1);
  - rows 1, 2, 5, 9, 11 and 12 under the daemon lane (T2);
  - the pool size (T3);
  - which cause the incident had (U0, then the sampler).

## Decisions for the user

| # | Decision | Options | Recommendation |
|---|---|---|---|
| D1 | Where the Mac runner runs | A, B, C, D | **D.** B for most jobs, one capacity-1 GUI lane in the CI user's own session for rows 6 to 8 and 10. C is the cheaper common practice (no WindowServer isolation). B alone needs D6. |
| D2 | The browser cap's mechanism | (a) a machine-wide pool (decided 2026-11-24), 3.5 days in total; (b) static per-job budgets, the common practice, 1.25 days | **(a)**, as decided: only it bounds agents' local runs. |
| D3 | The default pool size | 6, provisional, then T3 | **6, then T3.** |
| D4 | WebKit traces on macOS | (a) no screenshots, plus `caffeinate -d -i` on CI; (b) only `caffeinate`; (c) nothing | **(a)**, WP-0 first, today if possible: it removes H2's known trigger. |
| D5 | The GUI lane across reboots | (a) a manual login after a reboot, FileVault on; (b) auto-login, FileVault off | **(a).** Reboots are rare, and (b) exposes the Mac to anyone who restarts it. |
| D6 | Under B alone: rows 6 to 8 and 10 | (a) move them into Tart macOS guests (2 to 3 days); (b) drop macOS from them (rule 88 approval) | Only if D1 is B. Then **(a)** for Debrief's smoke (it ships), **(b)** for the manual window probe. |
| D7 | Install the sampler and the log collector (U1, U2) | yes or no | **Yes, for 14 days**, then remove them if nothing repeats. |
| D8 | Runner lanes against the layout | Today three plists of capacity 3, 1 and 1 against `PADDOCK_CI_JOBS_PER_HOST=3` | **Set it to the real total (4 under D) in the move.** |

### Decisions of 2026-11-25 (the user)

- **Accepted first:** D1 = D, D3 = 6, D7 = yes for 14 days, D8 = 4 lanes, as recommended.
- **D5 = (a), amended.** Manual login, FileVault on, no auto-login. Planned restarts use `sudo fdesetup authrestart`, which
  unlocks the FileVault disk once without the password at the pre-boot screen. A crash restart waits at the FileVault screen
  for the password; nothing runs until it is typed.
- **The move is brought forward.** The user's login session broke twice: the WindowServer abort of 2026-11-24, and on
  2026-11-25 no new WebKit page processes started, so Dyno showed a blank window and the nightly WebKit job hung. WP-0 has
  landed.
- **2026-11-25, later: one daemon runner, capacity 4, GUI jobs off the Mac.** This changes D1 and D8. D1 becomes option B
  without its own user (stage 1 below): the Mac keeps one runner, `d7-mac-mini`, as a LaunchDaemon with `capacity: 4`, and
  there is no GUI lane. D8's 4 lanes are that one runner's capacity, with `PADDOCK_CI_JOBS_PER_HOST=4`. The jobs that need a
  window server (inventory rows 6 to 8 and 10) move off the Mac on another branch; that move is the user's decision, as rule 88
  asks. D6 therefore applies, as the user's choice made there. D3 (the pool of 6) is unchanged.
- **Runners `d7-mac-mini-2` and `-3` are removed.** They were stopped and their plists parked in
  `~/gitea-runner-parked-2026-11-25/` on 2026-11-25; their Gitea registrations were already gone (below).
- **No macOS WebKit in CI for now (user, 2026-11-25).** With one daemon runner and no GUI lane, `e2e-webkit` runs Playwright's
  WebKit on Linux in the CI image; the WKWebView build is a local run on a Mac with a login session. Debrief's macOS start-up
  benchmark becomes a manual step of its release checklist (`apps/debrief/docs/releasing.md`). Branch `ci/mac-daemon-no-gui-jobs`
  makes these moves; the runner moves only after it lands.
- **Their files are in the Trash (user, 2026-11-25):** the runners' folders under `~/gitea-runners/` and
  `/Volumes/vault-dev/gitea-runners/`, and the two parked plists.

### The move (WP-4)

- **Stage 1 (2026-11-25, `tools/mac-runner-move.sh`).** `d7-mac-mini` runs as a LaunchDaemon in the system domain under the
  existing account (`UserName`), so it keeps that account's toolchains, folders and Full Disk Access grants. The plist is a
  template in `tools/mac-runners/`, filled with plutil from the runner's current agent. The tool checks that the runner
  declares itself to Gitea, and it has a rollback to the original agent. Runbook: docs/dev-setup.md, "Mac CI runner". This
  gives decision 7's main gain: CI's browsers, launches and windows leave the user's session. It does not give option B's
  security gain: the daemon still runs as the user, with the user's files.
- **Stage 2 (later, optional).** A dedicated `paddockci` user, as in steps 1, 2, 4, 5 and 7 of "Runner move" below. The same
  tool takes `--user paddockci` once that user's runner folder and agent exist. Steps 8 (the GUI lane) no longer apply.
- **Still WP-4's agents' part:** the build-jobs layout's macOS jobs per host (3) to 4, `PADDOCK_STEP_DEADLINE`, and T2 for
  rows 1, 2, 5, 9, 11 and 12 under the daemon. Until the GUI rows have moved off the Mac, a job that needs a window server
  still asks for `macos-latest` and fails on the daemon.
- **Found while preparing it (2026-11-25, read-only):** `d7-mac-mini-2` and `-3` had logged "unregistered runner" at every
  start since 2026-11-24 10:35, and launchd restarted them about every 10 s. So the Mac ran 3 jobs at once, not 5, from then on.

### Restarts, authrestart and the login window

A daemon runs before any login, so the runner no longer needs one: after `fdesetup authrestart` the disk is unlocked at boot
and the runner starts and takes jobs whether or not the Mac then stops at the login window. Its folders on the external disk
are mounted without a login, because `AutomountDisksWithoutUserLogin` is 1 on this Mac (read 2026-11-25; the install checks
it). The open question, whether an authrestart boots to the login window or logs the user in, therefore no longer affects CI;
it mattered only for the GUI lane, which is gone. After a crash restart nothing runs until the password is typed at the
FileVault screen.

Recurring: the class "a permission or service belongs to the session or responsible process, not the parent" (lessons of
2026-11-04 and 2026-11-06, TCC on the external volume). The runner move is a third instance, and inventory row 4 handles it
through the existing shared rule (`boot-volume.sh`, `paddock_testkit::boot_volume`). No new mechanism is needed.
