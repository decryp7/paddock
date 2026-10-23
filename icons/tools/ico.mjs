// Minimal .ico writer with PNG-compressed frames (supported since Windows Vista).
/** frames: [{ size, png: Buffer }] -> Buffer */
export function writeIco(frames) {
  const sorted = [...frames].sort((a, b) => a.size - b.size);
  const header = Buffer.alloc(6);
  header.writeUInt16LE(0, 0);
  header.writeUInt16LE(1, 2);
  header.writeUInt16LE(sorted.length, 4);
  const dir = Buffer.alloc(16 * sorted.length);
  let offset = 6 + dir.length;
  sorted.forEach((fr, i) => {
    const o = i * 16;
    dir.writeUInt8(fr.size >= 256 ? 0 : fr.size, o);
    dir.writeUInt8(fr.size >= 256 ? 0 : fr.size, o + 1);
    dir.writeUInt8(0, o + 2);
    dir.writeUInt8(0, o + 3);
    dir.writeUInt16LE(1, o + 4);
    dir.writeUInt16LE(32, o + 6);
    dir.writeUInt32LE(fr.png.length, o + 8);
    dir.writeUInt32LE(offset, o + 12);
    offset += fr.png.length;
  });
  return Buffer.concat([header, dir, ...sorted.map(f => f.png)]);
}
