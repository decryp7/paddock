// Checks of the family's isometric sets (STYLE.md section 9) that a machine can make:  node --test tools/iso-family.test.mjs
//   - coverage at 16 and in the tray, the tray measured on its glyph without the state badge (a badge adds ink by design);
//   - every tray pixel fully opaque or fully transparent, at every shipped size, except a state that dims the whole glyph;
//   - the docs' inlined icon copies equal the built 60 frames;
//   - the SVGs do not depend on what was rendered before them (explicit id prefixes).
import { Resvg } from '@resvg/resvg-js';
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { join, resolve } from 'node:path';
import { test } from 'node:test';
import { FAMILY, GLYPH_KIND, TRAY_STATES, bareSmall, master, small, tray, trayGlyphOnly } from './iso-family.mjs';

const ROOT = resolve(import.meta.dirname, '..');
const pixels = (svg, px) => new Resvg(svg, { fitTo: { mode: 'width', value: px } }).render().pixels;

/** Coverage limits in % (STYLE.md section 9): line glyphs 18 to 26, filled silhouettes 18 to 30. */
export const COVERAGE = { line: [18, 26], filled: [18, 30] };

/** Ink coverage at `px`: the share of pixels changed from the bare tile, weighted by how much (as tools/paddock-symbols.mjs). */
export function tileCoverage(svg, bare, px) {
  const a = pixels(svg, px), b = pixels(bare, px);
  let s = 0;
  for (let i = 0; i < px * px; i++) {
    const ch = [0, 1, 2].map(c => { const t = b[4 * i + c], d = a[4 * i + c] - t; return d >= 0 ? d / (255 - t || 1) : -d / (t || 1); });
    s += Math.min(1, Math.max(...ch));
  }
  return Math.round((100 * s) / (px * px));
}
/** Tray coverage: the alpha of the 16 px render, as a share of the 256 pixels. */
export function trayCoverage(svg) {
  const d = pixels(svg, 16);
  let s = 0;
  for (let i = 3; i < d.length; i += 4) s += d[i] / 255;
  return Math.round((100 * s) / 256);
}
/** The alpha values of a render other than 0 and 255. */
const partialAlpha = (svg, px) => { const d = pixels(svg, px), out = new Set(); for (let i = 3; i < d.length; i += 4) if (d[i] !== 0 && d[i] !== 255) out.add(d[i]); return [...out]; };

for (const app of FAMILY) {
  const [lo, hi] = COVERAGE[GLYPH_KIND[app]];

  test(`${app}: coverage at 16 is within the ${GLYPH_KIND[app]} limits`, () => {
    const c = tileCoverage(small(app, 16), bareSmall(app, 16), 16);
    assert.ok(c >= lo && c <= hi, `${app} 16 px coverage ${c} % is outside ${lo} to ${hi} %`);
  });

  test(`${app}: tray coverage, measured on the glyph without its state badge, is within the ${GLYPH_KIND[app]} limits`, () => {
    for (const st of TRAY_STATES[app]) {
      const c = trayCoverage(trayGlyphOnly(app, st));
      assert.ok(c >= lo && c <= hi, `${app} tray (${st}) glyph coverage ${c} % is outside ${lo} to ${hi} %`);
    }
  });

  test(`${app}: tray icons are whole pixels at every shipped size`, () => {
    for (const st of TRAY_STATES[app]) {
      for (const theme of ['template', 'light', 'dark']) {
        const renders = theme === 'template' ? [[tray(app, st, theme), [16, 32]], [tray(app, st, theme, { pt18: true }), [18, 36]]] : [[tray(app, st, theme), [16, 32]]];
        for (const [svg, sizes] of renders) {
          for (const px of sizes) {
            const partial = partialAlpha(svg, px);
            // The one allowed partial alpha: a state that dims the whole glyph (Pitwall idle, 70 %).
            const allowed = st === 'idle' ? partial.every(a => Math.abs(a - 0.7 * 255) <= 1) : partial.length === 0;
            assert.ok(allowed, `${app} tray ${st} ${theme} at ${px} px has partial alpha ${partial.slice(0, 5).join(', ')}`);
          }
        }
      }
    }
  });

  test(`${app}: SVGs are the same whatever was rendered before them`, () => {
    const first = [master(app, { frame: 'tiny' }), small(app, 32), tray(app, TRAY_STATES[app].at(-1), 'light')];
    master('debrief'); small('dyno', 24); tray('pitcrew', 'busy', 'dark');
    assert.deepEqual([master(app, { frame: 'tiny' }), small(app, 32), tray(app, TRAY_STATES[app].at(-1), 'light')], first);
  });

  test(`${app}: the docs' inlined icon is the built 60 frame`, () => {
    const docs = readFileSync(join(ROOT, '..', 'docs', 'images', 'src', 'icons', `${app}.svg`), 'utf8');
    assert.equal(docs, readFileSync(join(ROOT, app, `${app}-60.svg`), 'utf8'), `copy icons/${app}/${app}-60.svg to docs/images/src/icons/${app}.svg`);
  });
}

test('the tray coverage rule excludes the badge: Pitcrew with its warning badge is over the filled limit, its glyph within it', () => {
  assert.ok(trayCoverage(tray('pitcrew', 'warning', 'template')) > COVERAGE.filled[1]);
  assert.ok(trayCoverage(trayGlyphOnly('pitcrew', 'warning')) <= COVERAGE.filled[1]);
});
