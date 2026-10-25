// A `paddock-runtime` catalogue (crates/paddock-runtime: `format`, `name`, `version`, `[[platform]]` with `key`, `url`, `size`,
// `sha256`), read and rewritten line by line so comments and order stay as the author wrote them. The Rust crate is the
// parser that matters (it refuses anything off); this one only finds and replaces the pinned values.

/** Catalogue keys of the Node platform/arch pairs a runtime is pinned for. */
const KEYS = {
  'linux/x64': 'linux-x86_64',
  'linux/arm64': 'linux-aarch64',
  'darwin/x64': 'macos-x86_64',
  'darwin/arm64': 'macos-aarch64',
  'win32/x64': 'windows-x86_64',
  'win32/arm64': 'windows-aarch64',
};

/** The catalogue key of a Node platform/arch, or throws. */
export function platformKey(platform = process.platform, arch = process.arch) {
  const key = KEYS[`${platform}/${arch}`];
  if (!key) throw new Error(`no catalogue key for ${platform}/${arch}`);
  return key;
}

const str = (line, name) => line.match(new RegExp(`^\\s*${name}\\s*=\\s*"([^"]*)"\\s*(#.*)?$`))?.[1];
const num = (line, name) => line.match(new RegExp(`^\\s*${name}\\s*=\\s*(\\d+)\\s*(#.*)?$`))?.[1];

/**
 * The pinned values: `{ name, version, entries: [{ key, url, size, sha256 }] }`. Throws when the top-level `version` or an
 * entry's `key` or `url` is missing.
 */
export function readCatalogue(text) {
  const lines = text.split('\n');
  let name;
  let version;
  const entries = [];
  let cur = null;
  for (const line of lines) {
    if (/^\s*\[\[platform\]\]\s*$/.test(line)) {
      cur = {};
      entries.push(cur);
      continue;
    }
    const target = cur ?? null;
    if (!target) {
      name ??= str(line, 'name');
      version ??= str(line, 'version');
      continue;
    }
    for (const k of ['key', 'url', 'sha256']) target[k] ??= str(line, k);
    const size = num(line, 'size');
    if (size !== undefined) target.size ??= Number(size);
  }
  if (!version) throw new Error('the catalogue has no version');
  for (const e of entries) if (!e.key || !e.url) throw new Error('every [[platform]] needs a key and a url');
  return { name, version, entries };
}

/** The entry for `key`, or throws. */
export function entryFor(catalogue, key) {
  const e = catalogue.entries.find((x) => x.key === key);
  if (!e) throw new Error(`the catalogue pins no build for ${key}`);
  return e;
}

/**
 * Rewrites `text` with a new `version` and, per key in `entries`, its `url`, `size` and `sha256` (a missing `size` or `sha256`
 * line is added after the url). Every platform of the catalogue must be in `entries`.
 */
export function writeCatalogue(text, version, entries) {
  const out = [];
  let pending = null;
  const flush = () => {
    if (!pending) return;
    if (!pending.size) out.push(`size = ${pending.e.size}`);
    if (!pending.sha) out.push(`sha256 = "${pending.e.sha256}"`);
    pending = null;
  };
  let inHeader = true;
  let topVersion = false;
  for (const line of text.split('\n')) {
    if (/^\s*\[\[platform\]\]\s*$/.test(line)) {
      flush();
      inHeader = false;
      out.push(line);
      continue;
    }
    if (inHeader && str(line, 'version') !== undefined) {
      out.push(`version = "${version}"`);
      topVersion = true;
      continue;
    }
    const k = str(line, 'key');
    if (k !== undefined) {
      const e = entries[k];
      if (!e) throw new Error(`no new pin for ${k}`);
      pending = { e, size: false, sha: false };
      out.push(line);
      continue;
    }
    if (pending && str(line, 'url') !== undefined) {
      out.push(`url = "${pending.e.url}"`);
      continue;
    }
    if (pending && num(line, 'size') !== undefined) {
      pending.size = true;
      out.push(`size = ${pending.e.size}`);
      continue;
    }
    if (pending && str(line, 'sha256') !== undefined) {
      pending.sha = true;
      out.push(`sha256 = "${pending.e.sha256}"`);
      continue;
    }
    if (pending && line.trim() === '') flush();
    out.push(line);
  }
  flush();
  if (!topVersion) throw new Error('the catalogue has no version');
  return out.join('\n');
}
