import { contrast, deltaE2000, over, simulate, tryParseColor, type Cvd, type Rgba } from './color';

/**
 * Contrast and colour-vision checks for an app's accent (plan X49, WCAG 1.4.3, 1.4.11, 2.4.11): the targets the shell's
 * own token tests hold every accent to (`test/tokens`), as pure functions an app can run on its own accent in its
 * unit tests. No DOM and no Angular: it reads the compiled token stylesheet as text (`@decryp7/paddock-ui/css/tokens.css`).
 */

/** Custom properties by full name (`--pd-accent`). */
export type TokenVars = Record<string, string>;
/** Every rule of a stylesheet by selector (`@media (…) | :root[data-theme=dark]` for rules inside a media query). */
export type TokenRules = Map<string, TokenVars>;
export type TokenScheme = 'dark' | 'light';
/** `more` is the high-contrast mode (`data-contrast="more"` or the OS's Increase Contrast). */
export type TokenContrast = 'standard' | 'more';

const MAX_CSS = 2_000_000;

/**
 * The custom properties of every rule of a compiled stylesheet, by selector (media queries flattened into the key).
 * Comments are skipped; the text is bounded (parse once with limits).
 */
export function cssRules(css: string): TokenRules {
  if (css.length > MAX_CSS) throw new Error(`the stylesheet is larger than ${MAX_CSS} characters`);
  const rules: TokenRules = new Map();
  const walk = (text: string, prefix: string): void => {
    let i = 0;
    while (i < text.length) {
      const open = text.indexOf('{', i);
      if (open < 0) break;
      const head = text.slice(i, open).trim();
      let depth = 1;
      let j = open + 1;
      while (j < text.length && depth > 0) {
        if (text[j] === '{') depth++;
        else if (text[j] === '}') depth--;
        j++;
      }
      const body = text.slice(open + 1, j - 1);
      if (head.startsWith('@media')) walk(body, `${head} | `);
      else {
        const key = prefix + head.replace(/['"]/g, '').replace(/\s+/g, ' ');
        const vars = rules.get(key) ?? {};
        for (const m of body.matchAll(/(--[\w-]+)\s*:\s*([^;]+);/g)) vars[m[1] as string] = (m[2] as string).trim();
        rules.set(key, vars);
      }
      i = j;
    }
  };
  walk(css.replace(/\/\*[\s\S]*?\*\//g, ''), '');
  return rules;
}

/**
 * The custom properties of one scheme + accent (+ contrast mode) as the browser computes them, `var()` chains resolved.
 * Rules apply in source order, which is also their specificity order here.
 */
export function resolveTokens(rules: TokenRules, scheme: TokenScheme, accent: string, mode: TokenContrast = 'standard'): TokenVars {
  const merged: TokenVars = {
    ...rules.get(':root'),
    ...rules.get(`:root[data-theme=${scheme}]`),
    ...(mode === 'more' ? rules.get(':root[data-contrast=more]') : {}),
    ...rules.get(`:root[data-theme=${scheme}][data-accent=${accent}]`),
  };
  const deref = (v: string, depth = 0): string => {
    const m = /^var\((--[\w-]+)\)$/.exec(v);
    return m && depth < 8 ? deref(merged[m[1] as string] ?? v, depth + 1) : v;
  };
  return Object.fromEntries(Object.entries(merged).map(([k, v]) => [k, deref(v)]));
}

/** The accents a stylesheet defines (`[data-accent=…]` rules), sorted. */
export function definedAccents(rules: TokenRules): string[] {
  const out = new Set<string>();
  for (const key of rules.keys()) {
    const m = /^:root\[data-theme=(?:dark|light)\]\[data-accent=([\w-]+)\]$/.exec(key);
    if (m?.[1]) out.add(m[1]);
  }
  return [...out].sort();
}

const SURFACES = ['--pd-bg', '--pd-surface-1', '--pd-surface-2', '--pd-surface-3'] as const;
const STATUS = ['success', 'warning', 'danger', 'info'] as const;
const CVDS: readonly Cvd[] = ['deuteranopia', 'protanopia', 'tritanopia'];

export interface AccentCheckOptions {
  /** Other apps' accents the accent must be told apart from (CIEDE2000 >= 12 in each scheme, normal vision). */
  readonly others?: readonly string[];
  /** Also require CIEDE2000 >= 15 from the four status colours, so the accent never reads as success, warning, danger or info. */
  readonly apartFromStatus?: boolean;
}

/** `value` of a token as an opaque colour, or null when it is missing or not a colour. */
function colour(v: TokenVars, name: string): Rgba | null {
  const raw = v[name];
  return raw === undefined ? null : tryParseColor(raw);
}

const hue = (c: Rgba): number => {
  const [r, g, b] = [c.r / 255, c.g / 255, c.b / 255];
  const max = Math.max(r, g, b);
  const d = max - Math.min(r, g, b);
  if (d === 0) return 0;
  const h = max === r ? ((g - b) / d) % 6 : max === g ? (b - r) / d + 2 : (r - g) / d + 4;
  return (h * 60 + 360) % 360;
};

/**
 * What is wrong with `accent` in the stylesheet `rules`, as sentences (empty: nothing). In both schemes and both contrast
 * modes: the accent as text (links, the selected icon) is AA on every surface, the focus ring and the accent fill are
 * 3:1 against the surfaces, the on-accent text is AA on the fill; the accent as text stays AA on surface-1 under each
 * colour-vision deficiency (Machado 2009: legibility must not rest on hue) and the fill is 20 degrees of hue away from the warning colour. Missing tokens are reported,
 * never skipped. Optionally the accent must also stay clear of other accents and of the status colours.
 */
export function accentProblems(rules: TokenRules, accent: string, options: AccentCheckOptions = {}): string[] {
  const out: string[] = [];
  if (!definedAccents(rules).includes(accent)) return [`the stylesheet has no rule for the accent "${accent}" ([data-accent=${accent}])`];
  for (const scheme of ['dark', 'light'] as const) {
    for (const mode of ['standard', 'more'] as const) {
      const v = resolveTokens(rules, scheme, accent, mode);
      const at = `${scheme}${mode === 'more' ? ' (high contrast)' : ''}`;
      const need = (name: string): Rgba | null => {
        const c = colour(v, name);
        if (!c) out.push(`${at}: ${name} is missing or not a colour`);
        return c;
      };
      const ratio = (fg: Rgba, bg: Rgba): number => contrast(fg, over(bg, { r: 0, g: 0, b: 0, a: 1 }));
      const surfaces = SURFACES.map((s) => [s, need(s)] as const);
      const text = need('--pd-accent');
      const ring = need('--pd-focus-ring');
      const fill = need('--pd-accent-fill');
      const onFill = need('--pd-on-accent');
      for (const [name, s] of surfaces) {
        if (!s) continue;
        if (text && ratio(text, s) < 4.5) out.push(`${at}: the accent as text on ${name} is ${ratio(text, s).toFixed(2)}:1 (at least 4.5:1)`);
        if (ring && ratio(ring, s) < 3) out.push(`${at}: the focus ring on ${name} is ${ratio(ring, s).toFixed(2)}:1 (at least 3:1)`);
      }
      const surface1 = surfaces.find(([n]) => n === '--pd-surface-1')?.[1] ?? null;
      if (fill && onFill && ratio(onFill, fill) < 4.5) out.push(`${at}: the text on the accent fill is ${ratio(onFill, fill).toFixed(2)}:1 (at least 4.5:1)`);
      if (fill && surface1) {
        if (ratio(fill, surface1) < 3) out.push(`${at}: the accent fill on surface-1 is ${ratio(fill, surface1).toFixed(2)}:1 (at least 3:1)`);
      }
      if (text && surface1) {
        for (const kind of CVDS) {
          const r = ratio(simulate(text, kind), simulate(surface1, kind));
          if (r < 4.5) out.push(`${at}: under ${kind} the accent as text on surface-1 is ${r.toFixed(2)}:1 (at least 4.5:1)`);
        }
      }
      const warning = need('--pd-warning');
      if (mode === 'standard' && fill && warning) {
        const d = Math.abs(hue(fill) - hue(warning));
        if (Math.min(d, 360 - d) < 20) out.push(`${at}: the accent fill is within 20 degrees of the warning hue`);
      }
      if (mode === 'standard' && text) {
        if (options.apartFromStatus) {
          for (const name of STATUS) {
            const s = colour(v, `--pd-${name}`);
            if (s && deltaE2000(text, s) < 15) out.push(`${at}: the accent reads as ${name} (CIEDE2000 ${deltaE2000(text, s).toFixed(1)}, at least 15)`);
          }
        }
        for (const other of options.others ?? []) {
          const o = colour(resolveTokens(rules, scheme, other), '--pd-accent');
          if (o && deltaE2000(text, o) < 12) out.push(`${at}: the accent is too close to ${other}'s (CIEDE2000 ${deltaE2000(text, o).toFixed(1)}, at least 12)`);
        }
      }
    }
  }
  return out;
}
