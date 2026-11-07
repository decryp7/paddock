# Plan: Pitwall 1.1.0

- Date: 2026-11-07. Status: proposed, not yet independently reviewed. Docs only: nothing here is built or measured.
- Decision it follows (user, 2026-11-07): Pitwall 1.1 runs **alongside** Debrief 1.2, no longer after it, so the user can try
  the Dyno MCP proof of concept early: Pitwall's Playground chat using Dyno's read-only MCP tools. The scope is unchanged and
  is not split. Inside 1.1, the chat with an MCP client against Dyno's tools comes first. Shared-code changes still land
  only after the Dyno 1.1.0 tag (called **T** below).
- Inputs read (2026-11-07): `origin/pitwall/1.1-final` `b7151a57f` (the head of the 1.1 work), `origin/main` `87c9b8449`,
  `origin/dyno/1.1.0-rc` `f63a3eea2`, ADR 0023 revision 2 (this branch), the Debrief 1.2 plan
  `docs/plans/debrief-1-2.md` on `origin/docs/debrief-1-2-plan-refresh` `ac85e26c7`, `origin/docs/pitwall-rag-via-recce`
  `e075ab2b0`. A path with a line number is at the commit named for its branch.
- Estimates are agent-days, judged from similar family work, not measured. Dates count working days (Monday to Friday)
  from **S = Monday 2026-11-09** and ignore holidays.

## 0. Where 1.1 stands

| Fact | Evidence |
|---|---|
| Every 1.1 branch is in `1.1-final`: `1.1-backend`, `-quit`, `-release` and the nine `1.1-wp2*` branches have no commit missing from it by patch | `git cherry origin/pitwall/1.1-final origin/pitwall/<branch>`: 0 missing for each |
| `1.1-final` is 195 commits ahead of `main` and 41 behind; merging `main` into it conflicts in one file (`apps/pitwall/docs/releasing.md`) | `git rev-list --count`; `git merge-tree --write-tree origin/pitwall/1.1-final origin/main` |
| Merging the Dyno rc into it conflicts in **28 files**: `crates/paddock-service` 13, `paddock-proc` 2, `paddock-shell` 2, `paddock-updater` 2, `ui/projects/paddock-ui` 5, `ui/e2e` 2, `conformance/src/checks/index.mjs`, `docs/known-copies.toml`. The rc grew `paddock-service` by about 27,000 lines (ADR 0056) | `git merge-tree --write-tree origin/pitwall/1.1-final f63a3eea2`; `git diff --shortstat` from the merge base `f8dcd4440` |
| No 1.1 commit is in the rc's `apps/pitwall` | `git log f8dcd4440..f63a3eea2 -- apps/pitwall` is empty |
| **Shell move: done on the branch.** The app runs on `paddock-shell`; four dialogs and every confirmation (`pd.confirm`) are OS windows; About, Updates, What's new from the shared updater | `apps/pitwall/docs/roadmap.md:131-133`, `:141-144`; `cbdc0ba38`, `2cf50f603` |
| **Material: done.** Every control Material or paddock-ui, checked by `scripts/ci/controls.test.mjs`; every table `mat-table` | `roadmap.md:134-135`, `:138-139`; `fa1e2bc95`, `06e48454c` |
| **Long jobs: done.** Model load, benchmark, update check, service install and removal, Repair and the app update are jobs with phases, Cancel, "Stop waiting" and "Still working"; no UI timeout | `roadmap.md:106-119`; `eb55c06c0`, `c931418ca`. Left: the Playground stream opens with a 30 s limit (`app/src-tauri/src/commands.rs:387`), fixed by ADR 0023 WP-1 |
| **Repair fix: done for the old service API, to be redone.** `b7151a57f` moved macOS onto `paddock_service::Manager` and deleted `service_repair.rs`; ADR 0056 on the rc redesigns that Manager, and asks Pitwall and Pitcrew to adopt it | `b7151a57f`; `docs/adr/0056-service-repair-one-declared-state-machine.md:314-321` at `f63a3eea2` |
| **Layout: done in the browser.** No layout problem at 900, 1024 and 1280 px in Chromium and WebKit | `roadmap.md:129-130`; plan `docs/plans/1.1-wp2.md:133-139` |
| **Release machinery: partly done.** Version 1.1.0 and the CHANGELOG with the one-time reinstall note (`7e691f7c8`); third-party notices checked in CI (`4d373b67b`); `pitwall-release.yml` publishes only on a `pitwall-v*` tag (`.gitea/workflows/pitwall-release.yml:20-21`, `:109`). The CHANGELOG is dated 2026-10-30 and still says "Offline mode now stops the app update check" (`CHANGELOG.md:3`, `:18`), though `1bf262297` removed Offline mode |
| Left on the branch: Mod+Q with no window, the Settings footer wording, the tray popover not opened by the native tray, the C4 diagrams, `.changelog()`; the independent screenshot review "still to come" | `roadmap.md:145-149`; the pending-route follow-up `docs/plans/1.1-wp2.md:245-248` |
| Windows has never run on a real machine | `apps/pitwall/docs/windows.md:6` |
| The six `pitwall-v1.1.0` entries in the copies register: five of their files are already deleted on the branch (`window.rs`, `updater.rs`, `native-feel.ts`, `sparkline.ts`, the `lib.rs` launch plan was replaced); `tray.rs` remains | `docs/known-copies.toml:289`, `:317`, `:367`, `:534`, `:685`, `:733` on `main`; `git cat-file -e` on `1.1-final` |
| Six Pitwall waivers expire 2026-12-31 (C37 once, C39 three times, C53 twice); the user allows no extension | `paddock.waivers.toml:31`, `:74`, `:80`, `:86`, `:214`, `:220` on `1.1-final` |
| Pitwall keeps its own admin-key copy in the OS credential store through `keyring` | `app/src-tauri/Cargo.toml:60-62`, `src/credentials.rs:44-45` on `1.1-final` |
| Dyno 1.1.0 serves MCP on loopback with 10 read tools and `listChanged: false`; `bridge::run` holds the discovery check and handshake | ADR 0023 revision 2, Context and Decision 7 |
| Recce (knowledge collections) is not in 1.1: it waits for Recce's MCP server | `apps/pitwall/docs/roadmap.md` on `e075ab2b0` |

