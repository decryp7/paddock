#!/usr/bin/env node
// The bundler tools `tauri build` would download at bundle time, pinned by URL and SHA-256 and put where Tauri looks for them,
// so a bundle downloads nothing it has not pinned (ADR 0085 WP-9; Dyno 1.1.1 WP-2: a GitHub 504 failed dyno-release 7693 and
// 7694 without an annotation). Two kinds, chosen by the bundle list and the host:
//   appimage (Linux x64): AppRun, linuxdeploy, linuxdeploy-plugin-appimage (tauri-bundler 2.10.3 linuxdeploy.rs:208-278)
//   nsis (Windows x64):  the NSIS 3.13 zip and nsis_tauri_utils.dll (src/bundle/windows/nsis/mod.rs:36-79, :137-162)
//
// The tools folder is `<build folder>/.tauri`: tauri-bundle.sh passes `{"bundle":{"useLocalToolsDir":true}}` to `tauri build`,
// and tauri-cli 2.12.3 then gives the bundler cargo's `target_directory` (src/interface/mod.rs:79-83), which the bundler joins
// with `.tauri` (linuxdeploy.rs:45-50, nsis/mod.rs:82-85). This script asks cargo the same question (`cargo metadata` in
// src-tauri, where `.cargo/ci.toml` of the build-folder action applies), so both name one folder.
//
//   node tauri-tools.mjs key --bundles <list>                                   the actions/cache key of this host's pins
//   node tauri-tools.mjs install --bundles <list> --src-tauri <dir> [--tools <dir>] [--cache <dir>]
//   node tauri-tools.mjs check --bundles <list> --src-tauri <dir> [--tools <dir>]
//
// install empties the tools folder (a kept build folder's copy is never reused), takes each pinned file from the cache folder
// (re-hashed on every use) or downloads it, writes it where Tauri looks, and writes the manifest `<tools>.sha256`
// (`<sha256>  <path>` per file, the format of `./dev ci release-isolation verify --tauri-pins`): every file the folder may hold
// after the build, with the hash it must have then. check compares the folder with that manifest after the build: a file
// that is not in it (Tauri downloaded or wrote something unpinned) or has another hash is a warning, and an error on a tag.
//
// What Tauri then skips (so it downloads nothing pinned here):
// - appimage: AppRun, linuxdeploy and the plugin are downloaded only when missing (:212, :223, :249). Tauri zeroes linuxdeploy's
//   AppImage magic (3 bytes at offset 8) with `dd` on every build (:267-276), so the manifest pins the patched bytes.
//   linuxdeploy-plugin-gtk.sh and -gstreamer.sh are embedded in the bundler and written when they differ (:229-246): pinned by
//   their hash in tauri-bundler 2.10.3.
// - nsis: the NSIS folder is downloaded again unless every NSIS_REQUIRED_FILES entry exists (:47-61, :92), NSIS.exe has the
//   SHA-1 NSIS_EXE_SHA1 (:98-101) and nsis_tauri_utils.dll has NSIS_TAURI_UTILS_SHA1 (:72-77, :114-121). install checks both
//   SHA-1 values after it writes the folder, so a pin that Tauri would throw away fails here, not as a silent download.
//
// linuxdeploy-plugin-appimage: Tauri downloads it from linuxdeploy's moving `continuous` release whenever the tools folder
// lacks `linuxdeploy-plugin-appimage.AppImage` (:248-263). The project publishes no stable release, only `continuous` and dated
// `1-alpha-*` builds; the user chose on 2026-11-12 to pin the dated build 1-alpha-20250213-1 (its assets predate GitHub's
// release digests, so the SHA-256 was measured on two downloads that matched, and the size equals the release API's). With it
// in place Tauri downloads nothing, and `check` fails a tag on any file not in the manifest: there is no unpinned exception.
import { createHash } from 'node:crypto';
import { spawnSync } from 'node:child_process';
import { chmodSync, existsSync, lstatSync, mkdirSync, mkdtempSync, readdirSync, readFileSync, renameSync, rmSync, writeFileSync } from 'node:fs';
import { homedir, tmpdir } from 'node:os';
import { basename, dirname, isAbsolute, join } from 'node:path';
import { crc32, inflateRawSync } from 'node:zlib';
import { isMain } from '../src/util/is-main.mjs';

