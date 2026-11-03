// Checks of the family's isometric sets (STYLE.md section 9) that a machine can make:  node --test tools/iso-family.test.mjs
//   - coverage at 16 and in the tray, the tray measured on its glyph without the state badge (a badge adds ink by design);
//   - every tray pixel fully opaque or fully transparent, at every shipped size;
//   - every tray state of an app has its own shape (a different alpha mask);
//   - the README header icon and the docs' icon copies equal the built masters;
//   - the docs' inlined icon copies equal the built 60 frames;
//   - the SVGs do not depend on what was rendered before them (explicit id prefixes).
import { Resvg } from '@resvg/resvg-js';
import assert from 'node:assert/strict';
import { existsSync, readFileSync } from 'node:fs';
import { join, resolve } from 'node:path';
import { test } from 'node:test';
import { execFileSync } from 'node:child_process';
import { mkdtempSync, rmSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { decodePng, readIcns } from './icns.mjs';
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
            assert.ok(partial.length === 0, `${app} tray ${st} ${theme} at ${px} px has partial alpha ${partial.slice(0, 5).join(', ')}`);
          }
        }
      }
    }
  });

  test(`${app}: every tray state has a distinct shape (alpha mask)`, () => {
    const masks = TRAY_STATES[app].map(st => Buffer.from(pixels(tray(app, st, 'template'), 32).filter((_, i) => i % 4 === 3)).toString('hex'));
    assert.equal(new Set(masks).size, masks.length, `${app}: two tray states share one alpha mask`);
  });

  test(`${app}: SVGs are the same whatever was rendered before them`, () => {
    const first = [master(app, { frame: 'tiny' }), small(app, 32), tray(app, TRAY_STATES[app].at(-1), 'light')];
    master('debrief'); small('dyno', 24); tray('pitcrew', 'busy', 'dark');
    assert.deepEqual([master(app, { frame: 'tiny' }), small(app, 32), tray(app, TRAY_STATES[app].at(-1), 'light')], first);
  });

  test(`${app}: the .icns small frames and 32x32.png are the hand-tuned frames, not downscales of the master`, () => {
    const entries = new Map(readIcns(readFileSync(join(ROOT, app, 'tauri', 'icon.icns'))));
    const png = n => readFileSync(join(ROOT, app, 'small', `${app}-${n}.png`));
    assert.ok(entries.get('icp4').equals(png(16)), '16 px is the 16 frame');
    assert.ok(entries.get('icp5').equals(png(32)) && entries.get('ic11').equals(png(32)), '32 px and 16 pt at 2x are the 32 frame');
    assert.equal(entries.get('ic12').readUInt32BE(16), 64, '32 pt at 2x is 64 px wide');
    assert.ok(['is32', 's8mk', 'il32', 'l8mk'].every(t => entries.has(t)), 'the classic entries are there');
    assert.ok(entries.has('ic07') && entries.has('ic10'), 'the large frames stay');
    assert.ok(readFileSync(join(ROOT, app, 'tauri', '32x32.png')).equals(png(32)));
  });

  test(`${app}: iconutil decodes the .icns 16 and 32 frames to the hand-tuned ones`, { skip: process.platform !== 'darwin' }, () => {
    const dir = mkdtempSync(join(tmpdir(), 'icns-'));
    try {
      execFileSync('iconutil', ['-c', 'iconset', join(ROOT, app, 'tauri', 'icon.icns'), '-o', join(dir, 'x.iconset')]);
      for (const [name, px] of [['icon_16x16.png', 16], ['icon_16x16@2x.png', 32], ['icon_32x32.png', 32]]) {
        const got = decodePng(readFileSync(join(dir, 'x.iconset', name))), want = decodePng(readFileSync(join(ROOT, app, 'small', `${app}-${px}.png`)));
        assert.equal(got.w, px, name);
        assert.ok(got.rgba.equals(want.rgba), `${app} ${name} differs from the ${px} frame`);
      }
    } finally {
      rmSync(dir, { recursive: true, force: true });
    }
  });

  test(`${app}: the README header icon in the app is the macOS master`, { skip: !existsSync(join(ROOT, '..', 'apps', app)) }, () => {
    const copy = readFileSync(join(ROOT, '..', 'apps', app, 'docs', 'images', `${app}-icon.svg`), 'utf8');
    assert.equal(copy, readFileSync(join(ROOT, app, `${app}-macos.svg`), 'utf8'), `run node icons/tools/install.mjs`);
    assert.ok(readFileSync(join(ROOT, '..', 'apps', app, 'README.md'), 'utf8').slice(0, 400).includes(`docs/images/${app}-icon.svg`), 'the README header points at it');
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

test('the preview sheet references the built SVGs instead of inlining them', () => {
  const html = readFileSync(join(ROOT, 'preview.html'), 'utf8');
  assert.ok(html.length < 100_000, `preview.html is ${html.length} bytes`);
  const ids = [...html.matchAll(/\sid="([^"]+)"/g)].map(m => m[1]);
  assert.equal(new Set(ids).size, ids.length, 'duplicate ids in preview.html');
});

test('Pitwall paused: the light taskbar badge is dark enough (3:1 on white), the dark one stays light', () => {
  assert.ok(tray('pitwall', 'paused', 'light').includes('#5F6368'));
  assert.ok(!tray('pitwall', 'paused', 'light').includes('#9AA0A6'));
  assert.ok(tray('pitwall', 'paused', 'dark').includes('#9AA0A6'));
});

// ---------- the smooth small-frame proposal (STYLE.md section 6a) ----------
const smooth = await import('./small-smooth.mjs');
const { smoothIso } = await import('./iso.mjs');

test('smoothIso snaps every vertex x to a whole pixel and keeps y exact', () => {
  const g = smoothIso({ grid: 24, xs: [0, 15], ys: [-10, 5], k: 0.75 });
  for (const d of [g.face(0, 10, 0, 0, 13), g.end(10, 0, 5, 0, 13), g.flat(0, 10, 0, 5, 13)]) {
    for (const [, x] of d.matchAll(/[ML](-?[\d.]+) /g)) assert.equal(+x, Math.round(+x), d);
  }
  assert.deepEqual(g.P([1, 0, 0]).map(v => +v.toFixed(2)), [+(g.ox + 0.75).toFixed(2), +(g.oy + 0.375).toFixed(2)]);
});

for (const app of smooth.SMOOTH_FAMILY) {
  const [lo, hi] = COVERAGE[GLYPH_KIND[app]];
  test(`${app}: options A and B keep the coverage at 16 within the ${GLYPH_KIND[app]} limits`, () => {
    for (const [name, svg] of [['A', smooth.smallA(app, 16)], ['B', smooth.smallB(app, 16)]]) {
      const c = tileCoverage(svg, bareSmall(app, 16), 16);
      assert.ok(c >= lo && c <= hi, `${app} option ${name} 16 px coverage ${c} % is outside ${lo} to ${hi} %`);
    }
  });
  test(`${app}: the smooth tray template is alpha only and anti-aliased at 2x`, () => {
    const svg = smooth.smoothTray(app, 'template', { px: 36 }), d = pixels(svg, 36);
    for (let i = 0; i < d.length; i += 4) if (d[i + 3]) assert.deepEqual([d[i], d[i + 1], d[i + 2]], [0, 0, 0]);
    assert.ok(partialAlpha(svg, 36).length > 0 || app === 'paddock', `${app}: no anti-aliased edge at 36 px`);
  });
}