## 1. The base list as packages

Everything in 1.1 that is not the chat. Revision 1 of ADR 0023 quoted "base 12.5–20" from a draft readiness report that was
never committed; that figure left out the merge, ADR 0056's re-adoption (which did not exist yet), the Keychain finding,
per-machine install, the waivers and the VM round. This list replaces it.

| # | Package | Where | Role | Days | Needs |
|---|---|---|---|---|---|
| B0 | **Merge the Dyno rc into the 1.1 work now**, on `pitwall/1.1-merge-rc` (section 4): 28 conflicted files, generated files regenerated, a stale-revert check against both parents, the affected tests of the crates touched and of Dyno and Pitwall | shared | implementer, designer for `paddock-service` | 3–5 | |
| B0b | Re-merge after T: the rc's last changes and `main` (one known conflict, `releasing.md`) | shared | implementer | 0.5–1 | T, B0 |
| B1 | Service on ADR 0056's `Manager` on macOS, Windows and Linux: backend, service-only answer, `with_program`, data folder, `tracker()` and hold messages, the wiring contract on the simulator; Windows' elevated child and per-machine records in their own state file; the macOS 26 Repair test kept failing-first; `Copies-waived:` trailers as ADR 0056 says | app | designer | 2.5–4 | B0 |
| B2 | Offline remnants: the updater's Offline gate (`e52d0581f`, `00bd3e436`) reconciled with the family's "no Offline mode" (2026-11-03) in the merge; the CHANGELOG line and date corrected | shared + app | implementer | 0.5–1 | B0 |
| B3 | Shell left-overs: Mod+Q with no window, the Settings footer for a page with its own save bar (a paddock-ui gap), the native tray opening `#/tray`, the pending-route queue as the shared change `1.1-wp2.md:245-248` describes (Dyno adopts it too), `.changelog()`, the C4 diagrams | shared + app | designer | 1.5–3 | B0b |
| B4 | Report a problem on the shell's `paddock-diag`: Pitwall's sections, `pw_` and `pmk_` redaction, chats and attachments left out (test with a planted marker); found "nothing wired" in the 2026-11-05 bundle audit | app | implementer | 2–3 | B0 |
| B5 | Secrets on macOS (open question Q1): Pitwall's admin-key copy and the pasted MCP key in an owner-only file behind the `paddock-secrets` port; the OS store stays on Windows and Linux; a VM row proves no block after an update | shared (`paddock-secrets`) + app | implementer | 1–2 | Q1 |
| B6 | Per-machine install on Windows on the family mechanism (ADR 0053, Dyno first; user 2026-11-04: all Windows apps). Pitwall's service runs as LocalSystem (`windows.md:4`), and the NSIS bundle declares no per-machine mode (`tauri.conf.json:35-50`), so by reading the service's program lies in a per-user folder; confirm, then move | app | implementer | 1.5–3 | B0b |
| B7 | The six waivers removed before 2026-12-31: C37 (integration tests into `tests/it`), C39 (three freed-port tests to a refusing connector or `HeldPort::reset`), C53 (two kills through `paddock_proc`) | app | implementer | 1.5–2.5 | B0 |
| B8 | Copies register: the six `pitwall-v1.1.0` entries closed (`Copies-removed:`), `tray.rs` onto the shared tray or its entry kept with a reason; C58 green | app + register | implementer | 0.5–1.5 | B0 |
| B9 | ADR 0052 checks: `features.toml` rows (wired, visible outcome, real layout) for every 1.1 feature, the chat's included | app | implementer | 1.5–2.5 | 12 of ADR 0023 for the chat rows |
| B10 | Independent UI review on the merged base (ER-23, rule 22): screenshots at 1440x900, both themes, every route and dialog, the chat included; P1 and P2 fixed | app | reviewer, designer for fixes | 1.5–2.5 | B3, 4b |
| B11 | Release machinery: CHANGELOG and release notes (First launch, the 1.0.x reinstall by hand), rule 25's pre-tag checks, the rule 54 dispatches, the tag | app | implementer | 1–2 | all |
| B12 | **VM round** with rule 71 rows (section 8), on macOS 26, Windows 11 and Ubuntu; Windows is its first real run | app | implementer, designer to judge | 5–8 | everything but B11 |
| | **Base total** | | | **23.5–41** | |

