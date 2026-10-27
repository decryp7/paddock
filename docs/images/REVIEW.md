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

## Review 2 (2026-10-25)

Independent re-review under rule 22 (a reviewer who neither drew nor reviewed these images before) of the regenerated hero and
integration drawings and the new hand-offs drawing, on `docs/image-fixes` at 26f6a88. All six PNGs were viewed at full size in
both themes; the SVGs were checked against the PNGs (coordinates and text agree, same generator run), for type sizes at the
~900 px README width and for theme support (`prefers-color-scheme` in the hero, `light-dark()` in the draw.io exports). Text
checked against [ecosystem.md](../ecosystem.md), `crates/README.md` and shell 0.3.0 in `docs/plans/shared-next.md`; statuses
against the published releases (Debrief's newest published release is still v1.0.1; v1.1.1 is tagged, not published). The
`?v=` links in ecosystem.md and README.md match the files' SHA-256.

### Summary

| Image | Verdict | P1 | P2 | P3 |
|---|---|---|---|---|
| `paddock-ecosystem.svg`, `-light.png`, `-dark.png` | approve | | | 3 |
| `paddock-integrations.svg`, `-light.png`, `-dark.png` | fix | | 1 | 3 |
| `paddock-handoffs.svg`, `-light.png`, `-dark.png` (new) | approve | | | 3 |
| integrations and hand-offs (footer date) | | | | 1 |

6 approved (rows in [REVIEWED.md](REVIEWED.md)), 3 to fix (the integrations SVG and its two PNGs). New findings: 0 P1, 1 P2,
10 P3. Of the 16 earlier findings, 15 are fixed and 1 is not yet applicable (Debrief v1.1.1 is not published).

### Earlier findings

| # | Earlier finding | Verdict |
|---|---|---|
| Hero P1 | Job logs, Issues, Smoke tests run through the Pitwall band | **Fixed.** The band is 140–1430; Job logs (and Issues, which joins it) climb the left gutter at x=100, Smoke tests passes the right end at x=1470. No edge touches the band. |
| Hero P2 | Foundation band without shared crates | **Fixed.** "Rust crates" and "Conformance checks" chips; "UI kit, design system" merged. ecosystem.md lists both. |
| Hero P2 | App shell chips at 0.1/0.2 | **Fixed.** Safe mode, Report a problem, Services and login items, Storage, Undo added; the ecosystem.md paragraph matches chip for chip. |
| Hero P3 | Edge labels louder than descriptions | **Fixed.** 17 px weight 500, same size as the card text. |
| Hero P3 | Status chips 14 px | **Fixed.** 16 px (9.0 px at README width, exactly the floor). |
| Hero P3 | "Releasing soon" in hub violet | **Fixed.** Neutral grey chip. |
| Hero P3 | Orange dot under Pitcrew | **Fixed.** Plain muted text. |
| Hero P3 | Debrief chip to v1.1.1 when published | **Not yet applicable.** v1.0.1 is still the newest published release; the chip and the status table agree. |
| Integrations P1 | `paddock-pitwall` drawn as planned | **Fixed.** Solid green library node, no pill; ecosystem.md row no longer says planned. |
| Integrations P2 | Pacenotes missing as Pitwall client | **Fixed.** Pacenotes (planned) in "Apps on this machine" with its arrow. |
| Integrations P2 | Legend does not match | **Fixed.** Planned = solid outline plus pill, external = dashed; the legend shows exactly these. |
| Integrations P2 | Dashed means two things | **Fixed.** Hand-offs moved to their own drawing with solid arrows; only Smoke tests (later) is dashed, as in the hero. |
| Integrations P2 | Too small at README width | **Fixed.** 1042 px canvas: 12 px text is 10.4 px and 11 px pills and footer 9.5 px at 900 px. The hand-offs drawing (1059 px) is the same. |
| Integrations P3 | Local API too tall; empty hand-off gutter | **Fixed.** Local API is as tall as its text; the hand-off gutter is now ~75 px. |
| Integrations P3 | Debrief MCP server missing | **Fixed.** Under "MCP servers", planned. |
| Integrations P3 | Results hand-off missing | **Fixed.** Sighter → Pacenotes "Results" in the hand-offs drawing. |

### `paddock-ecosystem.svg`, `-light.png`, `-dark.png` (hero): approve

The hero now reads cleanly: no edge implies a Pitwall dependency, the foundation is current with shell 0.3.0, and the type
hierarchy is right (names, then descriptions and edge labels at one size, then chips). Polish only:

- P3 (author-flagged): the "Job logs" pill (x 53.5–146.5) sticks out 21.5 px past the card column's left edge (75), the only
  element outside the grid; "Smoke tests" (1411–1529) overhangs the right edge (1525) by 4 px. Move the left trunk to x≈122 so
  the pill starts at the column edge (it stays 18 px clear of the band, and the pill sits below the band anyway), and nudge the
  right line to x≈1462.
- P3: the shared trunk at y=540 crosses the dotted Pacenotes ↔ Pitwall stub at x=245, 26 px under the stub's arrowhead, and
  "Issues" merges into "Job logs" at (100, 820) with no junction mark. Both read correctly on a second look, but a small gap in
  the dotted stub at the crossing (a "bridge") and a 4 px junction dot would make them read at a glance.
- P3: the "PADDOCK" eyebrow is 15 px on the 1600 px canvas, 8.4 px at README width (minimum 9). Use 16 px.

### `paddock-integrations.svg`, `-light.png`, `-dark.png` (deployment): fix

- **P2: Spotter and Dyno are drawn as using `paddock-pitwall` today; the text says later.** All five app → `paddock-pitwall`
  arrows are solid "Request" arrows and the subtitle says "Apps reach it through one shared client", while the ecosystem.md
  contract row says "Debrief, Pacenotes and Sighter use it, Spotter and Dyno later" (and `crates/README.md` agrees). Neither
  Spotter nor Dyno has any Pitwall client code yet, and Debrief's own client has not been switched to the crate on `main`
  either. Draw the Spotter and Dyno arrows dashed with a "Later" legend entry (the grammar the hero and hand-offs already use),
  or change the table and alt text; the drawing and the text must say the same thing.
- P3: Debrief's MCP server note says "first, after Debrief 1.0.2"; Debrief has moved past 1.0.2 (1.1.1 tagged). Reword both here
  and in the ecosystem.md MCP row ("first, after Debrief 1.1" or simply "Debrief first").
- P3: about a quarter of the canvas is empty: the bottom-left below the Pitwall group beside "Other MCP clients" (~600 × 260 px)
  and the ~130 px band between the two groups that holds only the "HTTP, one key per app" label. Putting "Other MCP clients" in
  the MCP column under the servers' group, or the legend in the bottom-left, would make it about 200 px shorter.
- P3: the "Engine process" group uses the same grey dashed outline as the "Outside Paddock" legend swatch. llama.cpp is in fact
  third-party, so it is not wrong, but a process boundary and "outside Paddock" are different statements; give process groups
  a dotted or lighter outline, or label the legend swatch "Outside Paddock (node)".

### `paddock-handoffs.svg`, `-light.png`, `-dark.png` (new): approve

Correct, complete against the contract table (run bundles, job logs, issues, log checks, trace to code, video repro, test plans
and results, watch list, machine health, smoke tests later), legible in both themes, one arrow grammar, legend matches. Polish:

- P3: labels differ from the hero for the same hand-off: "Installed software" (hero) vs "Watch list" (here), "Test plans, MCP
  host" vs "Test plans (MCP)". Pick one wording per hand-off for both drawings.
- P3: "Newer version" (Spotter → Pacenotes) appears here and in Pacenotes' section but not in the contract table or the hero;
  "Supervises" (Pitcrew → Gitea runners) is not data although the legend says "arrow shows the data", and the runners are not
  an app although the subtitle says "between apps". Add a contract row for Newer version, and either drop the runners row (the
  CI row in the table covers it) or label its legend meaning.
- P3: 26 identical grey boxes, with Debrief repeated six times, make it read as a table drawn with boxes rather than a picture.
  The design guide gives each app an accent and an icon tile; a 3 px accent bar (or the 20 px icon tile) on each app box would
  let the eye find an app at once, and grouping the six "→ Debrief" rows into one fan-in would halve the repetition. "Video repro
  (MCP)" nearly fills its gap; one more 20 px of gap would let it breathe.

### Footer date (integrations, hand-offs)

- P3 (author-flagged): the footers say "updated 2026-10-24" because the generator stamps the UTC date; the commit was made
  2026-10-25 02:03 +0800, and every other date in the docs (ADRs, REVIEW.md, REVIEWED.md) is the author's local date. Not
  misleading, but a reader will see a date one day before the commit. Stamp the local date (or the commit date) in the
  generator; no regeneration needed only for this.

## Review 3 (2026-10-25)

Independent re-review under rule 22 (a reviewer who neither drew nor reviewed these images before) of the hero, integrations
and hand-offs drawings on `docs/image-fixes` at 30d0795. All six PNGs were viewed at full size in both themes, with crops of the
hero's crossing and junction, the integrations legend and the engine-process group; the SVGs were checked for canvas width and
type sizes (hero 1600 px, smallest text 16 px = 9.0 px at the ~900 px README width; integrations 1042 px and hand-offs 1059 px,
smallest text 11 px = 9.5 px) and theme support. Text checked against [ecosystem.md](../ecosystem.md) (status table, contract
table, alt text), `crates/README.md` and [diagrams/STYLE.md](../../diagrams/STYLE.md). The `?v=` links in README.md and
ecosystem.md match the first 8 hex of each file's SHA-256.

### Summary

| Image | Verdict | P1 | P2 | P3 |
|---|---|---|---|---|
| `paddock-ecosystem.svg`, `-light.png`, `-dark.png` | approve | | | 1 |
| `paddock-integrations.svg`, `-light.png`, `-dark.png` | approve | | | 5 |
| `paddock-handoffs.svg`, `-light.png`, `-dark.png` | approve | | | 2 |

9 approved (rows in [REVIEWED.md](REVIEWED.md)), none to fix. New findings: 0 P1, 0 P2, 8 P3. Of the 11 review-2 findings
(1 P2, 10 P3), 10 are fixed and 1 is partly fixed (empty canvas in the integrations drawing, accepted as the author noted).

### Review-2 findings

| # | Review-2 finding | Verdict |
|---|---|---|
| Hero P3 | Job logs pill outside the grid, Smoke tests overhang | **Fixed.** Job logs starts at the card column's left edge, Smoke tests ends inside the right edge. |
| Hero P3 | Trunk crosses the Pacenotes stub; Issues joins without a mark | **Fixed.** The dotted stub has a clean gap where the trunk crosses; a junction dot where Issues joins Job logs. Reads at a glance. |
| Hero P3 | "PADDOCK" eyebrow 15 px | **Fixed.** 16 px. |
| Integrations P2 | Spotter and Dyno drawn as using `paddock-pitwall` today | **Fixed.** Their arrows are dashed, the legend has "Later", the subtitle says "(Spotter and Dyno later)"; agrees with the ecosystem.md row and `crates/README.md`. |
| Integrations P3 | "after Debrief 1.0.2" | **Fixed.** The node says "first", the MCP row says "Debrief first". |
| Integrations P3 | A quarter of the canvas empty | **Partly fixed.** Other MCP clients now sit above the MCP servers and the canvas is 1114 px tall; a strip of about 80–100 px under the Pitwall group, left of the MCP column, remains (author-flagged). Acceptable; see P3 below for a cheap way to close it. |
| Integrations P3 | Engine-process outline same as Outside Paddock | **Fixed** (dotted now), but the dotted outline is nearly invisible; see P3 below. |
| Hand-offs P3 | Wording differs from the hero | **Fixed.** "Installed software" and "Test plans (MCP)" in both. |
| Hand-offs P3 | Newer version not in the contract; runners row | **Fixed.** Contract row added; the runners row is gone and the CI row covers it. |
| Hand-offs P3 | 26 identical grey boxes, Debrief six times | **Fixed.** One fan-in to and one fan-out from Debrief, Dyno and Spotter as fan-outs, an app-accent bar on every box (same accents as the hero). |
| Footer date | UTC date one day early | **Fixed.** Both footers say 2026-10-25. |

### `paddock-ecosystem.svg`, `-light.png`, `-dark.png` (hero): approve

Professional and finished: a calm grid, a clear hub, every line routed in gutters, one type hierarchy, real icons, a true dark
peer. Statuses match the status table (Debrief and Pitcrew v1.0.1, Pitwall v1.0.3; Debrief v1.1.1 still unpublished).

- P3: "Newer version" (Spotter → Pacenotes) is now a contract row and is in the hand-offs drawing, but not in the hero, which
  otherwise shows every app-to-app hand-off. Either add it (it must not cross the Pitwall band, so it needs a gutter route
  like Job logs) or mention it in the hero's `<desc>` and treat the hand-offs drawing as the complete list. Not
  misleading; the hand-offs drawing directly below it in ecosystem.md is complete.

### `paddock-integrations.svg`, `-light.png`, `-dark.png` (deployment): approve

Correct against the text: five clients, two of them later, one shared crate built today, one service process on loopback with
the credential store, llama.cpp as a child process and the tool router as an MCP client; planned pills only on what is not built.
Polish:

- P3: the "Engine process" outline (#C4C7CD on #F8F9FA, dasharray 1 4) is about 1.6:1 against the group fill with a 20% duty
  cycle, so at README width it all but disappears, in dark mode completely. Use a 2 3 dot pattern and the edge colour
  (#565E6B / #A0A7B2) for this one outline, or 1.5 px.
- P3: two process boundaries, two looks: "Pitwall: one service process" is the solid group outline and the nested "Engine
  process" is dotted. A reader will take dotted to mean something the solid group is not. Draw both process boundaries the
  same way (see the kit recommendation below).
- P3: legend swatches: the "Outside Paddock" dashed swatch ends on a short stub at its right edge (dash phase), so it reads as
  "[_ _ |"; the "Later" swatch is a single dash and an arrowhead and reads as a minus sign. Size the swatches to a whole number
  of 6 4 periods and give "Later" at least two dashes (as in the hero's "- - ->").
- P3: ecosystem.md's alt text for this drawing still says Pacenotes, Debrief, Sighter, Spotter and Dyno "reach Pitwall's local
  HTTP API through the shared paddock-pitwall client" with no "Spotter and Dyno later"; the picture now says later. Add it.
- P3: the remaining empty strip under the Pitwall group: the legend (about 1440 px wide in the PNG) could move up into it if it
  were trimmed to about 1250 px (shorter gaps between entries), saving ~90 px of height. Optional. Dropping the "Library" legend
  entry is fine: the green node labels itself "[shared Rust crate in each app]" and is the only node of its kind.

**Recommendation for the diagram kit (process boundaries).** STYLE.md today uses dashed for four things: external systems,
planned nodes (role stroke dashed), OS-process groups, and reply/dependency edges. The family drawings have already moved
planned to "solid plus pill" and dashed edges to "later", so a dashed process group now collides with both "later" and
"outside Paddock". Recommend the kit say:

- Edges: solid = works today; dashed 6 4 = later or optional (replies in sequence diagrams keep dashed with an open head,
  which is unambiguous there).
- Nodes: external = neutral dashed 6 4; planned = role stroke solid plus the "planned" pill (never dashed).
- Groups: a machine or logical area = solid 1 px group outline; an OS-process boundary = **dotted** (round caps, 2 3) in the
  edge colour (#565E6B / #A0A7B2) so it keeps ≥3:1 against the group fill, with the label ending in "process". Every process
  boundary in a drawing uses it, including an outer service process that contains child processes.

Update STYLE.md rows "Group / boundary" and "Planned" and the Deployment line ("a dashed group per OS process boundary") in the
same change, and redraw this drawing's Pitwall group to match.

### `paddock-handoffs.svg`, `-light.png`, `-dark.png`: approve

Complete against the contract table (run bundles, job logs, issues, log checks, trace to code, video repro, test plans and
results, installed software, machine health, newer version, smoke tests later), one arrow grammar, accents consistent with the
hero, legible in both themes. Polish:

- P3: the three bus boxes (Debrief about 530 px tall, Spotter about 320, Dyno about 255 in the PNG) are empty below their name;
  STYLE.md asks for nodes as tall as their text. Either put the app's one-line description from the hero in them, or make them
  narrow labelled buses so the empty grey area disappears.
- P3: the "planned" pill crowds the name in the Pacenotes boxes (about 13 px between "Pacenotes" and the pill) and sits on a
  different baseline from it. Put the pill on the name's baseline or widen the gap to 20 px.

## Isometric icon switch-over (2026-10-27, independent Opus review)

Approved, no P1. The 8 `src/icons/*.svg` are byte-identical to `icons/<app>/<app>-60.svg`; the ecosystem SVG and both PNGs
use only the new isometric frames and match each other in Chromium. P2s for the next diagram pass:
- `paddock-ecosystem.svg` repeats icon IDs per copy (210 duplicates): prefix IDs per copy in `src/ecosystem.mjs` and add an
  ID-uniqueness check.
- The ecosystem SVG has a transparent background: add a themed background rect.
- Pacenotes' olive tile is the least saturated; Pitwall and Sighter are close at 16–32 px (next icon pass).
- Right-hand connector labels sit about 5 px off the left-hand ones.
