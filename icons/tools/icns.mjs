// Minimal .icns reader/writer: `tauri icon` builds the small frames (16, 32 and their @2x) as downscales of the 1024 master, which looks muddy;
// tools/build.mjs swaps those for the hand-tuned frames with `withSmallFrames`.
import { inflateSync } from 'node:zlib';

const TYPES = { icp4: 16, icp5: 32, ic11: '16@2x', ic12: 64 };
const CLASSIC = { 16: ['is32', 's8mk'], 32: ['il32', 'l8mk'] };

/** Decodes an 8-bit, non-interlaced RGBA PNG (what resvg writes) to { w, h, rgba }. */
export function decodePng(png) {
  let w = 0, h = 0;
  const idat = [];
  for (let p = 8; p + 8 <= png.length; ) {
    const len = png.readUInt32BE(p), type = png.toString('latin1', p + 4, p + 8), d = png.subarray(p + 8, p + 8 + len);
    if (type === 'IHDR') {
      w = d.readUInt32BE(0); h = d.readUInt32BE(4);
      if (d[8] !== 8 || d[9] !== 6 || d[12] !== 0) throw new Error('expected an 8-bit RGBA PNG without interlace');
    } else if (type === 'IDAT') idat.push(d);
    p += 12 + len;
  }
  const raw = inflateSync(Buffer.concat(idat)), stride = w * 4, out = Buffer.alloc(h * stride);
  for (let y = 0; y < h; y++) {
    const ft = raw[y * (stride + 1)];
    for (let x = 0; x < stride; x++) {
      const v = raw[y * (stride + 1) + 1 + x], a = x >= 4 ? out[y * stride + x - 4] : 0, u = y ? out[(y - 1) * stride + x] : 0, c = x >= 4 && y ? out[(y - 1) * stride + x - 4] : 0;
      let r;
      if (ft === 0) r = v; else if (ft === 1) r = v + a; else if (ft === 2) r = v + u; else if (ft === 3) r = v + ((a + u) >> 1);
      else { const pp = a + u - c, pa = Math.abs(pp - a), pb = Math.abs(pp - u), pc = Math.abs(pp - c); r = v + (pa <= pb && pa <= pc ? a : pb <= pc ? u : c); }
      out[y * stride + x] = r & 255;
    }
  }
  return { w, h, rgba: out };
}

/** The icns run-length coding of one colour channel (control byte below 0x80: n+1 literals; from 0x80: a byte repeated n-0x80+3 times). */
function rle(bytes) {
  const out = [];
  for (let i = 0; i < bytes.length; ) {
    let run = 1;
    while (i + run < bytes.length && bytes[i + run] === bytes[i] && run < 130) run++;
    if (run >= 3) { out.push(0x80 + run - 3, bytes[i]); i += run; continue; }
    let n = 0;
    while (i + n < bytes.length && n < 128 && !(i + n + 2 < bytes.length && bytes[i + n] === bytes[i + n + 1] && bytes[i + n] === bytes[i + n + 2])) n++;
    n = Math.max(n, 1);
    out.push(n - 1, ...bytes.subarray(i, i + n));
    i += n;
  }
  return Buffer.from(out);
}

/** The classic entries of a 16 or 32 px frame: the RLE-coded RGB planes (`is32`, `il32`) and the 8-bit alpha mask (`s8mk`, `l8mk`). */
function classic(px, png) {
  const { w, h, rgba } = decodePng(png);
  if (w !== px || h !== px) throw new Error(`expected a ${px} px frame`);
  const plane = c => Buffer.from(Array.from({ length: px * px }, (_, i) => rgba[i * 4 + c]));
  const [rgbType, maskType] = CLASSIC[px];
  return [[rgbType, Buffer.concat([0, 1, 2].map(c => rle(plane(c))))], [maskType, plane(3)]];
}

/** Splits an .icns into its [type, data] entries. */
export function readIcns(buf) {
  if (buf.toString('latin1', 0, 4) !== 'icns') throw new Error('not an .icns file');
  const out = [];
  for (let p = 8; p + 8 <= buf.length; ) {
    const len = buf.readUInt32BE(p + 4);
    if (len < 8) throw new Error('corrupt .icns entry');
    out.push([buf.toString('latin1', p, p + 4), buf.subarray(p + 8, p + len)]);
    p += len;
  }
  return out;
}

/** Joins [type, data] entries into an .icns. */
export function writeIcns(entries) {
  const body = entries.map(([t, d]) => { const h = Buffer.alloc(8); h.write(t, 0, 'latin1'); h.writeUInt32BE(d.length + 8, 4); return Buffer.concat([h, d]); });
  const head = Buffer.alloc(8);
  head.write('icns', 0, 'latin1');
  head.writeUInt32BE(8 + body.reduce((n, b) => n + b.length, 0), 4);
  return Buffer.concat([head, ...body]);
}

/**
 * Replaces the small frames of an .icns with hand-tuned PNGs: `frames` maps a pixel size (16, 32, 64, and '16@2x' for the 16 pt slot
 * at 2x when it is not the 32 frame) to its PNG. 16 pt is `icp4`,
 * 32 px is `icp5` and 16 pt at 2x (`ic11`), 64 px is 32 pt at 2x (`ic12`). The classic entries (`is32`/`s8mk`, `il32`/`l8mk`: what
 * `iconutil` and older readers decode) are rewritten from the same two frames.
 */
export function withSmallFrames(icns, frames) {
  const keep = readIcns(icns).filter(([t]) => !(t in TYPES) && !Object.values(CLASSIC).flat().includes(t));
  // 16 pt at 2x (`ic11`) may differ from the 32 pt frame (`icp5`): the family shows the flat glyph in the 16 pt slot at every scale.
  const small = Object.entries(TYPES).map(([t, px]) => [t, frames[px] ?? frames[32]]);
  return writeIcns([...classic(16, frames[16]), ...classic(32, frames[32]), ...small, ...keep]);
}
