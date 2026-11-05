# Plan: every remaining app ready to start when its turn comes (2026-11-05)

Status: plan, docs only. Nothing here was built or run. Read at `origin/main` `0e667eca7` and the branch heads named below,
all fetched 2026-11-05. The user's decisions of 2026-11-05 are written in as decided ("Decisions" below); nothing here is open
for the user unless a section says so. Section numbers are not the release order; the table below is.

How to read the evidence:

- A citation `branch` `sha` `path:line` was read at that ref. "On main" means `origin/main`.
- **Note only** means it comes from the coordinator's notes or an out-of-repo hand-off file and could not be checked in git.
- Commit counts against main overstate unmerged work, because landings are squashed ("land X as one commit"). Ancestry says
  little here, so shared-code numbers below come from tree diffs. They are file counts, not conflict counts.
- `dyno/wp-int` `42bcc7e07` is Dyno 1.1.0's integration head (2 behind main, 309 ahead; it holds
  `feat/per-machine-app-side`, `feat/dyno-mcp-listener` and `feat/dyno-mcp-readings`). "The Dyno tag" means `dyno-v1.1.0`; it does
  not exist yet (`git tag`: latest is `dyno-v1.0.0`).

## 1. Overview

Release order (user, 2026-11-05, replaces 2026-11-04): **Dyno 1.1.0, Debrief 1.2 with its full scope (all discussed features and
fixes), Spotter 1.0, Pitwall 1.1, Pitcrew 1.1, Pacenotes, Sighter, Recce.** Slots on other apps (user, 2026-11-05): up to two design
agents with no builds, code agents stay at one, heavy lane `fix`, paused under load; Dyno's agents keep priority.

**Scrutineer is dropped (user, 2026-11-05):** a self-signed certificate does not help SmartScreen or Gatekeeper; nothing was built;
all apps stay unsigned and the family update key stays.

| App | Code today | Design today | Next step | Estimate (agent-days) |
|---|---|---|---|---|
| **Debrief 1.2** | Tracks A and B on branches; shared tracks not started | Plan with 65 packages; predates ADRs 0052, 0053, 0054 and today's order | Amend the plan for the new order; land the fuzz fix; keep tracks A and B going; tracks C and D land after the Dyno tag | About 209 plus unplanned family work |
| **Spotter 1.0** | Feature-complete on `spotter/1.0-prep`, not on main; no install-update pass recorded | Done | ADR 0052's 5 pre-tag days; ADR 0054 A-1; update test; own logger out; rebase on Dyno and Debrief; per-machine | About 12 to 14, starting after the Debrief tag, plus a merge of Debrief's shared changes |
| **Pitwall 1.1** | Base on `pitwall/1.1-final`; chat not started | ADR 0023 Proposed, unreviewed, decisions not written in | Write the decisions in, independent review, put the base's work list in the repo | 43.5 to 70 (chat part 28 to 45 from the ADR) |
| **Pitcrew 1.1** | Shell and core on `steward/pitcrew-1.1-prep`; Environment service and UI, auto-scale, headless not started | ADR 0022 Accepted, ADR 0021 Proposed, headless ADR missing, plan outside the repo | Headless ADR, ADR 0021 spike and second review, plan into the repo | About 70 to 76, firm after a re-baseline; the 30 to 45 in the notes does not add up (section 4) |
| **Pacenotes** | Stage 1 crates on `steward/pacenotes-stage1`, not on main | Plan and ADRs 0001 to 0012; five ADRs of its plan unwritten | Land stage 1; record the answered questions | 300 to 350, re-estimated after stage 1 |
| **Sighter** | Separate unlanded branches; main has docs only | ADR 0001 Accepted; plan contradicts a later decision | Land the stack; revise the plan for macOS | About 210.5 |
| **Recce** | None (placeholder page on main) | Draft plan; no ADR | Spikes S1 to S6, then ADRs | No firm estimate until design |
| **Rust tooling** | `./dev` on main; many unlanded branches; `paddock-tools` not started | ADR 0051 and ADR 0022's amendment Accepted, both off main | Refresh the status table; land the docs; code on branches | ADR 0051: 52 to 71; ADR 0022: about 105 |

### Waiver sweep: all 47 fixed by 2026-12-31 (decided, F1)

`paddock.waivers.toml` on main has **47 waivers that expire on 2026-12-31**, in five checks. The user chose no extension, over this
plan's partial one. An expired waiver is a W001 error (`conformance/src/util/waivers.mjs:40-42`), and C36, C37, C39, C53 and C54 are all
in Paddock's own CI run (`ci.yml:257`).