/** linuxdeploy's release in tauri-apps/binary-releases (tauri-bundler's LINUXDEPLOY_COMMIT_HASH, linuxdeploy.rs:209). */
export const LINUXDEPLOY = '07333c6';

/** The @tauri-apps/cli version the pins were read from (its tauri-bundler, =2.10.3); package-lock.json must match. */
export const PINNED_FOR_CLI = '2.12.3';

/** Which host builds which kind: the bundle name, the Node platform and arch. */
export const KINDS = {
  appimage: { bundle: 'appimage', platform: 'linux', arch: 'x64' },
  nsis: { bundle: 'nsis', platform: 'win32', arch: 'x64' },
};

/**
 * The pinned downloads per kind: the release URL, its SHA-256 (measured on download 2026-11-12; equal to the digest GitHub's
 * release API lists, except for the plugin, whose release lists none), the cache file name, and where it goes in the tools folder: `dest` for a file,
 * `unzip` (the archive's top folder) plus `dest` (the folder it becomes) for an archive. `patch: 'appimage-magic'` means Tauri
 * changes the file in place on every build, so the manifest holds the patched hash.
 */
export const PINS = {
  appimage: [
    {
      name: 'AppRun',
      version: 'apprun-old',
      url: 'https://github.com/tauri-apps/binary-releases/releases/download/apprun-old/AppRun-x86_64',
      sha256: 'f30140a43a0a59e46db21bdefdf749b9e9f2c6946e92afabbacf98b8ae73fb4f',
      file: 'AppRun-x86_64',
      dest: 'AppRun-x86_64',
    },
    {
      name: 'linuxdeploy',
      version: `linuxdeploy-${LINUXDEPLOY}`,
      url: `https://github.com/tauri-apps/binary-releases/releases/download/linuxdeploy-${LINUXDEPLOY}/linuxdeploy-x86_64.AppImage`,
      sha256: '36a2d7e274d12e1050d0e9ecfe11d339ed54720b2bec464c286d53f8b07f5c62',
      file: `linuxdeploy-${LINUXDEPLOY}-x86_64.AppImage`,
      dest: `linuxdeploy-${LINUXDEPLOY}-x86_64.AppImage`,
      patch: 'appimage-magic',
    },
    {
      name: 'linuxdeploy-plugin-appimage',
      version: '1-alpha-20250213-1',
      // linuxdeploy.rs:249-255 downloads the `continuous` build under this name; the pin is the dated build (user, 2026-11-12).
      url: 'https://github.com/linuxdeploy/linuxdeploy-plugin-appimage/releases/download/1-alpha-20250213-1/linuxdeploy-plugin-appimage-x86_64.AppImage',
      sha256: '992d502a248e14ab185448ddf6f6e7d25558cb84d4623c354c3af350c25fccb3',
      file: 'linuxdeploy-plugin-appimage-1-alpha-20250213-1-x86_64.AppImage',
      dest: 'linuxdeploy-plugin-appimage.AppImage',
    },
  ],
  nsis: [
    {
      name: 'NSIS',
      version: 'nsis-3.13',
      // nsis/mod.rs:38-39 (NSIS_URL); Tauri's own check is SHA-1 DB14F8AF2EA346B786C6AE19343F61AAF5E09B8E (:41).
      url: 'https://github.com/tauri-apps/binary-releases/releases/download/nsis-3.13/nsis-3.13.zip',
      sha256: 'ba63dffc4410ee89193e1cb5a41989991bd77c61068da17e3156d136b7b0b3d8',
      file: 'nsis-3.13.zip',
      unzip: 'nsis-3.13/',
      dest: 'NSIS',
    },
    {
      name: 'nsis_tauri_utils',
      version: 'nsis_tauri_utils-v0.5.3',
      // nsis/mod.rs:43 (NSIS_TAURI_UTILS_URL), written to Plugins/x86-unicode/additional (:149-159).
      url: 'https://github.com/tauri-apps/nsis-tauri-utils/releases/download/nsis_tauri_utils-v0.5.3/nsis_tauri_utils.dll',
      sha256: '5ba143b5db4a87d32d6e7802e033330aae56cbceabe0d1e3ba41948385ad4709',
      file: 'nsis_tauri_utils.dll',
      dest: 'NSIS/Plugins/x86-unicode/additional/nsis_tauri_utils.dll',
    },
  ],
};

