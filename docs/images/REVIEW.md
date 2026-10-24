# Image review: `docs/images` (2026-10-25)

Independent review under rule 22 of every image that conformance check C24 counts under `docs/images` (14 files), on
`shell/integrate` at 68a0601. Checked against [design-guide.md](../design-guide.md) (accents, icon tiles, the Paddock
umbrella mark), [diagrams/STYLE.md](../../diagrams/STYLE.md) (drawing spec), the text that embeds each image (README.md,
[ecosystem.md](../ecosystem.md)) and the current plans: shell 0.3.0 (X1–X54 in `docs/plans/shared-next.md` on `main`), the
workbench ([ADR 0013](../adr/0013-paddock-workbench.md), `docs/plans/workbench.md`) and the monorepo decision (ADR 0014 on
`main`). The PNGs were viewed at full size in both themes; the SVGs were checked against the PNGs (same generator run, same
statuses) and for type sizes and theme support; the icons were rendered and compared with the canonical masters in `icons/`.

Priorities: **P1** the image says something false or misleading; **P2** clearly below the spec or out of date with the plans;
**P3** polish.

## Summary

| Image | Verdict | P1 | P2 | P3 |
|---|---|---|---|---|
| `paddock-ecosystem.svg` | fix | 1 | 2 | 5 |
| `paddock-ecosystem-light.png` | fix (same findings; regenerate with the SVG) | 1 | 2 | 5 |
| `paddock-ecosystem-dark.png` | fix (same findings; regenerate with the SVG) | 1 | 2 | 5 |
| `paddock-integrations.svg` | fix | 1 | 4 | 3 |
| `paddock-integrations-light.png` | fix (same findings; regenerate with the SVG) | 1 | 4 | 3 |
| `paddock-integrations-dark.png` | fix (same findings; regenerate with the SVG) | 1 | 4 | 3 |
| `src/icons/debrief.svg` | approve | | | |
| `src/icons/dyno.svg` | approve | | | |
| `src/icons/pacenotes.svg` | approve | | | |
| `src/icons/paddock.svg` | approve | | | |
| `src/icons/pitcrew.svg` | approve | | | |
| `src/icons/pitwall.svg` | approve | | | |
| `src/icons/sighter.svg` | approve | | | |
| `src/icons/spotter.svg` | approve | | | |

8 approved (rows in [REVIEWED.md](REVIEWED.md)), 6 to fix (two drawings, each as SVG plus light and dark PNG). Distinct
findings: 2 P1, 6 P2, 8 P3.

## `paddock-ecosystem.svg`, `-light.png`, `-dark.png` (hero): fix

Strong overall: clean card grid, a clear hub, a correct two-layer foundation (Workbench for Debrief, Pacenotes, Sighter; App
shell for all seven, shell-only Pitcrew, Pitwall, Spotter, Dyno), real app icons, a full dark peer, system fonts, a legend that
names the three line kinds. The statuses match today's tags (Debrief v1.0.1, Pitwall v1.0.3, Pitcrew v1.0.1; Debrief v1.1.1 is
tagged but not published yet). Every hand-off in the image is in the ecosystem.md contract table and the directions match.

- **P1: three edges run through the Pitwall band and read as going to Pitwall.** "Job logs" (Pitcrew → Debrief), "Issues"
  (Dyno → Debrief) and "Smoke tests" (Spotter → Sighter) are vertical lines that stop at the band's bottom edge and start again
  at its top. With the label on one side of the band only, the picture says "Pitcrew sends job logs to Pitwall" and "Pitwall
  sends issues to Debrief", which contradicts the text ("Pitcrew does not need it", "the apps never depend on each other",
  "Pitwall is optional"). STYLE.md: never through a node. Fix: shorten the band (or give it gaps) so these three lines
  pass beside it, or route them around its ends through the outer gutters; put each label on the side where the line is
  unbroken.
