// Fixture helpers: apps are built as real folders in a temp dir, good and bad variants side by side.
import { mkdirSync, mkdtempSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { dirname, join } from 'node:path';
import { crc32, deflateSync } from 'node:zlib';
import { runChecks } from '../src/runner.mjs';

/** Writes `files` ({ path: string | Buffer }) into a new temp folder and returns it. */
export function makeApp(files) {
  const root = mkdtempSync(join(tmpdir(), 'x9-fixture-'));
  for (const [p, content] of Object.entries(files)) {
    mkdirSync(dirname(join(root, p)), { recursive: true });
    writeFileSync(join(root, p), content);
  }
  return root;
}

/** An 8-bit RGBA PNG; `pixel(x, y) -> [r, g, b, a]`. */
export function png(size, pixel) {
  const raw = Buffer.alloc((size * 4 + 1) * size);
  for (let y = 0; y < size; y++) {
    for (let x = 0; x < size; x++) {
      const o = y * (size * 4 + 1) + 1 + x * 4;
      const [r, g, b, a] = pixel(x, y);
      raw[o] = r; raw[o + 1] = g; raw[o + 2] = b; raw[o + 3] = a;
    }
    raw[y * (size * 4 + 1)] = 0;
  }
  const chunk = (type, data) => {
    const len = Buffer.alloc(4);
    len.writeUInt32BE(data.length);
    const body = Buffer.concat([Buffer.from(type, 'latin1'), data]);
    const crc = Buffer.alloc(4);
    crc.writeUInt32BE(crc32(body));
    return Buffer.concat([len, body, crc]);
  };
  const ihdr = Buffer.alloc(13);
  ihdr.writeUInt32BE(size, 0); ihdr.writeUInt32BE(size, 4);
  ihdr[8] = 8; ihdr[9] = 6;
  return Buffer.concat([Buffer.from([0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a]), chunk('IHDR', ihdr), chunk('IDAT', deflateSync(raw)), chunk('IEND', Buffer.alloc(0))]);
}

/** Artwork over the whole canvas (good) and artwork in the top-left corner only (the Pitwall 1.0.0 icon). */
export const fullIcon = (n = 64) => png(n, () => [20, 30, 200, 255]);
export const cornerIcon = (n = 64) => png(n, (x, y) => (x < n / 4 && y < n / 4 ? [20, 30, 200, 255] : [255, 255, 255, 255]));

/** Runs one check over a fixture; returns its findings. */
export async function findingsOf(id, root, { app = 'demo', options = {} } = {}) {
  const r = await runChecks({ root, app, only: [id], options });
  return r.findings;
}

export const ids = (findings) => findings.map((f) => f.check);
export const messages = (findings) => findings.map((f) => f.message).join('\n');

export const TAURI_CONF = (version = '1.2.3') => JSON.stringify({ productName: 'Demo', version });