/** Files the bundler writes from its own binary (include_bytes!, linuxdeploy.rs:229-246): their SHA-256 in tauri-bundler 2.10.3. */
export const EMBEDDED = {
  appimage: [
    { dest: 'linuxdeploy-plugin-gtk.sh', sha256: 'ef6b9a980417243bc62e0241b51dc49876032afd1bab9b4762389f961b406d9b' },
    { dest: 'linuxdeploy-plugin-gstreamer.sh', sha256: '2a15ce9da8de6e20159e1ab27861a7a5ef8758c81a6278ba4ab30cefa1d74c9f' },
  ],
  nsis: [],
};

/** Tauri's own SHA-1 checks of the files it keeps (nsis/mod.rs:42, :44): a seed that fails them is thrown away and downloaded. */
export const TAURI_SHA1 = {
  appimage: {},
  nsis: {
    'NSIS/NSIS.exe': '6d45d4e8ccf18fe1fdac9164058f356102a2ecf2',
    'NSIS/Plugins/x86-unicode/additional/nsis_tauri_utils.dll': '75197fee3c6a814fe035788d1c34ead39349b860',
  },
};

/** The kinds a bundle list needs on this host (none on a host whose tools are not pinned, e.g. macOS). */
export function kindsFor(bundles, platform = process.platform, arch = process.arch) {
  const list = String(bundles ?? '').split(',').map((b) => b.trim());
  return Object.entries(KINDS).filter(([, k]) => list.includes(k.bundle) && k.platform === platform && k.arch === arch).map(([n]) => n);
}

/** The actions/cache key: each kind and each tool's version and the start of its SHA-256, so any pin change is a new key. */
export function cacheKey(kinds, pins = PINS) {
  return ['tauri-tools', ...kinds.flatMap((k) => [k, ...pins[k].map((p) => `${p.version}-${p.sha256.slice(0, 12)}`)])].join('-');
}

/**
 * The kept cache folder of verified downloads (branch and dispatch runs): PADDOCK_TAURI_TOOLS_CACHE, else LOCALAPPDATA on
 * Windows (a host runner, where Git bash's HOME is not a Windows path), else beside the XDG cache.
 */
export function defaultCache(env = process.env, platform = process.platform) {
  if (env.PADDOCK_TAURI_TOOLS_CACHE) return env.PADDOCK_TAURI_TOOLS_CACHE;
  if (platform === 'win32') return join(env.LOCALAPPDATA || homedir(), 'paddock-tauri-tools');
  const base = env.XDG_CACHE_HOME && isAbsolute(env.XDG_CACHE_HOME) ? env.XDG_CACHE_HOME : join(env.HOME || env.USERPROFILE || homedir(), '.cache');
  return join(base, 'paddock-tauri-tools');
}

/** Whether this is a tag run (nothing kept is read there: ADR 0085 §4 item 1 and D9). */
const onTag = (env) => (env.GITHUB_REF ?? '').startsWith('refs/tags/');

/** `<cargo target_directory>/.tauri` for the Cargo project in `srcTauri`, asked the way tauri-cli asks (cwd = src-tauri). */
export function toolsDir(srcTauri, run = spawnSync) {
  const r = run('cargo', ['metadata', '--format-version', '1', '--no-deps'], { cwd: srcTauri, encoding: 'utf8', maxBuffer: 64 * 1024 * 1024, timeout: 120_000 });
  if (r.error || r.status !== 0) {
    throw Object.assign(new Error(`cargo metadata in ${srcTauri} failed`), {
      annotation: `::error::\`cargo metadata --no-deps\` in ${srcTauri} failed (exit ${r.status ?? r.error?.code}): ${String(r.stderr ?? '').trim().split('\n').at(-1) ?? ''}. Next step: run it there; the Tauri tools folder is <its target_directory>/.tauri.`,
    });
  }
  const target = JSON.parse(r.stdout).target_directory;
  if (!target) throw Object.assign(new Error('no target_directory'), { annotation: `::error::cargo metadata in ${srcTauri} printed no target_directory. Next step: check the cargo version.` });
  return join(target, '.tauri');
}

const sha256 = (bytes) => createHash('sha256').update(bytes).digest('hex');
const sha1 = (bytes) => createHash('sha1').update(bytes).digest('hex');
const wait = (ms) => new Promise((r) => setTimeout(r, ms));