The chat's packages are ADR 0023 revision 2's table: **30.5–48.5**, of which the PoC part (WP-1, 2a, 3a, 4a, 8, 9a, P) is
11–17. Independent code reviews of the landings: **3–5**.

**Total: 57–94.5 raw, 68.5–113.5 with 20 %.** Revision 1's whole figure was 43.5–70.

## 2. Order and critical path

### Phase A: from S to the PoC (before T if T is late; nothing lands on `main`)

| Agent | Day 0 | Then |
|---|---|---|
| 1 (implementer, designer for the service part) | B0 merge | B4, B7 (app-only, on the merged branch) |
| 2 (implementer) | ADR 0023 WP-8 on a branch from the rc (`paddock-mcp` only) | WP-1 on a branch from `1.1-final` (app-only, so it merges cleanly), then WP-3a |
| 3 (designer) | WP-2a on a branch from the rc (`paddock-assistant` only, unchanged on the rc since the merge base) | WP-4a against the mock, then WP-9a |
| night window | WP-6 (speech spike), benchmarks only, 01:00 to 06:00 | |

The PoC branch `pitwall/1.1-chat` starts from `pitwall/1.1-merge-rc` once B0 is done, and merges the WP-8 and WP-2a branches
(merges, not rebases), and the WP-1 branch. Later work goes straight onto it.

**PoC path:** B0 (3–5) → WP-9a (2.5–3.5) → the PoC build P (0.5–1), plus 0.5–1 to merge the shared branches into the PoC
branch: **6.5–10.5 working days**. WP-8, WP-2a, WP-1, WP-3a and WP-4a run beside it and have float. **The internal build
the user can try: about 2026-11-18 to 2026-11-24**, a local build per ER-15 (never published), with the chat switched on at
build time (ADR 0049), against Dyno 1.1.0 or, before T, an rc build of Dyno. It shows: connect to Dyno with a pasted key,
pick tools, ask, approve the first call of each tool, see every call in the chat, Stop in every phase. It does not yet
save chats, attach images or take speech. The PoC needs a starter model with the `tools` capability; WP-6 checks the
capability test against the catalog.

### Phase B: from T, the base lands

1. B0b (0.5–1), then **L1: land the 1.1 base on `main`** after the rule 54 dispatches (section 4, step 4). The chat code
   rides along behind its build switch, off.
2. Then the shared chat crates (WP-8, WP-2a) land, then B3's shared part, B5 if Q1 is yes. One shared landing at a time,
   through the integrator.
3. App-only base work in parallel: B1, B2, B4, B6, B7 (by **2026-12-31** at the latest), B8.

### Phase C: the rest of the chat

