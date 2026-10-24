#!/usr/bin/env node
// Pins a `paddock-runtime` catalogue: downloads every platform's archive once, computes its size and SHA-256 while reading
// (one pass, nothing kept on disk) and rewrites the catalogue's `version`, `url`, `size` and `sha256`. Run it by hand, review
// the diff, commit: nothing is downloaded at build time and an app downloads a runtime only on the user's request. Replaces
// Spotter's pin-chromium.mjs (and the hand-kept checksum tables of Debrief's runtimes).
//
//   node pin-runtime.mjs <catalogue.toml>                          re-pin the URLs as they are (sizes and checksums)
//   node pin-runtime.mjs <catalogue.toml> --version <v>            move every URL from the pinned version to <v>
//   node pin-runtime.mjs <catalogue.toml> --chrome-for-testing [Stable|Beta|..] [--endpoint <url>]
//                                                                  the channel's version and URLs from Chrome for Testing
import { createHash } from 'node:crypto';
import { readFileSync, writeFileSync } from 'node:fs';
import { pathToFileURL } from 'node:url';
import { readCatalogue, writeCatalogue } from './runtime-catalogue.mjs';

/** Chrome for Testing's channel list with download URLs. */
export const CFT_ENDPOINT = 'https://googlechromelabs.github.io/chrome-for-testing/last-known-good-versions-with-downloads.json';

/** Chrome for Testing platform ids by catalogue key (Windows on ARM has no build). */
export const CFT_PLATFORMS = {
  'windows-x86_64': 'win64',
  'macos-aarch64': 'mac-arm64',
  'macos-x86_64': 'mac-x64',
  'linux-x86_64': 'linux64',
  'linux-aarch64': 'linux-arm64',
};

/** `url` with its `/<from>/` segment replaced by `/<to>/`; throws unless the old version is in the URL exactly once. */
export function moveUrl(url, from, to) {
  const parts = url.split(`/${from}/`);
  if (parts.length !== 2) throw new Error(`${url} does not name version ${from} once`);
  return parts.join(`/${to}/`);
}

/** The version and per-key URLs of a Chrome for Testing channel. */
export function cftDownloads(json, channel, keys) {
  const c = json?.channels?.[channel];
  if (!c) throw new Error(`channel ${channel} not found`);
  const urls = {};
  for (const key of keys) {
    const id = CFT_PLATFORMS[key];
    const d = (c.downloads?.chrome ?? []).find((x) => x.platform === id);
    if (!d) throw new Error(`Chrome for Testing ${c.version} has no build for ${key}`);
    urls[key] = d.url;
  }
  return { version: c.version, urls };
}

/** Size and SHA-256 of what `url` serves, read once as a stream. Only https (or http to the loopback, for tests). */
export async function measure(url, fetchImpl = fetch) {
  if (!/^https:\/\//.test(url) && !/^http:\/\/(127\.0\.0\.1|localhost|\[::1\]):/.test(url)) throw new Error(`${url} is not an https address`);
  const res = await fetchImpl(url, { signal: AbortSignal.timeout(30 * 60 * 1000) });
  if (!res.ok) throw new Error(`${url}: HTTP ${res.status}`);
  const h = createHash('sha256');
  let size = 0;
  for await (const chunk of res.body) {
    h.update(chunk);
    size += chunk.length;
  }
  if (size === 0) throw new Error(`${url} is empty`);
  return { size, sha256: h.digest('hex') };
}

/**
 * The new catalogue text. `version` moves the URLs to another version; `channel` takes version and URLs from Chrome for
 * Testing (`endpoint`). Every archive is measured with `fetchImpl`.
 */
export async function pin(text, { version, channel, endpoint = CFT_ENDPOINT, fetchImpl = fetch, log = () => {} } = {}) {
  const cat = readCatalogue(text);
  let target = cat.version;
  let urls = Object.fromEntries(cat.entries.map((e) => [e.key, e.url]));
  if (channel) {
    const res = await fetchImpl(endpoint, { signal: AbortSignal.timeout(60 * 1000) });
    if (!res.ok) throw new Error(`${endpoint}: HTTP ${res.status}`);
    const d = cftDownloads(await res.json(), channel, Object.keys(urls));
    target = d.version;
    urls = d.urls;
  } else if (version) {
    target = version;
    urls = Object.fromEntries(Object.entries(urls).map(([k, u]) => [k, moveUrl(u, cat.version, version)]));
  }
  const entries = {};
  for (const [key, url] of Object.entries(urls)) {
    log(`${key}: ${url}`);
    entries[key] = { url, ...(await measure(url, fetchImpl)) };
  }
  return writeCatalogue(text, target, entries);
}

/** Parses the command line. */
export function parseArgs(argv) {
  const o = { file: undefined };
  for (let i = 0; i < argv.length; i++) {
    const a = argv[i];
    if (a === '--version' || a === '--endpoint') {
      if (argv[i + 1] === undefined) throw new Error(`${a} needs a value`);
      o[a.slice(2)] = argv[++i];
    } else if (a === '--chrome-for-testing') {
      o.channel = argv[i + 1] && !argv[i + 1].startsWith('--') ? argv[++i] : 'Stable';
    } else if (a.startsWith('--')) {
      throw new Error(`unknown option ${a}`);
    } else if (o.file === undefined) {
      o.file = a;
    } else {
      throw new Error(`one catalogue at a time (got ${a})`);
    }
  }
  if (!o.file) throw new Error('usage: pin-runtime.mjs <catalogue.toml> [--version <v> | --chrome-for-testing [channel]]');
  if (o.version && o.channel) throw new Error('give --version or --chrome-for-testing, not both');
  return o;
}

/** Runs the command line; returns the new text it wrote. */
export async function main(argv, { fetchImpl = fetch, log = console.error } = {}) {
  const o = parseArgs(argv);
  const next = await pin(readFileSync(o.file, 'utf8'), { ...o, fetchImpl, log });
  writeFileSync(o.file, next);
  log(`${o.file}: pinned ${readCatalogue(next).version}; review the diff before committing`);
  return next;
}

if (process.argv[1] && import.meta.url === pathToFileURL(process.argv[1]).href) {
  main(process.argv.slice(2)).catch((e) => {
    console.error(`pin-runtime: ${e.message}`);
    process.exit(1);
  });
}