/** The bytes Tauri leaves after its `dd if=/dev/zero bs=1 count=3 seek=8 conv=notrunc` (linuxdeploy.rs:267-276). */
export function patchAppImageMagic(bytes) {
  const out = Buffer.from(bytes);
  out.fill(0, 8, Math.min(11, out.length));
  return out;
}

/** A zip entry name that stays inside its folder: relative, forward slashes, no `..`, no drive. */
function safeName(name) {
  return !(name.startsWith('/') || name.includes('\\') || /^[A-Za-z]:/.test(name) || name.split('/').includes('..'));
}

/**
 * The files of a zip archive (stored or deflated, no zip64, no encryption): [{ name, data }], directories left out. Each file's
 * size and CRC-32 are checked; an unsafe name is refused. The archive itself is checked against its pin before this runs.
 */
export function unzip(buf) {
  let eocd = -1;
  for (let i = buf.length - 22; i >= Math.max(0, buf.length - 22 - 0xffff); i--) {
    if (buf.readUInt32LE(i) === 0x06054b50) {
      eocd = i;
      break;
    }
  }
  if (eocd < 0) throw new Error('not a zip archive (no end of central directory)');
  const count = buf.readUInt16LE(eocd + 10);
  const cd = buf.readUInt32LE(eocd + 16);
  if (count === 0xffff || cd === 0xffffffff) throw new Error('zip64 archives are not supported');
  const out = [];
  let p = cd;
  for (let n = 0; n < count; n++) {
    if (buf.readUInt32LE(p) !== 0x02014b50) throw new Error(`zip: central directory entry ${n} is damaged`);
    const flags = buf.readUInt16LE(p + 8);
    const method = buf.readUInt16LE(p + 10);
    const crc = buf.readUInt32LE(p + 16);
    const csize = buf.readUInt32LE(p + 20);
    const size = buf.readUInt32LE(p + 24);
    const nlen = buf.readUInt16LE(p + 28);
    const local = buf.readUInt32LE(p + 42);
    const name = buf.toString('utf8', p + 46, p + 46 + nlen);
    p += 46 + nlen + buf.readUInt16LE(p + 30) + buf.readUInt16LE(p + 32);
    if (!safeName(name)) throw new Error(`zip: entry ${JSON.stringify(name)} leaves its folder`);
    if (name.endsWith('/')) continue;
    if (flags & 1) throw new Error(`zip: ${name} is encrypted`);
    if (buf.readUInt32LE(local) !== 0x04034b50) throw new Error(`zip: the local header of ${name} is damaged`);
    const start = local + 30 + buf.readUInt16LE(local + 26) + buf.readUInt16LE(local + 28);
    const raw = buf.subarray(start, start + csize);
    let data;
    if (method === 0) data = Buffer.from(raw);
    else if (method === 8) data = inflateRawSync(raw);
    else throw new Error(`zip: ${name} uses compression method ${method}`);
    if (data.length !== size || crc32(data) !== crc) throw new Error(`zip: ${name} does not match its size or CRC-32`);
    out.push({ name, data });
  }
  return out;
}

/** Whether a failed download is worth another try: no answer, a 5xx, 408 or 429; never another 4xx. */
export const retryable = (err) => err.status === null || err.status >= 500 || err.status === 408 || err.status === 429;

/** "answered HTTP 504" or "got no answer (ECONNREFUSED)". */
const outcome = (err) => (err.status === null ? `got no answer (${err.reason})` : `answered HTTP ${err.status}`);

/** A failed download: the URL, the HTTP status (null when no answer came) and the cause. */
export class DownloadError extends Error {
  constructor(url, status, cause) {
    super(`${url} ${status === null ? `got no answer (${cause})` : `answered HTTP ${status}`}`);
    this.url = url;
    this.status = status;
    this.reason = cause;
  }
}

/** GETs `url` into a Buffer; throws DownloadError on an HTTP error or no answer. */
export async function fetchBytes(url, { timeoutMs = 5 * 60 * 1000 } = {}) {
  let res;
  try {
    res = await fetch(url, { signal: AbortSignal.timeout(timeoutMs), redirect: 'follow' });
  } catch (e) {
    throw new DownloadError(url, null, e.cause?.code ?? e.cause?.message ?? e.message);
  }
  if (!res.ok) throw new DownloadError(url, res.status);
  return Buffer.from(await res.arrayBuffer());
}

