// Pitwall's controls are Angular Material and paddock-ui (rule 47; 1.1 WP-2.3): no native select, no native checkbox, no
// unthemed input and none of the retired Pitwall controls (`.btn`, `.seg`, `pw-select`, the row and context menus, the
// skeleton, sparkline, toast host and empty state) in a template. Tables are Material's (`<table mat-table>`) or
// paddock-ui's `pd-table` (1.1 WP-2, tables): no hand-rolled `<table>` and no retired `.table` class. controls.test.mjs
// runs the check over every template.
//
//   node scripts/ci/controls.mjs      lists what is left, one line per finding
import { readFileSync, readdirSync, statSync } from 'node:fs';
import { join, relative } from 'node:path';
import { isMain } from '../../../../conformance/src/util/is-main.mjs';
import { APP } from './tokens.mjs';

/** Input types that are not text fields (a hidden or file input has no field to theme). */
const NON_TEXT = new Set(['hidden', 'file']);

const RULES = [
  { id: 'native-select', re: /<select[\s>]/g, why: 'a native <select>: use mat-select or pd-select' },
  { id: 'native-checkbox', re: /<input\b[^>]*\btype=["']checkbox["'][^>]*>/g, why: 'a native checkbox: use mat-checkbox (forms) or mat-slide-toggle (immediate)' },
  { id: 'btn-class', re: /\bclass=["'](?:[^"']*\s)?btn(?:\s[^"']*)?["']/g, why: 'a .btn button: use matButton / matIconButton' },
  { id: 'seg-class', re: /\bclass=["'](?:[^"']*\s)?seg(?:\s[^"']*)?["']/g, why: 'a .seg group: use pd-segmented-control or mat-button-toggle-group' },
  { id: 'table-class', re: /<table\b[^>]*\bclass=["'](?:[^"']*\s)?table(?:\s[^"']*)?["'][^>]*>/g, why: 'the retired .table class: use mat-table (with [fixedLayout]) or pd-table' },
  { id: 'native-table', re: /<table\b(?![^>]*\bmat-table\b)[^>]*>/g, why: 'a hand-rolled <table>: use mat-table or pd-table' },
  {
    id: 'retired-component',
    re: /<pw-(?:select|row-menu|context-menu|skeleton|sparkline|toast-host|empty-state)\b/g,
    why: "a retired Pitwall component: use Material or paddock-ui's",
  },
];

/** The findings in one source text: `{ id, line, text, why }`. Input elements need `matInput` or `matSliderThumb`. */
export function findNativeControls(source) {
  const out = [];
  const lineOf = (i) => source.slice(0, i).split('\n').length;
  for (const r of RULES) {
    for (const m of source.matchAll(r.re)) out.push({ id: r.id, line: lineOf(m.index), text: m[0].slice(0, 80), why: r.why });
  }
  for (const m of source.matchAll(/<input\b[^>]*>/g)) {
    const tag = m[0];
    const type = tag.match(/\btype=["']([a-z]+)["']/)?.[1] ?? 'text';
    if (type === 'checkbox' || NON_TEXT.has(type)) continue;
    if (/\bmatInput\b|\bmatSliderThumb\b|\bmatStartDate\b|\bmatEndDate\b/.test(tag)) continue;
    out.push({ id: 'unthemed-input', line: lineOf(m.index), text: tag.slice(0, 80), why: 'an input outside Material: use mat-form-field + matInput (or mat-slider)' });
  }
  return out.sort((a, b) => a.line - b.line);
}

/** Every template source of the app: `.html` files and the `.ts` components (specs excluded). */
export function templateFiles(dir = join(APP, 'src', 'app')) {
  const files = [];
  for (const name of readdirSync(dir)) {
    const p = join(dir, name);
    if (statSync(p).isDirectory()) files.push(...templateFiles(p));
    else if (/\.html$|(?<!\.spec)\.ts$/.test(name)) files.push(p);
  }
  return files.sort();
}

/** All findings of the app, as `path:line id: text`. */
export function appFindings() {
  return templateFiles().flatMap((f) => findNativeControls(readFileSync(f, 'utf8')).map((x) => `${relative(APP, f)}:${x.line} ${x.id}: ${x.text}`));
}

if (isMain(import.meta.url)) {
  const found = appFindings();
  for (const f of found) console.log(f);
  process.exitCode = found.length ? 1 : 0;
}