WP-2b → WP-3b → WP-4b → WP-5 and WP-7 (WP-7 also needs WP-6's result and the user's look at it), WP-9b → WP-10 (cuttable)
→ WP-11. WP-2b lands its shared part early (before Debrief's assistant work, section 3).

### Phase D: release

B9, ADR 0023 WP-12 and B10 (reviews in parallel with the last fixes), the full suite once on the combined tree (ER-8),
then B12 (the VM round), B11, the tag from CI.

**Critical path to the tag** (one agent per package): B0 (3–5) → WP-4b (2.5–4; WP-1, WP-4a and WP-2b are done by then) →
WP-7 (4–6.5; WP-6 done at night) → WP-12 (2–3) → B12 (5–8) → B11 (1–2) is about 17.5–28.5 working days, with B10, WP-5, WP-9b, WP-11 and the base packages beside it. That is below the capacity bound
(27–44.5 working days at 3 agents, section 6), so **capacity sets the end**. A late T
moves only what lands after it: the tag is no earlier than about T + 12 working days (B0b, L1, the remaining shared
landings, B12, B11).

## 3. Shared code, and how it is ordered against Debrief 1.2

Both releases now change shared crates in the same weeks. The rules that keep them apart: one shared landing at a time
through the integrator; a landing that changes an API updates every app that uses it in the same change (AGENTS.md, rules 39,
42, 43); whoever lands second merges `main` first. Debrief's wave times are from its plan (`docs/plans/debrief-1-2.md`
§9.4 at `ac85e26c7`).

| Shared code | Pitwall's packages | Debrief 1.2's packages | Order |
|---|---|---|---|
| `paddock-service` | B0 (merge), B1 (adoption, app side) | none | Dyno owns the crate until T; Pitwall's merge resolution lands in L1. Pitcrew 1.1 comes later |
| `paddock-proc` | B0 (two conflicted files) | 2.2 (`paddock-proc`, `paddock-metrics`, wave 1, `debrief-1-2.md:122`), P1 | **L1 first** (about T+1 to T+2); 2.2 merges `main` after it |
| `paddock-shell`, `paddock-window` | B0 (two files), B3 (pending-route queue, menu Quit) | M2a, M2c (shell services, `debrief-1-2.md:123-124`), M4 (Rust side, `debrief-1-2.md:168`), waves 3 (T+10) to 6 | L1 and B3 by about T+8, before M2a starts |
| `paddock-updater`, `ui/.../shell/about` | B0 (seven files), B2 | M2c (About, Updates) | L1 first; M2c builds on it |
| `paddock-mcp` | WP-8 (client, `connect_loopback`), WP-10 (`spawn_stdio`) | none | Additive, after T. Dyno's 1.1.x fixes to the bridge go through the same integrator; WP-8 makes `bridge::run` use the new client, so Dyno's tests run on it |
| `paddock-assistant`, `ui/.../workbench/assistant` | WP-2a, WP-2b, WP-4a, WP-4b | 2.7 (assistant turns adopt list identity, wave 5, T+20 to T+30, `debrief-1-2.md:145`), M3b (assistant pane, wave 6, `debrief-1-2.md:156`) | **Pitwall first**: WP-2a by about T+3, WP-2b and the component half of WP-4b by about T+15; Debrief's 2.7 and M3b build on the new API |
| `paddock-dialogs`, `ui/.../shell/dialogs` | none new (the MCP key dialog uses today's API) | 1.2 rest (`DialogFrame`, `debrief-1-2.md:112`), M2b (wave 4, T+15 to T+20, `debrief-1-2.md:133`) | Debrief's landing updates Pitwall's four dialogs, `pd.confirm` and the key dialog in the same change; L1 and WP-9a should be on `main` before M2b. If 1.2's small rest lands before L1, L1 merges it and adapts Pitwall's dialogs |
| `paddock-jobs` | none (consumer) | 2.6c (`paddock-jobs::patience`, waves 1-2, `debrief-1-2.md:126`), J1 (idempotency, `debrief-1-2.md:227`), 1.4's variants | Debrief's changes update Pitwall's job call sites; L1 first so they are on `main` to update |
| `paddock-limits` (new) | none in 1.1: the chat's limits are typed errors in `pitwall-core` | 1.4 creates it (wave 1, `debrief-1-2.md:114`) | Pitwall adopts it after 1.1, when its first version has settled; recorded as a roadmap item |
| `paddock-secrets` | B5 (owner-only file on macOS) if Q1 is yes | Debrief's AI keys are a later user of the same answer | Pitwall first, as the earlier release; one mechanism for the family |
| `paddock-ui` (tokens, lists, controls) | B3 (settings footer gap) | 1.3 list identity, the virtual primitive, 2.7 | Small and early on Pitwall's side; Debrief's later changes update Pitwall's users |

So: Pitwall's shared landings are front-loaded into about T+1 to T+15 (L1, WP-8, WP-2a, B3, B5, WP-2b), which is where
Debrief's waves 1 and 2 mostly land crates Pitwall only consumes (`paddock-limits`, `paddock-proc`, `paddock-jobs`). The two
real collisions, the assistant and the dialogs, are ordered Pitwall first, Debrief after.

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
   - Check the merge against each parent for silent reverts (the 2026-11-05 lesson: diff the result against each first
     parent's side). Run the affected tests of the touched crates and of Dyno and Pitwall (the merged crates are what Dyno
     ships too), the service code on Linux in a container (ER-9), `cargo xwin check` for Windows; heavy steps through the
     build lock, never in the night window.
2. **Until T:** re-merge the rc tip whenever it moves (small: it takes only critical fixes now).
3. **At T:** Dyno's landing brings the rc onto `main`. Then merge `origin/main` into `pitwall/1.1-merge-rc` (B0b; today's
   only conflict with `main` is `apps/pitwall/docs/releasing.md`).