/** The one-line `::error::` for a failed download (ER-27): the tool, the URL, the status and the next step. */
export function downloadAnnotation(kind, pin, err, key) {
  return `::error::Downloading ${pin.name} (${pin.version}) for the ${kind} bundle failed: ${pin.url} ${outcome(err)}. ` +
    `Next step: re-run the job when github.com answers again; a branch run that restores the actions/cache key ${key} needs no download.`;
}

/** The pinned bytes of `pin`: the cached copy when its SHA-256 matches, else a download (retried, verified, then cached). */
async function pinnedBytes({ kind, pin, cache, key, get, attempts, delayMs, sleep, write, log }) {
  const kept = join(cache, pin.file);
  if (existsSync(kept)) {
    const bytes = readFileSync(kept);
    if (sha256(bytes) === pin.sha256) return { bytes, source: 'cache' };
    log(`::warning::The cached ${pin.name} (${kept}) does not match its pinned SHA-256: downloading it again.`);
    rmSync(kept, { force: true });
  }
  let bytes;
  for (let i = 1; ; i++) {
    try {
      bytes = await get(pin.url);
      break;
    } catch (e) {
      const err = e instanceof DownloadError ? e : new DownloadError(pin.url, null, e.message);
      if (i >= attempts || !retryable(err)) throw Object.assign(new Error(err.message), { annotation: downloadAnnotation(kind, pin, err, key) });
      log(`tauri-tools: ${err.message}; try ${i + 1} of ${attempts} in ${(delayMs * 2 ** (i - 1)) / 1000} s`);
      await sleep(delayMs * 2 ** (i - 1));
    }
  }
  const got = sha256(bytes);
  if (got !== pin.sha256) {
    throw Object.assign(new Error(`checksum mismatch for ${pin.url}`), {
      annotation: `::error::${pin.name} from ${pin.url} has SHA-256 ${got}, but ${pin.sha256} is pinned: the file was not used. ` +
        'Next step: a changed release asset is a supply-chain question, not a retry; compare it with the release page and ' +
        'move the pin in conformance/ci/tauri-tools.mjs only after checking why it changed.',
    });
  }
  const part = `${kept}.part`;
  try {
    write(part, bytes);
    renameSync(part, kept);
  } catch (e) {
    rmSync(part, { force: true });
    throw Object.assign(e, { annotation: `::error::Writing ${pin.name} to the cache ${cache} failed: ${e.message}. Next step: check the runner's free disk and the folder's rights.` });
  }
  return { bytes, source: 'download' };
}

/** Writes `bytes` at `tools/rel`, executable. */
function place(tools, rel, bytes) {
  const dest = join(tools, ...rel.split('/'));
  mkdirSync(dirname(dest), { recursive: true });
  writeFileSync(dest, bytes);
  chmodSync(dest, 0o755);
}

/** The manifest next to the tools folder (outside it, so it is not one of the files it lists). */
export const manifestPath = (tools) => `${tools}.sha256`;

/**
 * Empties `tools` and fills it with the pinned files of `kinds`, then writes the manifest. Downloads try `attempts` times,
 * `delayMs` apart and doubling, only for a retryable failure. Returns [{ kind, name, source: 'cache' | 'download' }]. Throws
 * with `annotation` set on any failure; no `.part` file is left behind.
 */
