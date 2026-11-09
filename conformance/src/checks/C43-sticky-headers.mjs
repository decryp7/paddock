// C43: a sticky table header stays above its rows (rule 40; the user's report of 2026-10-28, "the cell will cover the
// header when scrolling", ADR 0027). In component styles and stylesheets (`.ts`, `.scss`, `.css` under `src/` or
// `projects/`), a rule that makes an element stick to the top or the bottom (`position: sticky` with `top`, `bottom`,
// `inset-block-*`, or an `inset`/`inset-block` shorthand with a side that is not `auto`) is a header or footer row: it must
// set a z-index of at least the header level, 3 (`--pd-z-table-header`). Without one (or with `auto` or 0) it paints in tree
// order with the rows, and any later cell that is positioned or makes a stacking context (opacity below 1 on hover actions,
// `position: relative` on a badge, a transform) paints over it. A column's header cell (sticky both ways) needs the corner
// level, 4 (`--pd-z-table-corner`), and a sticky column (`left`, `right`, `inset-inline-*`) must sit below the app's lowest
// header: the later cell wins a tie. A sticky element that is not a table row (a side panel, an action bar that is not a
// table's) waives with `/* conformance: not-a-header */` in the rule. Specs are skipped.
import { error } from '../finding.mjs';

const SOURCE = /(^|\/)(src|projects)\/.*\.(ts|scss|css)$/;
const SKIP = /\.spec\.ts$|\.test\.ts$|(^|\/)(e2e|node_modules|dist)\//;
const WAIVER = /conformance:\s*not-a-header/;

/** The CSS declaration blocks of a file: [{ selector, body, line }]. A block's body is its own text without its nested blocks (SCSS);
 * template interpolations (`${HEADER_H}px`) count as a value. Braces of TypeScript code make blocks of their own, which never stick. */
