# Diagram style (suite spec)

One look for every Debrief, Pitcrew, Pitwall, Dyno and Spotter diagram: C4, UML, flow and overview drawings. The spec is
identical in every repo; only the three role colours in "Per-app roles" change. Diagrams are draw.io files (uncompressed XML,
`.drawio`), exported to theme-aware SVG and a 2x PNG by `scripts/export-diagrams.sh`. Start from `templates/`, load
`paddock.xml` as the shape library (see [README.md](README.md)), and apply the app's colours with `tools/apply-accent.mjs`.

## Roles and colours

Tokens are the Paddock design tokens. Every colour in a `.drawio` is written `light-dark(<light>,<dark>)`, so the exported SVG
follows the viewer's light or dark setting. Fills are tints of the stroke: 10 % over white in light, 16 % over `#15171B` in dark.

| Role | Stroke (light / dark) | Fill (light / dark) | Use |
|---|---|---|---|
| Own container (the app accent) | per app | per app | the things the diagram is about |
| Pure core | per app | per app | libraries without I/O |
| UI | per app | per app | window, webview, stores |
| Plugin / extension | #B0508A / #D98BC0 | #F7EEF3 / #342A35 | only where plugins exist |
| Process / OS / data | #565E6B / #A0A7B2 | #F0F2F5 / #1B1E23 | helper, child process, credential store, data folder, people. Never amber, red or green: those mean status |
| External system | #6B7380 / #7F8794, dashed 6 4 | #F4F5F7 / #17191E | other people's systems |
| Planned | role stroke, dashed 6 4 | white / #15171B | plus a "planned" tag. Never a different hue |
| Group / boundary | #C4C7CD / #353A42, 1 px | #F8F9FA / #121418 | dashed only when it is a real OS-process boundary |
| Text / muted | #14171C / #ECEEF1 · #565E6B / #A0A7B2 | none | muted for the technology line, kind line, labels, footer |
| Edges | #565E6B / #A0A7B2, 1.5 px | none | block arrowhead for calls, open arrowhead and dashed for replies and dependencies |

One accent role per diagram family; everything else is neutral, so the accent reads as "this app".

### Per-app roles

| App | Own container (stroke on fill, light / dark) | Pure core | UI |
|---|---|---|---|
| Debrief | #1D5FD1 on #E8EFFA / #5B9BFF on #202C3F | #007A5E on #E6F2EF / #2EC4A0 on #193330 | #C24E00 on #F9EDE6 / #F07D3A on #382720 |
| Pitcrew | #BF3A0C on #F9EBE7 / #FF7A45 on #3A2722 | #007A5E on #E6F2EF / #2EC4A0 on #193330 | #4C5BD4 on #EDEFFB / #8FA3FF on #292D3F |
| Pitwall | #6D3FD6 on #F0ECFB / #A58BFA on #2C2A3F | #007A5E on #E6F2EF / #2EC4A0 on #193330 | #0072B2 on #E6F1F7 / #56B4E9 on #1F303C |
| Dyno | #B01E6E on #F7E9F1 / #F063A8 on #382332 | #007A5E on #E6F2EF / #2EC4A0 on #193330 | #4C5BD4 on #EDEFFB / #8FA3FF on #292D3F |
| Sighter | #884F92 on #F3EDF4 / #C488CF on #312938 | #007A5E on #E6F2EF / #2EC4A0 on #193330 | #4C5BD4 on #EDEFFB / #8FA3FF on #292D3F |
| Pacenotes | #576D07 on #EEF0E6 / #8DA74E on #282E23 | #4C5BD4 on #EDEFFB / #8FA3FF on #292D3F | #0072B2 on #E6F1F7 / #56B4E9 on #1F303C |
| Spotter | #0B7A6E on #E7F2F1 / #2CC4B0 on #193333 | #4C5BD4 on #EDEFFB / #8FA3FF on #292D3F | #0072B2 on #E6F1F7 / #56B4E9 on #1F303C |

