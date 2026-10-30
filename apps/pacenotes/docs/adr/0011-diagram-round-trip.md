# 11. Canonical draw.io form and the round-trip merge rules

Status: proposed (2026-10-30), plan row D-diagram, proposal section 9.2. Numbered at merge. Needs an independent review.

## Context

Pacenotes generates draw.io diagrams from the code index and must regenerate them without destroying what a person did in draw.io.
The proposal (section 9.2) set the rules as a table; this ADR settles the form of the file, names each hand edit that must survive
and how it is detected, and says what is built where.

The family already has the tool: `paddock-diagrams` (ADR 0022 R11, approved at 40f5f578f on `tools/dev-diagrams-r11`; the earlier
name was `paddock-drawio`) and `dev diagrams` in `tools/paddock-dev`. Share first (ER-22): Pacenotes builds on it and does not
carry a scheme of its own. Read from that branch (not yet on `main`), the tool gives this:

| Behaviour | Where | Meaning for Pacenotes |
|---|---|---|
| A neutral `Diagram` model (name, theme spec, shapes and edges back to front, theme tokens instead of colour strings, 8 px grid) kept as JSON data files `docs/diagrams/src/*.json` | `model.rs`, `data.rs` | Pacenotes' generator can produce a `Diagram`; layout stays outside the crate |
| `drawio::write`: one canonical text, deterministic (same model, same bytes) | `drawio/export.rs` | The canonical form exists already: uncompressed, one page, fixed attribute order, fixed model attributes |
| `generate --check`: a committed `.drawio` must draw what its data draws; a file without a data file fails | `diagrams.rs` | Detects a hand edit that changes the picture |
| `generate --check --strict`: also fails on a different serialisation of the same picture (key order, spacing) | same | Detects draw.io rewriting the file even when nothing visible changed |
| `drawio::import` / `dev diagrams import`: adopts a hand-edited or draw.io-saved file as a data file | `drawio/import.rs` | The only path today from a hand edit back to the model |
| Refusals, each a typed error naming the cause: compressed page (`Error::Compressed`), more than one page, unknown XML elements or attributes, a style key the model lacks (`Unsupported`, names the key and cell), a colour outside the theme, duplicate ids, dangling edge ends | `xml.rs`, `import.rs`, `error.rs` | Nothing is dropped silently, but also nothing the model lacks survives: `import` is all or nothing |
| Bounds: `MAX_INPUT_BYTES` (8 MiB, checked before reading), `MAX_ITEMS` (20,000), `MAX_HTML_DEPTH` (32) | `error.rs` | Parser never panics on input; Pacenotes adds no bound of its own |
| `MxFile::difference`: compares two files after canonicalisation (style keys sorted, last duplicate wins, numbers by value) | `xml.rs` | The "same picture" test used by `--check` |
| A fixture saved by draw.io itself (`saved-by-drawio.drawio`) | `tests/fixtures` | Precedent for DG1's recorded round trips |

What the tool **does not** do, and Pacenotes needs: it has no notion of a generated cell versus a manual one, no merge (regeneration
overwrites the file; a hand edit must be adopted with `import` first, which replaces the data), no stable ids from code identity, no
attributes beyond the model (`<object>` wrappers, `pn_*` fields are refused), and `import` keeps the page name but not the page id
(the family writes `id = name`, so a draw.io-saved random diagram id fails `--strict` forever).

## Decision

### 1. One canonical form, owned by `paddock-diagrams`

Pacenotes writes through `drawio::write` and checks with `--check --strict`. It does not add a second writer. Compressed files are
refused with the tool's message (turn Compressed off); the proposal's "reader for compressed files" (plan DG1) is dropped unless a
recording shows draw.io on the web or in VS Code compresses by default (open question for DG1, below).

### 2. Pacenotes diagrams are the family's data plus a generation record

The generated part of a diagram is the `Diagram` model plus, per generated item, the identity it came from. Ids are
`pn:<kind>:<hash>` of the code element's qualified name, kind and language (proposal 9.2), so order never matters. The generation
record (per item: id, `pn_hash` of the generated content, `pn_geo` the generated rectangle) lives **in the file** (proposal 9.2:
no sidecar), as the shared tool's extension in section 4.

### 3. Hand edits that must survive, and how each is detected

Detection compares three things: the file as saved (F), the generation record in it (R, what the generator wrote last time), and the
new generation (G, from the current index). A cell is *generated* when its id starts with `pn:` and it carries the marker; anything
else is *manual*.

