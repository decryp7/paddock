# Diagram kit

The shared way to draw Debrief, Pitcrew, Pitwall, Dyno and Spotter diagrams. The rules are in [STYLE.md](STYLE.md).

| Path | What |
|---|---|
| `STYLE.md` | the spec: roles, colours per app, type, grid, connectors, sizes, export |
| `paddock.xml` | draw.io shape library: person, own container, core, UI, plugin, process, external, planned, groups, note, legend, title block, sequence parts, state, class box, edges |
| `templates/` | one starting `.drawio` per kind: `c4-context`, `c4-containers`, `sequence`, `state`, `class`, `deployment`, `flow` |
| `scripts/export-diagrams.sh` | canonical export to theme-aware SVG and 2x PNG; apps copy it unchanged |
| `tools/apply-accent.mjs` | turns a template into an app's colours |
| `tools/overview.mjs` | the hand-drawn, theme-aware overview SVG (Pitcrew's content as the worked example) |
| `tools/lib.mjs`, `uml.mjs`, `themes.mjs`, `build-kit.mjs` | the generator that builds the templates and the library (`node diagrams/tools/build-kit.mjs`) |

## Start a new diagram

1. Copy the template of the kind you need into the app's `docs/diagrams/` and apply the app's colours:
   `node diagrams/tools/apply-accent.mjs pitcrew diagrams/templates/c4-containers.drawio docs/diagrams/c4-2-containers.drawio`
   (from a checkout of this repo; `<app>` is `debrief`, `pitcrew`, `pitwall`, `dyno` or `spotter`).
2. Open it in draw.io and edit. Keep the 8 px grid on, the labels beside the lines and the canvas inside the size limit.
3. Export: `scripts/export-diagrams.sh` (copy it from `diagrams/scripts/` once into the app's `scripts/`).
4. Look at the PNG and a dark render before you commit.
5. Never edit an exported SVG by hand (ER-40). CI re-exports every `.drawio` with the pinned draw.io (`ci-image/Dockerfile`,
   `DRAWIO_VERSION`) and fails on a difference: `node scripts/svg-drift.mjs` runs the same check where draw.io is installed
   (`DRAWIO=<path>` to point at it). It ignores what depends on the host's fonts (the fallback text's wrapping, up to 12 px of root size and a sideways
   shift of the whole drawing) and nothing else; `scripts/svg-drift.mjs` lists exactly what it normalises. A SVG is expected at `<dir>/svg/<name>.svg`, or where `EXPORT_DIRS` in the script says (Sighter's);
   a source whose SVG is nowhere fails as "not measured".

## Load the shape library

In draw.io: File ▸ Open Library from ▸ URL, then paste
`https://dev.decryptology.net/decryp7/paddock/raw/branch/main/diagrams/paddock.xml`
(or File ▸ Open Library from ▸ Device and pick the downloaded file). The shapes use the kit's teal placeholder accent; after placing
them, run `apply-accent.mjs` on the saved diagram to switch to the app's accent.

## Change the kit

Edit the generator in `tools/` (colours in `themes.mjs`, building blocks in `lib.mjs` and `uml.mjs`, templates in `build-kit.mjs`),
run `node diagrams/tools/build-kit.mjs`, export the templates and look at every one. The generated `paddock.xml` and
`templates/*.drawio` are committed.
