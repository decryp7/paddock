# Plan: every remaining app ready to start when its turn comes (2026-11-05)

Status: plan, docs only. Nothing here was built or run. Read at `origin/main` `0e667eca7` and the branch heads named below,
all fetched 2026-11-05.

How to read the evidence:

- A citation `branch` `sha` `path:line` was read at that ref. "On main" means `origin/main`.
- **Note only** means it comes from the coordinator's notes or an out-of-repo hand-off file and could not be checked in git.
- Commit counts against main overstate unmerged work, because landings are squashed ("land X as one commit"). Ancestry says
  little here, so shared-code numbers below come from tree diffs. They are file counts, not conflict counts.
- `dyno/wp-int` `42bcc7e07` is Dyno 1.1.0's integration head (2 behind main, 309 ahead; it holds
  `feat/per-machine-app-side`, `feat/dyno-mcp-listener` and `feat/dyno-mcp-readings`). "The Dyno tag" means `dyno-v1.1.0`; it does
  not exist yet (`git tag`: latest is `dyno-v1.0.0`).

## 1. Overview

Release order (user, 2026-11-04): Dyno 1.1.0, Spotter 1.0, Pitwall 1.1, Pitcrew 1.1, Debrief 1.2, Pacenotes and Sighter. Recce
follows them (its plan's D14, open). Scrutineer's plan places its work beside Spotter 1.0 (its D13, open). Slot: one agent at a
time on other apps, heavy lane `fix`, paused under load (user, 2026-11-05).

| App | Code today | Design today | Next step | Estimate (agent-days) |
|---|---|---|---|---|
| **Spotter 1.0** | Feature-complete on `spotter/1.0-prep`, not on main | Done | ADR 0052's 5 pre-tag days; rebase on Dyno; per-machine after Dyno's VM pass | 9 to 10 working days after the Dyno tag |
| **Pitwall 1.1** | Base on `pitwall/1.1-final`; chat not started | ADR 0023 Proposed, unreviewed, decisions not written in | Write the decisions in, independent review, put the base's work list in the repo | 43.5 to 70 (chat part 28 to 45 from the ADR) |
| **Pitcrew 1.1** | Shell and core on `steward/pitcrew-1.1-prep`; Environment service and UI, auto-scale, headless not started | ADR 0022 Accepted, ADR 0021 Proposed, headless ADR missing, plan outside the repo | Headless ADR, ADR 0021 spike and second review, plan into the repo | About 47 to 53 plus family work; the 30 to 45 in the notes does not add up (section 4) |
| **Debrief 1.2** | Tracks A and B on branches; shared tracks not started | Plan with 65 packages; predates ADRs 0052, 0053, 0054 | Amend the plan; land the fuzz fix; keep tracks A and B going | 190 to 197 plus unplanned family work |
| **Pacenotes** | Stage 1 crates on `steward/pacenotes-stage1`, not on main | Plan and ADRs 0001 to 0012; five ADRs of its plan unwritten | Land stage 1; record the answered questions | 300 to 350, re-estimated after stage 1 |
| **Sighter** | Separate unlanded branches; main has docs only | ADR 0001 Accepted; plan contradicts a later decision | Land the stack; revise the plan for macOS | About 210.5 |
| **Recce** | None (placeholder page on main) | Draft plan; no ADR | User decisions D2 to D16; spikes S1 to S6 | No firm estimate until design |
| **Scrutineer** | None (placeholder page on main) | Draft plan; no ADR | User decisions D1 to D18; S0 after the Dyno tag; ADR | No firm estimate until design |
| **Rust tooling** | `./dev` on main; many unlanded branches; `paddock-tools` not started | ADR 0051 and ADR 0022's amendment Accepted, both off main | Refresh the status table; land the docs; code on branches | ADR 0051: 52 to 71; ADR 0022: about 105 |

### Deadline nobody owns yet

`paddock.waivers.toml` on main expires C36 (`apps/*`), C37 (Dyno, Pitcrew, Pitwall, Spotter) and C39 (nine named files) on
**2026-12-31**. The user decided on 2026-10-30 not to extend them and to fix them inside Debrief 1.2, but Debrief 1.2 now tags after
Pitcrew and cannot make that date (`docs/plans/debrief-1-2.md`: "the mid-December tag is no longer a safe estimate"). The C39 sites
are in Debrief (4), Pitcrew (1), Pitwall (3) and Spotter (1). They are app-folder work. See decision F1.

ADR 0054 (Proposed) sets a second date: its lifecycle gate and C92, C93 bind other apps "at latest 2027-01-03". That falls before
Pitcrew's tag and probably Pitwall's. Revisit the date when the ADR is accepted.

### Order for design, one agent at a time

| # | Work | Size | Why here |
|---|---|---|---|
| 1 | **Spotter close-out list:** run ADR 0052 WP-S (the audit sweep) on `spotter/1.0-prep`, list findings and the tag blockers of section 2 | 1 | Next release; no design missing |
| 2 | **Pitwall:** revise ADR 0023, independent review, base work list in the repo | About 2 | Fixes the `paddock-assistant` and `paddock-mcp` shapes that Debrief, Pacenotes and Recce inherit, so it unblocks the most |
| 3 | **Family logging ADR** (only if F3 is yes): `run.*`, `EventId`, the reader | About 2 | Every app's release obligation; Spotter's and Pitwall's windows are next |
| 4 | **Scrutineer:** S0 on the local VMs, then the ADR and its review. Starts when the Dyno tag exists; skip ahead until then | 3.5 to 5.5 | Must finish before Pitwall's freeze if Pitwall 1.1 stays the first signed release |
| 5 | **Pitcrew:** headless ADR, ADR 0021 WP-0 on a fake server and second review, re-baselined plan in the repo | About 4 | Next after Pitwall; ADR 0021 is gated on nothing else |
| 6 | **Debrief:** amend the plan (section 5), review | About 1 | Plan exists; only additions |
| 7 | **Pacenotes:** the five ADRs, plan cleanup | About 3 | Its stage 2 waits for Debrief 1.2 anyway |
| 8 | **Sighter:** plan revision, sidecar and recorder state-machine ADR | About 2 | Same wait |
| 9 | **Recce:** spikes S1 to S6, then ADRs R1 to R9 | 17 to 24, then ADRs | Last in the release order; needs the D answers and the Pacenotes ADR 0012 walk |

Sizes are my judgement except rows 1 (ADR 0052) and 4 and 9 (their plans). Rust tooling needs no design slot: half a day to refresh
`rust-tooling-status` when one is free.

Coding competes for the same slot. Suggested order between design items: the C36, C37, C39 sweep (F1), landing Pacenotes stage 1,
landing the Sighter stack.

### Shared-code queue for right after the Dyno tag

Deduplicated by home (ER-22). "Order" is the release that needs it first. Nothing here lands before the tag if Dyno links the crate;
exceptions are marked.

| Home | Change | Needed by | Source | Order |
|---|---|---|---|---|
| `ui/.../paddock-e2e`, `conformance/` | `layoutProblems`, C87 and C88 report-only (absent at `dyno/wp-int` and `spotter/1.0-prep`) | Spotter, then all | ADR 0052 WP-C2, WP-B4a, WP-C1a | Spotter |
| `paddock-setup` and the NSIS include | Adoption; arrives with Dyno, absent on main | Spotter, Pitwall, Pitcrew, Debrief; Pacenotes, Sighter through the template; Scrutineer F2 | ADR 0053 "Adoption" | Spotter |
| `paddock-chromium`, `-jobs`, `-startup`, `-testkit`, `-dialogs`, `-download`, `-shell`, `ui` shell, `scripts/vm-smoke` | Spotter's own delta over Dyno | Spotter | Tree diff `steward/dyno-1.1.0..spotter/1.0-prep` | Spotter |
| `paddock-service`, `-shell`, `-build`, `-updater`, `-proc`, `-pitwall`, `-window`, `ui` shell | Pitwall's own delta | Pitwall | Tree diff against main | Pitwall |
| `paddock-assistant` | Parts, `ApprovalPolicy`, per-turn tools, native calls, store port. **Can land before the tag:** no Dyno or Spotter user (ADR 0023) | Pitwall, then Debrief and Pacenotes | ADR 0023 WP-2 | Pitwall |
| `paddock-mcp` | Client: connection, cancel, loopback, stdio; current protocol revision | Pitwall WP-8; Recce WP 4.2; Dyno's and Pacenotes' servers | ADR 0023 §6; Recce plan §5 | Pitwall |
| `paddock-log`, `-startup`, `-commands`, `-jobs`, `-proc` | `EventId`, `run.*`, `job.*`, `tool.run`, crash reports | All apps | Logging map WP-2 to WP-5 on `docs/logging-map`; ADR first | Pitwall |
| `paddock-about`, `paddock-deps` | About and one dependency inventory (ADR 0028, Accepted; crates absent) | All apps; ADR 0051 WP-12; Scrutineer F2 | [ADR 0028](../adr/0028-about-and-deps-crates.md); user, 2026-10-29: first after the tags | Pitwall |
| `paddock-sign` | One Authenticode verifier. ADR 0051 WP-6 would build it inside `paddock-tools`; Scrutineer D14 moves it. Answer D14 before WP-6 | `paddock-tools`, Scrutineer | ADR 0051 U5; Scrutineer plan §2.2 | Tooling |
| `paddock-jobs` | `Outcome`, `ReadBack`, `NextStep`, `Why`, `Stamped<T>` (**breaking**, every user in one change); J1 keys; per-thread walk hook | Pitcrew tag gate; Debrief J1; Pitcrew WP-6; `paddock-tools` imports the types | ADR 0052 WP-B1; Debrief plan J1; Pitcrew ADR 0022 WP-6 | Pitcrew |
| `paddock-proc` | setsid and job-object kill; `login_env`; spawn logging; Debrief's P1 | Pitcrew, `paddock-tools`, Debrief | Note on Pitcrew; ADR 0051 WP-14; ADR 0052 WP-B4b | Pitcrew |
| `paddock-net`, `paddock-metrics`, `paddock-health` | `probe_connect`; load and memory in `MachineProbe`; health writer | Pitcrew; TSA egress for Scrutineer | Pitcrew ADRs 0021, 0022 | Pitcrew |
| `paddock-testkit`, `conformance/`, `paddock-dev` | Contract tests, C83 to C90, C92 to C96 | All apps | ADR 0052, ADR 0054 §7.4 | Pitcrew |
| `paddock-runtime`, new `paddock-tools` | Single-file assets (ADR 0051 WP-7 **is** Sighter's SW11; build once) | Sighter, Pacenotes, Dyno, Debrief, Pitwall | ADR 0051 | Tooling |
| `paddock-repo-files` (ADR 0012 calls it `paddock-walk`) | Stat fields, cancel, progress, Windows enumeration | Pacenotes IG14, Recce WP 1.4 | ADR 0012; Recce plan §5. Name: decide once | Pacenotes |
| `paddock-watch` | `ChangeFeed` with cursors | Recce WP 1.5, Dyno ADR 0029 WP-8, Pacenotes | Recce plan §5 ("not started") | Recce |
| New `paddock-embed` | X55: BM25, RRF, Model2Vec | Debrief N7 (builds it), Pacenotes B1, Recce WP 2.3 | [shared-next](shared-next.md) X55 | Debrief |
| New `paddock-extract` | Guarded extraction, OOXML from Pacenotes | Recce WP 2.1, Pacenotes | Recce plan §3, family ADR needed | Recce |
| New `paddock-video` | From `debrief-video` | Sighter SW12, Debrief | Sighter stage-2 plan §3 | After Debrief 1.2 |
| Rules model | Plans disagree on the name: `paddock-repo-rules` (ADR 0022), `paddock-rules` (user, 2026-10-30, broader naming) | `./dev check`, Pacenotes C1 | ADR 0022 §6 | Tooling |
| Diagrams | Same: `paddock-diagrams` (on `tools/dev-diagrams-r11`) against `paddock-drawio` (Pacenotes plan §3.3); Pacenotes DG1's writer moves there | `./dev`, Pacenotes | ADR 0022 R11 | Tooling |
| `paddock-chromium` | Debrief and Spotter onto one driver; ADR first | Debrief, Spotter's next | User, 2026-10-30 (note only) | After Pitwall |
| New `paddock-pki`, `paddock-sign`, `paddock-trust-store`; `.gitea/actions/sign` | Signing mechanism | All apps from the first signed release | Scrutineer plan | Before Pitwall's freeze |
| `crates` gate, CI | Per-crate coverage; ring as the crypto provider; C58 | All | User, 2026-11-04 and 2026-10-30 (note only); rule 55 (e) | First free window |
| `scripts/new-app.mjs --in-repo` | Monorepo mode | Pacenotes, Sighter, Recce WP 1.1 | Pacenotes plan §3.3 | Pacenotes |
| Coverage readers, stack-trace parsers | Move from Pacenotes and Debrief | Pacenotes, Debrief, Sighter | Pacenotes plan §3.3 | Pacenotes |
| New `paddock-helper` | From Dyno's helper, if Recce D3 is yes | Recce WP 2.5 | Recce plan | Recce |

Docs-only landings that need no tag and that every brief cites: rule 56 (`docs/rule-logs-explain-failures`), rules 65 and 66
(`docs/rules-2026-11-04`), rule 67 with ADR 0052 (`docs/adr-wiring-outcomes-ui` `fed6b5855`). Main has rules 1 to 55 and 71 only.

### Where apps conflict in shared code

Files changed, by tree diff (Spotter and Pitcrew over `steward/dyno-1.1.0`, the others over main):

| Area | Dyno head | Spotter | Pitwall | Pitcrew |
|---|---|---|---|---|
| `paddock-ui/shell` | 33 | 12 | 19 | 10 |
| `paddock-service` | 70 | | 17 | |
| `paddock-updater` | 16 | | 6 | |
| `paddock-proc` | 20 | | 2 to 4 | |
| `paddock-shell` | 7 | 2 | 9 | |
| `paddock-dialogs` | 19 | 2 | | |
| `scripts/affected*.mjs`, `conformance/src`, `.gitea/workflows` | yes | yes | yes | yes |

- **Pitwall is the largest merge.** Its last merge from main is 2026-11-04 and it has none of Dyno's service, updater or shell
  changes. Spotter and Pitcrew merged an earlier Dyno state and must follow it to the tag.
- **`paddock-jobs` WP-B1 is breaking across every app.** ADR 0052 puts it before the Pitcrew tag. Land it between the Pitwall tag and
  Pitcrew's freeze, with Spotter, Pitwall and Pitcrew updated in the same change, and before Debrief's J1.
- **`paddock-assistant`, `paddock-mcp`:** Pitwall changes them first; Debrief's M-packages and Recce's server take what it leaves.
- **New checks:** ADR 0052 adds C83 onward under `conformance/`, ADR 0054 under `tools/paddock-dev/src/checks/`, and ADR 0022 R8 ports
  conformance to Rust. Settle one home before WP-A1.
- **`paddock-tools` and Scrutineer** both want the Authenticode verifier (D14).

### Family decisions for the user

| # | Decision | Recommendation |
|---|---|---|
| F1 | C36, C37, C39 waivers expire 2026-12-31 and Debrief 1.2 cannot make it. Sweep the sites as app-folder work in the slot before the date, or extend? | **Sweep now, app by app**; keeps your no-extension decision. Extend only C36 for parsers not done by mid-December |
| F2 | Logging map, decision 1: take Dyno's WP-1 log lines (about a day) despite the freeze? | The map recommends yes |
| F3 | Logging map, decision 4: write the ADR for `run.*`, `EventId` and the reader? | Yes |
| F4 | Logging map, decision 2: retention 30 days with a 100 MB cap, or keep 7 days? | 30 days (the map's recommendation) |
| F5 | Logging map, decision 5: Windows dumps opt-in through the installer, or names and Event Log text only? | Start without; decide after the spike |
| F6 | ADR 0022 G.7 keeps `crates/` and release-path changes out of a release window, and windows now run back to back until Debrief. When may they land? | **The days between a tag and the next app's freeze**, named by the coordinator |

## 2. Spotter 1.0

**State**

- `spotter/1.0-prep` `e98e37cf5`: version 1.0.0, on the shared shell; contains `steward/spotter-1.0-prep` `5f9204ca9` and
  `steward/spotter-1.0-final`, plus 3 commits. It merged `steward/dyno-1.1.0` (`72441f1d4`), so it carries Dyno code.
- Report a problem wired through `paddock_diag` with address redaction: `d37f2f181`, `app/src-tauri/src/diagnostics.rs:8-17`.
- Release doc: `apps/spotter/docs/releasing.md` (no SBOM for 1.0, decision 2026-11-04, line 102).
- Windows install: not yet per-machine (no `perMachine`, no hooks in `tauri.conf.json`).

**Design gaps.** None needing an ADR. ADR 0054 is still Proposed (revision 4 on `docs/adr-os-lifecycle-and-edges`).

**Tag blockers found**

- ADR 0052's pre-tag set is not built: `layoutProblems`, C87 and C88 have no hit on either head.
- `docs/known-copies.toml` on the branch still has `delete_by = "spotter-v1.0.0"` for `launch-plan`, `updater`, `native-feel`,
  `copy-menu`. The tag check refuses while they stand. Resolve or re-date each (rule 55).
- `releasing.md:134`: the Windows 11 Arm update row reads NOT RUN; under rule 71 that is `not measured`.

**First coding parts**

- App folder, now: WP-S (1 day), C37 and the C39 site `spotter-sources/tests/http_sources.rs`, known-copies items.
- Shared, after the Dyno tag: WP-C2 (2.5, `paddock-e2e`, under D7's waiver), WP-C1a (0.5), WP-B4a (1), per-machine adoption, the
  delta of section 1.

**Family obligations**

| Item | State |
|---|---|
| Diagnostic bundle P1 | Done on the branch; VM rows added in `e98e37cf5`, to run in the tag round |
| Per-machine | 2 to 2.5 days, after Dyno's VM pass (ADR 0053, O7) |
| Shell | Done |
| ADR 0052 | 5 days above; the rest after the tag |
| Rule 71 | Every VM row needs a positive control |
| ADR 0054 | Lifecycle run gates the tag (D6); C92, C93 not enforced |
| Logging | New log checks in `d37f2f181`; map WP-8 not started, not a tag blocker unless F3 says so |

**Decisions for the user.** One: ADR 0054 D6, Spotter's tag under the lifecycle gate. Coordinator default yes; user may revise.

**Estimate.** Firm: 9 to 10 working days after the Dyno tag (given to the user 2026-11-05; matches ADR 0052's 5 days, ADR 0053's 2 to
2.5 and a VM round).

## 3. Pitwall 1.1

**State**

- `pitwall/1.1-final` `b7151a57f`: version 1.1.0; on `paddock-shell`, dialogs, Material; service through
  `paddock_service::Manager`, `service_repair` deleted. CHANGELOG's 1.1.0 section is dated 2026-10-30 and needs redating.
- ADR 0023 on `docs/pitwall-1-1-chat` `9462f9589` (`apps/pitwall/docs/adr/`): Proposed, "not yet independently reviewed", 13 work
  packages.
- `docs/pitwall-rag-via-recce` `e075ab2b0`: knowledge collections wait for Recce (roadmap change, unmerged).
- Dyno's listener and read tools, which the chat connects to, are in `dyno/wp-int`.

**Design gaps**

- ADR 0023 still lists D1 to D8 as open. The user accepted D2 to D8 on 2026-11-05; D1 is settled by the listener branch. Write them
  in, then independent review.
- ADR 0023 points to a "1.1 readiness report" for the base (shell landing, Report a problem, ADRs 0052 to 0054, VM rounds). It is not
  in the repo; the 12.5 to 20 days are **note only**. Put the list in `apps/pitwall/docs/plans/`.
- WP-6, the speech test on real hardware (1 to 2 days), decides the engine and WP-7's size.

**First coding parts**

- App folder: WP-1 (wire, needs the base landed for merge, not for writing), WP-6, Report a problem, the three C39 sites.
- Shared, may land before the Dyno tag: WP-2 (`paddock-assistant`).
- Shared, after the tag: WP-8 (`paddock-mcp` client; Dyno links this crate), the base's shared delta.
- WP-3, 4, 5, 7, 9 follow WP-1 and WP-2.

**Family obligations**

| Item | State |
|---|---|
| Diagnostic bundle P1 | Not wired: `paddock_diag` has no use in `apps/pitwall` (only `Cargo.lock`). Adopt with a section and the `pw_` pattern |
| Per-machine | Already `perMachine` with own hooks; move onto the include, 2 to 3 days (ADR 0053) |
| Shell | Done on the branch, including the five dialogs |
| ADR 0052 | WP-S sweep, tour (WP-C3), adoption about 1.25 |
| ADR 0054 | A-1, 2 to 3 days |
| Rule 71 | VM rows; also the real-engine and Dyno connection tests of WP-12 |
| Logging | Map WP-8 adoption |
| Copies | `delete_by = "pitwall-v1.1.0"` on the branch: `launch-plan`, `window-manager`, `tray`, `updater`, `native-feel`, `sparkline` |
| Signing | First signed release if Scrutineer D13 holds; ADR 0023 expects microphone re-prompts per update until then |

**Decisions for the user.** None to answer now. Later: the WP-6 result ("the user sees the result", ADR 0023), and an 805 MB speech
model against a smaller engine.

**Estimate.** Chat 28 to 45 is from ADR 0023's packages (firm after review). 43.5 to 70 overall includes the note-only base.

## 4. Pitcrew 1.1

**State**

- `steward/pitcrew-1.1-prep` `40be30b04` (contains `wip/pitcrew-1.1-integration-2026-11-04`): version 1.1.0, shell, shared channels,
  Windows service through `paddock-service`, service protocol 7, cancellable registration. Environment core and presets exist
  (`pitcrew-core/src/environment/`, `packs/env-presets.toml`). Roll-back exists in `update/machine.rs`; a job object in
  `pitcrew-service/src/process.rs`.
- Not there, on this branch or the `1.1-e2`, `p6-ui-2`, `p8-material` branches: `pitcrew-service/src/environment`, a UI
  `features/environment`, any `autoscale` module. Also absent: a headless ADR and `.xz` downloads.
- Pipe security from 2026-11-03 not fixed: `pitcrew-service/src/ipc.rs:52` still grants `GRGW` to interactive users; no
  `connect_system` in `apps/pitcrew`.
- ADR 0022 Accepted. ADR 0021 Proposed, pending WP-0 and a second review. Family ADR 0025 Accepted on the branch, Proposed on main.
- The plan is out of the repo: hand-off `pitcrew-1.1-plan.md` (2026-10-31). `pitcrew/1.1-*` branches were squashed in, so I could not
  prove each one's content arrived.

**Design gaps**

- Headless ADR, Pitcrew's next free number (0023): not written. The plan's H0, 1.5 days. Accounts for LaunchDaemon and systemd modes
  are open.
- ADR 0021: WP-0 on a fake server (user chose no real token), second review.
- Plan into the repo, re-baselined.

**First coding parts**

- App folder: auto-scale planner and config (A1), Environment service core (E3 after E2's probe), Windows parts (E4), log-tail drain
  fix, Environment UI (E5), headless once designed (H1).
- Shared, after the Dyno tag: SDDL and client through `paddock-ipc` system endpoints (arrive with Dyno); `paddock-net` `probe_connect`
  and the `paddock-jobs` hook (WP-6); `paddock-metrics` probe (ADR 0021 WP-3); `paddock-health` writer and `paddock-proc` kill (P5); its
  `ui` shell and schedule delta.

**Family obligations**

| Item | State |
|---|---|
| Diagnostic bundle P1 | Own `diagnostics.rs` and `export_diagnostics`, no `paddock_diag`. Adopt; read `service.log` and runner logs from disk; include applog |
| Per-machine | Already `perMachine`; hooks onto the include, `UserRunEntry`, PM suite; 2 to 3 days. 1.0.x uninstallers lack the reinstall marker (ADR 0053) |
| Shell | Done on the branch; screenshots regenerated but marked unreviewed (`b233a9e6d`) |
| ADR 0052 | **Heaviest:** WP-B1 to B3, B5, C1b, B4b, A1 to A4, C3 are placed before this tag |
| ADR 0054 | A-1, 2 to 3 days |
| Rule 71 | Service re-registration, Repair and headless rows need positive controls |
| Logging | Map WP-8; the runner's last lines dropped on restart (user note 2026-11-03) |
| Copies | `delete_by = "pitcrew-v1.1.0"` on the branch: `launchd-job-report`, `service-registration`, `launch-at-login` |

**Decisions for the user**

1. Accept ADR 0021 after the spike and second review, or keep it Proposed in the release (D-ADR21: stays Proposed until then).
2. Headless accounts: which user runs LaunchDaemon and systemd runners. My recommendation: a dedicated service account; the ADR argues it.

**Estimate.** From the plan's packages not yet present: E3 9, E4 4.5, E5 6.5, A0 to A4 14, H0 and H1 7.5, P5 3, U1 up to 3, W1 up to 2,
R1 and R2 3: **about 47 to 53**, plus ADR 0053 and 0054 adoption (4 to 6) and the bundle. Excludes ADR 0052's family packages. The notes'
30 to 45 cannot be reproduced; firm after the re-baseline.

## 5. Debrief 1.2

**State**

- Plan on main: [debrief-1-2.md](debrief-1-2.md); section 8 (tracks A to E) only on `debrief/1.2-lean` `fdf577eb7`.
- Track A: `debrief/1.2-lean`, plus `wip/debrief-1.2-lean-2026-11-04` `eecea633f` (uncommitted work saved, not reviewed).
- Track B: `debrief/search-chain-rebase` `e6f0d1de2`. ADR 0062 Accepted, on `docs/debrief-adr-stable-event-ids`, off main.
- `fix/debrief-fuzz-2026-11-05` `d1f67d311`: 2 commits, `apps/debrief` only, 0 behind main.
- ADRs 0001 and 0003 Accepted. Of the shared crates, Debrief links only `paddock-proc` on main.

**Design gaps** (plan amendments, no new ADR)

- The plan names none of ADR 0052, 0053, 0054, `paddock-diag` or the logging map (grep). Add: bundle P1, ADR 0053 adoption, tour with
  the larger profile (ADR 0052 D6), logging adoption.
- C36 and C39 were planned inside 1.2; move per F1.
- The bounded DuckDB 2 columnar test (user, 2026-11-05) into package D-a or D-b, results in `performance.md`.
- ADR 0051's `delete_by = debrief-v1.2.0` for `browser/runtime.rs`. Calendar in §5 is stale, as the plan says.

**First coding parts**

- App folder: tracks A and B (libduckdb pin and fetch scripts only), the fuzz fix, C36 and four C39 sites.
- Shared, after Dyno, Spotter and Pitwall have landed (plan §8; stricter than the Dyno tag): tracks C and D. Prepare designs and failing
  tests on branches.

**Family obligations**

| Item | State |
|---|---|
| Diagnostic bundle P1 | On main: logs and child logs raw (`activity/report.rs:384-399`). Shared `Redactor`; screenshots off by default, per-report checkbox |
| Per-machine | Currently `currentUser`; 2.5 to 3 days with guard and notes (ADR 0053) |
| Shell | M1 to M5, in the plan |
| ADR 0052 | Adoption when it moves to the shell |
| ADR 0054 | The waiver covers Debrief's next tag, but A-1 lists only Spotter, Pitwall, Pitcrew; add a Debrief package |
| Rule 71 | Performance gates (D7) need a positive control |
| Logging | Already has `Lifecycle` (logging map) |
| Copies | 36 entries with `delete_by = debrief-v1.2.0` |

**Decisions for the user.** F1 only. D4 to D6 are decided inside packages. Ask before vm-smoke (plan §8).

**Estimate.** Firm from packages: 164, 190 to 197 with review and CI. Not counted: this section's additions (about 5 to 8, my judgement).

## 6. Pacenotes

**State**

- Main: docs, ADR 0001, `paddock.ci.json`. No crates.
- `steward/pacenotes-stage1` `c1bba3f9f`: 11 crates, ADRs 0002 to 0011, CI contract, CODEOWNERS and `scripts/affected.test.mjs` lines.
  **Not on main** although the user said to land it on 2026-11-04.
- `docs/pacenotes-fast-ingest` `9385413ff` contains that plus ADR 0012 (accepted) and plan §4.3a.
- Known holes: `pacenotes-diagram` not built; C52 waits, `scripts/check-no-cargo-metadata.mjs` guards meanwhile (plan §4.3).

**Design gaps**

- Plan §5's ADRs with no match by title: function port (in-engine first), vector index engine, requirement links and suspect model,
  code viewer and graph canvas, language-server trust.
- Plan §7.2 lists eight open questions the user answered on 2026-10-30 (note). Record them.
- Reconcile ADR 0012's IG packages with A3, A4 and C1 (the plan says at the stage-1 re-estimate).

**First coding parts**

- App folder: land stage 1, then fast ingest; DG1's crate; ADR 0012's stage-1 placement: IG0, IG-B1, IG-B6, IG3 types, pure halves of
  IG2, IG4, IG13, IG5, IG11, IG7 logic.
- Shared, not before Debrief 1.2 (user, 2026-10-29): `paddock-repo-files` IG14, X55 adoption, moves in section 1, UI.

**Family obligations.** From the first release, through the template: per-machine and shell (workbench) in its packaging WPs;
`paddock_diag` with a section; ADR 0052 tour and registers; ADR 0054 rows; IG15 is its logging package; rule 71 on its gates;
`paddock-tools` WP-15 by `pacenotes-v1.0.0` (ADR 0051), and `delete_by = pacenotes-v1.0.0` for `diagram-export-script` and
`release-publish-script`.

**Decisions for the user**

1. DG1's real round trips: you open, edit and save the generated files in draw.io desktop, web and VS Code.
2. Trademark check before stage 2 ends (light check accepted 2026-10-30); needs someone named to do it. Recommendation: an agent drafts,
   you decide.

**Estimate.** 300 to 350 from packages (plan §4.12 on `docs/pacenotes-fast-ingest`), ±50 % per package, re-estimated after stage 1.

## 7. Sighter

**State**

- Main: docs and scripts only.
- Branches, none on main, no one branch holds them all: `sighter/sw0-land` `2f8b40581`; `sw9-weights` `ef7bcc8d8` (has sw0);
  `sw3a-ports` `296a6dfae`; `sw3b-match` `0b57b05e0` and `sw3c-vnc-2` `e27c245b4` (each has 3a); `sw4-corpus` `a4905035c`;
  `sw5-ort` `cb741d1a8`; docs on `docs/sighter-sw7-sw8` `fb2803036` (stage-2 plan, ADR 0002).
- ADR 0001 Accepted. ADR 0002 Proposed; decision 4 awaits S7.
- Weights mirrored on Gitea (**note only**, 2026-11-04; not checked).

**Design gaps**

- Stage-2 plan §1 and U-S2 still say a Windows-only release with unpublished macOS builds. The user chose Windows and macOS on
  2026-11-04. Revise, including macOS's first-launch and ad-hoc path.
- A state-machine ADR for the sidecar supervisor and recorder, before SW17 (hand-off `sighter-plan.md`).
- The S2/P6 kit the user will run on two Windows PCs: I found no kit in git. Add a package; it needs a positive control (rule 71).

**First coding parts**

- App folder: land the stack as one batch (conflict expected in `stage-2.md` per the hand-off); SW4b, SW6; SW20 resolver core.
- Shared integration lines, not Dyno's: `sighter-ort.yml`, `scripts/affected.test.mjs`. CI capacity decides.
- Shared, after the tag: SW11 (= ADR 0051 WP-7), SW13.
- After Debrief 1.2 and the workbench move: SW12, UI, SW14 unless built elsewhere.

**Family obligations.** Per-machine and shell through the template; `paddock_diag`; ADR 0052 tour; rule 71 for S2, P6; logging from the
start; `paddock-tools` WP-16; `delete_by = sighter-v1.0.0`: `diagram-export-script`, `release-publish-script`.

**Decisions for the user**

| # | Decision | Recommendation |
|---|---|---|
| 1 | U-L2: send the RT-DETRv4 licence question | Later; nothing on the release path needs it |
| 2 | U-L3: ADR 0002 decision 4 | After the S7 measurement |
| 3 | U-L4: accept P6's result or waive S2 | No waiver (your 2026-10-30 line) |
| 4 | U-H2: an NVIDIA machine for S4's CUDA row | Ship the row marked unmeasured if none |
| 5 | U-H3: the VMware PC for the release try | Name it when SW44 nears |
| 6 | U-S5: S6 after the release | Yes |
| 7 | U-G1 to U-G3: mock-ups and the try | At SW26, SW39, SW44 |

**Estimate.** About 210.5 from packages (plan §10). The date follows the workbench move or the engine chain (about 110).

## 8. Recce

**State**

- `docs/pc-search-app-plan` `49451fa0b`: draft plan and two research notes. `docs/planned/recce/README.md` and placeholder art on main.
  No `apps/recce`, no ADR.
- D1 (name) and D17 to D22 accepted (user, 2026-11-04); the plan still shows them under open. Pitwall's collections depend on it.

**Design gaps.** D2 to D16; spikes S1 to S6 (17 to 24 days); ADRs R1 to R9; family ADR for `paddock-extract`, amendments to ADRs 0001 and
0007; independent review of each.

**First coding parts**

- Before any ADR, touching no shared code: S1 and S2; then S3, S4, S5 (local VMs, test tenant); S6 only when no release is building.
- After R1, app folder: WP 1.1 to 1.3, 1.6, 1.7, 1.8.
- Shared: section 1's `paddock-repo-files`, `paddock-watch`, `paddock-embed`, `paddock-extract`, `paddock-mcp`.

**Family obligations.** From its first release: shell (WP 1.1), per-machine through the template, plus a login-item agent; `paddock_diag`
with the index excluded; ADR 0052; rule 71 on S6's budgets; logging under the invented-names test of WP 2.6.

**Decisions for the user**

| # | Question | Recommended |
|---|---|---|
| D2 | Process model | Per-user agent (login item) plus app; index always warm |
| D3 | Windows privileged helper | Yes, stage 2, read-only (journal and MFT), shared with Dyno's pattern |
| D4 | Name index | Own in-memory table; S1 confirms |
| D5 | Stored text | No local file text stored; remote text encrypted with a keychain key; FDE check at first run |
| D6 | Clipboard history | Off by default; concealed markers honoured |
| D7 | Microsoft 365 registration | A family registration (multi-tenant plus personal) and a BYO client id field |
| D8 | MCP | Off until enabled; mail and M365 hidden by default; no paths unless allowed |
| D9 | Wayland hotkey | Portal where available, else a documented shortcut to `recce --toggle` |
| D10 | Plugins and workflows | Three tiers: built-in, TOML packs with a closed step set, Rust plugins; run-command only from your own packs after confirmation |
| D11 | Extraction toolkit | Decide after S2 |
| D12 | macOS sources | Own index everywhere; Spotlight only as an optional extra source |
| D13 | OCR | Off by default; platform OCR as a confirmed bulk job per folder |
| D14 | Timing | After Pacenotes and Sighter; spikes earlier only while load is low |
| D15 | Web search suggestions | On, only after an engine keyword, switch per engine |
| D16 | Family command registry | Shared declaration in `paddock-siblings` that every app adopts |

**Estimate.** No firm estimate until design. The plan's rough figure: 159 to 242 after spikes of 17 to 24, a guess by its own account.

## 9. Scrutineer

**State**

- `docs/code-signing-plan` `7a422b3f8`: `docs/plans/2026-11-04-code-signing.md` and research note. `tools/scrutineer/README.md` and
  placeholder art on main. No crate.
- Name accepted. Dyno 1.1.0 ships unsigned (user, 2026-11-04).

**Design gaps.** D1 to D18; S0 on local VMs (2.5 to 4 days, after the Dyno tag; D2's key type and the TCC claim depend on it); family ADR
(1 to 1.5); independent review. Closes ADR 0053's "What stays" and ADR 0051 U5.

**First coding parts**

- New crates once ADR exists: `paddock-pki` (A1, A2), `paddock-sign` (B1 to B3), `paddock-trust-store` (C1); `tools/scrutineer` (D1, D2).
  Nothing links them, so they can land beside a release while load allows, subject to F6.
- Shared release path, after the Spotter tag: E1, E2 (`.gitea/actions`, `conformance/`); F1 per app with rule 54 builds; F3 onto
  `paddock-tools`; rule 23 and 25 rewordings.
- Do not start A1 before S0: a wrong key type is baked into the root.

**Family obligations.** No shell or per-machine install (CLI). Own `scrutineer-v*` release, signed by itself (WP D2). Rule 71: the
`verify --policy` gate must fail on a known-bad artifact before it counts. Logging: no keys, passphrases or file contents.

**Decisions for the user** (plan §9, all 18 open)

| # | Question | Recommended |
|---|---|---|
| D1 | Chain | Three tiers: root, intermediate, leaf |
| D2 | Key types | ECDSA P-384 for CAs, P-256 for the leaf if S0 confirms; else RSA-4096/3072 |
| D3 | Lifetimes | Root 20 y, intermediate 5 y, leaf 397 days; warn at 60 days, fail at 14 |
| D4 | Root key storage | Encrypted file on two offline media plus paper; YubiKey later |
| D5 | Timestamps | Public TSAs with fallback and retries; the release fails rather than ship unstamped |
| D6 | Who uses the real leaf | Tags, `main` nightly, manual installer runs; branches use a throwaway CA |
| D7 | Windows engine | Native `SignerSignEx2`, `WinVerifyTrust`; portable adapter later |
| D8 | macOS engine | Apple `codesign` with an ephemeral keychain; rcodesign later |
| D9 | macOS hardened runtime | Stays off (rule 23 reworded "until notarized") |
| D10 | Where trust is installed | Windows LocalMachine `Root`, code-signing property; macOS admin domain, `codeSign` only; Linux never; TrustedPublisher on request |
| D11 | Installers also install the root | No; Scrutineer only; installers and About show read-only status |
| D12 | Revocation | CRL file on a Gitea release plus `trust sync`; no distribution point in leaves |
| D13 | First signed release | Pitwall 1.1; Spotter 1.0 does not wait |
| D14 | ADR 0051 U5 | `paddock-sign` owns the Authenticode verifier; `paddock-tools` calls it |
| D15 | Publisher name shown by UAC | The public account handle in CN and O; no e-mail or real name |
| D16 | Separate minisign key for Dyno's helper updates | **No, for now.** The plan says yes, but you decided on 2026-11-04: one family update key, no per-app keys. Revisit when helper-run updates come |
| D17 | Home | `tools/scrutineer/`, generic, own workspace and tag |
| D18 | Name constraints on the intermediate | Off; EKU constraints do the work |

**Estimate.** No firm estimate until design. The plan's rough figure: Must 35 to 48, Should 4 to 6.5; it says the ADR's packages make
these firm.

## 10. Rust tooling

**State**

- `./dev` on main: `tools/paddock-dev` with `cli`, `input_hash`, `plan`, `slots`, `wait`; crates `paddock-repo-files`, `-repo-graph`,
  `-slots`. ADR 0022 on main, its generic-tools amendment Accepted only on `docs/adr-0022-generic-tools` `9e3ae2bea`.
- Unlanded, all 97 or more behind main: `tools/wiki` `cf34c32b8`, `tools/heavy` `2a0347acf`, `ci/publish-tools` `e6ea1e547`,
  `steward/s31-tools` `78cdb999a`, `tools/dev-fetch-r3`, `tools/dev-diagrams-r11`, `tools/dev-notices`, `tools/dev-host-prune`,
  `tools/dev-it-failfast`. Status table: hand-off `rust-tooling-status.md`, 2026-10-31, stale.
- ADR 0051 Accepted on `docs/paddock-tools-design-v2` `a614485c9`; no crate; it says it lands with the crate's first change.

**Design gaps.** None for either ADR. Refresh the status table (half a day). Settle the conformance-check home and the two names of
section 1.

**First coding parts**

- `tools/` only, no `crates/`: land `wiki`, `heavy` after rebase; `./dev` subcommands outside the release-path list.
- New crate, on a branch: `paddock-tools` WP-1, WP-2, WP-4, WP-5 (WP-2 needs WP-14). ADR 0022 G.7 keeps new crates out of release
  windows, so landing follows F6.
- Docs now: ADR 0051 WP-0a (register entries).
- Shared, after the tag: WP-14 (`paddock_proc::login_env`), WP-7 (`paddock-runtime`). WP-13 (Dyno adoption) in Dyno's next line.
- Never inside a release window (G.7): R0b, R1, R2, R3, R5, R6, R7, R10, R14, G3, G4, G8, G11, G12.

**Family obligations.** Rule 71 and ER-27 for each ported check (`::error::`, exit codes, masked commands). Parity tests before a script
retires. No app obligations.

**Decisions for the user.** F6. ADR 0051's Q1 to Q10 and ADR 0022's G.10 are decided. The caps conflict: G.7 allows two tooling agents;
the 2026-11-05 slot allows one in total across other apps. I assume one.

**Estimate.** Firm from packages. ADR 0051: crate 42 to 57, adopters 9 to 13, WP-0a 1. ADR 0022: programme about 105, at least 11 weeks at
two agents; subtract what has landed once the table is refreshed.
