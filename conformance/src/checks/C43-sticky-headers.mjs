// C43: a sticky table header stays above its rows (rule 40; the user's report of 2026-10-28, "the cell will cover the
// header when scrolling", ADR 0027). In component styles and stylesheets (`.ts`, `.scss`, `.css` under `src/` or
// `projects/`), a rule that makes an element stick to the top (`position: sticky` with `top` or `inset-block-start`) must
// set a `z-index`: without one the header paints in tree order with the rows, and any later cell that is positioned or
// makes a stacking context (opacity below 1 on hover actions, `position: relative` on a badge, a transform) paints over it.
// And a sticky column (`position: sticky` with `left`, `right` or `inset-inline-*`) in the same file must not have a
// z-index at or above the header's: the later cell wins a tie. Use the ladder `--pd-z-table-header` above
// `--pd-z-table-sticky-column` above `--pd-z-table-row-action` (or an app's own numbers in that order). A rule that is not a
// table header (a sticky footer, a side panel) waives with `/* conformance: not-a-header */` in the rule. Specs are skipped.
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

/** What a declaration block makes sticky: 'top' (a header), 'side' (a column), or null. */
export function stickyKind(body) {
  if (!/position\s*:\s*sticky/i.test(body)) return null;
  if (decl(body, 'top') !== null || decl(body, 'inset-block-start') !== null) return 'top';
  if (['left', 'right', 'inset-inline-start', 'inset-inline-end'].some((p) => decl(body, p) !== null)) return 'side';
  return null;
}

/** Compares two z-index values: a number, or a custom property resolved by the family ladder; null when they cannot be compared. */
const LADDER = { '--pd-z-table-row-action': 1, '--pd-z-table-sticky-column': 2, '--pd-z-table-header': 3, '--pd-z-sticky': 10 };
export function zValue(z) {
  if (z === null) return null;
  if (/^-?\d+$/.test(z)) return Number(z);
  const v = /^var\(\s*(--[\w-]+)/.exec(z);
  return v && v[1] in LADDER ? LADDER[v[1]] : null;
}

/** The sticky rules of one file's text: headers ([{ line, selector, z }], z null when unset) and columns (same shape). */
export function stickyRules(text) {
  const blocks = cssBlocks(text).filter((b) => !WAIVER.test(b.body));
  const pick = (kind) => blocks.filter((b) => stickyKind(b.body) === kind).map((b) => ({ line: b.line, selector: b.selector, z: decl(b.body, 'z-index') }));
  return { headers: pick('top'), columns: pick('side') };
}

/**
 * The problems of a group of files (one app, or the shared UI): every header needs a z-index, and every sticky column must sit
 * below the lowest header of the group (a header and its columns are often styled in different files).
 * `files` is [{ file, text }]; the result is [{ file, line, message }].
 */
export function stickyProblems(files) {
  const rules = files.map(({ file, text }) => ({ file, ...stickyRules(text) }));
  const out = [];
  for (const r of rules) {
    for (const h of r.headers) {
      if (h.z === null) out.push({ file: r.file, line: h.line, message: `\`${h.selector}\` sticks to the top without a z-index: later positioned cells paint over it` });
    }
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
        error('C43', 40, p.file, p.line, p.message, 'Give the header `z-index: var(--pd-z-table-header)` (or a number above every cell and sticky column) and an opaque background, and sticky columns a lower z-index; mark a sticky element that is not a table header with `/* conformance: not-a-header */`.'),
      ),
    );
  },
};
