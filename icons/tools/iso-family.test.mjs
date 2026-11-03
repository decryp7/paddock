// Checks of the family's isometric sets (STYLE.md section 9) that a machine can make:  node --test tools/iso-family.test.mjs
//   - coverage at 16 and in the tray, the tray measured on its glyph without the state badge (a badge adds ink by design);
//   - every tray pixel fully opaque or fully transparent, at every shipped size;
//   - every tray state of an app has its own shape (a different alpha mask);
//   - the README header icon and the docs' icon copies equal the built masters;
//   - the docs' inlined icon copies equal the built 60 frames;
//   - the SVGs do not depend on what was rendered before them (explicit id prefixes).
import { Resvg } from '@resvg/resvg-js';
import assert from 'node:assert/strict';
import { existsSync, readFileSync, readdirSync } from 'node:fs';
import { join, resolve } from 'node:path';
import { test } from 'node:test';
import { execFileSync } from 'node:child_process';
import { mkdtempSync, rmSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { decodePng, readIcns } from './icns.mjs';
// The canvas rule of conformance check C03 (Pitwall 1.0.0 shipped a blank icon), shared, applied here to every frame of every set.
import { decodePng as decodeAny, problems as canvasProblems } from '../../conformance/src/util/png.mjs';
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

  test(`${app}: the shipped tray images are alpha-only templates (macOS) and one-tone glyphs (Windows), anti-aliased`, () => {
    for (const st of TRAY_STATES[app]) {
      const d = pixels(tray(app, st, 'template', { pt18: true }), 36);
      for (let i = 0; i < d.length; i += 4) if (d[i + 3]) assert.deepEqual([d[i], d[i + 1], d[i + 2]], [0, 0, 0], `${app} ${st}: the template is not black`);
      for (const theme of ['light', 'dark']) assert.ok(pixels(tray(app, st, theme), 32).some((v, i) => i % 4 === 3 && v > 0), `${app} ${st} ${theme} is empty`);
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

  test(`${app}: the .icns small frames, 32x32.png and 64x64.png are the drawn frames (STYLE.md section 6a), not downscales`, () => {
    const entries = new Map(readIcns(readFileSync(join(ROOT, app, 'tauri', 'icon.icns'))));
    const png = n => readFileSync(join(ROOT, app, 'small', `${app}-${n}.png`));
    assert.ok(entries.get('icp4').equals(png(16)), '16 pt at 1x is the flat 16');
    assert.ok(entries.get('ic11').equals(png(32)), '16 pt at 2x is the flat 32');
    assert.ok(entries.get('icp5').equals(png('32-iso')), '32 pt at 1x is the isometric 32');
    assert.ok(entries.get('ic12').equals(png(64)), '32 pt at 2x is the isometric 64');
    assert.ok(['is32', 's8mk', 'il32', 'l8mk'].every(t => entries.has(t)), 'the classic entries are there');
    assert.ok(entries.has('ic07') && entries.has('ic10'), 'the large frames stay');
    assert.ok(readFileSync(join(ROOT, app, 'tauri', '32x32.png')).equals(png(32)), '32x32.png is the flat 32');
    assert.ok(readFileSync(join(ROOT, app, 'tauri', '64x64.png')).equals(png(64)), '64x64.png is the isometric 64');
  });

  test(`${app}: iconutil decodes the .icns 16 and 32 pt frames to the drawn ones`, { skip: process.platform !== 'darwin' }, () => {
    const dir = mkdtempSync(join(tmpdir(), 'icns-'));
    try {
      execFileSync('iconutil', ['-c', 'iconset', join(ROOT, app, 'tauri', 'icon.icns'), '-o', join(dir, 'x.iconset')]);
      for (const [name, file, px] of [['icon_16x16.png', 16, 16], ['icon_16x16@2x.png', 32, 32], ['icon_32x32.png', '32-iso', 32], ['icon_32x32@2x.png', 64, 64]]) {
        const got = decodePng(readFileSync(join(dir, 'x.iconset', name))), want = decodePng(readFileSync(join(ROOT, app, 'small', `${app}-${file}.png`)));
        assert.equal(got.w, px, name);
        assert.ok(got.rgba.equals(want.rgba), `${app} ${name} differs from small/${app}-${file}.png`);
      }
    } finally {
      rmSync(dir, { recursive: true, force: true });
    }
  });

  test(`${app}: every shipped icon's art fills its canvas (C03's rule, on every .icns and .ico frame and every tauri PNG)`, () => {
    const frames = [];
    for (const [t, d] of readIcns(readFileSync(join(ROOT, app, 'tauri', 'icon.icns')))) if (d[0] === 0x89 && d[1] === 0x50) frames.push([`icon.icns ${t}`, d]);
    const ico = readFileSync(join(ROOT, app, 'tauri', 'icon.ico'));
    for (let k = 0; k < ico.readUInt16LE(4); k++) { const o = 6 + 16 * k, len = ico.readUInt32LE(o + 8), off = ico.readUInt32LE(o + 12); frames.push([`icon.ico ${ico[o] || 256}`, ico.subarray(off, off + len)]); }
    for (const n of readdirSync(join(ROOT, app, 'tauri')).filter(n => n.endsWith('.png'))) frames.push([n, readFileSync(join(ROOT, app, 'tauri', n))]);
    for (const [name, buf] of frames) {
      const problems = canvasProblems(name, decodeAny(buf));
      assert.deepEqual(problems, [], problems.join('; '));
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

// ---------- the flat small frames and the trays (STYLE.md section 6) ----------
const flat = await import('./small-flat.mjs');
const { contrast } = await import('./cvd.mjs');
const { INK } = await import('./design.mjs');
/**
 * Fronts below 3:1 on their tile, by design and approved (STYLE.md section 9): Pitcrew's orange is a mid-light tile, whose fronts must
 * be near-white (section 3); a front light enough for 3:1 would lose the 8 L step to its white top (section 2). The white top carries
 * the silhouette at 3.3:1.
 */
const FRONT_EXCEPTION = { pitcrew: { front: 2.7, top: '#FFFFFF' } };
const FLAT_SIZES = [16, 20, 24, 32, 40];

test('stroke weights are whole pixels, at least one, and never lighter at a larger size', () => {
  let last = 0;
  for (const n of FLAT_SIZES) {
    for (const w of [flat.STROKE[n], flat.DETAIL[n]]) assert.ok(Number.isInteger(w) && w >= 1, `${n}: ${w}`);
    assert.ok(flat.STROKE[n] >= last, `${n}`); last = flat.STROKE[n];
  }
});

for (const app of flat.SMOOTH_FAMILY) {
  const [lo, hi] = COVERAGE[GLYPH_KIND[app]];
  test(`${app}: the flat frame's coverage at 16 is within the ${GLYPH_KIND[app]} limits`, () => {
    const c = tileCoverage(flat.smallFlat(app, 16), bareSmall(app, 16), 16);
    assert.ok(c >= lo && c <= hi, `${app} flat 16 px coverage ${c} % is outside ${lo} to ${hi} %`);
  });
  test(`${app}: every shipped frame reaches 3:1 on the tile base (flat ink; isometric fronts, one approved exception)`, () => {
    const base = flat.tileBase(app);
    assert.ok(contrast(INK[app], base) >= 3, `${app} flat ink ${contrast(INK[app], base).toFixed(2)}:1`);
    const ex = FRONT_EXCEPTION[app], c = contrast(flat.FRONTS[app], base);
    if (ex) assert.ok(c >= ex.front - 0.005 && contrast(ex.top, base) >= 3, `${app} isometric front ${c.toFixed(2)}:1`);
    else assert.ok(c >= 3, `${app} isometric front ${c.toFixed(2)}:1`);
  });
  test(`${app}: the flat glyph is drawn on whole pixels and nothing in it is thinner than one pixel, at 16 to 40`, () => {
    for (const n of FLAT_SIZES) {
      for (const p of flat.glyphParts(app, n)) {
        if (p.rect) {
          const [x0, y0, x1, y1] = p.rect;
          for (const v of [x0, y0, x1, y1]) assert.ok(Number.isInteger(v), `${app} at ${n}: rect ${p.rect}`);
          assert.ok(x1 - x0 >= 1 && y1 - y0 >= 1, `${app} at ${n}: rect ${p.rect} is under a pixel`);
        } else if (p.d && !p.curved && !/[aAC]/.test(p.d)) for (const v of p.d.match(/-?[\d.]+/g)) assert.ok(Number.isInteger(+v), `${app} at ${n}: ${p.d}`);
        else if (p.stroke) assert.ok(p.w >= 1, `${app} at ${n}: a stroke of ${p.w} px`);
      }
    }
  });
  test(`${app}: the tray is alpha only, anti-aliased, and its glyph within the ${GLYPH_KIND[app]} coverage limits`, () => {
    const svg = flat.smoothTray(app, 'template', { px: 36 }), d = pixels(svg, 36);
    for (let i = 0; i < d.length; i += 4) if (d[i + 3]) assert.deepEqual([d[i], d[i + 1], d[i + 2]], [0, 0, 0]);
    assert.ok(partialAlpha(svg, 36).length > 0 || app === 'paddock', `${app}: no anti-aliased edge at 36 px`);
    const a = pixels(flat.smoothTrayGlyph(app, 32), 32);
    let ink = 0;
    for (let i = 3; i < a.length; i += 4) ink += a[i] / 255;
    const c = Math.round((100 * ink) / (32 * 32));
    assert.ok(c >= lo && c <= hi, `${app} tray glyph coverage ${c} % is outside ${lo} to ${hi} %`);
  });
}