**Only a landing on main clears a waiver.** A fix on an unlanded branch clears nothing, and with today's order none of these branches
lands by 2026-12-31 except Dyno's, so every other site is fixed **on main** and its branch takes it by merging main:

- `steward/pitcrew-1.1-prep` has no row for `notify-channels/src/tests.rs` (it deletes the file); the row still binds main.
- `spotter/1.0-prep` has no row for `apps/debrief/crates/chromium-driver/src/launch.rs` and a different file (208 changed lines by tree
  diff, the fix not read). Port what it holds to main, or fix it there; the row still binds main.

Who and when, by tree diff from each branch's merge base, except the second bullet above. Sizes are my judgement, not measured (no conformance
run here):

| Order | Owner | Sites | Size | Can land on main | Against the branches |
|---|---|---|---|---|---|
| 1 | Debrief, next in the release order | C36 7 crates, C39 4, C53 10, C54 1 | 4.25 to 5.75 | **Now, before the Dyno tag.** `dyno/wp-int` changes nothing in `apps/debrief` | `debrief/1.2-lean`, `search-chain-rebase` and `wip/debrief-1.2-lean-2026-11-04` change no site. `spotter/1.0-prep` also changes `health/commands/tests.rs` |
| 2 | Pitcrew, ahead of its turn | C36 2 crates, C37 2 test files, C39 1, C53 8 | 2 to 2.5 | **Now.** `dyno/wp-int` changes nothing in `apps/pitcrew` | Prep changes `launch.rs`, `process.rs`, `fake_runner.rs`, `supervision.rs`, deletes the C39 file; its owner merges main the same day. `usage.rs`, `disclaim.rs`, `probe.rs`, `terminate.rs` are free. Prep already has `tests/it` for C37 |
| 3 | Pitwall | C36 3 crates, C37 12 test files, C39 3, C53 2 | 2.25 to 2.75 | **Now.** `dyno/wp-int` changes nothing in `apps/pitwall` | `pitwall/1.1-final` changes all five sites and has 3 more test files; it takes them in its next merge from main, by its owner |
| 4 | Spotter | C36 1 crate, C37 17 test files, C39 1 | 1.5 | **Now.** `dyno/wp-int` changes only `spotter-sources/src/folder.rs` there, not a site | `spotter/1.0-prep` changes `http_sources.rs` and has 3 more test files |
| 5 | `tools/paddock-dev` tests | C53 6 | 0.25 to 0.5 | **Now.** `dyno/wp-int` changes only a golden JSON under `tests/` | `steward/s31-tools` changes 4 of the 6 |
| 6 | Dyno | C36 4 crates, C37 30 test files, C53 1, C54 1 | 3 to 4 | **After the Dyno tag and its landing on main** (scope frozen; user) | `dyno-helper/src/install.rs` is on `dyno/wp-int`; `procinfo.rs` is not |
| 7 | `crates/paddock-service` and `apps/shell-demo` | C54 2 and 2 | 0.75 to 1.5 | **Shared, Dyno-linked: after the Dyno tag.** Both crate files are on `dyno/wp-int`; the demo follows the renderer and ADR 0020's priority helper | The renderer also changes what services ask of the OS, so later apps' installers (rule 54) |

- **Size: 14 to 18.5 agent-days:** 10.25 to 13 can land before the tag (rows 1 to 5), 3.75 to 5.5 after it (rows 6 and 7). The earlier
  13 to 18 had no per-owner split; this one is the same judgement by owner.
- **The waiver file.** `dyno/wp-int` does not change `paddock.waivers.toml`, so removing rows conflicts with nothing in Dyno's
  landing. Each landing removes its own rows. C36 is one row, `apps/*`: the first C36 landing replaces it with one row per app, keeping the
  date, so that it can leave app by app.
- **Dates:** start in the code slot by **2026-11-10**, in the order above. Debrief goes first because it is next and its work restarts
  at full size after the tag; the others carry only merge cost, which grows slowly. Rows 1 to 5 take the one code slot for two to three
  weeks; Dyno's priority may pause them, and they have room until the tag. Rows 6 and 7 start the day Dyno lands; **all landed by
  2026-12-19**, which leaves room for red runs.
- **One landing per row** (rule 50), each with its app's affected tests (ER-7). A row 1 to 5 fix uses only what main already has
  (`paddock_proc::Supervised`, `descendants_of` and `paddock_testkit::net::HeldPort::reset` exist). A site that turns out to need a new shared API moves to the after-tag group.
- **If the Dyno tag slips.** Rows 6 and 7 need about 5.5 days plus CI, so a tag by about **2026-12-10** keeps 2026-12-19 and one by about
  2026-12-22 is the last that can make the date. If there is no tag by 2026-12-10, prepare both rows on a branch from `dyno/wp-int` and tell
  the user: no extension is assumed, and from 2027-01-01 expired rows turn Paddock's own CI red on every run.
