# Plan: Pitwall 1.1.0

- Date: 2026-11-07. Status: proposed, revision 2 after the independent review of `eeddb8189` ("not yet"). Docs only:
  nothing here is built or measured.
- Decision it follows (user, 2026-11-07): Pitwall 1.1 runs **alongside** Debrief 1.2, no longer after it, so the user can try
  the Dyno MCP proof of concept early: Pitwall's Playground chat using Dyno's read-only MCP tools. The scope is unchanged and
  is not split. Inside 1.1, the chat with an MCP client against Dyno's tools comes first. Shared-code changes still land
  only after Dyno 1.1.0.
- User decisions recorded here (2026-11-07): **Q1** the pasted MCP key on macOS is kept in an owner-only file; **Q2** Pitwall
  gets 3 builders after T.
- **T** is the **Dyno 1.1.0 landing on `main`**, not its tag: Dyno tags on its release branch and lands afterwards (ER-31
  (a), (d); the user's note of 2026-11-04). The dates below **assume T = Monday 2026-11-23**. That is an assumption to make
  the calendar concrete, not a forecast; every date after T moves day for day with it.
- Inputs read (2026-11-07): `origin/pitwall/1.1-final` `b7151a57f` (the head of the 1.1 work), `origin/main` `87c9b8449`,
  `origin/dyno/1.1.0-rc` `96c2a16ae` (first read at `f63a3eea2`; the two commits between change only
  `crates/paddock-mcp/tests/it/listener.rs` among the files cited, and the conflict count below is the same at both), ADR 0023
  revision 2 (this branch), family ADR 0057 on `origin/docs/adr-keychain-family` `d37b7f452`, the Debrief 1.2 plan
  `docs/plans/debrief-1-2.md` on `origin/docs/debrief-1-2-plan-refresh` `ac85e26c7` (cited as `debrief-1-2.md:<line>`),
  `origin/docs/pitwall-rag-via-recce` `e075ab2b0`. A path with a line number is at the commit named for its branch.
- Estimates are agent-days, judged from similar family work, not measured. Dates count working days (Monday to Friday)
  from **S = Monday 2026-11-09** and ignore holidays.

## 0. Where 1.1 stands

| Fact | Evidence |
|---|---|
| Every 1.1 branch is in `1.1-final`: `1.1-backend`, `-quit`, `-release` and the nine `1.1-wp2*` branches have no commit missing from it by patch | `git cherry origin/pitwall/1.1-final origin/pitwall/<branch>`: 0 missing for each |
| `1.1-final` is 195 commits ahead of `main` and 41 behind; merging `main` into it conflicts in one file (`apps/pitwall/docs/releasing.md`) | `git rev-list --count`; `git merge-tree --write-tree origin/pitwall/1.1-final origin/main` |
| Merging the Dyno rc into it conflicts in **28 files**: `crates/paddock-service` 13, `paddock-proc` 2, `paddock-shell` 2, `paddock-updater` 2, `ui/projects/paddock-ui` 5, `ui/e2e` 2, `conformance/src/checks/index.mjs`, `docs/known-copies.toml`. The rc grew `paddock-service` by about 27,000 lines (ADR 0056) | `git merge-tree --write-tree origin/pitwall/1.1-final 96c2a16ae` (same at `f63a3eea2`); `git diff --shortstat` from the merge base `f8dcd4440` |
| No 1.1 commit is in the rc's `apps/pitwall` | `git log f8dcd4440..96c2a16ae -- apps/pitwall` is empty |
| **Shell move: done on the branch.** The app runs on `paddock-shell`; four dialogs and every confirmation (`pd.confirm`) are OS windows; About, Updates, What's new from the shared updater | `apps/pitwall/docs/roadmap.md:131-133`, `:141-144`; `cbdc0ba38`, `2cf50f603` |
| **Material: done.** Every control Material or paddock-ui, checked by `scripts/ci/controls.test.mjs`; every table `mat-table` | `roadmap.md:134-135`, `:138-139`; `fa1e2bc95`, `06e48454c` |
| **Long jobs: done.** Model load, benchmark, update check, service install and removal, Repair and the app update are jobs with phases, Cancel, "Stop waiting" and "Still working"; no UI timeout | `roadmap.md:106-119`; `eb55c06c0`, `c931418ca`. Left: the Playground stream opens with a 30 s limit (`app/src-tauri/src/commands.rs:387`), fixed by ADR 0023 WP-1 |
| **Repair fix: done for the old service API, to be redone.** `b7151a57f` moved macOS onto `paddock_service::Manager` and deleted `service_repair.rs`; ADR 0056 on the rc redesigns that Manager, and asks Pitwall and Pitcrew to adopt it | `b7151a57f`; `docs/adr/0056-service-repair-one-declared-state-machine.md:314-321` at `96c2a16ae` |
| **Layout: done in the browser.** No layout problem at 900, 1024 and 1280 px in Chromium and WebKit | `roadmap.md:129-130`; plan `docs/plans/1.1-wp2.md:133-139` |
| **Release machinery: partly done.** The CHANGELOG with the one-time reinstall note (`7e691f7c8`; the version numbers were already 1.1.0 on the branch: `Cargo.toml:17`, `tauri.conf.json:4`, `app/package.json:3`); third-party notices checked in CI (`4d373b67b`); `pitwall-release.yml` publishes only on a `pitwall-v*` tag (`.gitea/workflows/pitwall-release.yml:20-21`, `:109`). The CHANGELOG is dated 2026-10-30 and still says "Offline mode now stops the app update check" (`CHANGELOG.md:3`, `:18`), though `1bf262297` removed Offline mode | |
| Left on the branch: Mod+Q with no window, the Settings footer wording, the tray popover not opened by the native tray, the C4 diagrams, `.changelog()`; the independent screenshot review "still to come" | `roadmap.md:145-149`; the pending-route follow-up `docs/plans/1.1-wp2.md:245-248`, which says "after the Dyno and Spotter tags" |
| Windows has never run on a real machine | `apps/pitwall/docs/windows.md:6` |
| **The Windows service can run a program other accounts can change, as LocalSystem.** Pitwall was per machine already in 1.0.x (`"installMode": "perMachine"` in `tauri.windows.conf.json`, the same at `pitwall-v1.0.3`; revision 2 read only `tauri.conf.json` and was wrong), so there is no per-user copy to move. The holes: (1) nothing checks that `$INSTDIR` is directly under Program Files and protected (the directory page, `/D=` and passive mode can point elsewhere); (2) `install()` registers whatever `current_exe()` is, from any folder; (3) none of ADR 0053's mechanism (`check-dir`, the guard, stop-all, the installer lock, `UserRunEntry`, the per-machine updater hand-off) | `crates/pitwall-server/src/service/windows.rs:513-515`; `app/src-tauri/tauri.windows.conf.json:21-27`; `app/src-tauri/windows/installer-hooks.nsh` |
| The six `pitwall-v1.1.0` entries in the copies register: the files of five are deleted on the branch (`window.rs`, `updater.rs`, `native-feel.ts`, `sparkline.ts`; the `lib.rs` launch plan replaced). `tray.rs` remains, but the registered items (`build_menu`, `create`, `update`) are gone: its functions are now `state_of`, `dark_taskbar`, `shell_view`, `tray` and `act` over `paddock_window` | `docs/known-copies.toml:289`, `:317`, `:367`, `:534`, `:685`, `:733` on `main`; the `tray` copy at `:376-380` on `1.1-final`; `app/src-tauri/src/tray.rs:12-13`, `:28-120` |
| Six Pitwall waivers expire 2026-12-31 (C37 once, C39 three times, C53 twice); the user allows no extension | `paddock.waivers.toml:31`, `:74`, `:80`, `:86`, `:214`, `:220` on `1.1-final` |
| Pitwall keeps its admin-key copy in the OS credential store through `keyring`, and so does its CLI | `app/src-tauri/Cargo.toml:60-62`, `src/credentials.rs:44-45`, `crates/pitwall-cli/src/discover.rs:162-164` on `1.1-final` |
| Family ADR 0057 (Proposed, D1 to D5 open) already assigns Pitwall 1.1 work: the admin key leaves the Keychain, `<data>/admin.key` becomes its only store, `OsKeyStore` and the CLI's `os_store` go, and pairing gets a re-pair flag and a revoke rule that Debrief 1.2's re-pair depends on (its WP-4) | `docs/adr/0057-no-blocking-keychain-on-macos.md:305`, `:315-323`, `:408` at `d37b7f452` |
| Dyno 1.1.0 serves MCP on loopback with 10 read tools and `listChanged: false`; `bridge::run` holds the discovery check and handshake | ADR 0023 revision 2, Context and Decision 7 |
| Recce (knowledge collections) is not in 1.1: it waits for Recce's MCP server | `apps/pitwall/docs/roadmap.md` on `e075ab2b0` |

**Side branches** (none deleted):

- `origin/pitwall/tray-widgets` `f214ea421` holds 4 commits that are not in `1.1-final` by patch. Two are shared:
  `dcf3a0a4e` (dialog re-targeting and the Settings anchor) and `795a5c0ea` (the duplicated chooser rule). `1.1-final` has
  same-subject commits for both (`00a3d56d3` with the later re-target fixes `c7baebd24`, `b59e11c70`; `52c97a68c`), and
  `widgets/src/chooser.ts` is identical to `1.1-final`'s. The other two have their same-subject versions in `7e02866f0` and
  `1169bcffb`. **Judged superseded** by subject and file; B0's stale-revert check confirms it on the merged tree.
- `origin/pitwall/1.1.0-bump` `e3230535c` differs from `7e691f7c8`: it bumps the version in five files and adds the
  CHANGELOG, while `7e691f7c8` adds only the CHANGELOG because the branch already carries 1.1.0. **Superseded.**
- `origin/docs/pitwall-1-1-chat` `aee241b62` is **superseded** by this branch, which carries its two commits (rebased onto
  `main` as `f63f9315c` and `dd520c8cf`).

## 1. The base list as packages

Everything in 1.1 that is not the chat. Revision 1 of ADR 0023 quoted "base 12.5–20" from a draft readiness report that was
never committed; that figure left out the merge, ADR 0056's re-adoption (which did not exist yet), ADR 0057, per-machine
install, the waivers and the VM round. This list replaces it.

| # | Package | Where | Role | Days | Needs | State |
|---|---|---|---|---|---|---|
| B0 | **Merge the Dyno rc into the 1.1 work now**, on `pitwall/1.1-merge-rc` (section 4): 28 conflicted files, generated files regenerated, a stale-revert check against both parents, the affected tests of the crates touched and of Dyno and Pitwall | shared | implementer, designer for `paddock-service` | 3–5 | | done (pitwall/1.1-merge-rc, `bb2ad6564`, `a68bf60ab`) |
| B0b | Re-merge after T: the rc's last changes and `main` (one known conflict, `releasing.md`) | shared | implementer | 0.5–1 | T, B0 | done (`origin/main` and `dyno/1.1.0-rc` are both ancestors of the train) |
| B1 | Service on ADR 0056's `Manager` on **macOS, Windows and Linux**: backend, service-only answer, `with_program`, data folder, `tracker()` and hold messages, the wiring contract on the simulator; on Windows the elevated child and the per-machine records in their own state file under the same type; the macOS 26 Repair test kept failing-first; `Copies-waived:` trailers as ADR 0056 says. Re-sized from 2.5–4: three backends, and Windows' elevated step is new to Pitwall's code | app | designer | 4–6 | B0 | done (`pitwall/1.1-b1` in the train) |
| B1b | **The rest of B1:** Pitwall's service on the shared `Manager` on **Windows and Linux** (B1 did the StartWatch port and macOS): the Windows backend and the elevated child, the per-machine records in their own state file under the same record type, the Linux (systemd user) backend, hold messages and `tracker()` on all three, and the wiring contract on the simulator; `Copies-waived:` trailers as ADR 0056 says. B6's Program Files check in `pitwall-server install` is agent 1's: B1b reads `pitwall/1.1-b6` and does not edit that check. Merged on `pitwall/1.1-b1b-b6` (decided 2026-11-08): `plan_install` runs before the shared manager's install; when this same checked program is registered already (`AlreadyInstalled`) the install repairs that registration in place instead of refusing, so Settings > Service > Repair works on Windows, and `check-install` passes it (security-neutral: every check on the program comes first and only the identical path qualifies) | app | designer (agent 2) | 3–5 | B1 (merge), B6 read-only | done (`pitwall/1.1-b1b-b6` in the train); the Windows and Linux backends have never run on a real machine (B12) |
| B2 | Offline remnants: the updater's Offline gate (`e52d0581f`, `00bd3e436`) reconciled with the family's "no Offline mode" (2026-11-03) in the merge; the CHANGELOG line and date corrected | shared + app | implementer | 0.5–1 | B0 | done (`pitwall/remove-offline-mode` in the train) |
| B3 | Shell left-overs: Mod+Q with no window, the Settings footer for a page with its own save bar (a paddock-ui gap), the native tray opening `#/tray`, the pending-route queue as the shared change `1.1-wp2.md:245-248` describes (Dyno adopts it too; after the Spotter tag, section 3), `.changelog()`, the C4 diagrams | shared + app | designer | 1.5–3 | B0b; Spotter's tag for the shared part | partly: the pending-route queue is left (the shared change, after the Spotter tag). Mod+Q with no window, the Settings footer and the native tray opening `#/tray` are in the train; `.changelog()` (`76bd14d41`) and the C4 diagrams (`1f0b5d18c`, `5cf18bc73`) are on `feat/pitwall-b3-b9-wp12`, not in the train |
| B4 | Report a problem on the shell's `paddock-diag`: Pitwall's sections, `pw_` and `pmk_` redaction, chats and attachments left out (test with a planted marker); found "nothing wired" in the 2026-11-05 bundle audit | app | implementer | 2–3 | B0 | done (`report.rs` with the `pw_` and `pmk_` redaction and a planted-marker test: `267c70e35`, `e9e251bac`, `a32ef24fc`; `pitwall/1.1-merge-rc`) |
| B4a | **Install the logger** in the app, the server and the CLI (ER-27): the shell's logging API as Dyno uses it (`paddock_shell::logging::install` in the app, `paddock-log` with process `server` and `cli` elsewhere), the family log folder and CLEF format, rotation and redaction, start, listen, stop and error lines in the server, the chat route's `pitwall_chat` lines. The app already calls `paddock_shell::logging::install` first in `run` on `1.1-final`; the server and the CLI have only `eprintln!`. A test installs the logger in a temporary folder and finds the `pitwall_chat` lines in the file with no content | app + server + cli | implementer (agent 2) | 1–1.5 | B0; B4 depends on it |  done (`pitwall/1.1-logging` in the train: logger installed in the app, the server and the CLI) |
| B5 | **ADR 0057 WP-4, plus the pasted key.** The admin key file-only: `<data>/admin.key` the only store, `OsKeyStore` (`credentials.rs`) and the CLI's `os_store` (`discover.rs:162-164`) removed, on every OS; the re-pair flag and the revoke rule in pairing (ADR 0057 §4: approving a flagged request revokes that app id's older keys with `revoke_key`, `pitwall-server/src/auth.rs:238`), with tests; Clippy entries in Pitwall's `clippy.toml` and its C99 waivers removed; the release note's delete command for the old item. The pasted `pmk_` key in `FileSecrets` on macOS (Q1), the OS store on Windows and Linux, through `paddock_secrets::for_app` once ADR 0057 WP-2 has landed, or `FileSecrets` named directly until then (one shared adapter either way, so no copy) | app (+ `paddock-secrets` as a user) | implementer | 1.25–2 | ADR 0057 D4 | done (`pitwall/1.1-b5` in the train); the follow-ups listed under "B5 state" are open |
| B6 | **The install verb refuses an unprotected program: a security fix and a release gate.** `pitwall-server install` refuses a program that is not in a folder directly under Program Files (as Windows names it) or that another account can change (paddock-service's protected-chain rule and known folders, no copy), and refuses a registration of another copy with what to do (uninstall Pitwall, then install again; ADR 0053 §5, no migration); the hook shows its words. Tests over fake folders and a fake service manager. Defence in depth that stays after B6b. Holes (1) and (2) of section 0 | app | implementer | 1–1.5 | B0 | done (`pitwall/1.1-b6`, `6c6597f02`) |
| B6b | **Full ADR 0053 adoption on Windows** (hole 3 of section 0): the shared include and `paddock-setup.exe` in the hooks (the stop list from the config, `check-dir`, the guard, stop-all, the installer lock), the updater's `InstallScope::PerMachine` with the elevated hand-off (install on quit goes), `UserRunEntry`, C91 green, the per-machine VM suite; ADR 0053's adoption row | app | implementer | 2–3 | B1, B6 | partly: ADR 0053 adoption is in the train (`4e2de7a3b`: shared include, elevated hand-off, `UserRunEntry`); the per-machine VM suite has not run (B12) |
| B7 | The six waivers removed before 2026-12-31: C37 (integration tests into `tests/it`), C39 (three freed-port tests to a refusing connector or `HeldPort::reset`), C53 (two kills through `paddock_proc`) | app | implementer | 1.5–2.5 | B0 | done (`db9ab0ee9`, `81831d009`, `a8b26d8f7`, `4cc7a6bee`, `457a35481`; no Pitwall C37, C39 or C53 entry is left in `paddock.waivers.toml`) |
| B8 | Copies register: the six `pitwall-v1.1.0` entries closed with `Copies-removed:` (`tray.rs` included: its registered items are gone, section 0; `tray_model.rs` keeps its registered `pitwall-v1.2.0`); `copies --check-register` and C58 green. Only if the check finds a registered item still present does the tray entry need a date move, which is a user decision (Q5) | app + register | implementer | 0.5–1.5 | B0 | done (`f3ca5da6f`; no `pitwall-v1.1.0` entry is left in `docs/known-copies.toml`) |
| B9 | ADR 0052 checks: `features.toml` rows (wired, visible outcome, real layout) for every 1.1 feature, the chat's included | app | implementer | 1.5–2.5 | ADR 0023 WP-12 for the chat rows | open (no `features.toml` for Pitwall; `feat/pitwall-b3-b9-wp12` is not in the train and adds only `features.md` and the user guide) |
| B9b | **The test kit the owed rows of `features.toml` wait on** (user decision 2026-11-12: build the kit before the Pitwall 1.1 tag): ADR 0037 WP-7 (`@decryp7/paddock-e2e/tauri`, the real-app kit, Linux and Windows legs; 3 days in 0037), ADR 0052's outcome harness (WP-B3, C86; 2 days) and the real-layout check (WP-C2 `layoutProblems`/`expectLayoutClean`, 2.5 days; WP-C3 C89), then the Pitwall specs that turn the `owed` entries (`tauri`, `outcome`, `layout`; 57 on 19 rows at ba09a44f7, `owed_until = "pitwall-v1.1.0"`) into named tests. C85's release-time gate fails the tag while one is owed | shared + app | implementer, designer for the kit | 8–12 | B9; ADR 0037 WP-7; ADR 0052 WP-B1, WP-C2 | open |
| B10 | Independent UI review on the merged base (ER-23, rule 22): screenshots at 1440x900, both themes, every route and dialog, the chat included; P1 and P2 fixed | app | reviewer, designer for fixes | 1.5–2.5 | B3, WP-4b | open |
| B11 | Release machinery: CHANGELOG and release notes (First launch, the 1.0.x reinstall by hand, ADR 0057's delete command), rule 25's pre-tag checks, the rule 54 dispatches, the tag | app | implementer | 1–2 | all | open |
| B12 | **VM round** with the rule 71 rows of section 7, on macOS 26, Windows 11 and Ubuntu; Windows is its first real run | app | implementer, designer to judge | 5–8 | everything but B11 | open (needs the VM round; Windows is its first real run) |
| | **Base total** | | | **26.75–44.5** | | |

**Known test flake (found 2026-11-08, not fixed): `dyno-agent` `term` tests exit 143 under load.** In a full `cargo nextest run --workspace` of
`apps/dyno` run while other builds ran, `dyno-agent::term the_agents_start_and_stop_are_in_its_own_dated_log_with_millisecond_counts`
and `dyno-agent::term term_stops_a_running_agent_cleanly_and_removes_its_discovery_file` failed with "a clean stop is exit code 0, got
ExitStatus(unix_wait_status(36608))", that is **exit code 143**; the same two passed 20 of 20 in isolation (`--stress-count 5`) and on
`pitwall/1.1-merge-rc`, and the whole workspace passed on the rerun. They are run with nextest: the Dyno CI leg runs `nextest`
(profile `ci`: `retries = 0`, `test-threads = "num-cpus"`, `slow-timeout` 30 s ×2; `apps/dyno/.config/nextest.toml`), and locally
`cargo nextest` too, so a load like a CI runner with several jobs is the normal case. **Unconfirmed hypothesis (ER-28):** the tests wait
for the discovery file, which `run` writes before it sets `RUNNING` (`cli.rs`, `RUNNING.store(true)` after "Agent running"), so a TERM
that lands between the two reaches `OnSignal::EndNow`, which ends with `end_process(143)` instead of the clean stop. The fix is
deterministic (wait on the condition "the agent is running", for example a ready line or a state the agent exposes, not on the file),
not a retry and not a longer wait (ER-12); it belongs to Dyno's owner and is not part of B1b.

The chat's packages are ADR 0023 revision 2's table: **32.5–51** (WP-7 re-sized to 6–9), of which the PoC part (WP-1, 2a,
3a, 4a, 8, 9a, P) is 11–17. Independent code reviews of the landings: **3–5**.

**Total: 62.25–100.5 raw, 74.7–120.6 with 20 %** (B6 split into B6 and B6b on 2026-11-08). Revision 1's whole figure was 43.5–70.

**ADR 0057 and the user's Q1.** The user's answer (an owner-only file on macOS) is ADR 0057's direction for macOS
(`paddock_secrets::for_app` is `FileSecrets` on macOS and the OS store elsewhere, ADR 0057 §2). ADR 0057 adds, beyond Q1:
the admin key file-only **on every OS**, the CLI included (§4; part of the mechanism the user accepts or not with D4); the
re-pair flag and revoke rule; rule 72 and check C99 (no `keyring` in a macOS build); the shared backup exclusion on the
secrets folder and `contains` (its WP-2, 4.5–5 days, family work after the Dyno tag, not counted here); the release-note
delete command; and its VM row (§6). Its trade-off, plainly (§5): **the file is plain text at rest** (FileVault protects it
only when on), **readable by any program running as the same user** (an npm `postinstall` or Cargo build script included,
silently), and **copied in plain text by backup and clone tools** other than Time Machine (Carbon Copy Cloner, `rsync`, a
migration). ADR 0057's D1 to D5 are still open (section 9).

**B5 state (2026-11-08, branch `pitwall/1.1-b5`, `056d86e31`, `5ffb943a8`, `cda8cdbcc`).** Built: `paddock_secrets::SecretFile`
(one owner-only file at a chosen path; `FileSecrets` is a folder of them), the admin key in `admin.key` only on every OS
(app and CLI; `keyring` out of both), the `"rePair"` flag and revoke rule, Pitwall's Clippy entries, the release note's
delete commands, and C99 layers 1 and 2. **The pasted `pmk_` key stays as the user decided on 2026-11-07:** an owner-only
file on macOS, the OS store on Windows and Linux (rule 72 bans only the macOS Keychain), so the chat branch keeps its split
and `paddock-secrets/keyring` off macOS; only the admin key is file-only on every OS (ADR 0057 D4). The chat merge has a
`app/src-tauri/Cargo.toml` conflict (both add `paddock-secrets`), resolved at integration. Follow-ups:
- the rest of C99, ADR 0057 WP-2: the dependency source scan, the macOS graph probe, the Clippy preset entries and C13's
  `disallowed-types`, the lint fixture, the CI time measured; and C99 in ci.yml's root `--only` list;
- rule 72's text lands with ADR 0057 (this tree's `docs/rules.md` ends at 55);
- the Windows tests (`paddock-secrets` `file_windows.rs`: the owner-only DACL, the refused shared DACL) have only been
  type-checked with `cargo xwin`; they run on a real Windows machine in CI, on the `windows-latest` crate job.

## 2. Order and critical path

### Phase A: from S to the PoC (nothing lands on `main`)

| Agent | Day 0 | Then |
|---|---|---|
| 1 (implementer, designer for the service part) | B0 merge | B4, B7 (app-only, on the merged branch) |
| 2 (implementer) | ADR 0023 WP-8 on a branch from the rc (`paddock-mcp` only) | WP-1 on a branch from `1.1-final` (app-only, so it merges cleanly), then WP-3a |
| 3 (designer) | WP-2a on a branch from the rc (`paddock-assistant` only, unchanged on the rc since the merge base) | WP-4a against the mock, then WP-9a |

This needs 3 builders before T, which come out of Debrief's wave 0 (question Q3).

The PoC branch `pitwall/1.1-chat` starts from `pitwall/1.1-merge-rc` once B0 is done, and merges the WP-8, WP-2a and WP-1
branches (merges, not rebases), and agent 3's `pitwall/1.1-wp4a`, `pitwall/1.1-wp4a-app` and `pitwall/1.1-wp9a`. Later work
goes straight onto it.

**Temporary: the chat's build switch** (user, 2026-11-07). ADR 0049's switches are not built, so Rust decides from a Cargo
feature: `chat` in `apps/pitwall/app/src-tauri/Cargo.toml`, reported by the command `chat_enabled`; the Playground shows its
Chat tab only when it says so, and is the 1.0 Playground otherwise. Since 2026-11-12 (user decision, 2026-11-12: Pitwall 1.1 ships the chat on) it is a default
feature, so the release workflow builds with it. **Remove** when ADR 0049 WP-5 lands: the feature becomes a `switches.toml`
entry and the command reads the generated table.

**Note for WP-3a (agent 2), from the user's answer of 2026-11-07:** the Chat tab gets a model picker of the loaded and catalog
models; models without the `tools` capability are listed disabled with the reason (ADR 0023 D6), and the starter
tool-capable model is the default. The pane has no picker of its own: WP-3a's provider takes the chosen model, and the
picker sits in the Chat tab (`apps/pitwall/app/src/app/features/playground/playground-chat.ts` on `pitwall/1.1-wp4a-app`).

**PoC path:** agent 3's chain WP-2a (2–3) → WP-4a (2–3) → WP-9a (2.5–3.5), then merging the branches into the PoC branch
and the PoC build P (1–2): **7.5–11.5 working days**. B0 (3–5) reaches WP-9a in time, and WP-8, WP-1 and WP-3a have float.
**The internal build the user can try: about 2026-11-19 to 2026-11-25**, a local build per ER-15 (never published), with the
chat switched on at build time (ADR 0049; history, until 2026-11-12: the chat is now on in every build), against Dyno 1.1.0 or, before Dyno is tagged, an rc build of Dyno. It shows:
connect to Dyno with a pasted key, pick tools, ask, approve the first call of each tool, see every call in the chat, Stop in
every phase. It does not yet save chats, attach images, take speech, or show "Still working" in a quiet spell (that waits
for `paddock-jobs::patience`, section 3). It needs a starter model with the `tools` capability; WP-6 checks the
capability test against the catalog.

**WP-6 (the speech spike) moves out of the night window.** The 01:00 to 06:00 window is Debrief's quiet-benchmark window
(Debrief D4, `debrief-1-2.md:745`; CPU-1, NB-1 and the others at `:669`). WP-6 runs on a weekend day when no agent builds, or
in a night the coordinator frees after CPU-1 and NB-1. It is needed only before WP-7 (Phase C), so it can wait until about
T+5.

### Phase B: from T, the base lands

1. B0b (0.5–1), then **L1: land the 1.1 base on `main`** after the rule 54 dispatches (section 4, step 4). The chat code
   rides along (history, until 2026-11-12: behind its build switch, off; the chat is on in the release build since).
2. Then the shared chat crates (WP-8, WP-2a) land. One shared landing at a time, through the integrator.
3. App-only base work in parallel: B1, B2, B4, B5, B6, B6b (after B1), B7 (by **2026-12-31** at the latest), B8.

### Phase C: the rest of the chat

WP-2b → WP-3b → WP-4b → WP-5 and WP-7 (WP-7 also needs WP-6's result and the user's look at it), WP-9b → WP-10 (cuttable)
→ WP-11. WP-2b lands its shared part early (before Debrief's assistant work, section 3). The quiet-spell note of WP-4b lands
after Debrief's 2.6c (section 3).

### Phase D: release

B9, ADR 0023 WP-12 and B10 (reviews in parallel with the last fixes), the full suite once on the combined tree (ER-8),
then B12 (the VM round), B11, the tag from CI.

**Critical path to the tag** (one agent per package): WP-2a (2–3) → WP-4a (2–3) → WP-4b (2.5–4) → WP-7 (6–9) → WP-12 (2–3)
→ B12 (5–8) → B11 (1–2) is about **20.5–32 working days** (18.5–29.5 at WP-7's old size of 4–6.5). B0, B1, B10, WP-5,
WP-9b, WP-11 and the base packages run beside it. The capacity bound is 29–47 working days at 3 agents (section 6), so
**capacity sets the end**, with the path close behind it at the top of its range.

**Floor after T:** B0b (0.5–1) → L1 (0.5–1.5) → B1 (4–6) → B12 (5–8) → B11 (1–2) is **11–18.5 working days**, plus the time
L1 and the shared landings wait in the landing queue (9.5–16.5 at B1's old size). With T = 2026-11-23 that is no earlier
than about 2026-12-08 to 2026-12-18, inside the capacity estimate; a T after about 2026-12-10 makes the floor the end.

## 3. Shared code, and how it is ordered against Debrief 1.2 and Spotter 1.0

Three releases now touch shared code in the same weeks: Dyno 1.1.0 lands at T, Spotter 1.0 tags when its checks pass (about
9 to 14 working days after Dyno, on `steward/spotter-1.0-prep`, not `main`), and Debrief 1.2 runs its waves. Rule 1 holds by
the user's choice of overlap: one shared landing at a time through the integrator; a landing that changes an API updates
every app that uses it in the same change (AGENTS.md, rules 39, 42, 43); whoever lands second merges `main` first. Debrief's
wave times are from `debrief-1-2.md` §9.4 (`:674-678` and the rows after it).

### L1's real shared diff

`git diff --stat f8dcd4440 origin/pitwall/1.1-final -- crates ui conformance .gitea`: **95 files, 4,513 insertions, 270
deletions** (`f8dcd4440` is the merge base with `main`; B0 adds the rc's side on top):

| Area | Files |
|---|---|
| `crates/paddock-service` | 17 files, 2,000 insertions (StartWatch, StartWait from Pitcrew, KillMode, JobReport, `sc.exe`, stop timeout) |
| `crates/paddock-shell` | 9: `builder.rs` and its tests, `command_names.rs`, `lib.rs`, the permissions (`window-classes.toml`, autogenerated, schema), `tests/it/plugin.rs` |
| `ui/projects/paddock-ui/shell` | **19**: About and Updates (9, three generated), the Settings window (7: close hold, status, provider, window), channels page (2), `shell/generated/shell-commands.ts` |
| `crates/paddock-updater` | 7 (installer declaration, releases page, offline gate) |
| `crates/paddock-build` | 7 (sidecar placeholders) |
| `crates/paddock-pitwall` | 2: `testing.rs`, `wire.rs` |
| `crates/paddock-proc` | 2: `instance.rs`, `lib.rs` |
| `crates/paddock-window` | 1: `tauri/tests.rs` |
| scss | `_tokens.scss`, `_material.scss`, `_controls.scss`, with `ui/test/tokens/controls.spec.ts` |
| workbench | `panels/src/panel-snapshot.ts` and its spec, `dock/src/layout-manage-dialog.ts` |
| testing, widgets, i18n | `testing/src/dom.ts`, `setup.ts`, `testing.spec.ts`; `widgets/src/frame.ts`, `widgets.spec.ts`; `i18n/messages.pd.xlf`, `messages.pd.en-XA.json` |
| demos and e2e | gallery (2), shell-demo (4), `ui/e2e` (3) |
| `.gitea/workflows` | `ci.yml`, `nightly.yml`, `pitwall-release.yml` |
| `conformance` | `src/checks/C56-build-env-names.mjs`, `C59-updater-installer.mjs`, `index.mjs`, its test, README |
| also | `crates/Cargo.lock`, `crates/README.md`; outside the shared folders, 5 files of `apps/pitcrew` (sidecar placeholders) and 1 of `apps/shell-demo` |

`_tokens.scss` and `.gitea/` are shared integration points: the integrating agent changes them, and the token test stays
green.

### Collisions, re-checked against Debrief's packages

| Shared code in L1 or later | Debrief 1.2 | Order |
|---|---|---|
| Settings window, About, Updates, i18n, channels page, `paddock-updater`, `paddock-pitwall` | M1 contributions: settings pages, notification kinds (wave 2, T+5 to T+10, `debrief-1-2.md:115`); M2c: settings, About, Updates, i18n merge, and Debrief's Pitwall client onto `paddock-pitwall` (wave 3, `:124`) | **L1 before M1** (L1 at about T+1 to T+3). M1 and M2c then build on L1's settings hold, status and `wire.rs` |
| `paddock-shell` builder, command names, permissions; B3's pending-route queue | M2a shell services (wave 3, `:123`), M4 Rust side (wave 6, `:168`) | L1 first; B3's shared part after the Spotter tag (`1.1-wp2.md:245`) and before M2a where it can, otherwise M2a lands first and B3 merges it |
| Workbench panels and dock | M3a dock, pop-outs (wave 5, `:147`), Y12 workbench move (`:609`) | L1 first, weeks ahead; no ordering problem |
| scss tokens, Material, controls | Y10 rule 47 views (from 2026-12-01, §10.4); no token change named in Debrief's plan | L1 first; Debrief's views use L1's tokens |
| testing, widgets | 1.3 list contract test (`:113`); Y5 tray popup moved out of 1.2 (§10.3) | L1 first; small |
| `paddock-proc` | 2.2 (`paddock-proc`, `paddock-metrics`, wave 1, `:122`), P1 | L1 first; 2.2 merges `main` after it |
| `ci.yml`, `nightly.yml`, conformance | D-a CI image and workflows (`:139`), A1's checks (`:220`), 1.3's and 1.4's checks | Through the integrator; `index.mjs` is a union |
| `paddock-assistant`, `workbench/assistant` (WP-2a, 2b, 4a, 4b) | 2.7 assistant turns adopt list identity (wave 5, T+20 to T+30, `:145`), M3b assistant pane (wave 6, `:156`) | **Pitwall first**: WP-2a by about T+3, WP-2b and WP-4b's component half by about T+15 |
| `paddock-jobs::patience` (WP-4b's quiet-spell note) | **2.6c creates it** (`paddock-jobs::patience`, `paddock_proc::runstate`; waves 1 and 2, `:126`) | **Debrief first.** Pitwall's quiet-phase label is written against 2.6c's branch API and lands after 2.6c (about T+5 to T+7). Pitwall writes no stall judgement of its own (rule 55); the PoC goes without the note |
| `paddock-dialogs`, `shell/dialogs` | 1.2's rest (`DialogFrame`, `:112`), M2b (wave 4, T+15 to T+20, `:133`) | Debrief's landing updates Pitwall's four dialogs, `pd.confirm` and the MCP key dialog in the same change; L1 and WP-9a on `main` before M2b. If 1.2's small rest lands before L1, L1 merges it and adapts Pitwall's dialogs |
| `paddock-jobs` otherwise | J1 idempotency (`:227`), 1.4's variants | Debrief's changes update Pitwall's call sites; L1 first so they are on `main` |
| `paddock-limits` (new) | 1.4 creates it (wave 1, `:114`) | Pitwall does not adopt it in 1.1; the chat's limits are typed errors in `pitwall-core`; a roadmap item for after 1.1 |
| `paddock-mcp` (WP-8, WP-10) | none | Additive, after T; WP-8 makes `bridge::run` use the new client, so Dyno's tests run on it |
| `paddock-service` (B1 app side) | none | Dyno owns the crate until T; Pitwall's merge resolution lands in L1; Pitcrew 1.1 later |
| `paddock-secrets` (B5 as a user; ADR 0057 WP-2) | WP-5: Debrief's key to `FileSecrets` and its re-pair **sends the re-pair flag** that Pitwall's B5 adds | **Pitwall's flag first.** ADR 0057 assumed Debrief 1.2 ships first (`:322-323`: until Pitwall 1.1, Debrief's notes tell the user to revoke the old key by hand); with Pitwall now tagging months before Debrief, Debrief's WP-5 can rely on the flag, and that sentence of ADR 0057 needs its author's amendment |

**Spotter 1.0.** Spotter is not on `main`, so each Pitwall shared landing before its tag is one more merge into Spotter's
branch. Pitwall batches its pre-Spotter shared landings into two (L1, then WP-8 with WP-2a), takes B3's shared part after
Spotter's tag as `1.1-wp2.md:245` says, and leaves ADR 0057 WP-3 to Spotter (Spotter's own `Secrets` port).

**Conclusion.** L1 is the large collision, not the assistant: it changes the Settings window, About, Updates, i18n,
`paddock-pitwall`, the shell builder and the workbench that Debrief's M1, M2c, M3a and M4 rewrite. It must land before
Debrief's M1 (about T+5); it can, because it is resolved before T. After L1, the assistant is ordered Pitwall first and the
patience crate and dialogs Debrief first, and nothing else collides.

## 4. Merge plan: bringing `pitwall/1.1-final` up to date

1. **Now (B0), branch `pitwall/1.1-merge-rc`** from `pitwall/1.1-final` `b7151a57f`; `git merge origin/dyno/1.1.0-rc` (a
   merge, never a rebase). Resolve:
   - `paddock-service` (13 files): take the rc's ADR 0056 side, then check each of `1.1-final`'s service commits that the rc
     does not have by patch (`git cherry`: among them `f837758ad` StartWatch and StartWait, `19362d9c8` KillMode, `a775ee4e4`
     JobReport fields, `a84c7fb5f` and `41c060c0d` `sc.exe` by full path, `a912ed70c` stop timeout, `232e60c1a`, `48285cd9b`,
     `ba57e01aa`, `bf6d9f129`) against the rc's behaviour with its test; port only what the rc lacks, as commits on top with
     their tests. A designer judges this part.
   - `paddock-proc`, `paddock-shell`, `paddock-updater`, `ui/projects`, `ui/e2e`: union by tests; generated files
     (`permissions/schemas/schema.json`, `about/generated/about.ts`) regenerated, never hand-merged; the updater's Offline
     gate by B2's rule.
   - `conformance/src/checks/index.mjs` and `docs/known-copies.toml`: union, then B8.
   - Check the merge against each parent for silent reverts (the 2026-11-05 lesson). Run the affected tests of the touched
     crates and of Dyno and Pitwall (the merged crates are what Dyno ships too), the service code on Linux in a container
     (ER-9), `cargo xwin check` for Windows; heavy steps through the build lock, never in the night window.
2. **Until T:** re-merge the rc tip whenever it moves (small: it takes only critical fixes now).
3. **At T** (Dyno's landing on `main`): merge `origin/main` into `pitwall/1.1-merge-rc` (B0b; today's only conflict with
   `main` is `apps/pitwall/docs/releasing.md`).
4. **L1, rule 54.** Re-run `node scripts/affected.mjs origin/main --explain` on the merged branch and dispatch every release
   workflow it reaches, with gates on and the bundle built. Run on `1.1-final` against today's `main` (2026-11-07) it reaches
   all nine apps: Debrief, Dyno, Pacenotes, Pitcrew, Pitwall, shell-demo, Sighter, Spotter, workbench-demo (the shared
   crates reach every Cargo workspace; Pitcrew also through its own 5 files). So the dispatches are
   `pitwall-release.yml`, `dyno-release.yml`, `pitcrew-release.yml`, `spotter-release.yml` (on Spotter's branch merged with
   L1), `shell-demo-release.yml`, `workbench-demo-release.yml` and Debrief's release jobs in `debrief.yml`. Pacenotes and
   Sighter have no release workflow. That run compares `main` with the branch head, so it also lists files that only `main`
   changed (Debrief's `mock-jobs.ts`), which the run on the merged branch will not. Then merge into `main` (no squash), CI on
   `main` once.
5. `pitwall/1.1-chat` merges `main` after L1 and from then on lands in small merges behind its switch (ADR 0049), each with
   its own `--explain` and dispatches.

Allowance: B0 3–5 days, B0b 0.5–1; it feeds the PoC path, which is why it starts now.

## 5. Agents beside Debrief

- **Before T.** Debrief's plan runs wave 0 on all seven agents "now to T" (`debrief-1-2.md:676-678`; §10.5: "wave 0 uses all
  seven", `:833`): L-0 landing, X1 then WP-SL1, X7 waivers, SI-0, Z0, F1 and F3, and the 0.5 design. Pitwall's 3
  builders in Phase A come out of that, on top of what Dyno's release still uses. The proposal (Q3): Debrief keeps L-0, X1
  and WP-SL1, X7 (its waivers end 2026-12-31) and 0.5 (the start of its chain); **SI-0, Z0, F1 and F3 wait until T**. Cost
  at T = 2026-11-23: about 10 working days × 3 agents × 0.85 ≈ **25.5 agent-days**, about **4.5 working days** later at
  Debrief's end (at 5.5 a day). With 1 Pitwall builder before T (B0 only), Debrief loses about 8.5 agent-days (about 1.5
  days), and the PoC moves to about 2026-11-27 to 2026-12-01.
- **After T (decided, Q2): Pitwall 3, Debrief the rest.** Spawns follow the load rule (start one only while the 5-minute load
  stays under about 12, none while the 1-minute load is over 20 or a release CI job runs on this Mac; at most 7).
- **The lock decides the split, not the head count.** Debrief's plan puts its mix at about 0.29 slot-days per agent-day and
  the two slots at about 5.5 agent-days a day at the 80 % a lock queue reaches (`debrief-1-2.md:664-666`). Pitwall's packages
  are mostly Rust in one app workspace, about a third of their time in a heavy step: 3 agents × 0.85 × 0.33 ≈ 0.84 of a slot.
  Debrief at 4 agents: 4 × 0.85 × 0.29 ≈ 0.99. Together about **1.83 of 2 slots, above the about 1.6 slots a queue reaches
  in practice**, so 3 + 4 saturates the lock: the total stays about 5.5 agent-days a day, and a seventh builder only waits.
  In practice Pitwall's 3 give about 2.55 a day and Debrief gets about 2.95 instead of 5.5, so Debrief at 4 builders gains
  little over 3.5. Reviewers, designs and specs against the mock do not take the lock.

## 6. Calendar

| Milestone | Estimate |
|---|---|
| PoC internal build (section 2) | S+8 to S+12: **about 2026-11-19 to 2026-11-25** (with 3 builders before T) |
| Base on `main` (L1) | about T+1 to T+3: about 2026-11-24 to 2026-11-26 |
| Pitwall 1.1.0 tag, 3 agents (73.5–119.5 days at 2.55 a day: 29–47 working days) | **about 2026-12-18 to 2027-01-13**, middle about 2026-12-31; never earlier than the floor of section 2 |
| Same with 4 agents (3.4 a day; the lock allows it only if Debrief drops to about 3) | about 2026-12-09 to 2026-12-29 |
| Same with 2 agents (1.7 a day) | about 2027-01-08 to 2027-02-16 |
| Effect on Debrief 1.2 | Pitwall's 73.5–119.5 days come out of the same lock capacity: at Debrief's 5.5 a day its end (about 2027-04-21, `debrief-1-2.md:777`) moves about **13 to 22 working days**, to about mid-May 2027, plus Q3's 1.5 to 4.5 days |

Holidays are not counted; the end of December will move these dates.

## 7. Release gates

- CI green on the release commit: Pitwall's leg on all three OS, the crates and UI the plan reaches, conformance (C58
  copies, C59 updater install mode, C99 once ADR 0057 lands), ER-8's full suite once on the combined tree.
- **Rule 54:** before L1 and before every later landing on Pitwall's release path, the dispatches of section 4 step 4 (from
  `--explain`, the demos included), with gates on and the bundle built.
- **Rule 25:** `cargo deny check advisories`, one CI build with bundles, the icon check, signing secrets stripped of
  whitespace, release notes with the First launch section, the 1.0.x reinstall by hand (`CHANGELOG.md:5-9`) and ADR 0057's
  delete command for the old Keychain item.
- **Rule 23:** `codesign --verify --deep --strict` on the .app and the DMG; `pitwall-server` started from the built app.
- **ADR 0052:** every 1.1 feature has its wired, outcome and layout rows (B9).
- **Rule 55:** no `pitwall-v1.1.0` entry left in `docs/known-copies.toml` (B8; `copies --tag pitwall-v1.1.0`). **Waivers:**
  none of Pitwall's left (B7).
- **B6 and B6b are a gate:** no Windows build ships whose LocalSystem service runs a program the user can write (B6: the install verb refuses one; B6b: the installer never puts one there).
- **B6b lands before any 1.1 release claim.** Until it does, the installer's PREINSTALL hook and the app's elevated Settings > Service path still run `"$INSTDIR\pitwall-server.exe" uninstall` before any folder check (B6 security review P2-4, 2026-11-08); B6 only refuses at `install`.
- **Rule 22 / ER-23:** independent reviews of the screenshots and diagrams (B10), of the host's security (ADR 0023 WP-11),
  of ADR 0023 and this plan, and of each landing's code (ER-25).
- **ER-15, ER-16:** the tag from CI, on a commit that is not `[skip ci]`; ER-31 if the tag is made on a release branch.
- **The VM round (B12), rule 71 / ER-33.** Each row's precondition is verified before the probe runs, and the probe first
  passes its positive control on the same VM; a row without both reports `not measured` or `check broken`, and the release
  does not pass. The secrets row is ADR 0057 §6's (its WP-8), run for Pitwall's admin key and pasted key.

| Row | Precondition, and how it is verified | Probe | Positive control (must be seen first) |
|---|---|---|---|
| Install on macOS 26, Windows 11, Ubuntu (deb, AppImage) | clean snapshot restored (its id logged); installer's hash equals the release build's | `vm-smoke.json`'s process roles (app, `pitwall-server`), by pid and start time | a known-running process (the VM's own login shell) is seen by the same probe; a stopped server is reported missing |
| **Per-machine install (Windows)** | per-machine installer from the release build; a standard user account exists | the program is under `%ProgramFiles%`, its ACL denies the standard user write, and the service's `ImagePath` points there | a per-user install of 1.0.3 is flagged by the same probe (writable by the user, `ImagePath` under the profile) |
| Windows service, first real run | the per-machine row passed; UAC answered | `sc.exe query Pitwall`; the server's pid in the logged-on user's session | a service stopped on purpose reads stopped |
| Service and **Repair on macOS 26** | the registration broken on purpose, confirmed by `launchctl print` | `launchctl print` state before and after Repair | the probe reads a known-good registration as healthy, here and on Dyno's VM |
| Linux systemd user unit | lingering on, then off, confirmed by `loginctl` | `systemctl --user status` | a unit removed on purpose reads absent |
| Update 1.0.3 → 1.1.0 by hand; 1.1.0 → a test feed in the app; the service survives (rule 24) | a 1.0.3 install with settings, a key, a model, confirmed by listing them | the kept files and settings compared; the service re-registered from the per-machine path on Windows | a fresh install shows none of them |
| Dialogs as OS windows | the window-list probe excludes the app's own buttons (the Dyno 1.1.0 lesson) | OS window list per dialog | a window opened on purpose is counted and a dialog left closed is not |
| Quit with and without a window, Mod+Q | the app's processes listed by pid and start time | the same pids gone, or kept, as the quit question says | a known-running process is still seen by the probe after the quit |
| Long jobs | a stall injected in the harness, confirmed in the job log | phases, "Still working" after the stall, Cancel in every phase | a job that is not stalled shows its phases and no "Still working" |
| Layout at 900, 1024, 1280 in real windows | window sizes confirmed by the OS | the layout spec on the VM | a page with a known overflow, planted for the run, is flagged |
| Chat with a tiny tools model; an image with a tiny vision model | `PITWALL_LLAMA_E2E=1`; the models' hashes checked | the answer and the tool card in the transcript | a known tool card from a scripted fake turn is seen first; a model without tools shows the disabled switch |
| **Dyno connected on each OS** | Dyno 1.1.0 installed, AI access on, a key issued, the discovery file's pid alive | the call in Dyno's audit log | a call through `dyno mcp` appears the same way first; Dyno quit then reads "Dyno isn't open" |
| Microphone | a device the probe has heard a sound on (a played tone recorded and detected) | a transcript of an invented clip; denial shows the Privacy pane | the tone is detected before the clip |
| Secrets across an update (ADR 0057 §6) | build A installed, its admin key and a pasted key created | build B installed and opened: the first window within the row's bound and no SecurityAgent window | reading an item that another build created shows the dialog, found in the window list; otherwise `check broken` |
| Report a problem | a planted marker in a chat, confirmed on disk | the bundle has its sections and no marker, no `pw_` or `pmk_` | a bundle made with the exclusion switched off in a test build contains the marker |

## 8. Risks

- **T slips:** branch work continues and the PoC is unaffected; L1 and every shared landing wait, and the tag moves day for
  day once T is later than about 2026-12-10 (section 2's floor).
- **ADR 0056 is Proposed (revision 4)**; if its review changes `Manager`, B1 is redone in part.
- **ADR 0057 is Proposed** with D1 to D5 open; if D4 is no, B5 shrinks to the pasted key and the macOS admin-key copy.
- **Windows has never run for real** (`windows.md:6`): B12 may find more than 5–8 days of work.
- **Small models and tools:** native tool calls on small local models may be unreliable; the PoC's model choice comes from
  WP-6's capability check.
- **Speech:** WP-6 may choose the helper (+3–4 days to WP-7).
- **Build lock:** the two releases saturate the two slots (section 5); Debrief keeps the night window.
- **Rule 1:** three releases overlap by the user's decision; the integrator keeps one landing at a time.

## 9. Questions for the user

Decided (2026-11-07): **Q1** an owner-only file on macOS for the pasted key; **Q2** 3 Pitwall builders after T.

Open:

- **Q3. Builders before T.** Recommended: Pitwall 3 from Monday 2026-11-09, so the PoC is about 2026-11-19 to 2026-11-25.
  Debrief's wave 0 then defers SI-0, Z0, F1 and F3 until T (keeping L-0, X1, WP-SL1, X7 and 0.5): about 25.5 agent-days,
  about 4.5 working days later at Debrief's end. Or Pitwall 1 (the merge only): PoC about 2026-11-27 to 2026-12-01, Debrief
  about 1.5 days later.
- **Q4. ADR 0057's decisions** (Proposed, `docs/adr/0057-no-blocking-keychain-on-macos.md:420-432` at `d37b7f452`):
  - **D1** Dyno 1.1.0 lands the key-file fix before the tag (recommended). The rc already keeps the key list in
    `mcp-keys.json` (`mcp.rs:118-120`, `:147`), so this is mostly a confirmation.
  - **D2** Spotter's and Pitcrew's real tokens on macOS: (a) owner-only file (recommended) or (b) Keychain with no-UI reads
    and an "Allow access" action (about 2 days per app). Not Pitwall's.
  - **D3** Amend Debrief ADR 0059 to allow the file (recommended).
  - **D4** Accept the mechanism (one door, `for_app` as the platform default, C99) and rule 72. Pitwall's B5 follows it: with
    D4, the admin key is file-only on every OS.
  - **D5** Spend half a day testing a free Apple Development certificate.
- **Q5. Only if B8 finds the tray copy still present** (our reading says its registered items are gone): approve moving
  `tray`'s `apps/pitwall/app/src-tauri/src/tray.rs` from `pitwall-v1.1.0` to `pitwall-v1.2.0`, which rule 55 (d) allows only
  with `Copies-date: tray apps/pitwall/app/src-tauri/src/tray.rs: pitwall-v1.1.0 -> pitwall-v1.2.0: approved: user: <reason>`.
