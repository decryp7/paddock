import { describe, expect, it } from 'vitest';
import { accentProblems, cssRules, definedAccents, resolveTokens } from '../../projects/paddock-ui/shell/src/theme/token-check';
import { ACCENTS, readRules } from './css';

// The reusable accent check (plan X49) against the shell's own tokens, and against small stylesheets that are wrong in
// one way each, so the check itself is known to fail where it should.

/** A stylesheet with one accent `mine`, written for both schemes; `over` replaces tokens of the dark scheme. */
function sheet(over: Record<string, string> = {}, accent = 'mine'): string {
  const dark: Record<string, string> = {
    '--pd-bg': '#101114',
    '--pd-surface-1': '#16171b',
    '--pd-surface-2': '#1d1e23',
    '--pd-surface-3': '#25262c',
    '--pd-accent': '#6ea8fe',
    '--pd-accent-fill': '#2f6fd6',
    '--pd-focus-ring': 'var(--pd-accent)',
    '--pd-on-accent': '#ffffff',
    '--pd-warning': '#e0a000',
    ...over,
  };
  const light = { ...dark, '--pd-bg': '#ffffff', '--pd-surface-1': '#f6f7f9', '--pd-surface-2': '#eceef2', '--pd-surface-3': '#e2e5ea', '--pd-accent': '#1f56b8', '--pd-accent-fill': '#1f56b8' };
  const body = (v: Record<string, string>): string => Object.entries(v).map(([k, x]) => `${k}: ${x};`).join(' ');
  return `/* a comment { with braces } */\n:root[data-theme=dark][data-accent=${accent}] { ${body(dark)} }\n:root[data-theme=light][data-accent=${accent}] { ${body(light)} }\n`;
}

describe('cssRules and resolveTokens', () => {
  it('reads rules by selector, flattens media queries into the key and skips comments', () => {
    const rules = cssRules('/* x { y: z; } */ :root { --a: 1; --b: var(--a); }\n@media (prefers-contrast: more) { :root:not([data-contrast=off]) { --a: 2; } }');
    expect(rules.get(':root')).toEqual({ '--a': '1', '--b': 'var(--a)' });
    expect(rules.get('@media (prefers-contrast: more) | :root:not([data-contrast=off])')).toEqual({ '--a': '2' });
  });

  it('resolves var() chains, the scheme over the base and the accent over both; unknown names stay as written', () => {
    const rules = cssRules(':root { --a: red; --b: var(--a); --c: var(--missing); } :root[data-theme=dark] { --a: blue; } :root[data-theme=dark][data-accent=x] { --b: var(--a); }');
    expect(resolveTokens(rules, 'dark', 'x')).toMatchObject({ '--a': 'blue', '--b': 'blue', '--c': 'var(--missing)' });
    expect(resolveTokens(rules, 'light', 'x')['--b']).toBe('red');
  });

  it('refuses a stylesheet over the size limit', () => {
    expect(() => cssRules('a'.repeat(2_000_001))).toThrow(/larger than/);
  });

  it('lists the accents a stylesheet defines', () => {
    expect(definedAccents(cssRules(sheet({}, 'mine') + sheet({}, 'other')))).toEqual(['mine', 'other']);
  });
});

describe('accentProblems', () => {
  it('passes a sound accent', () => {
    expect(accentProblems(cssRules(sheet()), 'mine')).toEqual([]);
  });

  it('names an accent the stylesheet does not define', () => {
    expect(accentProblems(cssRules(sheet()), 'nope')).toEqual(['the stylesheet has no rule for the accent "nope" ([data-accent=nope])']);
  });

  it('fails an accent text that is too faint on the surfaces, and says which', () => {
    const p = accentProblems(cssRules(sheet({ '--pd-accent': '#3a3d46' })), 'mine');
    expect(p.some((x) => /dark: the accent as text on --pd-surface-1 is [\d.]+:1/.test(x))).toBe(true);
    expect(p.some((x) => /the focus ring/.test(x))).toBe(true);
  });

  it('fails white text on a pale fill, and a fill that disappears into the surface', () => {
    expect(accentProblems(cssRules(sheet({ '--pd-accent-fill': '#a8c8ff' })), 'mine').some((x) => /text on the accent fill/.test(x))).toBe(true);
    expect(accentProblems(cssRules(sheet({ '--pd-accent-fill': '#1a1b20' })), 'mine').some((x) => /accent fill on surface-1/.test(x))).toBe(true);
  });

  it('fails a fill in the hue of the warning colour', () => {
    expect(accentProblems(cssRules(sheet({ '--pd-accent-fill': '#b07a00', '--pd-on-accent': '#ffffff' })), 'mine').some((x) => /within 20 degrees of the warning hue/.test(x))).toBe(true);
  });

  it('reports a missing or unreadable token instead of skipping it', () => {
    const p = accentProblems(cssRules(sheet({ '--pd-accent-fill': 'hotpink' })), 'mine');
    expect(p).toContain('dark: --pd-accent-fill is missing or not a colour');
  });

  it('can require distance from other accents and from the status colours', () => {
    const rules = cssRules(sheet() + sheet({}, 'twin'));
    expect(accentProblems(rules, 'mine', { others: ['twin'] }).some((x) => /too close to twin/.test(x))).toBe(true);
    const status = cssRules(sheet({ '--pd-success': '#6ea8fe', '--pd-danger': '#ff6b6b', '--pd-info': '#40c4ff' }));
    expect(accentProblems(status, 'mine', { apartFromStatus: true }).some((x) => /reads as success/.test(x))).toBe(true);
    expect(accentProblems(cssRules(sheet()), 'mine', { apartFromStatus: true })).toEqual([]);
  });
});

describe('every accent of the family passes the reusable check', () => {
  const rules = readRules();
  it.each(ACCENTS)('%s', (accent) => {
    expect(accentProblems(rules, accent)).toEqual([]);
  });

  it('knows exactly the accents the appearance module lists', () => {
    expect(definedAccents(rules)).toEqual([...ACCENTS].sort());
  });
});