export function cssBlocks(source) {
  // Interpolations count as a value; braces and semicolons inside comments do not count (the comment itself stays, for the waiver).
  const text = source.replace(/\$\{[^{}]*\}/g, (m) => '0'.padEnd(m.length, ' ')).replace(/\/\*[\s\S]*?\*\//g, (m) => m.replace(/[{};]/g, ' '));
  const out = [];
  const stack = [];
  let seg = 0;
  for (let i = 0; i < text.length; i++) {
    const c = text[i];
    if (c === '{') {
      const chunk = text.slice(seg, i);
      // A selector starts after the last declaration (`;`) or the backtick that opens a component's styles.
      const cut = Math.max(chunk.lastIndexOf(';'), chunk.lastIndexOf('`'));
      if (stack.length) stack[stack.length - 1].body += chunk.slice(0, cut + 1);
      stack.push({ selector: chunk.slice(cut + 1).replace(/\/\*[\s\S]*?\*\//g, '').replace(/\s+/g, ' ').trim().slice(-120), line: text.slice(0, i).split('\n').length, body: '' });
      seg = i + 1;
    } else if (c === '}') {
      const block = stack.pop();
      if (block) {
        block.body += text.slice(seg, i);
        out.push(block);
      }
      seg = i + 1;
    }
  }
  return out;
}

const decl = (body, prop) => {
  const m = new RegExp(`(?:^|[;{\\s])${prop}\\s*:\\s*([^;}]+)`, 'i').exec(body);
  return m ? m[1].trim().replace(/\s*!important$/, '') : null;
};

/** The values of a box shorthand (`inset: 0 auto`): [top, right, bottom, left] (or [start, end] for the logical pairs). */
const sides = (value, n) => {
  const v = value.split(/\s+/).filter(Boolean);
  return n === 4 ? [v[0], v[1] ?? v[0], v[2] ?? v[0], v[3] ?? v[1] ?? v[0]] : [v[0], v[1] ?? v[0]];
};
const set = (v) => v !== undefined && v !== null && v !== 'auto';

/** The edges a sticky element sticks to: `block` (top or bottom: a header or footer row) and `inline` (left or right: a column). */
export function stickyEdges(body) {
  if (!/position\s*:\s*sticky/i.test(body)) return null;
  const inset = decl(body, 'inset');
  const [it, ir, ib, il] = inset ? sides(inset, 4) : [];
  const ibk = decl(body, 'inset-block');
  const iin = decl(body, 'inset-inline');
  const block = [decl(body, 'top'), decl(body, 'bottom'), decl(body, 'inset-block-start'), decl(body, 'inset-block-end'), it, ib, ...(ibk ? sides(ibk, 2) : [])].some(set);
  const inline = [decl(body, 'left'), decl(body, 'right'), decl(body, 'inset-inline-start'), decl(body, 'inset-inline-end'), ir, il, ...(iin ? sides(iin, 2) : [])].some(set);
  return block || inline ? { block, inline } : null;
}

/** What a declaration block makes sticky: 'edge' (a header or footer row), 'side' (a column), 'corner' (both: a column's header cell), or null. */
export function stickyKind(body) {
  const e = stickyEdges(body);
  if (!e) return null;
  return e.block && e.inline ? 'corner' : e.block ? 'edge' : 'side';
}

/** The family ladder (`_tokens.scss`): what a z-index custom property is worth; anything else cannot be compared. */
const LADDER = { '--pd-z-table-row-action': 1, '--pd-z-table-sticky-column': 2, '--pd-z-table-header': 3, '--pd-z-table-corner': 4, '--pd-z-sticky': 10 };
const HEADER = LADDER['--pd-z-table-header'];
const CORNER = LADDER['--pd-z-table-corner'];
/** A z-index as a number: an integer, or a ladder token; null when it is not set or cannot be compared (`auto`, another property). */
export function zValue(z) {
  if (z === null) return null;
  if (/^-?\d+$/.test(z)) return Number(z);
  const v = /^var\(\s*(--[\w-]+)/.exec(z);
  return v && v[1] in LADDER ? LADDER[v[1]] : null;
}

/** The sticky rules of one file's text by kind: [{ line, selector, z }] (z: the declared text, null when unset). */
export function stickyRules(text) {
  const blocks = cssBlocks(text).filter((b) => !WAIVER.test(b.body));
  const pick = (kind) => blocks.filter((b) => stickyKind(b.body) === kind).map((b) => ({ line: b.line, selector: b.selector, z: decl(b.body, 'z-index') }));
  return { headers: pick('edge'), corners: pick('corner'), columns: pick('side') };
}

/**
 * The problems of a group of files (one app, or the shared UI). A header or footer row needs a z-index of at least the header
 * level (3, `--pd-z-table-header`), a column's header cell (sticky both ways) at least the corner level (4), and every sticky
 * column must sit below the lowest header of the group (a header and its columns are often styled in different files).
 * `files` is [{ file, text }]; the result is [{ file, line, message }].
 */
export function stickyProblems(files) {
  const rules = files.map(({ file, text }) => ({ file, ...stickyRules(text) }));
  const out = [];
  const atLeast = (r, h, min, what, token) => {
    const z = zValue(h.z);
    if (h.z === null) out.push({ file: r.file, line: h.line, message: `\`${h.selector}\` is a sticky ${what} without a z-index: later positioned cells paint over it` });
    else if (z === null) out.push({ file: r.file, line: h.line, message: `\`${h.selector}\` is a sticky ${what} with z-index ${h.z}, which is not a number or a ladder token: use ${token} (${min})` });
    else if (z < min) out.push({ file: r.file, line: h.line, message: `\`${h.selector}\` is a sticky ${what} at z-index ${z}, below ${token} (${min}): a cell or column at its level paints over it` });
  };
  for (const r of rules) {
    for (const h of r.headers) atLeast(r, h, HEADER, 'header or footer row', '`--pd-z-table-header`');
    for (const c of r.corners) atLeast(r, c, CORNER, 'header cell of a sticky column', '`--pd-z-table-corner`');
  }
  const headerZs = rules.flatMap((r) => r.headers.map((h) => zValue(h.z))).filter((z) => z !== null);
  if (headerZs.length) {
    const lowest = Math.min(...headerZs);
    for (const r of rules) {
      for (const c of r.columns) {
        const z = zValue(c.z);
        if (z !== null && z >= lowest) out.push({ file: r.file, line: c.line, message: `the sticky column \`${c.selector}\` has z-index ${z}, not below the lowest header's ${lowest}: its cells paint over the header` });
      }
    }
  }
  return out;
}

/** The group a file belongs to: its app (`apps/<app>/`), or one entry point of a shared package (`ui/projects/<pkg>/<entry>/`), or the repo. */
const groupOf = (f) => /^apps\/[^/]+\//.exec(f)?.[0] ?? /^ui\/projects\/[^/]+\/[^/]+\//.exec(f)?.[0] ?? '';

export default {
  id: 'C43',
  scope: 'both',
  mode: 'gate',
  title: 'sticky table headers stay above their rows',
  rule: 40,
  run(ctx) {
    const groups = new Map();
    for (const file of ctx.files()) {
      if (!SOURCE.test(file) || SKIP.test(file)) continue;
      const text = ctx.read(file) ?? '';
      if (!/position\s*:\s*sticky/i.test(text)) continue;
      const g = groupOf(file);
      groups.set(g, [...(groups.get(g) ?? []), { file, text }]);
    }
    return [...groups.values()].flatMap((files) =>
      stickyProblems(files).map((p) =>
        error('C43', 40, p.file, p.line, p.message, 'Give a header or footer row `z-index: var(--pd-z-table-header)` (3; an app without the --pd tokens writes the number), a sticky column\'s header cell `var(--pd-z-table-corner)` (4), sticky columns `var(--pd-z-table-sticky-column)` (2), and an opaque background; mark a sticky element that is not a table row with `/* conformance: not-a-header */`.'),
      ),
    );
  },
};