export async function install({
  kinds, tools, cache, pins = PINS, embedded = EMBEDDED, sha1s = TAURI_SHA1, fetch: get = fetchBytes, attempts = 3, delayMs = 2000, sleep = wait, write = writeFileSync, log = console.log,
}) {
  if (basename(tools) !== '.tauri') {
    throw Object.assign(new Error(`not a Tauri tools folder: ${tools}`), { annotation: `::error::The Tauri tools folder must be <build folder>/.tauri, not ${tools}: install empties it. Next step: pass --src-tauri (cargo metadata names it).` });
  }
  rmSync(tools, { recursive: true, force: true });
  rmSync(manifestPath(tools), { force: true });
  mkdirSync(tools, { recursive: true });
  mkdirSync(cache, { recursive: true });
  const manifest = new Map();
  const done = [];
  for (const kind of kinds) {
    const key = cacheKey([kind], pins);
    for (const pin of pins[kind]) {
      const { bytes, source } = await pinnedBytes({ kind, pin, cache, key, get, attempts, delayMs, sleep, write, log });
      if (pin.unzip) {
        let n = 0;
        for (const e of unzip(bytes)) {
          if (!e.name.startsWith(pin.unzip)) throw Object.assign(new Error(`${pin.file}: ${e.name} is outside ${pin.unzip}`), { annotation: `::error::${pin.file} holds ${e.name}, outside its folder ${pin.unzip}. Next step: check the pin.` });
          const rel = `${pin.dest}/${e.name.slice(pin.unzip.length)}`;
          place(tools, rel, e.data);
          manifest.set(rel, sha256(e.data));
          n++;
        }
        log(`tauri-tools: ${pin.name} ${pin.version} (sha256 ${pin.sha256.slice(0, 12)}) from the ${source}: ${n} files -> ${join(tools, pin.dest)}`);
      } else {
        place(tools, pin.dest, bytes);
        manifest.set(pin.dest, sha256(pin.patch === 'appimage-magic' ? patchAppImageMagic(bytes) : bytes));
        log(`tauri-tools: ${pin.name} ${pin.version} (sha256 ${pin.sha256.slice(0, 12)}) from the ${source} -> ${join(tools, pin.dest)}`);
      }
      done.push({ kind, name: pin.name, source });
    }
    for (const e of embedded[kind] ?? []) manifest.set(e.dest, e.sha256);
    for (const [rel, want] of Object.entries(sha1s[kind] ?? {})) {
      const f = join(tools, ...rel.split('/'));
      const got = existsSync(f) ? sha1(readFileSync(f)) : 'missing';
      if (got !== want) {
        throw Object.assign(new Error(`${rel}: SHA-1 ${got}`), {
          annotation: `::error::${rel} has SHA-1 ${got}, but tauri-bundler expects ${want}: Tauri would delete the pinned folder and download it again, unpinned. Next step: the pins no longer match the bundler of @tauri-apps/cli; read its nsis/mod.rs and move them.`,
        });
      }
    }
  }
  writeFileSync(manifestPath(tools), [...manifest].sort(([a], [b]) => (a < b ? -1 : 1)).map(([rel, h]) => `${h}  ${rel}\n`).join(''));
  return done;
}

/** Every regular file under `dir` as a path relative to it (forward slashes), and every link (never followed). */
function walk(dir, base = dir, files = [], links = []) {
  for (const e of readdirSync(dir, { withFileTypes: true })) {
    const p = join(dir, e.name);
    const rel = p.slice(base.length + 1).split('\\').join('/');
    if (e.isSymbolicLink() || lstatSync(p).isSymbolicLink()) links.push(rel);
    else if (e.isDirectory()) walk(p, base, files, links);
    else files.push(rel);
  }
  return { files, links };
}

/**
 * Compares the tools folder after the build with its manifest: { matched, findings }. A file not in the manifest, a changed
 * hash or a link is a finding.
 */
export function check({ tools }) {
  const file = manifestPath(tools);
  if (!existsSync(file)) return { matched: 0, findings: [`check broken: no manifest ${file} (install did not run for this folder)`] };
  const want = new Map(readFileSync(file, 'utf8').split('\n').filter(Boolean).map((l) => {
    const [h, ...rest] = l.split('  ');
    return [rest.join('  '), h];
  }));
  if (!existsSync(tools)) return { matched: 0, findings: [`check broken: the tools folder ${tools} is gone`] };
  const { files, links } = walk(tools);
  const findings = links.map((l) => `${l} is a link (the tools must be files in the build folder)`);
  let matched = 0;
  for (const rel of files.sort()) {
    const h = want.get(rel);
    if (h === undefined) findings.push(`${rel} is not pinned (Tauri downloaded or wrote it itself)`);
    else if (sha256(readFileSync(join(tools, ...rel.split('/')))) !== h) findings.push(`${rel} does not have its pinned SHA-256`);
    else matched++;
  }
  return { matched, findings };
}

const OPTIONS = ['--bundles', '--src-tauri', '--tools', '--cache'];

function option(argv, name) {
  const i = argv.indexOf(name);
  if (i < 0) return undefined;
  if (argv[i + 1] === undefined) throw new Error(`${name} needs a value`);
  return argv[i + 1];
}