- **After the tag the sweep runs beside Debrief's agents.** This assumes the one-code-agent cap is about Dyno's window and ends at
  the tag; the coordinator confirms.
- **Superseded by F1:** the decision of 2026-10-30 to fix C36, C37 and C39 inside Debrief 1.2 (`debrief-1-2.md` §8 track C).

ADR 0054 (Proposed) sets a second date: its lifecycle gate and C92, C93 bind other apps "at latest 2027-01-03". That now falls before
every remaining tag but Dyno's. Revisit the date when the ADR is accepted.

### Order for design, up to two agents, in release order

| # | Work | Size | Why here |
|---|---|---|---|
| - | **Family logging ADR** (F3, yes): ADR 0055 on `docs/adr-family-logging`, in independent review | Written | Every app's release obligation; Debrief adopts first |
| 1 | **Debrief:** amend the plan (section 5) for the order, the full scope and the shared tracks; review | About 1.5 | Next release; its section 8 still assumes three other apps land first |
| 2 | **Spotter close-out list:** run ADR 0052 WP-S (the audit sweep) on `spotter/1.0-prep`, list findings and the tag blockers of section 2 | 1 | No design missing; feature-complete and waiting |
| 3 | **Pitwall:** revise ADR 0023, independent review, base work list in the repo. **Running in another agent** on its own branch | About 2 | Fixes the `paddock-assistant` and `paddock-mcp` shapes that Debrief, Pacenotes and Recce inherit, so Debrief's M-packages need it first |
| 4 | **Pitcrew:** headless ADR, ADR 0021 WP-0 on a fake server and second review, P7's design pass, re-baselined plan in the repo | About 5 | Next after Pitwall; ADR 0021 is gated on nothing else |
| 5 | **Pacenotes:** the five ADRs, plan cleanup | About 3 | Its stage 2 waits for Debrief 1.2 anyway |
| 6 | **Sighter:** plan revision, sidecar and recorder state-machine ADR | About 2 | Same wait |
| 7 | **Recce:** spikes S1 to S6, then ADRs R1 to R9 | 17 to 24, then ADRs | Last in the release order; needs the Pacenotes ADR 0012 walk |

Sizes are my judgement except rows 2 (ADR 0052) and 7 (its plan). Rust tooling needs no design slot: half a day to refresh
`rust-tooling-status` when one is free.

Coding has the one code slot. The waiver sweep comes first, from 2026-11-10 at the latest, then landing Pacenotes stage 1 and the
Sighter stack.

### First parts truly free now

They touch no shared code that Dyno depends on:

- **Pacenotes:** land stage 1 (`steward/pacenotes-stage1`), with ADR 0051's WP-0b register line in the landing.
- **Debrief:** the fuzz fix; tracks A and B.
- **Tooling:** `tools/heavy` after a rebase (it changes only `tools/heavy`, `tools/README.md`, `.gitignore`).
- **Spotter:** copy deletions. The three files are already gone on the branch, so what is left is the register lines (section 2).
- **Recce:** spikes S1 to S6 (S6 only when no release is building).
- **Pitcrew:** A1, the auto-scale planner, only.

### Shared-code queue for right after the Dyno tag

Deduplicated by home (ER-22). "Order" is the release that needs it first. Nothing here lands before the tag if Dyno links the crate;
exceptions are marked.

