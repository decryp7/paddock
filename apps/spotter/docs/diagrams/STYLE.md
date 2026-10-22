# Diagram style

Spotter's diagrams follow the suite spec in the Paddock repo: [`diagrams/STYLE.md`](https://dev.decryptology.net/decryp7/paddock/src/branch/main/diagrams/STYLE.md)
(roles, type, grid, connectors, size limits, export). Start new diagrams from Paddock's templates and shape library
([`diagrams/README.md`](https://dev.decryptology.net/decryp7/paddock/src/branch/main/diagrams/README.md)); `scripts/export-diagrams.sh` is Paddock's,
copied unchanged.

## Spotter's colours

Defined for app `spotter` in Paddock's `diagrams/tools/themes.mjs`; the generators in `src/` use it directly. Light / dark, stroke on fill.

| Role | Colours | Use here |
|---|---|---|
| Own container (accent) | #0B7A6E on #E7F2F1 / #2CC4B0 on #193333 | the Tauri shell, `spotter-engine`, `spotter-sources`, `spotter-render`, watch states |
| Pure core | #4C5BD4 on #EDEFFB / #8FA3FF on #292D3F | `spotter-core` and the ports it defines (the teal accent is near green, so the core is periwinkle) |
| UI | #0072B2 on #E6F1F7 / #56B4E9 on #1F303C | the Angular UI, the tray popup |
| Process / OS / data | #565E6B on #F0F2F5 / #A0A7B2 on #1B1E23 | the user, the credential store, the data folder, packs, a browser process |
| External | #6B7380 dashed on #F4F5F7 / #7F8794 on #17191E | release hosts, shares, Chromium, notification channels |

Rules that matter most here: amber, red and green are never role colours; there is no plugin role (ADR 0001); planned work is the
role's colour, dashed, with a "planned" tag (nothing is planned in the current diagrams); a diagram has at most 12 edges and fits
1600 × 1200 (sequences 1800 tall).

The footer reads "Spotter · updated YYYY-MM-DD". Update the date in `src/common.mjs` when a diagram changes, then regenerate. The 2x PNGs
under `docs/diagrams/png/` are for local review and are not committed.
