// Minimal .icns reader/writer: `tauri icon` builds the small frames (16, 32 and their @2x) as downscales of the 1024 master, which looks muddy;
// tools/build.mjs swaps those for the hand-tuned frames with `withSmallFrames`.
const TYPES = { icp4: 16, icp5: 32, ic11: 32, ic12: 64 };

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
 * Replaces the small frames of an .icns with hand-tuned PNGs: `frames` maps a pixel size (16, 32, 64) to its PNG. 16 pt is `icp4`,
 * 32 px is `icp5` and 16 pt at 2x (`ic11`), 64 px is 32 pt at 2x (`ic12`). The old 8-bit RLE entries (is32, il32 and their masks) are
 * dropped; they were downscales too.
 */
export function withSmallFrames(icns, frames) {
  const keep = readIcns(icns).filter(([t]) => !(t in TYPES) && !['is32', 's8mk', 'il32', 'l8mk'].includes(t));
  const small = Object.entries(TYPES).map(([t, px]) => [t, frames[px]]);
  return writeIcns([...small, ...keep]);
}
