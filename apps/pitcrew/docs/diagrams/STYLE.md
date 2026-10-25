# Diagram style

Pitcrew's diagrams follow the suite spec in the Paddock repo: [`diagrams/STYLE.md`](https://dev.decryptology.net/decryp7/paddock/src/branch/main/diagrams/STYLE.md)
(roles, type, grid, connectors, size limits, export). Start new diagrams from Paddock's templates and shape library
([`diagrams/README.md`](https://dev.decryptology.net/decryp7/paddock/src/branch/main/diagrams/README.md)); `scripts/export-diagrams.sh` is Paddock's,
copied unchanged.

## Pitcrew's colours

Applied with `node diagrams/tools/apply-accent.mjs pitcrew <template>` from the monorepo root. Light / dark, stroke on fill.

| Role | Colours | Use here |
|---|---|---|
| Own container (accent) | #BF3A0C on #F9EBE7 / #FF7A45 on #3A2722 | Pitcrew service, shell, tray, states, use cases, classes |
| Pure core | #007A5E on #E6F2EF / #2EC4A0 on #193330 | `pitcrew-core` and its components |
| UI | #4C5BD4 on #EDEFFB / #8FA3FF on #292D3F | the Angular UI |
| Process / OS / data | #565E6B on #F0F2F5 / #A0A7B2 on #1B1E23 | runner processes, credential store, service manager, data folder, people |
| External | #6B7380 dashed on #F4F5F7 / #7F8794 on #17191E | Gitea / Forgejo, release hosts, Pitcrew releases |

Rules that matter most here: runner processes are neutral (amber means warning); planned work is the role's colour, dashed, with
a "planned" tag (nothing is planned in the current diagrams); a diagram has at most 12 edges and fits 1600 × 1200
(sequences 1800 tall), so the large ones are split (core in three, service in two, deployment per OS, use cases in three,
domain model in three, safe update in two).

The footer reads "Pitcrew · updated YYYY-MM-DD". Update the date when a diagram changes. The 2x PNGs under
`docs/diagrams/png/` are for local review and are not committed.