- **P2: the foundation band is out of date with shell 0.3.0.** Paddock's chips are "UI kit, Design system, Diagram kit, Icon
  set, Release practices, Lessons": no shared Rust crates, although `shell/integrate` now has 24 `paddock-*` crates (log, diag,
  service, secrets, proc, env, pitwall, packs, …) and the conformance checks. Add "Rust crates" and "Conformance checks" (or
  merge "Design system" and "UI kit" to make room). The ecosystem.md "What Paddock provides" list has the same gap ("Shared Rust
  crates (planned)"); fix both together.
- **P2: the App shell chips show the 0.1/0.2 shell, not 0.3.0.** Six chips (startup protection, notifications and service
  health, dialogs, jobs and progress, settings/about/updates, theme). 0.3.0 adds what users notice most: Report a problem
  (X2), crash recovery and safe mode (X3), background services and login items (X4), the Storage page (X32) and Undo (X44).
  Swap in "Report a problem" and "Safe mode, restore" at least, and keep the ecosystem.md paragraph in step.
- P3: edge labels (21.5 px, weight 500) are larger than the card descriptions (17 px) and close to the app names (25 px), so
  the 14 connector pills are the loudest text after the title. Use 17 px.
- P3: status chips are 14 px on a 1600 px canvas, about 8 px at README width (STYLE.md: at least 9 px). Use 16 px.
- P3: "Releasing soon" uses the Pitwall hub violet (`.chip-soon` = `--hubf`/`--hubs`), which reads as "related to Pitwall".
  Use the neutral or info chip style; status chips must not borrow an app accent.
- P3: the "Runs CI for all apps" dot under Pitcrew is the Pitcrew orange, which next to a card reads as a warning/danger status
  dot (design guide: accent is not meaning). Drop the dot or use a neutral bullet.
- P3: when Debrief v1.1.1 is published, update its chip to v1.1.1 (and ecosystem.md's status table). C24 will ask for a new
  review of the regenerated files then. ADR 0014 (monorepo) needs no change in the image until the move happens; afterwards the
  foundation subtitle can say "one repository" if wanted.

## `paddock-integrations.svg`, `-light.png`, `-dark.png` (deployment): fix

Good structure (apps → Pitwall process → engine and credential store; MCP on the right; hand-offs as a separate grid), correct
spec typography and colours, `light-dark()` export with a real dark peer, a footer with the date, a legend. Process boundaries
are right (llama.cpp in its own dashed engine-process group; Pitwall loopback only).

- **P1: `paddock-pitwall` is drawn as "planned".** The crate exists on this branch (`crates/paddock-pitwall`, extracted from
  Debrief 1.1's client: discovery, pairing, keychain keys, retries, `AiState`, the engine switch) and `crates/README.md` lists
  it as built. Drop the "planned" pill and the dashed outline; show it as a solid shared library, and fix the ecosystem.md
  contract table row ("`paddock-pitwall` (planned)") in the same change.
- **P2: Pacenotes is missing as a Pitwall client.** The text and the hero say Pacenotes uses Pitwall for answers, explanations
  and embeddings (a two-way link); here Pacenotes appears only as an MCP server behind the Tool router. Add it to "Apps on this
  machine" (planned) with an arrow to the Local API.
- **P2: the legend does not match the drawing.** "Not built yet" shows a violet dashed swatch that no node uses; planned apps
  are grey dashed with a "planned" pill, which is the same outline as "Outside Paddock" (Gitea runners, Other MCP clients), so
  planned family apps read as external systems. STYLE.md: planned = the role's stroke dashed plus the pill. Make the legend
  swatch grey dashed with a pill, and give external systems their own look (the spec's #6B7380 on #F4F5F7 is close to the app
  fill; keep external dashed but planned apps solid-outlined with only the pill, or the reverse, so the two never share a look).
- **P2: dashed means two things across the family's two drawings.** In the hero dashed = "later"; here every hand-off is dashed,
  including ones that work today (Pitcrew → Debrief job log, Dyno → Debrief). Use one grammar: solid arrow for a hand-off,
  dashed only for later, as in the hero.
- **P2: too small at README width.** 1483 × 1386 canvas (STYLE.md cap 1600 × 1200) with 12 px body and labels and 10–11 px
  pills and footer: about 7 px and 6 px at the ~900 px README width (minimum 9 px). Tighten the canvas (see P3 below on empty
  space) to about 1200 wide, or split "Hand-offs between apps" into its own drawing.
- P3: the Local API box is about 570 px tall for 4 lines of text (STYLE.md: as tall as its text); the arrows can land on a
  narrower bus. The hand-off grid has a ~290 px empty gutter between its two columns.
- P3: Debrief as an MCP server (ADR 0007: Debrief first, after 1.0.2) is not shown; add it beside Pacenotes and Sighter under
  "MCP servers" so the drawing matches the contract table.
- P3: "Results" (Sighter → Pacenotes), which the hero and the "Test plans and results" contract row show, is missing from the
  hand-off grid.

## `src/icons/*.svg` (8 app and family icons): approve

All eight are the canonical macOS masters from `icons/<app>/<app>-macos.svg` with gradient ids prefixed for inlining into the
hero (same colours and same path data, checked file by file). Tiles and glyphs match the design guide: Sighter lavender
#A686D6 with the dark #2D1D43 glyph, Pacenotes bronze-olive #6D590F with white, Spotter amber tile, and the Paddock umbrella
mark on the asphalt-graphite #2B3038 tile with white ink and the seven app hues as the roof stripe. They render crisply at
1024, read at the 40 px size used in the hero's foundation rows, and work on both light and dark pages (opaque tiles). These
icons were already approved in the icon reviews; nothing here differs from those masters.

Note for the generator: the copies in `src/icons` are hand-synced. A small check (or having `ecosystem.mjs` read
`icons/<app>/<app>-macos.svg` and prefix ids at build time) would keep them from drifting; not a finding against the images.
