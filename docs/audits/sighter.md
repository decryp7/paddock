# Audit: Sighter against the problem catalogue

Read-only, 2026-10-24. Source: `main` @ a377d29. Design stage: docs only (README, `docs/proposal.md`, `AGENTS.md`,
`deny.toml`, `renovate.json`, `cliff.toml`, `scripts/release/publish.mjs`); no code, no workflows, no tags. The audit is therefore a
**day-one checklist**: what must exist before the first line of code. Severity as in [debrief.md](debrief.md); "H" here means
"must be in the first commit of that area". Class ids from [problem-catalogue.md](../problem-catalogue.md).

## Gaps, by severity

| # | Sev | Class | Finding | Fix |
|---|---|---|---|---|
| SG1 | H | T03, T02, S04 | Sighter drives real desktops and browsers (local, RDP/VNC, IP-KVM targets, agent mode). Its `AGENTS.md` says tests never touch TCC-protected folders or capture the developer's screen, but nothing enforces it. A test that clicks on the developer's real session, launches the installed Chrome (T02) or captures a real screen is one run away. | Day one: `C14` hermetic wrapper, `C15` pinned browser, and a test target abstraction: every automation test runs against a headless display or a VM target started by the test harness (xvfb / a throwaway virtual display), with a guard that refuses to run when the target is the user's console session. Fixtures synthetic (`C25`). |
| SG2 | H | P06 | `AGENTS.md` (116 lines) has Sighter rules but not the 40 Paddock rules; `suite-lessons.md` (65 lines) is a day-one copy with none of the later lessons. | Add the rules block and lessons (`C23`) in the first commit. |
| SG3 | H | B03, B04, B16, R03 | The design plans a recorder/agent that runs as a service and needs OS permissions (Screen Recording, Accessibility). Both the ad-hoc seal/Team-ID class and services-after-update apply, and TCC permissions are tied to the code signature, so an ad-hoc re-sign on update is expected to reset the grant (verify in the first spike). | Start from `C04`/`C21` and `paddock-service`; record in an ADR that macOS permission grants do not survive ad-hoc updates and what the app tells the user (a Repair that opens the right Settings pane, ADR 0010 service health). |
| SG4 | H | U01, U02, U03 | First dialogs must be written with kinds (ADR 0009), one notification/health model (ADR 0010), and declared layout constraints (ADR 0011). | Adopt from day one; no hand-configured dialog. |
| SG5 | H | L01 to L07 | Recording, labelling, training and locating are long operations. | Use `paddock-jobs` for every job from the first command (phase labels, cancel, monotonic counts, hash while writing). |
| SG6 | M | A01 to A06 | Angular app not yet created. | Start zoneless/OnPush, `withRequestStatus` from `paddock-ui`, store files `*.store.ts`, narrow scopes; `C09` without a waiver baseline. |
| SG7 | M | B01 to B17 | No CI yet, no icons directory use, no `check-index`, no `tauri.macos.conf.json`. | Copy the conformance job and bundle legs from the template; add the icon set from `paddock/icons/sighter`. |
| SG8 | M | S02, R06, R07 | Rust lint baseline is not set (`rustfmt.toml` exists). | `C13` preset in the workspace manifest from the first crate. |
| SG9 | M | P05 | No `commit-lint.mjs`. | `C23` (shared rule set). |
| SG10 | L | S03 | Plugins (if any) follow Debrief ADR 0017 out-of-process model; the guard and quarantine tests must be copied with the code, not rewritten. | Wait for the shared plugin host (ADR 0001/0006). |

## Addressed (with evidence)

- Rule 7 for tests is written into `AGENTS.md` ("Tests never touch TCC-protected folders ... derive data folder, port and socket names from a test-specific folder").
- Security by design: recordings redacted before write, password-field pause, retention caps; licence rules for models (no AGPL/GPL weights) in `AGENTS.md`.
- `scripts/release/publish.mjs` with a test; `deny.toml`, `renovate.json`, `cliff.toml` present; Pitwall optional (graceful "Needs local AI" state).