4. **L1:** dispatch `pitwall-release.yml` with gates on and the bundle built (rule 54), and `dyno-release.yml` too, since the
   landing changes shared crates Dyno imports (rule 54's release path includes "the app's imports of shared packages"); then
   merge into `main` (no squash), CI on `main` once.
5. `pitwall/1.1-chat` merges `main` after L1 and from then on lands in small merges behind its switch (ADR 0049).

Allowance: B0 3–5 days, B0b 0.5–1; it is the PoC path's first step, which is why it starts now.

## 5. Agents beside Debrief

- **Before T** (Dyno's release first, ER-1; Debrief uses 1 to 2): Pitwall **3 builders** (section 2's agents 1 to 3), so 4
  to 5 in all. Each spawn follows the load rule: start one only while the 5-minute load stays under about 12, none while the
  1-minute load is over 20 or a release CI job runs on this Mac; at most 7 working agents.
- **After T:** the 2026-11-06 decision gave Debrief the whole machine (5.5 agent-days a working day at about 6.5 agents,
  `debrief-1-2.md:666`). With Pitwall alongside, the proposal is **Pitwall 3, Debrief 3.5 to 4** (open question Q2). The
  two build-lock slots are the shared bound: Pitwall's packages are mostly Rust in one app workspace, so it takes about a third of one slot
  per agent; reviewers, designs and specs against the mock do not count.

## 6. Calendar

| Milestone | Estimate |
|---|---|
| PoC internal build (section 2) | S+7 to S+11: **about 2026-11-18 to 2026-11-24** |
| Base on `main` (L1) | about T+1 to T+3 |
| Pitwall 1.1.0 tag, 3 agents (68.5–113.5 days at 3 × 0.85 = 2.55 a day: 27–44.5 working days) | **about 2026-12-16 to 2027-01-11**, middle about 2026-12-29; never earlier than T+12 |
| Same with 4 agents (3.4 a day) | about 2026-12-08 to 2026-12-25 |
| Same with 2 agents (1.7 a day) | about 2027-01-05 to 2027-02-10 |
| Effect on Debrief 1.2 | Pitwall's 68.5–113.5 days come out of the same capacity: at Debrief's 5.5 a day, its end (about 2027-04-21, `debrief-1-2.md:777`) moves about 12 to 21 working days, to about mid-May 2027 |

Holidays are not counted; the end of December will move these dates.

## 7. Release gates

- CI green on the release commit: Pitwall's leg on all three OS, the crates and UI the plan reaches, conformance (C58
  copies, C59 updater install mode), ER-8's full suite once on the combined tree.
- **Rule 54:** `pitwall-release.yml` dispatched with gates on and the bundle built before L1 and before every later landing
  on Pitwall's release path; the same for every other app a shared landing reaches.
- **Rule 25:** `cargo deny check advisories`, one CI build with bundles, the icon check, signing secrets stripped of
  whitespace, release notes with the First launch section and the 1.0.x reinstall by hand (`CHANGELOG.md:5-9`).
