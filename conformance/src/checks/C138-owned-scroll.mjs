// C138: a scroller whose position our code computes makes no scroll moves of its own (family ADR 0105; ER-21, the class: ADR 0072's
// bug 3, a write clamped before the spacer grew, and Debrief's "resize while not pinned" row, CSS scroll anchoring moved the view two
// rows; both in a virtual grid). In the TypeScript of `apps/*/` and `ui/projects/` (specs, tests, e2e and the e2e kit skipped), a write
// of the scroll position is `scrollTop`/`scrollLeft` assigned (`.x` or `['x']`, with `=`, `+=`, `-=`, `||=`, `&&=`, `??=`, or
// `Object.assign(el, { scrollTop })`), or a call of `scrollTo(`/`scrollBy(` on an element (any receiver, also `?.`, `)` and `]`, except
// `window`, `document` and `globalThis`) or of the CDK's `scrollToOffset(`/`scrollToIndex(`. A component file with one passes when its
// template (inline or `templateUrl`) or its styles carry the shared style, read with comments (TypeScript and HTML) blanked: the
// `pdOwnedScroll` directive (`@decryp7/paddock-ui/scroll`), the `pd-owned-scroll` class or `owned-scroll` mixin (`_base.scss`), or a
// `cdk-virtual-scroll-viewport` (the CDK sets the same two properties on it). Anything else carries `// conformance: owned-scroll — <why>`
// on the line or on a comment-only line above: a helper that scrolls an element it is given, a third-party element, and a chat log or
// assistant log, which keeps the browser's anchoring on purpose (ADR 0105 decision 2: it keeps the reader's place).
// Accepted gaps (not seen): `scrollIntoView` (it scrolls every ancestor, and the element it reveals is not the scroller),
// `Renderer2.setProperty(el, 'scrollTop', ..)`, and per file, not per element: a component with two scrollers passes when either carries
// the style. The reasoned waivers are listed in one warning, as C115 does. ER-41: blocking (mode gate) since its first green run on the real tree, ci.yml run 9030 (b3ad2dd2d, 2026-11-25); ER-33: a positive control runs on in-memory text
// first.
import { posix } from 'node:path';
import { unwaivable, error, warn } from '../finding.mjs';

