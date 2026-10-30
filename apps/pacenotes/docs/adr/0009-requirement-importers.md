# 9. `pacenotes-req`: requirement importers and the mapping pack

Status: proposed (2026-10-30), design note of package RQ1 (plan, stage 1). Numbered at merge.

## Context

Requirements live in documents people already have: a spreadsheet, a Word file, a CSV export, a Markdown table. Pacenotes links them to code
and tests, so it needs them as `Requirement`s with a stable id. The documents are untrusted: an office file is a zip of XML.

## Decision

- **Crate** `pacenotes-req`, pure: `zip` (deflate only), `roxmltree`, `csv`, `toml`. The port `RequirementSource` returns bytes and a format;
  an adapter reads the file the user chose.
- **One path.** Every importer produces a `Table` (rows of text cells); one function (`apply`) maps rows onto requirements with a **mapping
  pack** (TOML, version 1): which sheet or table, the header row, which header (or column letter) is the id, the text, the title and
  other named fields, and which columns fill down. A pack is checked on load and every refusal says what to fix.
- **Counted, not dropped**: rows with no id, no text, a repeated id (the first is kept) or nothing in them, and texts cut at 16 KiB.
- **Merged cells.** A workbook's merged range shows its top-left value in every cell; a Word table's `gridSpan` leaves empty cells to the
  right and a continuing `vMerge` takes the value above; `fill_down` in the pack covers the rest (a section column filled only on the
  first row).
- **Hostile documents.**
  - A zip is read with limits on entries (2,000), per part (64 MiB), in total (128 MiB) and on the **compression ratio (100)**, checked
    while reading and against what is really read, not against the sizes the archive declares; a bomb stops after the first megabyte past
    the ratio.
  - XML is read with the **document type definition off**: a `DOCTYPE` is refused, so an external entity names no file that is opened and
    an entity bomb never expands.
  - Sheets are read sparsely (a cell at `XFD1048576` is a limit error, not a grid), with limits on rows (100,000), columns (256), cells
    (2,000,000), merged cells and XML nodes (8,000,000).
  - Nothing panics (a never-panics test over the truncations and bit flips of each format).
- **Not read**: PDF, Google Docs (an export is a docx), formulas (the cached value is read), images, comments, tracked changes.

## Parallelism and cost (rule 51)

One pass per document; memory is proportional to the cells kept (bounded by the limits). A workbook is parsed as a tree per part; the
node limit bounds it. Nothing is measured yet; requirement documents are small next to code.

## Consequences

- A format that needs more (a second table in one sheet, a header over two rows) is a pack feature, added without changing the importers.
- Requirement ids come from the document, so a renumbered document is a different set; matching renumbered requirements is the link
  layer's problem, not this crate's.
