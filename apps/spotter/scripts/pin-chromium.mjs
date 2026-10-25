// Pins the Chromium build Spotter may download: reads Chrome for Testing's "last known good" JSON, downloads each
// archive, computes its SHA-256 and rewrites crates/spotter-render/src/chromium-manifest.toml. Run it by hand, review
// the diff, commit. Nothing downloads at build time and the app only downloads on the user's request (ADR 0004).
//
//   node scripts/pin-chromium.mjs [Stable|Beta|...]    (default: Stable)
import { createHash } from 'node:crypto';
import { writeFileSync } from 'node:fs';
import { fileURLToPath } from 'node:url';

export const ENDPOINT = 'https://googlechromelabs.github.io/chrome-for-testing/last-known-good-versions-with-downloads.json';

/** Chrome for Testing platform id to Spotter's manifest key. Windows on ARM has no build and is left out. */
export const PLATFORMS = {
  win64: 'windows-x86_64',
  'mac-arm64': 'macos-aarch64',
  'mac-x64': 'macos-x86_64',
  linux64: 'linux-x86_64',
  'linux-arm64': 'linux-aarch64',
};

/** The manifest text for a version and its `{key: {url, sha256}}` entries (pure). */
export function renderManifest(version, entries) {
  const head = `# The pinned Chromium builds Spotter may download (ADR 0002, ADR 0004): one entry per platform with the archive URL and
# its SHA-256. \`install\` refuses anything that does not match the checksum.
#
# Source: Chrome for Testing (https://googlechromelabs.github.io/chrome-for-testing/), Google's builds of Chrome meant
# for automation. The URLs are versioned and do not change. The JSON endpoints publish no hashes, so the SHA-256 values
# are computed from the downloaded archives by \`node scripts/pin-chromium.mjs\` (which rewrites this file); review the
# diff and the version before committing a new pin.
#
# Keys: <windows|macos|linux>-<x86_64|aarch64>.

version = "${version}"
`;
  const body = Object.entries(entries)
    .map(([key, e]) => `\n[[platform]]\nkey = "${key}"\nurl = "${e.url}"\nsha256 = "${e.sha256}"\n`)
    .join('');
  return head + body;
}

/** The download entries of one channel, by manifest key (pure). */
export function pickDownloads(json, channel) {
  const c = json?.channels?.[channel];
  if (!c) throw new Error(`channel ${channel} not found`);
  const out = {};
  for (const d of c.downloads?.chrome ?? []) {
    const key = PLATFORMS[d.platform];
    if (key) out[key] = d.url;
  }
  return { version: c.version, urls: out };
}

async function sha256Of(url) {
  const res = await fetch(url);
  if (!res.ok) throw new Error(`${url}: ${res.status}`);
  const h = createHash('sha256');
  for await (const chunk of res.body) h.update(chunk);
  return h.digest('hex');
}

if (process.argv[1] === fileURLToPath(import.meta.url)) {
  const channel = process.argv[2] ?? 'Stable';
  const { version, urls } = pickDownloads(await (await fetch(ENDPOINT)).json(), channel);
  const entries = {};
  for (const [key, url] of Object.entries(urls)) {
    process.stderr.write(`hashing ${key} ...\n`);
    entries[key] = { url, sha256: await sha256Of(url) };
  }
  const file = new URL('../crates/spotter-render/src/chromium-manifest.toml', import.meta.url);
  writeFileSync(file, renderManifest(version, entries));
  process.stderr.write(`pinned ${version}\n`);
}