export const SEVERITY = error;
const SOURCE = /^(apps\/[^/]+\/|ui\/projects\/).*\.ts$/;
const SKIP = /\.spec\.ts$|\.test\.ts$|\.d\.ts$|(^|\/)(e2e|e2e-tauri|node_modules|dist|scripts|test|testing)\/|^ui\/projects\/paddock-e2e\//;
const WAIVER = /conformance:\s*owned-scroll\s*[—–-]+\s*(\S.{2,})/;
const BARE_WAIVER = /conformance:\s*owned-scroll/;
const MARKER = /\bpdOwnedScroll\b|\bpd-owned-scroll\b|@include\s+[\w.-]*owned-scroll\b|<cdk-virtual-scroll-viewport\b/;
const PROP = String.raw`(?:\.\s*(scrollTop|scrollLeft)\b|\[\s*['"](scrollTop|scrollLeft)['"]\s*\])`;
const ASSIGN = String.raw`\s*(?:=(?![=>])|\+=|-=|\|\|=|&&=|\?\?=)`;
const WRITES = [
  { re: new RegExp(PROP + ASSIGN, 'g'), what: (m) => m[1] ?? m[2] },
  { re: /Object\.assign\s*\([^)]*?\{[^}]*?\b(scrollTop|scrollLeft)\b/g, what: (m) => m[1] },
  { re: /(\?\.|\.)\s*(scrollTo|scrollBy)\s*\(/g, what: (m) => m[2], receiver: true },
  { re: /(\?\.|\.)\s*(scrollToOffset|scrollToIndex)\s*\(/g, what: (m) => m[2] },
];
const PAGE = /(?:^|[^\w$.])(window|document|globalThis)\s*$/;

/**
 * The text with comments blanked (newlines kept, so indexes and lines stay). Strings and template literals stay (a template is markup the
 * writes never match), and `//` inside a string is not a comment. HTML comments inside a template literal are blanked too.
 */
export function maskComments(text) {
  let out = '';
  let i = 0;
  const blank = (s) => s.replace(/[^\n]/g, ' ');
  while (i < text.length) {
    const c = text[i];
    const n = text[i + 1];
    if (c === '/' && n === '/') {
      const end = text.indexOf('\n', i);
      const stop = end < 0 ? text.length : end;
      out += blank(text.slice(i, stop));
      i = stop;
    } else if (c === '/' && n === '*') {
      const end = text.indexOf('*/', i + 2);
      const stop = end < 0 ? text.length : end + 2;
      out += blank(text.slice(i, stop));
      i = stop;
    } else if (c === "'" || c === '"' || c === '`') {
      let j = i + 1;
      while (j < text.length && text[j] !== c) {
        if (text[j] === '\\') j++;
        else if (c !== '`' && text[j] === '\n') break;
        j++;
      }
      const lit = text.slice(i, Math.min(j + 1, text.length));
      out += c === '`' ? lit.replace(/<!--[\s\S]*?-->/g, blank) : lit;
      i = j + 1;
    } else {
      out += c;
      i++;
    }
  }
  return out;
}

/** The scroll-position writes of a file: [{ line, what, waived }] (`waived`: 'reason', 'no-reason' or undefined). */
export function scrollWrites(text) {
  const masked = maskComments(text);
  const lines = text.split('\n');
  const out = [];
  const seen = new Set();
  for (const w of WRITES) {
    for (const m of masked.matchAll(w.re)) {
      if (w.receiver && PAGE.test(masked.slice(Math.max(0, m.index - 80), m.index))) continue;
      const line = masked.slice(0, m.index).split('\n').length;
      if (seen.has(line)) continue;
      seen.add(line);
      const here = lines[line - 1] ?? '';
      const above = lines[line - 2] ?? '';
      const near = here + (/^\s*(\/\/|\*|\/\*)/.test(above) ? `\n${above}` : '');
      out.push({ line, what: w.what(m), waived: WAIVER.test(near) ? 'reason' : BARE_WAIVER.test(near) ? 'no-reason' : undefined });
    }
  }
  return out.sort((a, b) => a.line - b.line);
}

/** Whether a component file's template or styles carry the shared style (comments blanked); `read(relative)` reads a `templateUrl`/`styleUrl(s)` file. */
export function carriesStyle(text, read) {
  const masked = maskComments(text);
  if (MARKER.test(masked)) return true;
  const urls = [...masked.matchAll(/(?:templateUrl|styleUrl)\s*:\s*['"]([^'"]+)['"]/g)].map((m) => m[1]);
  for (const m of masked.matchAll(/styleUrls\s*:\s*\[([^\]]*)\]/g)) urls.push(...[...m[1].matchAll(/['"]([^'"]+)['"]/g)].map((x) => x[1]));
  return urls.some((u) => {
    const t = read(u) ?? '';
    const clean = u.endsWith('.html') ? t.replace(/<!--[\s\S]*?-->/g, ' ') : maskComments(t);
    return MARKER.test(clean);
  });
}

const isComponent = (text) => /@Component\s*\(/.test(text);

/** The problems of one file: [{ line, message }]. */
export function ownedScrollProblems(text, read) {
  const writes = scrollWrites(text).filter((w) => w.waived !== 'reason');
  if (!writes.length) return [];
  const styled = isComponent(text) && carriesStyle(text, read);
  return writes
    .filter((w) => w.waived === 'no-reason' || !styled)
    .map((w) => ({
      line: w.line,
      message:
        w.waived === 'no-reason'
          ? `the \`owned-scroll\` waiver for this \`${w.what}\` has no reason`
          : isComponent(text)
            ? `writes \`${w.what}\` but no scroller in this component carries the shared style: the browser may move it by itself (scroll anchoring) and the code reads that as the user's scroll`
            : `writes \`${w.what}\` outside a component: the scroller it is given is not known to carry the shared style`,
    }));
}

/** ER-33: the probe must see a known write, a known marker and a known waiver before its result counts. */
export function positiveControl() {
  const bad = '@Component({ template: `<div #s></div>` })\nclass A { f(el: HTMLElement) { el.scrollTop = 3; } }\n';
  const good = '@Component({ template: `<div #s pdOwnedScroll></div>` })\nclass A { f(el: HTMLElement) { el.scrollTop = 3; } }\n';
  const waived = 'export function f(el: HTMLElement) {\n  // conformance: owned-scroll — a test helper\n  el.scrollBy(0, 1);\n}\n';
  const none = () => null;
  return ownedScrollProblems(bad, none).length === 1 && ownedScrollProblems(good, none).length === 0 && ownedScrollProblems(waived, none).length === 0;
}

const FIX =
  'Put `pdOwnedScroll` (`@decryp7/paddock-ui/scroll`) on the scroller in the template (or the `pd-owned-scroll` class / `owned-scroll` mixin in its styles); a helper or a third-party element carries `// conformance: owned-scroll — <why>`.';

export default {
  id: 'C138',
  scope: 'both',
  mode: 'gate',
  title: 'Scrollers our code positions make no scroll moves of their own',
  rule: 40,
  run(ctx) {
    if (!positiveControl()) return [unwaivable(error('C138', 40, 'conformance/src/checks/C138-owned-scroll.mjs', 1, 'the positive control failed: the check cannot see a known scroll write', 'Fix the check before trusting its result.'))];
    const out = [];
    const waived = [];
    for (const file of ctx.files()) {
      if (!SOURCE.test(file) || SKIP.test(file)) continue;
      const text = ctx.read(file);
      if (!text || !/scroll(Top|Left|To|By)/.test(text)) continue;
      const dir = file.split('/').slice(0, -1).join('/');
      const read = (rel) => ctx.read(posix.normalize(posix.join(dir, rel)));
      for (const p of ownedScrollProblems(text, read)) out.push(SEVERITY('C138', 40, file, p.line, p.message, FIX));
      for (const w of scrollWrites(text)) if (w.waived === 'reason') waived.push({ file, line: w.line });
    }
    // The reasoned waivers in one warning (as C115 does), so the summary shows them and a reviewer sees where they are.
    if (waived.length) {
      const list = waived.slice(0, 12).map((s) => `${s.file}:${s.line}`).join(', ');
      out.push(warn('C138', 40, waived[0].file, waived[0].line, `${waived.length} site${waived.length === 1 ? '' : 's'} waived by an owned-scroll comment: ${list}${waived.length > 12 ? ', ...' : ''}`, 'Check that each reason still holds when the site changes.'));
    }
    return out;
  },
};
