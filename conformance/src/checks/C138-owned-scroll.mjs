// C138: a scroller whose position our code sets makes no scroll moves of its own (family ADR 0105; ER-21, the class: ADR 0072's bug 3,
// a write clamped before the spacer grew, and Debrief's "resize while not pinned" row, CSS scroll anchoring moved the view two rows).
// In the TypeScript of `apps/*/` and `ui/projects/` (specs, tests, e2e and the e2e kit skipped), a write of the scroll position is
// `.scrollTop`/`.scrollLeft` assigned (`=`, `+=`, `-=`, `Object.assign(el, { scrollTop })`) or a call of `.scrollTo(`, `.scrollBy(`
// on an element (not `window` or `document`) or of the CDK's `.scrollToOffset(`/`.scrollToIndex(`. A component file with one passes
// when its template (inline or `templateUrl`) or its styles carry the shared style: the `pdOwnedScroll` directive
// (`@decryp7/paddock-ui/scroll`), the `pd-owned-scroll` class or `owned-scroll` mixin (`_base.scss`), or a `cdk-virtual-scroll-viewport`
// (the CDK sets the same two properties on it). A file that is no component (a helper that scrolls an element it is given) and a
// write the style cannot reach (a third-party element) carry `// conformance: owned-scroll — <why>` on the line or on a comment-only
// line above. Known limit: the check is per file, not per element: a component with two scrollers passes when either carries the style.
// ER-41: report-only (warnings) until its first green run on the real tree; ER-33: a positive control runs on in-memory text first.
import { posix } from 'node:path';
import { unwaivable, error, warn } from '../finding.mjs';

export const SEVERITY = warn;
const SOURCE = /^(apps\/[^/]+\/|ui\/projects\/).*\.ts$/;
const SKIP = /\.spec\.ts$|\.test\.ts$|\.d\.ts$|(^|\/)(e2e|e2e-tauri|node_modules|dist|scripts|test|testing)\/|^ui\/projects\/paddock-e2e\//;
const WAIVER = /conformance:\s*owned-scroll\s*[—–-]+\s*(\S.{2,})/;
const BARE_WAIVER = /conformance:\s*owned-scroll/;
const MARKER = /\bpdOwnedScroll\b|\bpd-owned-scroll\b|@include\s+[\w.-]*owned-scroll\b|<cdk-virtual-scroll-viewport\b/;
const WRITES = [
  /\.(scrollTop|scrollLeft)\s*(?:=(?!=)|\+=|-=)/g,
  /Object\.assign\([^)]*\{[^}]*\b(scrollTop|scrollLeft)\b/g,
  /(?<![\w$])(?!window\.|document\.)([\w$]+(?:\(\))?(?:\.[\w$]+(?:\(\))?)*)\.(scrollTo|scrollBy)\s*\(/g,
  /\.(scrollToOffset|scrollToIndex)\s*\(/g,
];

/** The text with comments blanked (newlines kept, so indexes and lines stay); strings stay, a template is markup the writes never match. */
function maskComments(text) {
  return text.replace(/\/\*[\s\S]*?\*\//g, (m) => m.replace(/[^\n]/g, ' ')).replace(/(^|[^:\\])\/\/[^\n]*/g, (m, p) => p + ' '.repeat(m.length - p.length));
}

/** The scroll-position writes of a file: [{ line, what, waived }] (`waived`: 'reason', 'no-reason' or undefined). */
export function scrollWrites(text) {
  const masked = maskComments(text);
  const lines = text.split('\n');
  const out = [];
  const seen = new Set();
  for (const re of WRITES) {
    for (const m of masked.matchAll(re)) {
      if (/^(window|document|globalThis)$/.test(m[1] ?? '')) continue;
      const line = masked.slice(0, m.index).split('\n').length;
      if (seen.has(line)) continue;
      seen.add(line);
      const here = lines[line - 1] ?? '';
      const above = lines[line - 2] ?? '';
      const near = here + (/^\s*(\/\/|\*|\/\*)/.test(above) ? `\n${above}` : '');
      const what = m[2] ?? m[1];
      out.push({ line, what, waived: WAIVER.test(near) ? 'reason' : BARE_WAIVER.test(near) ? 'no-reason' : undefined });
    }
  }
  return out.sort((a, b) => a.line - b.line);
}

/** Whether a component file's template or styles carry the shared style; `read(relative)` reads a `templateUrl`/`styleUrl(s)` file. */
export function carriesStyle(text, read) {
  if (MARKER.test(text)) return true;
  const urls = [...text.matchAll(/(?:templateUrl|styleUrl)\s*:\s*['"]([^'"]+)['"]/g)].map((m) => m[1]);
  for (const m of text.matchAll(/styleUrls\s*:\s*\[([^\]]*)\]/g)) urls.push(...[...m[1].matchAll(/['"]([^'"]+)['"]/g)].map((x) => x[1]));
  return urls.some((u) => MARKER.test(read(u) ?? ''));
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
  mode: 'report-only',
  title: 'Scrollers our code positions make no scroll moves of their own',
  rule: 40,
  run(ctx) {
    if (!positiveControl()) return [unwaivable(error('C138', 40, 'conformance/src/checks/C138-owned-scroll.mjs', 1, 'the positive control failed: the check cannot see a known scroll write', 'Fix the check before trusting its result.'))];
    const out = [];
    for (const file of ctx.files()) {
      if (!SOURCE.test(file) || SKIP.test(file)) continue;
      const text = ctx.read(file);
      if (!text || !/scroll(Top|Left|To|By)/.test(text)) continue;
      const dir = file.split('/').slice(0, -1).join('/');
      const read = (rel) => ctx.read(posix.normalize(posix.join(dir, rel)));
      for (const p of ownedScrollProblems(text, read)) out.push(SEVERITY('C138', 40, file, p.line, p.message, FIX));
    }
    return out;
  },
};
