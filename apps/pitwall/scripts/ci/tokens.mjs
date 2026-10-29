// Pitwall's UI takes its tokens from @decryp7/paddock-ui (1.1 WP-2.1): the CSS uses the shared `--pd-*` custom properties
// only, and the colours the charts need in TypeScript (`app/src/app/shared/theme-tokens.ts`, ECharts takes values, not
// variables) are a copy derived here from the shared `_tokens.scss`. The checks in tokens.test.mjs keep both honest.
//
//   node scripts/ci/tokens.mjs        prints the derived chart tokens as JSON
import { readFileSync, readdirSync, statSync } from 'node:fs';
import { dirname, join, relative } from 'node:path';
import { fileURLToPath } from 'node:url';
import { isMain } from '../../../../conformance/src/util/is-main.mjs';

const here = dirname(fileURLToPath(import.meta.url));
export const APP = join(here, '..', '..', 'app');
export const SHARED_TOKENS = join(here, '..', '..', '..', '..', 'ui', 'projects', 'paddock-ui', 'scss', '_tokens.scss');

/** The `--pd-*` declarations of one `@mixin <name> { ... }` block of the shared tokens. */
export function mixinVars(scss, name) {
  const start = scss.indexOf(`@mixin ${name} {`);
  if (start < 0) throw new Error(`no @mixin ${name} in the shared tokens`);
  const end = scss.indexOf('\n}', start);
  const out = {};
  for (const m of scss.slice(start, end).matchAll(/^\s*(--pd-[a-z0-9-]+):\s*([^;]+);/gm)) out[m[1]] = m[2].trim();
  return out;
}

/** An app accent of the shared `$accents` map: `{ dark, light, fill }`. */
export function accent(scss, app) {
  const m = scss.match(new RegExp(`'${app}': \\(dark: (#[0-9a-f]{6}), light: (#[0-9a-f]{6}), fill: (#[0-9a-f]{6})\\)`, 'i'));
  if (!m) throw new Error(`no accent '${app}' in the shared tokens`);
  return { dark: m[1].toLowerCase(), light: m[2].toLowerCase(), fill: m[3].toLowerCase() };
}

const hex = (c) => `#${c.map((v) => Math.round(v).toString(16).padStart(2, '0')).join('')}`;

/** `#rrggbb` or `rgb(r g b / a)` as `[r, g, b, a]`; anything else throws (a token the copy cannot take as a value). */
export function parseColour(v) {
  const h = v.match(/^#([0-9a-f]{6})$/i);
  if (h) return [0, 2, 4].map((i) => parseInt(h[1].slice(i, i + 2), 16)).concat(1);
  const r = v.match(/^rgb\((\d+) (\d+) (\d+)(?: \/ ([\d.]+))?\)$/);
  if (r) return [+r[1], +r[2], +r[3], r[4] === undefined ? 1 : +r[4]];
  throw new Error(`not a colour value: ${v}`);
}

/** `colour` drawn over the opaque `under`, as `#rrggbb` (the charts take opaque colours). */
export function over(colour, under) {
  const [r, g, b, a] = parseColour(colour);
  const [R, G, B] = parseColour(under);
  return hex([r * a + R * (1 - a), g * a + G * (1 - a), b * a + B * (1 - a)]);
}

/** The chart tokens per scheme, derived from the shared tokens, in the shape of theme-tokens.ts `ThemeTokens`. */
export function chartTokens(scss = readFileSync(SHARED_TOKENS, 'utf8')) {
  const a = accent(scss, 'pitwall');
  const scheme = (name) => {
    const v = mixinVars(scss, name);
    const get = (k) => {
      if (!(k in v)) throw new Error(`no ${k} in @mixin ${name}`);
      return v[k];
    };
    const surface = get('--pd-surface-1');
    const opaque = (k) => over(get(k), surface);
    const series = [1, 2, 3, 4, 5, 6, 7, 8].map((i) => opaque(`--pd-chart-${i}`));
    return {
      surface: opaque('--pd-surface-1'),
      window: opaque('--pd-bg'),
      text: opaque('--pd-text'),
      text2: opaque('--pd-text-muted'),
      text3: opaque('--pd-text-subtle'),
      accent: a[name],
      ok: opaque('--pd-success'),
      warn: opaque('--pd-warning'),
      bad: opaque('--pd-danger'),
      info: opaque('--pd-info'),
      okTint: opaque('--pd-success-tint'),
      warnTint: opaque('--pd-warning-tint'),
      badTint: opaque('--pd-danger-tint'),
      infoTint: opaque('--pd-info-tint'),
      grid: opaque('--pd-chart-grid'),
      axis: opaque('--pd-border-strong'),
      // Material's tooltip in paddock-ui's material.css: text colour on the card colour.
      tooltipBg: opaque('--pd-text'),
      tooltipText: opaque('--pd-surface-1'),
      series,
      history: series.slice(0, 3),
    };
  };
  return { light: scheme('light'), dark: scheme('dark') };
}

/** The values written in theme-tokens.ts, read back: `{ light: {...}, dark: {...} }`. */
export function mirroredTokens(ts) {
  const block = (name) => {
    const start = ts.indexOf(`  ${name}: {`);
    if (start < 0) throw new Error(`no ${name} block in theme-tokens.ts`);
    const body = ts.slice(start, ts.indexOf('\n  },', start));
    const out = {};
    for (const m of body.matchAll(/^\s{4}(\w+): '(#[0-9a-f]{6})',/gm)) out[m[1]] = m[2];
    for (const m of body.matchAll(/^\s{4}(\w+): \[([^\]]*)\],/gm)) out[m[1]] = [...m[2].matchAll(/'(#[0-9a-f]{6})'/g)].map((x) => x[1]);
    return out;
  };
  return { light: block('light'), dark: block('dark') };
}

/** Every `var(--name` in the UI sources that is neither shared (`--pd-`), Pitwall's one layout layer (`--pw-`), Material's
 * own (`--mat-`, `--mdc-`) nor a property the same file declares or binds (a component-local value such as `--seg`). */
export function foreignVars(dir = join(APP, 'src')) {
  const found = [];
  const walk = (d) => {
    for (const name of readdirSync(d)) {
      const p = join(d, name);
      if (statSync(p).isDirectory()) walk(p);
      else if (/\.(ts|scss|html|css)$/.test(name) && !name.endsWith('.spec.ts')) {
        const src = readFileSync(p, 'utf8');
        // Declared in the file: in a rule (`--seg: ...`) or bound by Angular (`[style.--cols]="..."`).
        const declared = new Set([...src.matchAll(/(?:^|[\s;{'"]|style\.)(--[a-z][a-z0-9-]*)\s*[:\]'"]/g)].map((m) => m[1]));
        for (const m of src.matchAll(/var\((--[a-z][a-z0-9-]*)/g)) {
          const v = m[1];
          if (/^--(pd|pw|mat|mdc)-/.test(v) || declared.has(v)) continue;
          found.push(`${relative(dir, p)}: ${v}`);
        }
      }
    }
  };
  walk(dir);
  return found;
}

if (isMain(import.meta.url)) console.log(JSON.stringify(chartTokens(), null, 2));
