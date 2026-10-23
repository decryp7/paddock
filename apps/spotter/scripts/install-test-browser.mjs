// Installs the pinned Chrome for Testing build (the one in crates/spotter-render/src/chromium-manifest.toml, ADR 0004)
// into the folder the real-browser tests look in, so tests never launch an installed Chrome, Edge or Chromium:
//
//   node scripts/install-test-browser.mjs [dir]     (default: $SPOTTER_TEST_BROWSER_DIR or ~/.cache/spotter-test-browser)
//
// The archive is verified against the manifest's SHA-256 before it is extracted. `unzip` (or `tar` on Windows) is used.
import { createHash } from 'node:crypto';
import { execFileSync } from 'node:child_process';
import { mkdirSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from 'node:fs';
import { homedir, tmpdir } from 'node:os';
import { join } from 'node:path';
import { fileURLToPath } from 'node:url';

const MANIFEST = fileURLToPath(new URL('../crates/spotter-render/src/chromium-manifest.toml', import.meta.url));

/** The manifest key of this machine, as the Rust side computes it. */
export function platformKey(platform = process.platform, arch = process.arch) {
  const os = { win32: 'windows', darwin: 'macos', linux: 'linux' }[platform];
  const cpu = { x64: 'x86_64', arm64: 'aarch64' }[arch];
  if (!os || !cpu) throw new Error(`no pinned Chromium for ${platform}/${arch}`);
  return `${os}-${cpu}`;
}

/** `{url, sha256}` of one platform from the manifest text (pure). */
export function entryFor(text, key) {
  for (const block of text.split('[[platform]]').slice(1)) {
    const get = (k) => block.match(new RegExp(`^${k} = "([^"]*)"`, 'm'))?.[1];
    if (get('key') === key) return { url: get('url'), sha256: get('sha256') };
  }
  throw new Error(`no entry for ${key} in the manifest`);
}

async function main() {
  const dir = process.argv[2] ?? process.env.SPOTTER_TEST_BROWSER_DIR ?? join(homedir(), '.cache', 'spotter-test-browser');
  const { url, sha256 } = entryFor(readFileSync(MANIFEST, 'utf8'), platformKey());
  const work = mkdtempSync(join(tmpdir(), 'spotter-cft-'));
  try {
    const res = await fetch(url);
    if (!res.ok) throw new Error(`${url}: ${res.status}`);
    const bytes = Buffer.from(await res.arrayBuffer());
    const got = createHash('sha256').update(bytes).digest('hex');
    if (got !== sha256) throw new Error(`checksum mismatch for ${url}: ${got}`);
    const zip = join(work, 'cft.zip');
    writeFileSync(zip, bytes);
    rmSync(dir, { recursive: true, force: true });
    mkdirSync(dir, { recursive: true });
    if (process.platform === 'win32') execFileSync('tar', ['-xf', zip, '-C', dir]);
    else execFileSync('unzip', ['-q', zip, '-d', dir]);
    console.log(`installed the test browser into ${dir}`);
  } finally {
    rmSync(work, { recursive: true, force: true });
  }
}

if (process.argv[1] === fileURLToPath(import.meta.url)) await main();
