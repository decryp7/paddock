// Markdown rendering problems that Gitea (goldmark, GFM) shows wrongly. Pure: text in, problems out; no dependencies.
// Kinds: table, fence, link, anchor, list, html. `exists(relPath)` says whether a repo-relative path exists.
import { posix } from 'node:path';

const VOID_OR_FREE = new Set(['br', 'hr', 'img', 'p', 'li', 'td', 'th', 'tr', 'thead', 'tbody', 'input', 'wbr']);
const PAIRED = new Set(['details', 'summary', 'div', 'span', 'a', 'b', 'i', 'em', 'strong', 'kbd', 'sup', 'sub', 'code', 'pre', 'table', 'ul', 'ol', 'center', 'blockquote', 'h1', 'h2', 'h3', 'h4', 'h5', 'h6']);

/** Cells of a table row; `\|` is not a separator. Leading and trailing pipes are dropped. */
export function splitRow(line) {
  let s = line.trim();
  if (s.startsWith('|')) s = s.slice(1);
  const cells = [];
  let cur = '';
  for (let i = 0; i < s.length; i++) {
    if (s[i] === '\\' && s[i + 1] === '|') {
      cur += '\\|';
      i++;
    } else if (s[i] === '|') {
      cells.push(cur);
      cur = '';
    } else cur += s[i];
  }
  if (cur.trim() !== '' || cells.length === 0) cells.push(cur);
  return cells;
}

/** A line that starts another block, so it ends a table instead of joining it. */
const BLOCK_START = /^\s*([-*+]\s|\d+[.)]\s|#{1,6}\s|>|`{3,}|~{3,}|<[a-zA-Z!/])/;
const DELIM_CELL = /^\s*:?-+:?\s*$/;
/** A GFM delimiter row: only dashes, colons, pipes and spaces, with at least one pipe. */
export function isDelimiterRow(line) {
  return line.includes('|') && /^[\s|:-]+$/.test(line) && line.includes('-') && splitRow(line).every((c) => DELIM_CELL.test(c));
}

/** Lines outside fenced code, and the fence problems. `inFence[i]` is true for fence lines and their content. */
function scanFences(lines, problems) {
  const inFence = new Array(lines.length).fill(false);
  let open = null;
  for (let i = 0; i < lines.length; i++) {
    const m = /^\s*(`{3,}|~{3,})(.*)$/.exec(lines[i]);
    if (open) {
      inFence[i] = true;
      if (m && m[1][0] === open.ch && m[1].length >= open.len && m[2].trim() === '') open = null;
      else if (m && m[1][0] === open.ch && m[2].trim() === '' && m[1].length < open.len) open.short = i + 1;
    } else if (m && !(m[1][0] === '`' && m[2].includes('`'))) {
      open = { ch: m[1][0], len: m[1].length, line: i + 1, short: 0 };
      inFence[i] = true;
    }
  }
  if (open) {
    const hint = open.short ? ` (line ${open.short} has a shorter closing fence than the ${open.len} of the opening one)` : '';
    problems.push({ line: open.line, kind: 'fence', message: `code fence opened here never closes${hint}` });
  }
  return inFence;
}

const stripCode = (s) => s.replace(/(`+)(.+?)\1/g, (m) => ' '.repeat(m.length));

function checkTables(lines, inFence, problems) {
  const live = (i) => !inFence[i];
  let i = 0;
  while (i < lines.length) {
    if (live(i) && i > 0 && live(i - 1) && lines[i - 1].includes('|') && isDelimiterRow(lines[i]) && !/^\s{4,}/.test(lines[i - 1])) {
      const headerAt = i - 1;
      const n = splitRow(lines[headerAt]).length;
      const d = splitRow(lines[i]).length;
      if (n !== d) problems.push({ line: i + 1, kind: 'table', message: `delimiter row has ${d} cells, header has ${n}: not a table` });
      const before = headerAt > 0 ? lines[headerAt - 1] : '';
      if (before.trim() !== '' && !before.includes('|') && !/^\s*(#|>|[-*+] |\d+[.)] |<)/.test(before) && live(headerAt - 1))
        problems.push({ line: headerAt + 1, kind: 'table', message: 'table starts right after a paragraph line without a blank line' });
      let j = i + 1;
      while (j < lines.length && lines[j].trim() !== '' && live(j)) {
        if (BLOCK_START.test(lines[j])) break;
        if (!lines[j].includes('|')) problems.push({ line: j + 1, kind: 'table', message: 'line without a pipe inside a table: renders as a one-cell row' });
        else {
          const c = splitRow(lines[j]).length;
          if (c !== n) {
            const hint = /`[^`]*[^\\]\|[^`]*`/.test(lines[j]) ? ' (an unescaped | inside backticks splits the cell: write \\|)' : ' (an unescaped | in a cell: write \\|)';
            problems.push({ line: j + 1, kind: 'table', message: `row has ${c} cells, header has ${n}${c > n ? hint : ''}` });
          }
        }
        j++;
      }
      // a blank line, then more rows: the table was split
      let k = j;
      while (k < lines.length && lines[k].trim() === '') k++;
      if (k > j && k < lines.length && live(k) && /^\s*\|/.test(lines[k]) && lines[k].trim().endsWith('|') && !isDelimiterRow(lines[k + 1] ?? ''))
        problems.push({ line: j + 1, kind: 'table', message: `blank line splits the table; rows continue at line ${k + 1}` });
      i = j;
    } else i++;
  }
}

function slug(text) {
  return text
    .replace(/<[^>]*>/g, '')
    .replace(/\[([^\]]*)\]\([^)]*\)/g, '$1')
    .replace(/[`*_~]/g, '')
    .trim()
    .toLowerCase()
    .replace(/[^\p{L}\p{N}\s_-]/gu, '')
    .replace(/\s/g, '-');
}

function headingSlugs(lines, inFence) {
  const seen = new Map();
  const out = new Set();
  lines.forEach((l, i) => {
    const m = !inFence[i] && /^ {0,3}#{1,6}\s+(.+?)\s*#*\s*$/.exec(l);
    if (!m) return;
    const base = slug(m[1]);
    const n = seen.get(base) ?? 0;
    seen.set(base, n + 1);
    out.add(n === 0 ? base : `${base}-${n}`);
    out.add(base);
  });
  return out;
}

function checkLinks(lines, inFence, file, exists, problems) {
  const slugs = headingSlugs(lines, inFence);
  const dir = posix.dirname(file);
  lines.forEach((raw, i) => {
    if (inFence[i]) return;
    const line = stripCode(raw);
    const targets = [];
    for (const m of line.matchAll(/!?\[(?:[^\]\\]|\\.)*\]\(\s*<?([^)\s>]*)>?(?:\s+(?:"[^"]*"|'[^']*'))?\s*\)/g)) targets.push(m[1]);
    const ref = /^\s{0,3}\[[^\]]+\]:\s+<?(\S+?)>?(?:\s+.*)?$/.exec(line);
    if (ref) targets.push(ref[1]);
    for (const t of targets) {
      if (t === '' || /^[a-z][a-z0-9+.-]*:/i.test(t) || t.startsWith('//') || t.startsWith('/') || t.startsWith('~') || /[{}$]|<|>/.test(t)) continue;
      const [pathPart, frag] = [t.split('#')[0].split('?')[0], t.includes('#') ? t.slice(t.indexOf('#') + 1) : null];
      if (pathPart === '') {
        if (frag && frag !== '' && !/^(L\d+|top)/.test(frag) && !slugs.has(decodeURIComponent(frag).toLowerCase()))
          problems.push({ line: i + 1, kind: 'anchor', message: `anchor #${frag} matches no heading in this file` });
        continue;
      }
      let decoded = pathPart;
      try {
        decoded = decodeURIComponent(pathPart);
      } catch {
        /* keep as written */
      }
      const target = posix.normalize(posix.join(dir, decoded));
      if (target.startsWith('..')) continue;
      if (!exists(target.replace(/\/$/, ''))) problems.push({ line: i + 1, kind: 'link', message: `link target ${t} does not exist (${target})` });
    }
  });
}

