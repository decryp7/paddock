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

// ---- the shared sources (templates/repo, docs/blocks, lint) of this very checkout --------------------------------------
import { fileURLToPath } from 'node:url';
import { readFileSync } from 'node:fs';
import { loadManifest } from '../src/sync/baseline.mjs';
import { fill } from '../src/sync/fill.mjs';
import { loadBlocks, inspectBlock } from '../src/sync/blocks.mjs';

/** This checkout of Paddock: the reference for baseline and doc-block tests. */
export const PADDOCK = fileURLToPath(new URL('../..', import.meta.url));

/** The files of an app repo that is exactly in line with the templates (placeholders filled for `app`). */
export function baselineFiles(app = 'demo') {
  const config = { app, appDir: 'app' };
  const files = {};
  for (const e of loadManifest(PADDOCK)) {
    const text = fill(e.text, config);
    const path = e.path.startsWith('app/') ? e.path : e.path;
    files[path] = e.mode === 'sections' && e.path === 'Cargo.toml' ? `[workspace]\nmembers = []\n\n${text}` : text;
  }
  return files;
}

/** AGENTS.md and the other doc files with every default block in place, plus an app section of its own. */
export function blockFiles(app = 'demo') {
  const config = { app, appDir: 'app' };
  const reg = loadBlocks(PADDOCK);
  const files = {};
  for (const [file, names] of Object.entries(reg.defaultTargets)) {
    let text = file === 'AGENTS.md' ? `# ${app} rules\n\nOwn intro.\n\n## Own section\n\nKept.\n` : '';
    for (const n of names) text = inspectBlock(text, reg.blocks.find((b) => b.name === n), config).desired;
    files[file] = text;
  }
  return files;
}
export const readText = (p) => readFileSync(p, 'utf8');

/**
 * A minimal PE image with one section (.idata at RVA 0x1000, file offset 0x200) holding the import and delay-import
 * descriptors and the DLL names. `plus` selects PE32+ (x64) or PE32; `delayAttributes` 0 writes the old VA-based delay-load
 * descriptors.
 */
export function pe({ imports = [], delay = [], plus = true, delayAttributes = 1 } = {}) {
  const optSize = plus ? 240 : 224;
  const peOff = 0x40;
  const secTable = peOff + 24 + optSize;
  const raw = 0x200;
  const va = 0x1000;
  const sec = Buffer.alloc(0x400);
  // Layout inside the section: import descriptors, a zero one, delay descriptors, a zero one, then the names.
  const impAt = 0;
  const delayAt = (imports.length + 1) * 20;
  let nameAt = delayAt + (delay.length + 1) * 32;
  const putName = (n) => {
    const at = nameAt;
    sec.write(`${n}\0`, at, 'latin1');
    nameAt += n.length + 1;
    return va + at;
  };
  imports.forEach((n, i) => sec.writeUInt32LE(putName(n), impAt + i * 20 + 12));
  delay.forEach((n, i) => {
    sec.writeUInt32LE(delayAttributes, delayAt + i * 32);
    sec.writeUInt32LE(putName(n), delayAt + i * 32 + 4);
  });

  const b = Buffer.alloc(raw + sec.length);
  b.writeUInt16LE(0x5a4d, 0);
  b.writeUInt32LE(peOff, 0x3c);
  b.writeUInt32LE(0x00004550, peOff);
  b.writeUInt16LE(plus ? 0x8664 : 0x14c, peOff + 4);
  b.writeUInt16LE(1, peOff + 6);
  b.writeUInt16LE(optSize, peOff + 20);
  const opt = peOff + 24;
  b.writeUInt16LE(plus ? 0x20b : 0x10b, opt);
  const dirs = opt + (plus ? 112 : 96);
  b.writeUInt32LE(16, opt + (plus ? 108 : 92));
  if (imports.length) b.writeUInt32LE(va + impAt, dirs + 8);
  if (delay.length) b.writeUInt32LE(va + delayAt, dirs + 13 * 8);
  b.write('.idata', secTable, 'latin1');
  b.writeUInt32LE(sec.length, secTable + 8);
  b.writeUInt32LE(va, secTable + 12);
  b.writeUInt32LE(sec.length, secTable + 16);
  b.writeUInt32LE(raw, secTable + 20);
  sec.copy(b, raw);
  return b;
}
