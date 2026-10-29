import test from 'node:test';
import assert from 'node:assert/strict';
import { mkdtempSync, mkdirSync, readdirSync, readFileSync, statSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { APP, accent, chartTokens, foreignVars, mirroredTokens, mixinVars, over, parseColour } from './tokens.mjs';

const SCSS = `
$accents: (
  'pitwall': (dark: #A58BFA, light: #6d3fd6, fill: #7546e0),
);
@mixin dark {
  --pd-bg: #0e0f12;
  --pd-surface-1: #15171b;
  --pd-success-tint: rgb(61 214 140 / 0.14);
}
@mixin light {
  --pd-bg: #f4f5f7;
  --pd-surface-1: #ffffff;
}
`;

test('reads a scheme mixin and an app accent of the shared tokens', () => {
  assert.deepEqual(mixinVars(SCSS, 'light'), { '--pd-bg': '#f4f5f7', '--pd-surface-1': '#ffffff' });
  assert.equal(mixinVars(SCSS, 'dark')['--pd-success-tint'], 'rgb(61 214 140 / 0.14)');
  assert.deepEqual(accent(SCSS, 'pitwall'), { dark: '#a58bfa', light: '#6d3fd6', fill: '#7546e0' });
  assert.throws(() => mixinVars(SCSS, 'sepia'), /no @mixin sepia/);
  assert.throws(() => accent(SCSS, 'nobody'), /no accent 'nobody'/);
});

test('draws a translucent token over the card as an opaque colour', () => {
  assert.deepEqual(parseColour('#ff8000'), [255, 128, 0, 1]);
  assert.deepEqual(parseColour('rgb(1 2 3 / 0.5)'), [1, 2, 3, 0.5]);
  assert.throws(() => parseColour('var(--pd-chart-1)'), /not a colour value/);
  assert.equal(over('#123456', '#ffffff'), '#123456');
  assert.equal(over('rgb(0 0 0 / 0.5)', '#ffffff'), '#808080');
  // The dark success tint over the dark card, as the chart pill draws it.
  assert.equal(over('rgb(61 214 140 / 0.14)', '#15171b'), '#1b322b');
});

test('a token the copy needs and the shared file lacks fails with its name', () => {
  assert.throws(() => chartTokens(SCSS), /no --pd-[a-z0-9-]+ in @mixin light/);
});

test("theme-tokens.ts holds exactly the shared tokens (Pitwall's accent, translucent ones over the card)", () => {
  const derived = chartTokens();
  const written = mirroredTokens(readFileSync(join(APP, 'src', 'app', 'shared', 'theme-tokens.ts'), 'utf8'));
  for (const scheme of ['light', 'dark']) {
    assert.deepEqual(written[scheme], derived[scheme], `${scheme}: paste the output of node scripts/ci/tokens.mjs into theme-tokens.ts`);
  }
});

test('the UI uses the shared --pd-* tokens only (Pitwall-local --pw-* and component-local properties aside)', () => {
  assert.deepEqual(foreignVars(), []);
});

test('the check finds an old token and lets the allowed kinds through', () => {
  const dir = mkdtempSync(join(tmpdir(), 'pw-tokens-'));
  mkdirSync(join(dir, 'a'));
  writeFileSync(join(dir, 'a', 'x.ts'), "template: `<i [style.--h]='1'></i>`, styles: `a { color: var(--text-2); --seg: 1px; width: var(--seg); height: var(--h); gap: var(--pd-space-2); }`");
  writeFileSync(join(dir, 'a', 'x.spec.ts'), "'var(--anything)'");
  writeFileSync(join(dir, 'y.scss'), 'b { top: var(--pw-titlebar-h); c: var(--mat-sys-primary); d: var(--bg-card, red); }');
  assert.deepEqual(foreignVars(dir).sort(), ['a/x.ts: --text-2', 'y.scss: --bg-card']);
});

test("every --pw-* property is declared once, in styles.scss, and styles.scss names no colour", () => {
  const styles = readFileSync(join(APP, 'src', 'styles.scss'), 'utf8');
  const declared = new Set([...styles.matchAll(/^\s*(--pw-[a-z0-9-]+):/gm)].map((m) => m[1]));
  assert.ok(declared.size > 0);
  const used = new Set();
  const walk = (d) => {
    for (const name of readdirSync(d)) {
      const p = join(d, name);
      if (statSync(p).isDirectory()) walk(p);
      else for (const m of readFileSync(p, 'utf8').matchAll(/var\((--pw-[a-z0-9-]+)/g)) used.add(m[1]);
    }
  };
  walk(join(APP, 'src'));
  assert.ok(used.has('--pw-titlebar-h'), 'the walk reaches the components');
  for (const u of used) assert.ok(declared.has(u), `${u} is used but not declared in styles.scss`);
  const literals = [...styles.matchAll(/#[0-9a-f]{3,8}\b|\brgba?\(|\bhsla?\(/gi)].map((m) => m[0]);
  assert.deepEqual(literals, [], 'colours come from --pd-* tokens');
});