- **Rule 23:** `codesign --verify --deep --strict` on the .app and the DMG; `pitwall-server` started from the built app.
- **ADR 0052:** every 1.1 feature has its wired, outcome and layout rows (B9).
- **Rule 55:** no `pitwall-v1.1.0` entry left in `docs/known-copies.toml` (B8). **Waivers:** none of Pitwall's left (B7).
- **Rule 22 / ER-23:** independent reviews of the screenshots and diagrams (B10), of the host's security (ADR 0023 WP-11),
  of ADR 0023 and this plan, and of each landing's code (ER-25).
- **ER-15, ER-16:** the tag from CI, on a commit that is not `[skip ci]`; ER-31 if the tag is made on a release branch.
- **The VM round (B12), rule 71 / ER-33:** each row states its precondition and the probe's positive control; a row whose
  precondition was not verified reports `not measured` or `check broken`, and the release does not pass.

| Row | Precondition verified | Probe, and its positive control |
|---|---|---|
| Install on macOS 26, Windows 11, Ubuntu (deb, AppImage) | clean VM snapshot, installer from the release build | `vm-smoke.json`'s process roles (app, `pitwall-server`); control: a run with the server stopped reports it missing |
| Service and **Repair on macOS 26** | registration broken on purpose | `launchctl print` state before and after; control: the same probe on a known-good registration and on Dyno's VM |
| Windows service, first real run | per-machine install done (B6), UAC answered | `sc.exe query Pitwall`, server in the user's session; control: service stopped reads stopped |
| Linux systemd user unit | lingering on and off | `systemctl --user status`; control: unit removed reads absent |
| Update 1.0.3 → 1.1.0 by hand; 1.1.0 → a test feed in the app; the service survives (rule 24) | a 1.0.3 install with settings, keys, models | the kept files and settings compared; control: a fresh install shows none |
| Dialogs as OS windows | a window list probe that does not count the app's own buttons (the Dyno 1.1.0 lesson) | OS window list per dialog; control: a dialog left closed is not counted |
| Quit with and without a window, Mod+Q | | processes gone or kept as the quit question says |
| Long jobs | a stall injected in the harness | phases, "Still working", Cancel in every phase |
| Layout at 900, 1024, 1280 in real windows | | the layout spec on the VM |
| Chat with a tiny tools model; an image with a tiny vision model | `PITWALL_LLAMA_E2E=1`, models present | the answer and the tool card; control: a model without tools shows the disabled switch |
| **Dyno connected on each OS** | Dyno 1.1.0 installed, AI access on, a key issued | the call in Dyno's audit log; control: a call through `dyno mcp` appears the same way; Dyno quit reads "Dyno isn't open" |
| Microphone | a device the probe has heard a sound on | a transcript of an invented clip; denial shows the Privacy pane |
| macOS update with a stored key (Q1) | a key stored by the previous build | the window appears after the update; control: the Keychain build blocks, as Dyno measured |
| Report a problem | a planted marker in a chat | the bundle has the sections and no marker, no `pw_` or `pmk_` |

## 8. Risks

- **T slips:** branch work continues and the PoC is unaffected; L1 and every shared landing wait, and the tag moves day for
  day after about T+12.
- **ADR 0056 is Proposed (revision 4)**; if its review changes `Manager`, B1 is redone in part.
- **Windows has never run for real** (`windows.md:6`): B12 may find more than 5–8 days of work.
- **Small models and tools:** native tool calls on small local models may be unreliable; the PoC's model choice comes from
  WP-6's capability check.
- **Speech:** WP-6 may choose the helper (+3–4 days to WP-7).
- **Build lock:** two releases share two slots; Debrief's benchmarks keep the night window.
- **ER-1:** two releases are in progress at once by the user's decision; the integrator keeps one landing at a time.

## 9. Open questions for the user

- **Q1. Where Pitwall keeps secrets on macOS.** Pitwall must store the pasted Dyno key itself, and its admin-key copy is
  already in the Keychain. On ad-hoc builds a Keychain item blocks the next build (Dyno measured it and moved to a file).
  Recommended: an owner-only file on macOS behind the `paddock-secrets` port, the OS store on Windows and Linux (B5, 1–2
  days). The alternative, Keychain with an "Allow" step after each update, blocks or prompts after every update.
- **Q2. Agents after the Dyno tag.** Recommended: Pitwall 3, Debrief 3.5 to 4: Pitwall tags about 2026-12-16 to
  2027-01-11, Debrief's end moves about 12 to 21 working days (about mid-May 2027). With Pitwall at 2, Pitwall tags about
  2027-01-05 to 2027-02-10 and Debrief moves less. The PoC date does not depend on this.
