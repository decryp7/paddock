// C46: no hand-written `<svg>` icon in a component template (rule 47 Material first, rule 42 share first; AGENTS.md "Icons are
// inline SVG registered with MatIconRegistry"). An icon is `<mat-icon svgIcon="ns:name">` (the shared `pd:` set, or an app's own
// literals registered in its namespace), so one registry draws every glyph with one stroke, size and colour rule. Flagged:
// an `<svg` element in an `.html` template, or in the `template:` of a component in a `.ts` file, or built in code with `createElementNS(.., 'svg')`, under `ui/projects/**` or
// `apps/*/app/src/**` (an app repo's own `app/src/**` too). Specs, tests and e2e are skipped.
// Charts, diagrams and illustrations are not icons and carry a marker with a reason, on the `<svg` line or the line above it:
// `<!-- conformance: svg-ok — <reason> -->` (in markup) or `// conformance: svg-ok — <reason>` (above the `<svg`, or in a
// component's decorator head, before `template:`, for the whole template).
import { error } from '../finding.mjs';

const SOURCE = /^(?:ui\/projects\/.+\/src\/.+|(?:apps\/[^/]+\/)?app\/src\/.+)\.(?:html|ts)$/;
const SKIP = /\.(?:spec|test)\.ts$|(?:^|\/)(?:e2e|node_modules|dist|testing)\//;
const SVG = /<(?:svg:)?svg(?=[\s>/])/g;
/** `text` with the insides of HTML comments blanked (newlines and offsets kept), so an svg in a comment does not count. */
const unComment = (text) => text.replace(/<!--[\s\S]*?-->/g, (m) => m.replace(/[^\n]/g, ' '));
const MARKER = /conformance:\s*svg-ok\s*(?:[—–]|-{1,2}(?=\s))\s*(?!-*>)(\S.{2,})/;
const BARE_MARKER = /conformance:\s*svg-ok/;

const lineOf = (text, at) => text.slice(0, at).split('\n').length;

/** Whether the marker covers `line` (1-based) of `lines`: on it or the line above. Returns 'reason' | 'no-reason' | false. */
function markerAt(lines, line) {
  const here = `${lines[line - 1] ?? ''}\n${lines[line - 2] ?? ''}`;
  return MARKER.test(here) ? 'reason' : BARE_MARKER.test(here) ? 'no-reason' : false;
}

/** The component templates of a `.ts` source: [{ start, text, headMarker }]; `start` is the offset of the template's text. */
export function inlineTemplates(source) {
  const out = [];
  for (const m of source.matchAll(/\btemplate\s*:\s*(`|'|")/g)) {
    const quote = m[1];
    const start = m.index + m[0].length;
    let i = start;
    while (i < source.length && source[i] !== quote) i += source[i] === '\\' ? 2 : 1;
    // The decorator head: from the last `@Component(` before `template:` to `template:` (plus the line above the decorator).
    const dec = source.lastIndexOf('@Component', m.index);
    const head = dec < 0 ? '' : source.slice(source.lastIndexOf('\n', source.lastIndexOf('\n', dec) - 1) + 1, m.index);
    out.push({ start, text: source.slice(start, i), headMarker: MARKER.test(head) ? 'reason' : BARE_MARKER.test(head) ? 'no-reason' : false });
  }
  return out;
}

/** The `<svg` elements of a source file: [{ line, waived: false | 'no-reason' | 'reason' }]. */
export function svgElements(file, source) {
  const lines = source.split('\n');
  const out = [];
  const scan = (text, offset, head) => {
    const found = [...unComment(text).matchAll(SVG)];
    // A marker in the decorator head covers a template with a single svg; with several, each needs its own marker.
    const whole = found.length === 1 ? head : false;
    for (const m of found) {
      const line = lineOf(source, offset + m.index);
      out.push({ line, waived: markerAt(lines, line) || whole });
    }
  };
  if (file.endsWith('.html')) scan(source, 0, false);
  else {
    for (const t of inlineTemplates(source)) scan(t.text, t.start, t.headMarker);
    // An svg built in code (`document.createElementNS(ns, 'svg')`: dockview's tabs and group actions live outside Angular).
    for (const m of source.matchAll(/createElementNS\([^)]*['"`]svg['"`]\s*\)/g)) {
      const line = lineOf(source, m.index);
      out.push({ line, waived: markerAt(lines, line) });
    }
  }
  return out;
}

export default {
  id: 'C46',
  title: 'Icons are mat-icon, not hand-written <svg>',
  rule: 47,
  run(ctx) {
    const out = [];
    const fix =
      'Draw it with `<mat-icon svgIcon="pd:name">` (the shared set; `registerPaddockIcons`) or register the app\'s glyph as a literal in its namespace with `outlineIcon`. A chart, diagram or illustration carries `<!-- conformance: svg-ok — <reason> -->` (or `// conformance: svg-ok — <reason>`).';
    for (const f of ctx.files()) {
      if (!SOURCE.test(f) || SKIP.test(f)) continue;
      const text = ctx.read(f);
      if (!text || !/<(?:svg:)?svg|createElementNS/.test(text)) continue;
      for (const s of svgElements(f, text)) {
        if (s.waived === 'reason') continue;
        if (s.waived === 'no-reason') out.push(error('C46', 47, f, s.line, 'the svg-ok marker gives no reason', 'Write `conformance: svg-ok — <why this is a chart, diagram or illustration, not an icon>`.'));
        else out.push(error('C46', 47, f, s.line, 'a hand-written <svg> in a template or built in code: icons are <mat-icon svgIcon="...">', fix));
      }
    }
    return out;
  },
};
