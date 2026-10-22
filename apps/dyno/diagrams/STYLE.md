# Diagram style guide

All diagrams in `diagrams/` are draw.io files that follow this guide so the set reads as one system. Edit them with
draw.io (desktop or diagrams.net); keep the source `.drawio` uncompressed XML. `scripts/export-diagrams.sh` exports each
one to `diagrams/svg/<name>.svg`, which `docs/design.md` and `docs/architecture.md` embed (Markdown viewers cannot render
`.drawio`). Ported from Pitcrew's and Debrief's guide; the palette is Dyno's.

## Canvas

- White page background (`background="#FFFFFF"`), grid off in the saved file, page view off.
- Generous margins: nothing closer than 40 px to the content bounds; 60–80 px between unrelated groups.
- **Title block** top-left: diagram name (20 px bold), type on the line below (e.g. "C4 · Level 2 — Containers",
  "UML · Sequence diagram"; 12 px muted) and a one-sentence description (11 px muted).
- **Legend** bottom-left: one sample of every element kind and line style used.
- **Footer** bottom-right: `Dyno · updated 2026-10-22` (10 px muted). Update the date when the diagram changes.

## Typography

Two font roles, both system fonts that ship with Windows and macOS, so every viewer renders the same text. No web
fonts: never add `fontSource=`.

| Role | Used for | Cell style / inline markup |
| ---- | -------- | -------------------------- |
| Text | Titles, descriptions, element names, technology lines, edge labels, notes, legends | `fontFamily=Arial,Helvetica,sans-serif;` on **every** cell |
| Code | Anything you could grep for: types, traits, modules, requests, fields, files, commands, crate names | whole label `fontFamily=Consolas,Menlo,monospace;` (class members), or a `<span style="font-family:Consolas,Menlo,monospace;white-space:nowrap">…</span>` inside a mixed label |

- Element names in C4 and deployment boxes, participant names, titles and legend entries stay in the text font, even when
  they name a crate. Technology lines, descriptions and message labels use spans for code.
- Sizes: title 20 px bold; diagram type 12 px muted; description 11 px muted; element names 13 px bold; technology line
  11 px italic muted in square brackets (`[Rust crate]`); descriptions, edge labels, notes, members and messages 11 px;
  legend heading 12 px bold muted; footer 10 px muted.
- Real characters: `→`, `…`, `×`, `≤`, `≥`, en dash in ranges, curly quotes in prose (straight only inside code).
- Labels are HTML (`html=1;whiteSpace=wrap`). Text never overflows a shape: size the shape to the text and check every
  shape in a rendered preview.

## Palette

Dyno's design tokens (`app/src/app/shared/tokens.ts`, light theme; ADR 0011). Fills are ~12 % tints of the stroke.

| Role | Stroke | Fill | Token |
| --- | --- | --- | --- |
| Dyno containers and libraries (primary) | `#0B6FB0` | `#E1EEF7` | chart blue |
| People, actors and the Angular UI | `#BE3584` | `#FBEAF3` | `--accent` |
| Pure Rust core (`dyno-core`) | `#00845F` | `#DFF2EB` | chart green |
| Separate OS processes (agent, helper, plugins) | `#8F7400` | `#F8F1D2` | chart olive |
| External systems and the OS (neutral) | `#686B73` | `#EEEEF1` | `--text3` |

- Text `#17181C`; muted text `#555A65`; edges `#5A6375`; message arrows `#3869BB`; lifelines `#9A9FAB`.
- Strokes 1.5 px. No gradients, no shadows.

## Shapes

- Rounded rectangles `rounded=1;arcSize=8;absoluteArcSize=1`.
- Every element label has three lines: **Name** (bold) / *[Technology]* (italic muted) / one-line description.
- **Planned** elements (not in the code yet): role colour, dashed border, white fill, `(planned)` after the name.
- **External** elements use the neutral palette; a dashed border marks an optional or planned external.
- **System / process boundary**: dashed rounded group, label top-left, 12 px bold muted.
- Data stores: `shape=cylinder3`.

## Relationships

- Orthogonal edges with rounded corners, `endArrow=block;endFill=1;endSize=6`; planned or optional flows dashed.
- Labels are verb plus protocol, the protocol as the italic muted technology line (`[dyno-ipc]`); white label background.
- "Implements" (a port's adapter) is dashed. Where two edges would cross, route the lowest target through the innermost
  corridor so the others do not cross it.
- UML: use cases are ellipses joined by plain lines to the near tip; sequence messages are solid with filled heads and
  replies dashed with open heads, labels above the line; classes are swimlane boxes (composition: filled diamond at the
  whole; dependency: dashed open arrow); states are rounded boxes with an initial dot and a bullseye final node;
  deployment nodes are `shape=cube`.

## Verifying

Parse each file as XML, export it (`draw.io --export --format png`) and look at the picture for overlaps and clipped
text before committing. No cell may lack `fontFamily=`; no `fontSource=` may appear.

## Generating

The files were produced by a small generator that applies this guide to every cell (the same sizes, fonts, colours and
edge styles), then exported and checked in draw.io. Hand edits in draw.io are fine and become the source.
