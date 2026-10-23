# Audit: Pacenotes against the problem catalogue

Read-only, 2026-10-24. Source: `main` (12 commits). Design stage: docs only (README, `docs/proposal.md`,
`docs/architecture-direction.md`, `AGENTS.md`, `deny.toml`, `renovate.json`, `cliff.toml`, `scripts/release/publish.mjs`); no
code, no workflows, no tags. Day-one checklist as in [sighter.md](sighter.md); severity as in [debrief.md](debrief.md). Class ids
from [problem-catalogue.md](../problem-catalogue.md).

## Gaps, by severity

| # | Sev | Class | Finding | Fix |
|---|---|---|---|---|
| PN1 | H | T03, S04, S01 | Pacenotes reads other people's repositories and serves them over MCP. Its rules (never execute repository code, read-only towards repositories, secrets redacted, allow-listed workspaces) are strong but unenforced: nothing yet proves, for example, that a test fixture repository is synthetic or that no plugin runs a build script. | Day one: a conformance test that runs the whole indexer against a fixture repo containing a trap (a build script, a `.env`, a symlink escaping the workspace) and asserts none ran, none leaked, none escaped; `C25` fixture scan. |
| PN2 | H | P06 | `AGENTS.md` (133 lines) has Pacenotes rules but not the 40 Paddock rules; `suite-lessons.md` (65 lines) is a day-one copy. | `C23` in the first commit. |
| PN3 | H | L01 to L08 | Indexing (tree-sitter, SCIP, embeddings, vector index) is long, bulk, parallel work; embeddings also come from Pitwall. | `paddock-jobs` from the start: pipeline with bounded queues, phase labels, growing counts, cancel, deterministic IDs (rule 14); memory budget for the vector index (rule 13, 16). |
| PN4 | H | U01, U02, U03 | Dialogs, notifications and layout from day one through ADRs 0009 to 0011. | As Sighter SG4. |
| PN5 | H | R09, R03 | Pacenotes runs as a desktop app plus an MCP server and out-of-process plugins; quit must end plugins and workers (exit watchdog), and any background service must survive updates. | `C20`, `C21` from the first release; plugin processes killed on parent exit (job object / process group), tested by the harness. |
| PN6 | M | A01 to A06 | Angular app not yet created. | As Sighter SG6. |
| PN7 | M | B01 to B17 | No CI, no bundle legs. | Copy the conformance job and bundle legs. |
| PN8 | M | S02, R06, R07 | Lint baseline not set. | `C13` from the first crate; `C22` if it lists processes. |
| PN9 | M | B04 | Out-of-process plugins and language servers ship as separate signed binaries (`plugin ... is signed`); third-party tools under hardened runtime and library validation repeat the Debrief 1.0.1 failure. | `C04` sidecar self-test for every bundled plugin, `hardenedRuntime:false` while ad-hoc; do not bundle vendor dylibs without re-signing. |
| PN10 | L | P05 | No `commit-lint.mjs`. | `C23`. |

## Addressed (with evidence)

- Security rules in `AGENTS.md`: local only, no telemetry, loopback MCP with per-client keys and budgets, logs without source text, licence list for dependencies and rule packs.
- Vector search decision recorded (Pacenotes only, hybrid, behind a `VectorIndex` port); plugin architecture follows ADR 0017 (out of process, guarded, quarantined).
- `scripts/release/publish.mjs` with a test; `deny.toml`, `renovate.json`, `cliff.toml` present.