The table is generated from `tools/themes.mjs`, which is the source. The kit itself uses a teal placeholder accent (#0E7490);
`apply-accent.mjs` swaps it, and the core and UI colours, for the app's.

## Type

- Fonts: system stack `-apple-system,BlinkMacSystemFont,Segoe UI,system-ui,sans-serif`; identifiers (types, functions, fields,
  paths, commands, events) in `ui-monospace,SF Mono,Cascadia Mono,Consolas,monospace`. No Arial or Helvetica, no web fonts, nothing embedded.
- Sizes and weights: title 24/600; kind line (for example "C4 · Level 2 — Containers") 14/400 muted; one-line description 12/400
  muted; node name 14/600; technology line 12/400 muted in square brackets, no italics; descriptions and edge labels 12/400;
  group labels 12/600 muted; footer 11. Weights 400 and 600 only; nothing below 11 px.
- Real typography: `→`, `…`, `×`, en dash in ranges, curly quotes in prose; sentence case; UML multiplicities `0..*`.

## Shape and grid

- Nodes: 6 px radius, 1.5 px stroke. Groups: 10 px radius, 1 px stroke. Legend: 14 px radius, 1 px. No shadows, no gradients.
- 8 px grid. Gaps: 40 between nodes (96 or more when the gap carries a label), 24 inside groups, 56 or more between groups. Page margin 48.
  Node widths in multiples of 8 (typically 264, 288, 320), heights sized to the text (multiples of 8, at least 56). Align rows and
  columns; a box is as tall as its text needs and no taller. Notes are as small as their text.
- Person: a neutral box with a 14 × 20 stick-figure glyph in the top-right corner. Planned: dashed outline plus a "planned" pill
  (11/600) in the top-right corner.
- Furniture: title block top-left (title, kind line, description), legend bottom-left in one row with only the colours and line
  styles used, footer bottom-right "App · updated YYYY-MM-DD".

## Connectors and labels

- Orthogonal, rounded: `edgeStyle=orthogonalEdgeStyle;rounded=1;jettySize=auto` with `exitX/exitY/entryX/entryY` pinned. Where two
  boxes face each other the line is straight, through the middle of their overlap. Waypoints only for a bend, always orthogonal.
- At most one crossing per edge; never through a node or a group label; no shared channels for unrelated edges.
- Labels: at most 3 words, 12 px, beside the line (above a horizontal one, to the right of a vertical one), no background box, never on
  the line. Labels are separate text cells so their position is exact; keep them with their edge when you move it. Protocols and
  technologies go in the node text or the legend, not on the line.
- More than 12 edges on one canvas means split the diagram.

## Size

- Canvas at most 1600 × 1200 (sequence diagrams at most 1600 × 1800, at most 8 participants and 25 messages). Larger: split into
  two diagrams that share one legend, and link them from the doc.
- Check at README width (about 900 px): the smallest text must still be readable (at least 9 px). Prefer 1100 to 1400 px wide canvases.

## Diagram kinds

- **C4 context / containers / components:** accent for what the diagram is about, neutral for context, dashed for external; groups
  for process boundaries.
- **Sequence:** participants across the top in the role colour; dashed lifelines that break around labels; messages solid with a
  block head, replies dashed with an open head, label above the arrow; self-messages as a small loop with the label to its right;
  fragments (`opt`, `alt`, `loop`) as an outlined frame with a tab and the condition in brackets.
- **State:** states in the accent; start and end dots, a diamond for a choice; events and guards as labels, entry actions in the state.
  Send "return to" transitions to one place instead of crossing the page.
- **Class:** one box per type, members in monospace, enumerations in the core colour; composition has the diamond at the owner.
- **Deployment:** a group per machine, a dashed group per OS process boundary; one diagram per OS when they differ.
- **Flow:** top to bottom or left to right, one direction; decisions as diamonds with one labelled exit per outcome.

## Export

`scripts/export-diagrams.sh [src [out [name...]]]` (defaults `docs/diagrams`) writes `svg/<name>.svg` (transparent,
`light-dark()` colours, system fonts, no embedded fonts, no PNG fallbacks) and `png/<name>.png` (2x, light, opaque white).
Reference the SVG from docs; the PNG is for review and for places that cannot render SVG (it is large: apps git-ignore `png/`). A hand-drawn overview SVG uses the same tokens in a
`prefers-color-scheme` style block and lives at one path (`tools/overview.mjs` draws one; one SVG serves light and dark READMEs). SVGs follow the viewer's operating-system theme (draw.io's `color-scheme`).

Before committing: view every export (light) and a dark render, at README width. Look for labels touching lines or boxes, text
cut by a box edge, jogs in lines that should be straight, and anything that reads as unfinished.
