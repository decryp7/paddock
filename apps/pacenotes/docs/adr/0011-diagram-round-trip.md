# 11. Canonical draw.io form and the round-trip merge rules

Status: proposed (2026-10-30), revised after the first independent review. Plan row D-diagram, proposal section 9.2. Numbered at merge.

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
attributes beyond the model (`<object>` wrappers and custom fields are refused, and `parent` other than the default layer is refused), and `import` keeps the page name but not the page id
(the family writes `id = name`, so a draw.io-saved random diagram id fails `--strict` forever).

## Decision

### 1. One canonical form, owned by `paddock-diagrams`

Pacenotes writes through the shared writer and checks with `--check --strict`. It adds no second writer. Compressed files are refused
with the tool's message (turn Compressed off). The proposal's "reader for compressed files" is dropped; whether the VS Code extension
compresses by default stays a DG1 recording, and a "yes" reopens this point. Embedded `.drawio.svg` and `.drawio.png` are out of scope.
`UserObject` and `object` are the same thing to draw.io, which keeps the element name it read and never converts one to the other;
the reader accepts both and the writer keeps the name it found (new cells use `object`).

### 2. What is recorded, and where (no sidecar file)

All records are custom data in the file, so one save carries them. DG1 must confirm with real saves that each survives.

- **Page record**, on the root cell as `<object id="0" gen_page="...">`: the generated ids in generation order, the **tombstones**
  (generated ids the user deleted and chose to remember, H7), and a format version. One combined bound: ids plus tombstones together are at most `MAX_ITEMS`.
- **Per-cell record**, as wrapper attributes on the cell: `gen_key` (the identity string: qualified name, kind, language, plus an app
  tag value such as `pacenotes`), `gen_label` and `gen_style` (one hash per part, of what was **last written**), `gen_geo`
  (the written `x,y,w,h`), `gen_pts` (the written waypoints of an edge, and its label offset), `gen_ends` (the written source and
  target ids of an edge) and `gen_parent` (the written parent).
- **Names are neutral.** The shared crate reserves the prefix `gen_`; the app tag lives in the value, or the app declares its own
  prefix in `diagrams.json`. `pn_` is not used. The marker `pnGen=1` in the style string is dropped: the presence of `gen_key` is the
  only mark of a generated cell. `pn_rev` is dropped too: a revision stamp changes on every commit and would make every file differ in
  every regeneration, while the hashes already say exactly which parts changed.
- **Ids** are `pn:<kind>:<hash>` of the identity string (proposal 9.2), so order never matters. The hash is the first 16 hex digits
  (64 bits) of SHA-256 over the identity string; part hashes are the first 16 hex digits of SHA-256 of the part's canonical text. A
  collision between two different identities (about n^2/2^65, roughly 1.1e-11 for 20,000 ids) is detected when generating (two identities, one id):
  the generator fails with both names and extends the hash for that diagram to 24 digits, and the extension is recorded in the page record so ids do not flip back. It is never merged silently.