| Home | Change | Needed by | Source | Order |
|---|---|---|---|---|
| `ui/.../paddock-e2e`, `conformance/` | ADR 0052's `layoutProblems`, C87 and C88 report-only (absent at `dyno/wp-int` and `spotter/1.0-prep`; the `layoutProblems` that exists is an unrelated workbench and Debrief helper) | Debrief, Spotter, then all | ADR 0052 WP-C2, WP-B4a, WP-C1a (the ADR places them before Spotter) | Debrief |
| `paddock-setup` and the NSIS include | Adoption; arrives with Dyno, absent on main | Debrief, Spotter, Pitwall, Pitcrew; Pacenotes, Sighter through the template | ADR 0053 "Adoption" | Debrief |
| `paddock-chromium`, `-jobs`, `-startup`, `-testkit`, `-dialogs`, `-download`, `-shell`, `ui` shell, `scripts/vm-smoke` | Spotter's own delta over Dyno | Spotter | Tree diff `steward/dyno-1.1.0..spotter/1.0-prep` | Spotter |
| `paddock-service`, `-shell`, `-build`, `-updater`, `-proc`, `-pitwall`, `-window`, `ui` shell | Pitwall's own delta | Pitwall | Tree diff against main | Pitwall |
| `paddock-assistant` | Parts, `ApprovalPolicy`, per-turn tools, native calls, store port. No Dyno or Spotter user (ADR 0023), but it regenerates TypeScript in `ui/projects/paddock-ui/workbench/assistant/generated`, in the package Dyno builds: **branch only until F6** | Debrief, Pitwall, Pacenotes | ADR 0023 WP-2 | Debrief, whichever package lands first; shape from ADR 0023 |
| `paddock-mcp` | Client: connection, cancel, loopback, stdio; current protocol revision | Debrief's M-packages if they need it; Pitwall WP-8; Recce WP 4.2; Dyno's and Pacenotes' servers | ADR 0023 §6; Recce plan §5 | Debrief or Pitwall |
| `paddock-log`, `-startup`, `-commands`, `-jobs`, `-proc` | `EventId`, `run.*`, `job.*`, `tool.run`, crash reports | All apps | ADR 0055 on `docs/adr-family-logging` | Debrief |
| `paddock-about`, `paddock-deps` | About and one dependency inventory (ADR 0028, Accepted; crates absent) | All apps; ADR 0051 WP-12 | [ADR 0028](../adr/0028-about-and-deps-crates.md); user, 2026-10-29: first after the tags | Debrief |
| `paddock-tools` | One Authenticode verifier: home to be settled in `paddock-tools` (ADR 0051 WP-6, U5); no separate signing crate | `paddock-tools` | ADR 0051 U5 | Tooling |
| `paddock-jobs` | `Outcome`, `ReadBack`, `NextStep`, `Why`, `Stamped<T>` (**breaking**, every user in one change); J1 keys; per-thread walk hook | Debrief J1; Pitcrew tag gate and WP-6; `paddock-tools` imports the types | ADR 0052 WP-B1; Debrief plan J1; Pitcrew ADR 0022 WP-6 | Debrief |
| `paddock-proc` | setsid and job-object kill; `login_env`; spawn logging; Debrief's P1 | Debrief, Pitcrew, `paddock-tools` | Note on Pitcrew; ADR 0051 WP-14; ADR 0052 WP-B4b | Debrief |
| `paddock-net`, `paddock-metrics`, `paddock-health` | `probe_connect`; load and memory in `MachineProbe`; health writer | Pitcrew | Pitcrew ADRs 0021, 0022 | Pitcrew |
| `paddock-testkit`, `conformance/`, `paddock-dev` | Contract tests, C83 to C90, C92 to C96 | All apps | ADR 0052, ADR 0054 §7.4 | Pitcrew |
| `paddock-runtime`, new `paddock-tools` | Single-file assets (ADR 0051 WP-7 **is** Sighter's SW11; build once) | Sighter, Pacenotes, Dyno, Debrief, Pitwall | ADR 0051 | Tooling |
| `paddock-repo-files`, to be renamed `paddock-walk` (user, 2026-10-30, at the freeze lift) | Stat fields, cancel, progress, Windows enumeration | Pacenotes IG14, Recce WP 1.4 | ADR 0012; Recce plan §5 | Rename first; IG14 Pacenotes |
| `paddock-watch` | `ChangeFeed` with cursors | Recce WP 1.5, Dyno ADR 0029 WP-8, Pacenotes | Recce plan §5 ("not started") | Recce |
| New `paddock-embed` | X55: BM25, RRF, Model2Vec | Debrief N7 (builds it), Pacenotes B1, Recce WP 2.3 | [shared-next](shared-next.md) X55 | Debrief |
| New `paddock-extract` | Guarded extraction, OOXML from Pacenotes | Recce WP 2.1, Pacenotes | Recce plan §3, family ADR needed | Recce |
| New `paddock-video` | From `debrief-video` | Sighter SW12, Debrief | Sighter stage-2 plan §3 | After Debrief 1.2 |
| Rules model | Plans disagree on the name: `paddock-repo-rules` (ADR 0022), `paddock-rules` (user, 2026-10-30, broader naming) | `./dev check`, Pacenotes C1 | ADR 0022 §6 | Tooling |
| Diagrams | Same: `paddock-diagrams` (on `tools/dev-diagrams-r11`) against `paddock-drawio` (Pacenotes plan §3.3); Pacenotes DG1's writer moves there | `./dev`, Pacenotes | ADR 0022 R11 | Tooling |
| `paddock-chromium` | Rename to `paddock-browser-driver` (user, 2026-10-30: after the Dyno and Spotter tags, before more apps adopt it); Debrief and Spotter onto one driver, ADR first | Debrief, Spotter's next | Notes only | **To re-time:** the notes say rename after the Spotter tag and move after Pitwall, but Debrief now comes first. Rename after the Dyno tag; the move's ADR before Debrief's driver work, or the move waits past 1.2 |
| `crates` gate, CI | Per-crate coverage; ring as the crypto provider; C58 | All | User, 2026-11-04 and 2026-10-30 (note only); rule 55 (e) | First free window |
| `.gitea/workflows/ci.yml` | Wire C51, C13, C23 into Paddock's own run. None is in the `--only` list at `ci.yml:257`; `ci.yml:245-249` gives old reasons for C13 and C23 | All | Coordinator, 2026-11-05 | Right after the Dyno tag |
| CI capacity | Parked Mac runners `d7-mac-mini-2` and `-3` online when load is low; fix the 16-minute plan step | All | User, 2026-11-04 (note only) | Right after the Dyno tag |
| `scripts/new-app.mjs --in-repo` | Monorepo mode | Pacenotes, Sighter, Recce WP 1.1 | Pacenotes plan §3.3 | Pacenotes |
| Coverage readers, stack-trace parsers | Move from Pacenotes and Debrief | Pacenotes, Debrief, Sighter | Pacenotes plan §3.3 | Pacenotes |
| New `paddock-helper` | From Dyno's helper (Recce D3: yes) | Recce WP 2.5 | Recce plan | Recce |

Docs-only landings that need no tag and that every brief cites: rule 56 (`docs/rule-logs-explain-failures`), ADR 0036 (Accepted, only on
`docs/adr-logging-root-cause` `021dc3c8d`) and the logging map (`docs/logging-map`), rules 65 and 66
(`docs/rules-2026-11-04`), rule 67 with ADR 0052 (`docs/adr-wiring-outcomes-ui` `fed6b5855`), ADR 0053 (Proposed, revision 3 with
amendment 1, only on `docs/dyno-adr-0027-per-machine` `3a6be49b5`, not on `dyno/wp-int`, though Dyno builds it). Main has rules 1 to 55
and 71 only.

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

- **Pitwall is the largest merge, and it grows.** Its last merge from main is 2026-11-04 and it has none of Dyno's service, updater or
  shell changes. Spotter and Pitcrew merged an earlier Dyno state and must follow it to the tag. All three now also take Debrief's shared
  changes (tracks C and D), which land first.
- **`paddock-jobs` WP-B1 is breaking across every app.** ADR 0052 puts it before the Pitcrew tag and this plan put it before Debrief's
  J1. With Debrief next, land WP-B1 inside Debrief's track C, before J1, with Spotter's, Pitwall's and Pitcrew's main code updated in the same
  change and their branches following by merge.
- **`paddock-assistant`, `paddock-mcp`:** ADR 0023 still fixes the shapes, but Debrief may land its use first. Pitwall then takes
  what Debrief leaves, the reverse of the earlier order.
- **New checks:** ADR 0052 adds C83 onward under `conformance/`, ADR 0054 under `tools/paddock-dev/src/checks/`, and ADR 0022 R8 ports
  conformance to Rust. Settle one home before WP-A1.

### Decisions (user, 2026-11-05)

Decided, no longer open:

| # | Decision |
|---|---|
| Order | Dyno, Debrief 1.2 with full scope, Spotter, Pitwall, Pitcrew, Pacenotes, Sighter, Recce |
| Scrutineer | Dropped. No app is signed in any planned release; the family update key stays |
| F1 | No extension of the waivers: all 47 fixed by 2026-12-31 (above). Dyno's and shared sites wait for the Dyno tag; Pitcrew's come ahead of their turn |
| F3 | Yes: the family logging ADR (ADR 0055) |
| F6 | `crates/` and release-path changes land in **the days between a tag and the next app's freeze**, named by the coordinator (ADR 0022 G.7 otherwise holds) |
| Logs | Dyno takes the logging in 1.1.0; logs keep 30 days with a 100 MB per-process cap; no Windows crash dumps yet |
| Per app | All recommendations in sections 2 to 8 accepted as listed |

**What follows from the new order, no longer as this plan had it**

- Spotter is feature-complete and now waits for Debrief 1.2: about 209 agent-days, or 70 to 105 working days at the two to three agents of
  `debrief-1-2.md` §8. That puts Spotter's start months after the Dyno tag, and its branch must keep following main meanwhile.
- `debrief-1-2.md` §8 on `debrief/1.2-lean` still says Dyno, Spotter and Pitwall land first; tracks C and D now wait only for the Dyno tag.
- ADR 0052's "before Spotter 1.0" packages and ADR 0054 A-1 name Spotter as next; they now bind Debrief first. Those ADRs are unchanged here.

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
  `copy-menu`. The obligation stands but nothing enforces it: C58 does not exist, so rule 55 (e) has the steward check by hand. The three
  copy files are already gone on the branch and `launch-plan` uses the shared `second_instance` (`lib.rs:389`), so the entries only need
  removing. `dyno/wp-int` has 6 such entries for `dyno-v1.1.0` too: `launchd-job-report`, `service-registration`, `launch-at-login` (2),
  `personal-data-redaction`, `sparkline`.
- **No install-update pass is recorded on any OS** (`releasing.md`, "Pre-release update test"): macOS FAIL on 2026-10-29, then PASS for
  start only on 2026-10-30 (steps 3 to 7 not run); Windows 11 Arm NOT RUN, the VM cannot start headless. Under rule 71 that is `not
  measured`. A Windows VM that can run it is a precondition.
- ADR 0054 D6 puts the tag under the lifecycle run, and Spotter has no `wiring.toml` or lifecycle rows, so A-1's Spotter part is needed.

**First coding parts**

- App folder, now: WP-S (1 day), register lines above. C37, C36 and the C39 site `spotter-sources/tests/http_sources.rs` go to main in the sweep.
- App folder, after ADR 0054 7.3 lands: A-1 (2 to 3 days).
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
| ADR 0054 | Lifecycle run gates the tag (D6), needs A-1's rows; C92, C93 not enforced |
| Logging | New log checks in `d37f2f181`. ADR 0055's adoption: `run.*` through the shell once Debrief has landed the shared part. **Tag obligation found:** `applog.rs` is still used (12 references on the branch, not in `known-copies.toml`); ADR 0036 U5 replaces each app's own logger before its next tag: about 0.75 day |
| Signing | Unsigned, as all apps; nothing to do |

**Decided.** ADR 0054 D6: Spotter's tag runs under the lifecycle gate.

**Estimate.** About 12 to 14 days of work, starting after the Debrief tag: the 9 to 10 working days given to the user on 2026-11-05
(ADR 0052's 5, ADR 0053's 2 to 2.5, a VM round) left out ADR 0054 A-1 (2 to 3) and the logger (0.75). ADR 0052's 4 shared days are done
by Debrief if it lands them first. Not counted: following Debrief's shared changes. The update test could add more if it finds the macOS
hang again.

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
- Shared, on a branch until F6: WP-2 (`paddock-assistant` and its generated TypeScript in `paddock-ui`).
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
| Signing | Unsigned, as all apps. ADR 0023 expects microphone re-prompts per update until signing; with signing dropped they stay, and that line needs rewording by ADR 0023's pass |

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

- Headless ADR, Pitcrew's next free number (0023): not written. The plan's H0, 1.5 days. Accounts are decided (user, 2026-10-31): one
  dedicated service account per machine for runners that share caches; Windows per-runner virtual accounts.
- P7, the tray popup with widgets, is user scope (2026-10-31; hand-off, "P7"); it needs a designer pass and review.
- ADR 0021: WP-0 on a fake server (user chose no real token), second review.
- Plan into the repo, re-baselined.

**First coding parts**

- App folder, free now: A1, the auto-scale planner and config, only.
- Blocked: E3 needs E2's `probe_connect`, on neither `steward/pitcrew-1.1-prep` nor `dyno/wp-int`, and Dyno links `paddock-net`. E4 and E5
  follow E3. H1 follows the headless ADR. P7 needs `paddock-widgets` from Dyno.
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
| Logging | ADR 0055's adoption; the runner's last lines dropped on restart (user note 2026-11-03) |
| Copies | `delete_by = "pitcrew-v1.1.0"` on the branch: `launchd-job-report`, `service-registration`, `launch-at-login` |

**Decided.** ADR 0021 stays Proposed until the spike and the second review (D-ADR21).

**Estimate.** 47 to 53 was a floor. From the hand-off's rows (`pitcrew-1.1-plan.md:184` and "P7"), about 84 remaining on 2026-10-31:

| Item | Days |
|---|---|
| Environment service and UI (E3, E4, E5) | 20 |
| Auto-scale (A1 to A4) and ADR 0025's package G (G **unverified**) | 17 |
| Headless (H0, H1) | 8 |
| Safer updates and job object (U1, W1); `.xz` still absent | 5 |
| P7 tray popup with widgets, not started | 8 |
| The three remaining `pitcrew-v1.1.0` copies, release mechanics, docs and reviews | 7 |
| P4d, P4e (**unverified:** `a2040bf04` and `678195125` on the branch read like them, so possibly done) | up to 3 |
| ADR 0053 and 0054 adoption, the bundle (my judgement for the bundle) | 5 to 8 |

**About 70 to 76.** It is below 84 because seven of the ten copies have left the register. It excludes ADR 0052's family packages, which
depend on ADR 0049 WP-4 and ADR 0037 WP-2 and WP-7. The notes' 30 to 45 cannot be reproduced. Firm after the
re-baseline.

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
- C36 and C39 were planned inside 1.2; they go to main in the sweep, first in its order.
- Full scope (user, 2026-11-05: all discussed features and fixes); next after Dyno. Rewrite §8's order and add ADR 0055's adoption.
- The bounded DuckDB 2 columnar test (user, 2026-11-05) into package D-a or D-b, results in `performance.md`.
- ADR 0051's `delete_by = debrief-v1.2.0` for `browser/runtime.rs`. Calendar in §5 is stale, as the plan says.

**First coding parts**

- App folder: the sweep's row 1 first, then tracks A and B (libduckdb pin and fetch scripts only), the fuzz fix.
- Shared, after the Dyno tag: tracks C and D, on the F6 days. Prepare designs and failing tests on branches until then.

**Family obligations**

| Item | State |
|---|---|
| Diagnostic bundle P1 | On main: logs and child logs raw (`activity/report.rs:384-399`). Shared `Redactor`; screenshots off by default, per-report checkbox |
| Per-machine | Currently `currentUser`; 2.5 to 3 days with guard and notes (ADR 0053) |
| Shell | M1 to M5, in the plan |
| ADR 0052 | Adoption when it moves to the shell |
| ADR 0054 | The waiver covers Debrief's next tag, but A-1 lists only Spotter, Pitwall, Pitcrew; add a Debrief package, now the first of them |
| Rule 71 | Performance gates (D7) need a positive control |
| Logging | First adopter of ADR 0055: own logger and `clef.rs` out, workers on `paddock-log`, own `lifecycle.rs` already due by `debrief-v1.2.0`; 3 to 5 days |
| Copies | 36 entries with `delete_by = debrief-v1.2.0` |

**Decisions for the user.** None now. D4 to D6 are decided inside packages. Ask before vm-smoke (plan §8).

**Estimate.** From packages: 164, plus N0 to N8's 10.5 (`debrief-1-2.md:313`) is 174.5, **about 209** at +20 %. The plan's own 190 to 197
omits N0 to N8. Not counted: this section's additions (about 5 to 8, my judgement).

## 6. Pacenotes

**State**

- Main: docs, ADR 0001, `paddock.ci.json`. No crates.
- `steward/pacenotes-stage1` `c1bba3f9f`: 11 crates, ADRs 0002 to 0011, CI contract, `scripts/affected.test.mjs` lines. **Not on main**
  although the user said to land it on 2026-11-04. Pacenotes' CODEOWNERS line is already on main. ADR 0051 WP-0b's register line is due
  in this landing.
- `pacenotes/dg1-fixtures` `1a6f2d43c`: partial round-trip fixtures made by scripted VM edits. The user named it for the landing, but it is
  not an ancestor of `steward/pacenotes-stage1`.
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

**Decided as listed**

1. DG1: only what the fixtures do not cover is the user's task. Their README (line 6) says "PARTIAL, see Gaps", but it has no Gaps section. By its
   layout, desktop and web have four inputs and VS Code has one, so the other three VS Code inputs look like what is left. Check before
   asking the user.
2. Trademark check before stage 2 ends (light check accepted 2026-10-30); needs someone named to do it. An agent drafts,
   the user decides.

**Estimate.** 300 to 350 from packages (plan §4.12 on `docs/pacenotes-fast-ingest`), ±50 % per package, re-estimated after stage 1.

## 7. Sighter

**State**

- Main: docs and scripts only.
- Branches, none on main, 322 to 728 commits behind it, no one branch holds them all: `sighter/sw0-land` `2f8b40581`; `sw9-weights` `ef7bcc8d8` (has sw0);
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
- Shared lines, not Dyno's: `sighter-ort.yml`, `conformance/release/telemetry-bytes.mjs` and its test, `conformance/README.md`,
  `docs/benchmarks.md`, `scripts/affected.test.mjs`. CI capacity decides.
- Shared, after the tag: SW11 (= ADR 0051 WP-7), SW13.
- After Debrief 1.2 and the workbench move: SW12, UI, SW14 unless built elsewhere.

**Family obligations.** Per-machine and shell through the template; `paddock_diag`; ADR 0052 tour; rule 71 for S2, P6; logging from the
start; `paddock-tools` WP-16; `delete_by = sighter-v1.0.0`: `diagram-export-script`, `release-publish-script`.

**Decided as listed**

| # | Decision | Answer |
|---|---|---|
| 1 | U-L3: ADR 0002 decision 4 | After the S7 measurement |
| 2 | U-H3: the VMware PC for the release try | Name it when SW44 nears |
| 3 | U-S5: S6 after the release | Yes |
| 4 | U-G1 to U-G3: mock-ups and the try | At SW26, SW39, SW44 |

Already decided, though the plan lists them: U-L4 (no waiver; the user runs the kit), U-H2 (CUDA row unmeasured if no NVIDIA machine), U-L2
(RT-DETRv4 is off the release path and not mirrored).

**Estimate.** About 210.5 from packages (plan §10). The date follows the workbench move or the engine chain (about 110).

## 8. Recce

**State**

- `docs/pc-search-app-plan` `49451fa0b`: draft plan and two research notes. `docs/planned/recce/README.md` and placeholder art on main.
  No `apps/recce`, no ADR.
- D1 (name) and D17 to D22 accepted (user, 2026-11-04), D2 to D16 as listed below (2026-11-05); the plan still shows them under open. Pitwall's collections depend on it.

**Design gaps.** Spikes S1 to S6 (17 to 24 days); ADRs R1 to R9; family ADR for `paddock-extract`, amendments to ADRs 0001 and
0007; independent review of each.

**First coding parts**

- Before any ADR, touching no shared code: S1 and S2; then S3, S4, S5 (local VMs, test tenant); S6 only when no release is building.
- After R1, app folder: WP 1.1 to 1.3, 1.6, 1.7, 1.8.
- Shared: section 1's `paddock-repo-files`, `paddock-watch`, `paddock-embed`, `paddock-extract`, `paddock-mcp`.

**Family obligations.** From its first release: shell (WP 1.1), per-machine through the template, plus a login-item agent; `paddock_diag`
with the index excluded; ADR 0052; rule 71 on S6's budgets; logging under the invented-names test of WP 2.6.

**Decided as listed (user, 2026-11-05)**

| # | Question | Answer |
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

## 9. Rust tooling

**State**

- `./dev` on main: `tools/paddock-dev` with `cli`, `input_hash`, `plan`, `slots`, `wait`; crates `paddock-repo-files`, `-repo-graph`,
  `-slots`. ADR 0022 on main, its generic-tools amendment Accepted only on `docs/adr-0022-generic-tools` `9e3ae2bea`.
- Unlanded, all 97 or more behind main: `tools/wiki` `cf34c32b8`, `tools/heavy` `2a0347acf`, `ci/publish-tools` `e6ea1e547`,
  `steward/s31-tools` `78cdb999a`, `tools/dev-fetch-r3`, `tools/dev-diagrams-r11`, `tools/dev-notices`, `tools/dev-host-prune`,
  `tools/dev-it-failfast`. Status table: hand-off `rust-tooling-status.md`, 2026-10-31, stale.
- ADR 0051 Accepted on `docs/paddock-tools-design-v2` `a614485c9`; no crate; it says it lands with the crate's first change.

**Design gaps.** None for either ADR. Refresh the status table (half a day). Settle the conformance-check home and the two names of
section 1. Remove `tools/scrutineer/` from main in the next docs landing (placeholder only).

**First coding parts**

- `tools/heavy` after a rebase: no shared change beyond `tools/README.md` and `.gitignore`.
- `tools/wiki` is **not** tools-only: it changes `.gitea/workflows/ci.yml`, `scripts/affected.mjs` and its test,
  `tools/paddock-dev/src/plan.rs`, `CODEOWNERS`, `.gitignore`. Hold it until the Dyno tag, or split out the crate.
- `./dev` subcommands outside the release-path list, on branches.
- New crate, on a branch: `paddock-tools` WP-1, WP-2, WP-4, WP-5 (WP-2 needs WP-14). ADR 0022 G.7 keeps new crates out of release
  windows, so landing follows F6.
- Docs now: ADR 0051 WP-0a (register entries).
- Shared, after the tag: WP-14 (`paddock_proc::login_env`), WP-7 (`paddock-runtime`). WP-13 (Dyno adoption) in Dyno's next line.
- Never inside a release window (G.7): R0b, R1, R2, R3, R5, R6, R7, R10, R14, G3, G4, G8, G11, G12.

**Family obligations.** Rule 71 and ER-27 for each ported check (`::error::`, exit codes, masked commands). Parity tests before a script
retires. No app obligations.

**Decisions.** F6 decided (section 1). ADR 0051's Q1 to Q10 and ADR 0022's G.10 are decided, except that the Authenticode verifier's home
is to be settled in `paddock-tools` now that no signing crate comes. The caps conflict: G.7 allows two tooling agents; the 2026-11-05
code slot allows one in total across other apps. I assume one.

**Estimate.** Firm from packages. ADR 0051: crate 42 to 57, adopters 9 to 13, WP-0a 1. ADR 0022: programme about 105, at least 11 weeks at
two agents; subtract what has landed once the table is refreshed.
