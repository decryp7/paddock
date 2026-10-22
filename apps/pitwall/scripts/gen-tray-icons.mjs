// Draws Pitwall's tray icons (app/src-tauri/icons/tray): a ring with a state glyph inside, so each state differs in
// shape and not only in colour. macOS gets black template images (the system tints them); Windows gets white (dark
// taskbar) and near-black (light taskbar) variants. Pure Node, no dependencies; the output is committed, so this runs
// only when the glyphs change:  node scripts/gen-tray-icons.mjs
import { deflateSync } from 'node:zlib';
import { mkdirSync, writeFileSync } from 'node:fs';
import { dirname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

const out = resolve(dirname(fileURLToPath(import.meta.url)), '..', 'app', 'src-tauri', 'icons', 'tray');

// Signed distance helpers in a unit square [-1, 1]^2; negative inside.
const circle = (x, y, r) => Math.hypot(x, y) - r;
const box = (x, y, cx, cy, hw, hh) => Math.max(Math.abs(x - cx) - hw, Math.abs(y - cy) - hh);
const ring = (x, y, r, w) => Math.abs(circle(x, y, r)) - w / 2;
const GLYPHS = {
  // Idle: ring and a small dot.
  ok: (x, y) => Math.min(ring(x, y, 0.84, 0.2), circle(x, y, 0.28)),
  // Busy: ring and a solid disc.
  busy: (x, y) => Math.min(ring(x, y, 0.84, 0.2), circle(x, y, 0.55)),
  // Paused: ring and two bars.
  paused: (x, y) => Math.min(ring(x, y, 0.84, 0.2), box(x, y, -0.25, 0, 0.12, 0.38), box(x, y, 0.25, 0, 0.12, 0.38)),
  // Unavailable: ring with a bar across it.
  error: (x, y) => {
    const c = Math.SQRT1_2;
    const d = Math.abs((x * c) + (y * c)) - 0.09; // a diagonal band
    return Math.min(ring(x, y, 0.84, 0.2), Math.max(d, circle(x, y, 0.84)));
  },
};

function png(size, glyph, [r, g, b]) {
  const ss = 4;
  const raw = Buffer.alloc((size * 4 + 1) * size);
  for (let py = 0; py < size; py++) {
    raw[py * (size * 4 + 1)] = 0;
    for (let px = 0; px < size; px++) {
      let hit = 0;
      for (let sy = 0; sy < ss; sy++)
        for (let sx = 0; sx < ss; sx++) {
          const x = ((px + (sx + 0.5) / ss) / size) * 2 - 1;
          const y = ((py + (sy + 0.5) / ss) / size) * 2 - 1;
          if (glyph(x, y) <= 0) hit++;
        }
      const o = py * (size * 4 + 1) + 1 + px * 4;
      raw[o] = r; raw[o + 1] = g; raw[o + 2] = b; raw[o + 3] = Math.round((hit / (ss * ss)) * 255);
    }
  }
  const crcTable = Array.from({ length: 256 }, (_, n) => { let c = n; for (let k = 0; k < 8; k++) c = c & 1 ? 0xedb88320 ^ (c >>> 1) : c >>> 1; return c >>> 0; });
  const crc = (buf) => { let c = 0xffffffff; for (const v of buf) c = crcTable[(c ^ v) & 255] ^ (c >>> 8); return (c ^ 0xffffffff) >>> 0; };
  const chunk = (type, data) => {
    const len = Buffer.alloc(4); len.writeUInt32BE(data.length);
    const td = Buffer.concat([Buffer.from(type), data]);
    const c = Buffer.alloc(4); c.writeUInt32BE(crc(td));
    return Buffer.concat([len, td, c]);
  };
  const ihdr = Buffer.alloc(13); ihdr.writeUInt32BE(size, 0); ihdr.writeUInt32BE(size, 4); ihdr[8] = 8; ihdr[9] = 6;
  return Buffer.concat([Buffer.from([137, 80, 78, 71, 13, 10, 26, 10]), chunk('IHDR', ihdr), chunk('IDAT', deflateSync(raw)), chunk('IEND', Buffer.alloc(0))]);
}

mkdirSync(join(out, 'macos'), { recursive: true });
mkdirSync(join(out, 'windows'), { recursive: true });
for (const [state, glyph] of Object.entries(GLYPHS)) {
  writeFileSync(join(out, 'macos', `pitwall-tray-${state}-18ptTemplate.png`), png(18, glyph, [0, 0, 0]));
  writeFileSync(join(out, 'macos', `pitwall-tray-${state}-18ptTemplate@2x.png`), png(36, glyph, [0, 0, 0]));
  writeFileSync(join(out, 'windows', `pitwall-tray-${state}-dark-32.png`), png(32, glyph, [255, 255, 255]));
  writeFileSync(join(out, 'windows', `pitwall-tray-${state}-light-32.png`), png(32, glyph, [27, 27, 31]));
}
console.log(`tray icons written to ${out}`);