function checkLists(lines, inFence, problems) {
  for (let i = 0; i + 2 < lines.length; i++) {
    if (inFence[i]) continue;
    const m = /^(\s*)([-*+]|\d+[.)])(\s+)\S/.exec(lines[i]);
    if (!m) continue;
    const offset = m[1].length + m[2].length + m[3].length;
    if (lines[i + 1].trim() !== '') continue;
    const next = lines[i + 2];
    const ind = /^ */.exec(next)[0].length;
    if (next.trim() === '' || inFence[i + 2] || ind === 0 || ind >= offset || ind <= m[1].length) continue;
    if (/^\s*([-*+]|\d+[.)])\s/.test(next) || /^\s*[|>#<`~]/.test(next)) continue;
    problems.push({ line: i + 3, kind: 'list', message: `continuation of the list item at line ${i + 1} is indented ${ind}, needs ${offset}: renders as a new paragraph` });
  }
}

function checkHtml(lines, inFence, problems) {
  const masked = lines.map((l, i) => (inFence[i] ? '' : stripCode(l))).join('\n').replace(/<!--[\s\S]*?-->/g, (m) => m.replace(/[^\n]/g, ' '));
  const stack = [];
  for (const m of masked.matchAll(/<(\/?)([a-zA-Z][a-zA-Z0-9-]*)((?:\s+[^<>]*?)?)(\/?)>/g)) {
    const name = m[2].toLowerCase();
    if (!PAIRED.has(name) || VOID_OR_FREE.has(name) || m[4] === '/') continue;
    const line = masked.slice(0, m.index).split('\n').length;
    if (m[1] === '') stack.push({ name, line });
    else {
      const at = stack.map((s) => s.name).lastIndexOf(name);
      if (at < 0) problems.push({ line, kind: 'html', message: `closing </${name}> without an opening tag` });
      else stack.length = at;
    }
  }
  for (const s of stack) problems.push({ line: s.line, kind: 'html', message: `<${s.name}> is never closed` });
}

/** All rendering problems of one Markdown file: [{ line, kind, message }], sorted by line. */
export function lintMarkdown(text, { file, exists }) {
  const lines = text.replace(/\r\n/g, '\n').split('\n');
  const problems = [];
  const inFence = scanFences(lines, problems);
  checkTables(lines, inFence, problems);
  checkLinks(lines, inFence, file, exists, problems);
  checkLists(lines, inFence, problems);
  checkHtml(lines, inFence, problems);
  return problems.sort((a, b) => a.line - b.line);
}