| # | Hand edit | Detected by | Rule on regeneration |
|---|---|---|---|
| H1 | Manual shape or edge added (no `pn:` id) | id has no `pn:` prefix or marker | Never touched, never removed; stays after generated cells in original order |
| H2 | Generated cell duplicated (copy keeps style and marker, gets a new id) | marker present but the id is not in R | Treated as manual (H1) |
| H3 | Generated cell moved or resized | F geometry differs from `pn_geo` | Pinned: rectangle kept |
| H4 | Style tweak (colour, font, dash) | F style differs from the generated style | Kept; only tokens the generator owns (marker, `pn_` keys) are rewritten |
| H5 | Label edited | F label differs from the label for `pn_hash` | Kept and flagged "label edited"; if code content also changed, a conflict shown in the preview |
| H6 | Edge waypoints edited | F waypoints differ from R | Pinned like geometry (H3) |
| H7 | Generated cell deleted | id in R, not in F | Not silently re-added: listed in the preview as "deleted by you", the user chooses to restore or to remember the deletion (recorded in the data file) |
| H8 | Cell reparented, grouped or put on another layer | F `parent` differs from R | Parent kept; geometry is then relative, so H3 is judged in the parent's frame |
| H9 | Cell order (z-order) changed | order of ids in F differs from R | Kept for pinned cells; new cells go in front of generated ones, behind manual |
| H10 | Id changed by draw.io (paste, import) | `pn_key` present, id differs from the derived id | Matched by `pn_key`, id restored on write; if two cells share a `pn_key`, the first is the original and the other is H2 |
| H11 | Page renamed, diagram id changed by draw.io | page id or name differs | Not a content edit: the name comes from the data, the id is normalised (section 4) and reported as `reformat_only`, not as a failure |
| H12 | File re-serialised by draw.io (key order, spacing, `UserObject` for `object`, numbers off the grid) | `--check` passes, `--strict` fails | Reported as serialisation only; regeneration normalises the file once |
| H13 | Code element gone | id in R, not in G | Cell marked "removed in code" and kept until the user confirms in the preview |

Not preserved, by design: edits to the model attributes (grid, page size), a second page, a compressed page. They are refused or
normalised with a message, not merged.

### 4. What Pacenotes needs added to the shared tool (proposal for `paddock-diagrams`, not a copy)

Each item is a change to `crates/paddock-diagrams` and `tools/paddock-dev`, made by the tool's owner, with its adoption by Pacenotes
in the same change (AGENTS.md: shared change and adoption land together). Until then `paddock-diagrams` is frozen for this work and
nothing here is copied into Pacenotes.

1. **Normalise the diagram id on `import` and compare it leniently in `--check`.** Reviewers of the tool already asked for this.
   `import` sets the id from the page name; `difference` ignores the id and `--strict` reports an id-only difference as
   `reformat_only` (H11). Test: the saved-by-drawio fixture with a random id round trips with no finding.
2. **Item metadata.** A small, bounded `meta: BTreeMap<String, String>` on `Shape` and `Edge` (keys limited to a fixed prefix list,
   such as `pn_key`, `pn_hash`, `pn_geo`; value length bounded), written as an `<object>` wrapper in canonical position and read
   back; `UserObject` accepted on read (H12). Other unknown attributes stay refused. This is the only model change.
3. **Item roles.** `import` keeps the marker style key (`pnGen=1`) as metadata instead of refusing it, so the generated/manual
   split (H1, H2) survives a round trip. A per-app allow-list of extra style keys would also do; the choice is the tool's.
4. **A pure `merge(previous_file, generated, options) -> MergeResult`** in `paddock-diagrams`, format-neutral where possible: it takes
   the saved `MxFile`, the new `Diagram`, and returns the merged `Diagram` plus a list of `Change { id, kind: Added | Updated |
   Removed | Pinned | LabelEdited | Conflict | DeletedByUser }`. It implements the table in section 3, with no I/O. Pacenotes supplies
   identity and layout; the crate owns matching and pinning, so Debrief or Dyno can use it too (rule of three not yet met, so it
   stays a function, not a framework).
5. **`dev diagrams merge`** (apps opt in through `diagrams.json`): writes the merged file, prints the change list, and with
   `--check` fails on a stale generated part (hash or identity set differs) and ignores manual cells.
6. **Tolerant read for a draw.io-saved file with extra, ignorable data.** `import` today fails on any unknown element or style
   key. For merge the reader must keep unknown *style keys* on manual cells verbatim (opaque) rather than refuse them, since a
   person may use any draw.io shape; the model-level `Unsupported` stays for generated cells and for `import` into data.
   Open point for the tool's owner: whether opaque cells fit the "neutral model" or need a separate `Opaque` item.
7. **Golden and fixture tests in the shared crate**, not in Pacenotes: each of H1 to H13 has a test there; Pacenotes tests only
   its identity function, its layout and the wiring.

### 5. Parallelism and cost (rule 51)

Diagrams are small (bounded at 20,000 items and 8 MiB) and merge is a linear pass over ids in a map, so no parallelism: the
measurable cost is the draw.io CLI export, which `dev diagrams export` already runs one file at a time with a 180 s timeout. Cheaper
work is already designed in: `--check` is a pure compare, and the stale check hashes the generated part only. Measure the merge on a
20,000 item diagram when item 4 lands; the budget is the existing `MAX_ITEMS` case under one second.

## Consequences

- Pacenotes' DG1 has a smaller job: identity, layout and the wiring to `dev diagrams merge`. It does not write a writer or a reader.
- DG1's recordings must cover H2, H3, H4, H5, H6, H7, H8, H10, H11, H12 in draw.io desktop, web and the VS Code extension; the
  list of edits the agent hands the user is this table. Whether a platform compresses by default, renames ids on paste, or
  converts `object` to `UserObject` is unobserved; each result is recorded in the fixture's README and may change section 3.
- The shared tool must grow items 1 to 6 before DG1 can pass; if the owner declines item 4, Pacenotes proposes it as a shared
  `paddock-diagrams` module in the same change, never as a private copy.
- Drops from the proposal: the optional `pacenotes diagram merge-driver` (Later) and the recommended `.gitattributes` entry stay
  as they were; the canonical form in the shared tool already gives one line per cell, which is what a text merge needs.
- Risk: the opaque-cell question (item 6) decides whether a person's own draw.io shapes survive; resolve it in the review.
