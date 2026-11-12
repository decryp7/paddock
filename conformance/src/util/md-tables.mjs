// GFM table checks over Markdown text (what Gitea renders, GFM spec section 4.10). Pure: text in, findings out.
// A table is a header row, a delimiter row with the same cell count, then body rows up to the first blank line or other block.
// A row splits at every `|` not escaped as `\|`, backticks do not protect a pipe (a pipe inside code must be written `\|`).

const DELIM_CELL = /^\s*:?-+:?\s*$/;

/** Cells of a table row, GFM way: outer pipes optional, `\|` is text. */
export function splitRow(line) {
  let s = line.trim();
  if (s.startsWith('|')) s = s.slice(1);
  if (s.endsWith('|') && !s.endsWith('\\|')) s = s.slice(0, -1);
  const cells = [];
  let cur = '';
  for (let i = 0; i < s.length; i++) {
    if (s[i] === '\\' && s[i + 1] === '|') { cur += '\\|'; i++; } else if (s[i] === '|') { cells.push(cur); cur = ''; } else cur += s[i];
  }
  cells.push(cur);
  return cells;
}

/** True for a delimiter row (`| --- | :-: |`): every cell dashes with optional colons, and a pipe or a single cell with a pipe. */
export function isDelimiterRow(line) {
  if (!line.includes('|') && !/^\s*:?-+:?\s*$/.test(line)) return false;
  if (!line.includes('|')) return false;
  return splitRow(line).every((c) => DELIM_CELL.test(c));
}

/** Lines that start another block, which ends a table without becoming a row (a list item, heading, quote, fence, HTML, rule). */
const BLOCK_START = /^\s{0,3}([-*+]\s|\d+[.)]\s|#{1,6}(\s|$)|>|<|(`{3,}|~{3,})|([-*_]\s*){3,}$)/;
const hasPipe = (l) => /(^|[^\\])\|/.test(l);
const strip = (l) => l.replace(/^(\s*>)+/, '');

/** Findings `{ line, problem, fix }` (1-based lines) for the tables in `text`. */
export function tableFindings(text) {
  const lines = text.split(/\r?\n/);
  const out = [];
  let fence = null;
  let table = null; // { cols, trailing, leading, start }
  const add = (line, problem, fix) => out.push({ line, problem, fix });
  for (let i = 0; i < lines.length; i++) {
    const raw = lines[i];
    const l = strip(raw);
    const fm = /^\s*(`{3,}|~{3,})/.exec(l);
    if (fence) { if (fm && fm[1][0] === fence[0] && fm[1].length >= fence.length) fence = null; continue; }
    if (fm) { fence = fm[1]; table = null; continue; }
    const blank = l.trim() === '';
    if (table) {
      if (blank) { table = null; continue; }
      if (!hasPipe(l) && BLOCK_START.test(l)) { table = null; continue; }
      if (!hasPipe(l)) { add(i + 1, 'a plain text line right after a table becomes one more row of it (GFM)', 'Put a blank line between the table and the text.'); table = null; continue; }
      const n = splitRow(l).length;
      if (n !== table.cols) add(i + 1, `row has ${n} cells, the header has ${table.cols} (an unescaped | inside a cell, a missing cell or a row split over two lines)`, 'Escape a literal pipe as \\|, join split rows, and give the row exactly the header\'s cell count.');
      const t = l.trimEnd();
      if (table.trailing && !(t.endsWith('|') && !t.endsWith('\\|'))) add(i + 1, 'row does not end with | where the header does', 'Add the closing | (a row that lost it often means a following row was merged into it).');
      continue;
    }
    // A table starts at a header line followed by a delimiter row.
    const next = i + 1 < lines.length ? strip(lines[i + 1]) : '';
    if (!blank && hasPipe(l) && isDelimiterRow(next)) {
      const cols = splitRow(l).length;
      const dcols = splitRow(next).length;
      if (cols !== dcols) { add(i + 1, `header has ${cols} cells but the delimiter row has ${dcols}: GFM does not render a table`, 'Make the header and delimiter rows the same cell count.'); i++; continue; }
      table = { cols, trailing: l.trimEnd().endsWith('|') };
      i++;
      continue;
    }
    if (!blank && isDelimiterRow(l)) { add(i + 1, 'delimiter row without a header row above it: not rendered as a table', 'Add the header row directly above, with no blank line between.'); continue; }
    // A pipe-bordered line outside a table: a row cut off from its table (usually by a blank line).
    const t = l.trim();
    if (t.startsWith('|') && t.endsWith('|') && !t.endsWith('\\|') && splitRow(t).length >= 2 && !/^\|+$/.test(t)) {
      add(i + 1, 'table row outside a table (a blank line or a broken row above ends the table, so this shows as plain text)', 'Remove the blank line above, or restore the header and delimiter rows.');
    }
  }
  return out;
}