- Wrapper attributes that are not in the generated-meta names (link, tooltip, tags, Edit Data fields, a person's own) are **kept
  verbatim**; the merge owns only the `gen_` names.

### 3. Matching and the three-way rule

F is the saved file, R the record in it, G the new generation from the current index.

**Matching order, per generated item in G:** (1) a cell whose id equals the derived id wins. (2) Only if no cell has that id, a cell
carrying the same `gen_key` is the same item, its id was changed by draw.io (H10) and is restored on write. (3) Any other cell
carrying that key (a copy, H2) is **manual**: merge strips its `gen_` meta, so it stays as the person's own shape.

**Per part (label, style), using the stored hash:** an *edit* is hash(F part) differs from the stored hash; *code changed* is hash(G
part) differs from the stored hash. Edit only: keep F. Code changed only: take G. Both: **conflict**, F kept, G shown in the preview.
Neither: take G (identical). Geometry and waypoints are compared with the stored `gen_geo` and `gen_pts`; a difference pins them (H3,
H6). Off-grid numbers are a difference like any other and count as H3, not as serialisation.

### 4. Hand edits that must survive, and how each is detected

| # | Hand edit | Detected by | Rule on regeneration |
|---|---|---|---|
| H1 | Manual shape or edge added | no `gen_key` | Never touched, never removed; kept after generated cells in file order |
| H2 | Generated cell duplicated | same `gen_key`, id not derived, derived id also present | Manual; its `gen_` meta is stripped (section 3) |
| H3 | Moved, resized, or placed off the grid | F geometry differs from `gen_geo` | Pinned |
| H4 | Style changed | hash(F style) differs from `gen_style` | Kept; conflict only if the code side also changed |
| H5 | Label edited | hash(F label) differs from `gen_label` | Kept and flagged "label edited"; both sides changed is a conflict |
| H6 | Edge waypoints edited | F points differ from `gen_pts` | Pinned like geometry |
| H7 | Generated cell deleted | id in the page record's id list, not in F | Tombstone, see below |
| H8 | Cell reparented, grouped, moved to another layer | F parent differs from `gen_parent` | Parent kept; geometry is then judged in the parent's frame. Layers are kept as cells; the generator writes to the default layer |
| H9 | Z-order changed | order of ids in F differs from the page record's order | Kept for pinned cells; new cells go in front of generated, behind manual |
| H10 | Id changed by draw.io | no cell with the derived id, one with the `gen_key` | Section 3 rule 2; the id is restored and manual edges that referenced it are rewritten (below) |
| H11 | Page renamed, diagram id changed | page name or id differs | Not content: name from G, id normalised, reported as `reformat_only` |
| H12 | Re-serialised by draw.io (key order, spacing, whitespace) | `--check` passes, `--strict` fails | Serialisation only; regeneration normalises once |
| H13 | Code element gone | id in the page record, not in G | Cell marked "removed in code" and kept until the user confirms; its tombstone, if any, is dropped |
| H14 | A generated edge reconnected to another cell | source or target differs from `gen_ends` (the written source and target ids) | Pinned with the edge; reported "reconnected". An edge's identity includes both end identities, so a changed code relationship is a new edge, not a reconnection |
| H15 | Edge label (child vertex of the edge, or an offset) | a child cell with the edge as parent, or label geometry | Kept as a manual child; the label offset is folded into `gen_pts` and pinned with the waypoints (H6) |
| H16 | Link, tooltip, tags, Edit Data on a generated cell | wrapper attributes outside the `gen_` names | Kept verbatim |
| H17 | `visible=0` or `locked` | style or attribute on the cell | Kept; `visible=0` cells still count as present (not deleted) |
| H18 | Collapsed container, layer visibility | container and layer attributes | Kept; children keep their pins |
| H19 | Page view toggles (grid, guides, page view, page size; also background, shadow, math) | model attributes differ from the canonical set | Only grid, guides, page view and page size are normalised and reported as `reformat_only`. Background, shadow and math are kept verbatim on merge-managed files (a change to them is a visible change that the generator does not overwrite) |

**H7, deleted generated cells.** Detection is the page record, and the tombstone is tested only after matching rules 1 and 2 (section 3) have failed for that id. The non-interactive default is: do **not** re-add, report "deleted by
you (pending)", and do **not** fail `--check`. In the preview the user chooses *restore* (the cell is placed fresh, as a new item,
not at the old position) or *remember* (the id goes into the page record's tombstones, and is not reported again). Tombstones are
removed when the code element is removed (H13). A rename gives a new id, so it is a new item, not a tombstone hit.

**Manual edges that end at a generated cell.** On an id restore (H10) their `source` and `target` are rewritten to the restored id. On
removal in code (H13) the cell is kept until confirmed; if the user confirms, an attached manual edge becomes free at its last
point (the endpoint stays where the cell was), and when the user prefers, removal is blocked and the edge listed instead.

**New cells** are placed in free grid slots next to their neighbours by edge, else by group, **around** pinned cells (never on
them), and edges are routed around pinned cells. After a merge the stored `gen_geo` is the geometry **as written** (the pinned value
for a pinned cell), so a pin stays a pin until the user chooses "re-layout" (which asks first).

**Not preserved, by design:** a second page, a compressed page, embedded `.drawio.svg/.png`. They are refused with a message.

### 5. What Pacenotes needs added to the shared tool (a proposal; the tool's owner builds it, Pacenotes adopts it in the same change)

1. **Normalise the diagram id on `import`** and compare it leniently in `--check` (the tool's reviewers asked for this); an id-only
   difference is `reformat_only` (H11, H19). Test: the saved-by-drawio fixture with a random id round trips clean.
2. **`drawio::merge(saved: &MxFile, generated: &Diagram, opts) -> Merged { file: MxFile, changes: Vec<Change> }`**, working at file
   level, pure, no I/O. Generated cells are written from G, then overlaid with what is pinned (as raw strings: geometry, style,
   label, waypoints, parent). `Change` is `Added | Updated | Removed | Pinned | LabelEdited | StyleEdited | Conflict | Reconnected |
   DeletedByUser | Rewired`. It implements sections 3 and 4.
3. **`RawCell`** in the `drawio` module and a bounded **`MxFile::parse_lenient`**: manual cells (and their opaque style keys,
   wrapper attributes and children) pass through verbatim. The strict `parse`, `import` and `--check` keep every refusal they have
   today, and `RawCell` never enters the neutral `Diagram` model. This answers the earlier open question: opaque cells do not need a
   model item, because the merge works on files.
4. **Item metadata** is read and written only by the merge and the strict writer: a bounded map (reserved `gen_` prefix, length
   bounds), `UserObject` accepted. The strict parse accepts exactly the reserved names and the page record, and refuses other unknown
   attributes. Manual cells and pinned raw styles pass through unfiltered; there is no style-key allow-list.
5. **`dev diagrams merge`**, opted in per diagram in `diagrams.json`: writes the merged file and prints the change list. `merge
   --check` fails on a stale generated part (identity set or part hashes differ from G) and ignores manual cells. **Merge-managed
   files are exempt from `generate`'s "committed file needs a data file" rule**; `merge --check` checks them instead, and `generate
   --check --strict` skips them by name.
6. **Parent handling:** `parent` other than the default layer is accepted by the lenient parse and the merge (H8) and stays refused
   by the strict parse used by `import`.
7. **Tests live in the shared crate:** one per row H1 to H19, fixtures first from `saved-by-drawio.drawio`, then from DG1's
   recordings. Pacenotes tests only its identity function, layout and wiring.

### 6. Parallelism and cost (rule 51)

Diagrams are bounded (20,000 items, 8 MiB). The merge is a linear pass over id maps. Where it helps: **files merge in parallel**,
one task per diagram, bounded by cores minus one (CPU-bound, deterministic output because each file is independent and results are
reported in name order). The costly step is the **draw.io CLI export** (an Electron process per file): it is parallel only up to a
bound set by RAM (a few at a time; the tool's default stays one until measured), and it is **skipped when the diagram's hash is
unchanged** since the last export (hash of the `.drawio` kept next to the SVG/PNG, git-ignored). `--check` is a pure compare and
needs no draw.io. The merge on a 20,000-item diagram is measured when item 2 lands, budget under one second, and the number goes in
`apps/<app>/docs/performance.md` and the family `docs/benchmarks.md` (not in this ADR).

## Consequences

- DG1 shrinks to identity, layout and wiring to `dev diagrams merge`; it writes no writer or reader.
- DG1's recordings must confirm that the page record and the per-cell records survive a save, and cover H2 to H10 and H14 to H19 in
  draw.io desktop, web and the VS Code extension (including compression by default, paste id changes, and whether wrapper
  attributes keep their order). Each result is recorded in the fixtures' README and may change section 4.
- The shared tool must grow items 1 to 6 before DG1 can pass. If the owner declines item 2, Pacenotes proposes it as a module in
  the shared crate, never as a private copy.
- The optional `pacenotes diagram merge-driver` and the `.gitattributes` advice are unchanged; the canonical form's one line per cell
  is what a text merge needs.