/**
 * The command line; returns the exit code. `PADDOCK_TAURI_PINS` names a JSON file { pins, embedded?, sha1s?, host?,
 * retryDelayMs? } that replaces the pins (the tests only: refused under GITHUB_ACTIONS=true, exit 2). `deps.host` =
 * { platform, arch } replaces the host (tests). On a tag run the kept cache is not read: a fresh folder is used (ADR 0085 D9).
 */
export async function main(argv, env = process.env, deps = {}) {
  const [cmd, ...rest] = argv;
  for (let i = 0; i < rest.length; i += 2) {
    if (!OPTIONS.includes(rest[i])) throw new Error(`unknown option ${rest[i]}`);
  }
  const log = deps.log ?? console.log;
  if (env.PADDOCK_TAURI_PINS && env.GITHUB_ACTIONS === 'true') {
    log('::error::PADDOCK_TAURI_PINS is set in a CI run: it replaces the pinned Tauri tools and is for the tests only. Next step: remove it from the workflow or the runner environment.');
    return 2;
  }
  const o = env.PADDOCK_TAURI_PINS ? JSON.parse(readFileSync(env.PADDOCK_TAURI_PINS, 'utf8')) : {};
  const pins = o.pins ?? PINS;
  const host = deps.host ?? (o.host ? o.host : { platform: process.platform, arch: process.arch });
  const bundles = option(rest, '--bundles');
  if (bundles === undefined) throw new Error('--bundles is required');
  const kinds = kindsFor(bundles, host.platform, host.arch).filter((k) => pins[k]);
  if (cmd === 'key') {
    log(kinds.length ? cacheKey(kinds, pins) : '');
    return 0;
  }
  if (cmd !== 'install' && cmd !== 'check') throw new Error('usage: tauri-tools.mjs key|install|check --bundles <list> [--src-tauri <dir>] [--tools <dir>] [--cache <dir>]');
  if (!kinds.length) {
    if (cmd === 'install') log(`tauri-tools: no pinned bundler tools for ${bundles} on ${host.platform}/${host.arch}: nothing to put in place.`);
    return 0;
  }
  const tag = onTag(env);
  try {
    const srcTauri = option(rest, '--src-tauri');
    const tools = option(rest, '--tools') ?? (srcTauri ? toolsDir(srcTauri, deps.run) : undefined);
    if (!tools) throw new Error('--src-tauri or --tools is required');
    if (cmd === 'install') {
      let cache = option(rest, '--cache') ?? defaultCache(env);
      let fresh = null;
      if (tag) {
        fresh = mkdtempSync(join(env.RUNNER_TEMP || tmpdir(), 'paddock-tauri-tools-'));
        cache = fresh;
        log('tauri-tools: a tag run reads no kept cache: every tool is downloaded and verified now (ADR 0085 D9).');
      }
      try {
        await install({
          kinds, tools, cache, pins, log,
          ...(o.embedded ? { embedded: o.embedded } : {}),
          ...(o.sha1s ? { sha1s: o.sha1s } : {}),
          ...(deps.fetch ? { fetch: deps.fetch } : {}),
          ...(o.retryDelayMs !== undefined ? { delayMs: o.retryDelayMs } : {}),
          ...(deps.delayMs !== undefined ? { delayMs: deps.delayMs } : {}),
          ...(deps.sleep ? { sleep: deps.sleep } : {}),
          ...(deps.write ? { write: deps.write } : {}),
        });
      } finally {
        if (fresh) rmSync(fresh, { recursive: true, force: true });
      }
      log(`tauri-tools: manifest ${manifestPath(tools)}`);
      return 0;
    }
    const r = check({ tools });
    for (const f of r.findings) {
      log(`::${tag ? 'error' : 'warning'}::Tauri tools: ${f}. Next step: pin it in conformance/ci/tauri-tools.mjs (URL and SHA-256 from tauri-bundler, see the header), or find what wrote it.`);
    }
    log(`tauri-tools: ${r.matched} files in ${tools} match their pins; ${r.findings.length} finding(s).`);
    return tag && r.findings.length ? 1 : 0;
  } catch (e) {
    log(e.annotation ?? `::error::Preparing the Tauri bundler tools failed: ${e.message}. Next step: see conformance/ci/tauri-tools.mjs.`);
    return 1;
  }
}

if (isMain(import.meta.url)) {
  main(process.argv.slice(2)).then(
    (code) => process.exit(code),
    (e) => {
      console.log(`::error::tauri-tools: ${e.message}`);
      process.exit(2);
    },
  );
}
