# 9. `pacenotes-req`: requirement importers and the mapping pack

Status: proposed (2026-10-30), design note of package RQ1 (plan, stage 1). Numbered at merge.

## Context

Requirements live in documents people already have: a spreadsheet, a Word file, a CSV export, a Markdown table. Pacenotes links them to code
and tests, so it needs them as `Requirement`s with a stable id. The documents are untrusted: an office file is a zip of XML.

## Decision

- **Crate** `pacenotes-req`, pure: `zip` 9 (flate2 with zlib-rs; no zopfli), `pacenotes-xml` (the one safe XML entry point), `csv`, `toml`. The port `RequirementSource` returns bytes and a format;
  an adapter reads the file the user chose.
- **Columns** are header texts (any case) or `column:C` for a letter, written as one: a header that merely looks like a letter (`ID`) is
  never read as column 238. `header_row` and `index` cannot be zero (they are non-zero types).
- **One path.** Every importer produces a `Table` (rows of text cells); one function (`apply`) maps rows onto requirements with a **mapping
  pack** (TOML, version 1): which sheet or table, the header row, which header (or column letter) is the id, the text, the title and
  other named fields, and which columns fill down. A pack is checked on load and every refusal says what to fix.
- **Counted, not dropped**: rows with no id, no text, a repeated id (the first is kept) or nothing in them, and texts cut at 16 KiB.
- **Merged cells.** A workbook's merged range shows its top-left value in every cell; a Word table's `gridSpan` leaves empty cells to the
  right and a continuing `vMerge` takes the value above; `fill_down` in the pack covers the rest (a section column filled only on the
  first row).
- **Hostile documents.**
  - A zip is read with limits on entries (2,000), per part (64 MiB), in total (128 MiB) and on the compression ratio (100). The byte
    limits are the hard bound; the ratio divides the bytes really read by the compressed size the archive *declares*, so a declared size
    that is too small can only make the check stricter and one that is too large cannot get past the byte limits. A bomb stops as it is
    read, not after it has filled memory.
  - **Every XML part goes through `pacenotes-xml` (ADR 0010) before the parser sees it**: a `DOCTYPE` with declarations is refused,
    so no entity expands and no file is named; nesting is counted by a scan that understands quoted attributes (a document of 5,000
    nested `<w:p>` is 523 bytes and used to overflow the stack) and refused past 256 levels; the node count is bounded. The guarantee
    is that of that crate, not of the parser's defaults.
  - Sheets are read sparsely, with limits on rows (100,000), columns (256), cells (2,000,000), merged cells and XML nodes. **A limit is
    checked only for a cell that has a value**: a styled, empty cell in `A200000` costs nothing. A cell reference that is not a cell
    (row 0, no column) is counted (`bad_cells`), and a shared-string index that does not exist is a malformed document that names the cell.
  - A Markdown table is collected only if the pack asks for it, and its columns and cells are checked while it is collected, from a byte
    count of `|` (4 MB of pipes is a limit error, not four million strings).
  - Nothing panics: the never-panics tests mutate the bytes **and** replace references and numbers by the values that have broken a
    reader (row 0, the largest of each width, one past it, a negative, a far corner, a span of zero).
- **Not read**: PDF, Google Docs (an export is a docx), formulas (the cached value is read), images, comments, tracked changes.

## Parallelism and cost (rule 51)

One pass per document; memory is proportional to the cells kept (bounded by the limits). A workbook is parsed as a tree per part; the
node limit bounds it. Nothing is measured yet; requirement documents are small next to code.

## Consequences

- A format that needs more (a second table in one sheet, a header over two rows) is a pack feature, added without changing the importers.
- Requirement ids come from the document, so a renumbered document is a different set; matching renumbered requirements is the link
  layer's problem, not this crate's.
