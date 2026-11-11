import { defineContributionKind, type ContributionKind } from '@decryp7/paddock-ui/shell/contributions';
import { CONTRIBUTION_ID, isMessage, type Message } from '@decryp7/paddock-ui/shell/contributions';
import { contrast, over, tryParseColor, type Rgba } from './color';

/** The colour schemes a theme pack can tune. */
export type ThemeScheme = 'dark' | 'light';

/** Custom properties by full name (`--pd-accent`), as a pack overrides them. */
export type ThemeTokens = Readonly<Record<string, string>>;

/**
 * An app theme pack (ADR 0012 §5): overrides of the shell's `--pd-*` tokens, never new colours for components to read.
 * Debrief's precision, console and contrast looks become packs. The shell applies the pack as inline custom properties on
 * `<html>` (`data-pack="<id>"`): `common` always, then the tokens of the resolved scheme. In high contrast no pack is
 * applied, so a pack can never weaken the high-contrast boundaries.
 */
export interface ThemeDef {
  /** Namespaced id: `db.theme.console`. */
  readonly id: string;
  /** The name in the Appearance choice. */
  readonly name: Message;
  readonly tokens: {
    readonly common?: ThemeTokens;
    readonly dark?: ThemeTokens;
    readonly light?: ThemeTokens;
  };
}

/** Declares a theme pack (identity function for the types). */
export function defineTheme(def: ThemeDef): ThemeDef {
  return Object.freeze(def);
}

/** Pack limits: tokens per set and characters per value (parse once with limits, rule 21). */
export const THEME_LIMITS = Object.freeze({ tokens: 200, valueLength: 200 });

const TOKEN_NAME = /^--pd-[a-z0-9]+(?:-[a-z0-9]+)*$/;
/** Values a custom property may take from a pack: no blocks, no statements, no URLs (no remote content, CSP). */
const UNSAFE_VALUE = /[;{}<>\\]|url\s*\(|@import|expression\s*\(/i;

/** Text tokens and the surfaces they sit on: each pair a pack sets in full must stay AA (4.5:1). */
const TEXT_PAIRS: readonly (readonly [string, string])[] = [
  ['--pd-text', '--pd-bg'],
  ['--pd-text', '--pd-surface-1'],
  ['--pd-text', '--pd-surface-2'],
  ['--pd-text', '--pd-surface-3'],
  ['--pd-text-muted', '--pd-surface-1'],
  ['--pd-text-muted', '--pd-surface-2'],
  ['--pd-text-subtle', '--pd-surface-1'],
  ['--pd-accent', '--pd-surface-1'],
  ['--pd-on-accent', '--pd-accent-fill'],
  ['--pd-on-danger', '--pd-danger-fill'],
  ['--pd-success', '--pd-surface-1'],
  ['--pd-warning', '--pd-surface-1'],
  ['--pd-danger', '--pd-surface-1'],
  ['--pd-info', '--pd-surface-1'],
];
/** Boundaries and marks: 3:1 against surface-1 (WCAG 1.4.11). */
const BOUNDARY_PAIRS: readonly (readonly [string, string])[] = [
  ['--pd-border-control', '--pd-surface-1'],
  ['--pd-focus-ring', '--pd-surface-1'],
  ['--pd-accent-fill', '--pd-surface-1'],
];

/**
 * Defects of a theme pack, as sentences. Structural checks always run (id, name, token names and values, limits); the
 * contrast checks run on every pair whose two colours the pack sets (with `common` merged under each scheme), so a pack
 * that recolours text and surfaces is measured by the same targets as the shell's own tokens (test/tokens). Pure.
 */
export function themeProblems(def: unknown): string[] {
  if (typeof def !== 'object' || def === null) return ['is not a theme definition'];
  const t = def as Partial<ThemeDef>;
  const out: string[] = [];
  if (typeof t.id !== 'string' || !CONTRIBUTION_ID.test(t.id)) out.push(`id ${JSON.stringify(t.id)} is not a namespaced id (db.theme.console)`);
  if (!isMessage(t.name)) out.push(`name has no message id (use msg('<ns>.<name>', 'Text'))`);
  const tokens = t.tokens;
  if (typeof tokens !== 'object' || tokens === null) return [...out, 'has no tokens'];
  const sets: [string, unknown][] = [
    ['common', tokens.common],
    ['dark', tokens.dark],
    ['light', tokens.light],
  ];
  for (const [set, value] of sets) {
    if (value === undefined) continue;
    if (typeof value !== 'object' || value === null) {
      out.push(`tokens.${set} is not a map of custom properties`);
      continue;
    }
    const entries = Object.entries(value as Record<string, unknown>);
    if (entries.length > THEME_LIMITS.tokens) out.push(`tokens.${set} has ${entries.length} tokens (at most ${THEME_LIMITS.tokens})`);
    for (const [name, v] of entries) {
      if (!TOKEN_NAME.test(name)) out.push(`tokens.${set} names ${JSON.stringify(name)}, which is not a --pd-* token`);
      if (typeof v !== 'string' || !v.trim()) out.push(`tokens.${set}.${name} is empty`);
      else if (v.length > THEME_LIMITS.valueLength) out.push(`tokens.${set}.${name} is longer than ${THEME_LIMITS.valueLength} characters`);
      else if (UNSAFE_VALUE.test(v)) out.push(`tokens.${set}.${name} has a value a custom property must not take (${JSON.stringify(v)})`);
    }
  }
  for (const scheme of ['dark', 'light'] as const) {
    const merged: Record<string, string> = { ...(tokens.common ?? {}), ...(tokens[scheme] ?? {}) };
    const color = (name: string): Rgba | null => {
      const v = merged[name];
      return typeof v === 'string' ? tryParseColor(v) : null;
    };
    const measure = (pairs: readonly (readonly [string, string])[], min: number): void => {
      for (const [fg, bg] of pairs) {
        const f = color(fg);
        const b = color(bg);
        if (!f || !b) continue;
        const ratio = contrast(f, over(b, { r: 0, g: 0, b: 0, a: 1 }));
        if (ratio < min) out.push(`${scheme}: ${fg} on ${bg} is ${ratio.toFixed(2)}:1 (at least ${min}:1)`);
      }
    };
    measure(TEXT_PAIRS, 4.5);
    measure(BOUNDARY_PAIRS, 3);
  }
  return out;
}

/** Theme packs as a contribution kind of the shell's registry (one mechanism, plan §5.2): `provideThemes(...)`. */
export const THEMES: ContributionKind<ThemeDef> = defineContributionKind<ThemeDef>({
  name: 'themes',
  label: 'theme',
  identity: (t) => t.id,
  check: (t) => themeProblems(t),
});

/** The tokens a pack sets for `scheme` (`common` first, the scheme's own over it). */
export function themeTokensFor(def: ThemeDef, scheme: ThemeScheme): ThemeTokens {
  return Object.freeze({ ...(def.tokens.common ?? {}), ...(def.tokens[scheme] ?? {}) });
}
