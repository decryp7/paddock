#!/usr/bin/env node
// Interim reader of build-inputs.toml (ADR 0062, WP-1; replaced by `./dev inputs` in WP-3, registered in
// docs/known-copies.toml as `build-input-fetch`). It resolves a pinned input from the private store, verifies size and
// SHA-256, and seeds what `tauri build` would otherwise download from GitHub: the Linux AppImage helpers and the Windows
// NSIS toolset. It never contacts a publisher; a miss is a named error (ER-27), not a fallback.
//
//   node conformance/ci/build-inputs-reader.mjs seed-tauri     prints `export NAME='value'` lines for `eval` (LDAI_RUNTIME_FILE) and
//                                                       one summary line per input (stderr)
//   node conformance/ci/build-inputs-reader.mjs install-tool <id>   installs a prebuilt tool (git-cliff, ...) and prints `export PATH=...`
//   node conformance/ci/build-inputs-reader.mjs scan <logfile> prints the last `Downloading https://...` URL of a failed build
//
// Environment: BUILD_INPUTS_TOKEN (a read-only token for the private store; a secret), BUILD_INPUTS_STORE (default below).
import { spawnSync } from 'node:child_process';
import { createHash } from 'node:crypto';
import { appendFileSync, chmodSync, copyFileSync, existsSync, mkdirSync, mkdtempSync, readdirSync, readFileSync, renameSync, rmSync, statSync, writeFileSync } from 'node:fs';
import { homedir, tmpdir } from 'node:os';
import { basename, join } from 'node:path';
import { fileURLToPath } from 'node:url';
import { isMain } from '../src/util/is-main.mjs';

export const DEFAULT_STORE = 'https://dev.decryptology.net/api/packages/paddock-build-inputs/generic';

const str = (line, name) => line.match(new RegExp(`^\\s*${name}\\s*=\\s*"([^"]*)"\\s*(#.*)?$`))?.[1];
const num = (line, name) => line.match(new RegExp(`^\\s*${name}\\s*=\\s*(\\d+)\\s*(#.*)?$`))?.[1];
const bool = (line, name) => line.match(new RegExp(`^\\s*${name}\\s*=\\s*(true|false)\\s*(#.*)?$`))?.[1];

/**
 * Reads the subset of build-inputs.toml this interim reader needs, line by line (like runtime-catalogue.mjs): `[[input]]`
 * with string and boolean keys and `[[input.platform]]` with key, url, size, sha256. Returns
 * `[{ id, version, package, file, tied_to, ..., platforms: [{ key, url, size, sha256 }] }]`.
 */
export function readInventory(text) {
  const entries = [];
  let entry = null;
  let platform = null;
  for (const line of text.split('\n')) {
    if (/^\s*\[\[input\]\]\s*$/.test(line)) {
      entry = { platforms: [] };
      platform = null;
      entries.push(entry);
    } else if (/^\s*\[\[input\.platform\]\]\s*$/.test(line)) {
      platform = {};
      entry.platforms.push(platform);
    } else if (platform) {
      const k = str(line, 'key') ?? undefined;
      if (k !== undefined) platform.key = k;
      for (const n of ['url', 'sha256', 'checked']) {
        const v = str(line, n);
        if (v !== undefined) platform[n] = v;
      }
      const s = num(line, 'size');
      if (s !== undefined) platform.size = Number(s);
    } else if (entry) {
      for (const n of ['id', 'kind', 'version', 'source', 'licence', 'package', 'file', 'bin', 'tied_to', 'used_by']) {
        const v = str(line, n);
        if (v !== undefined) entry[n] = v;
      }
      for (const n of ['ships', 'redistribute', 'release_path']) {
        const v = bool(line, n);
        if (v !== undefined) entry[n] = v === 'true';
      }
    }
  }
  return entries;
}

/** The address of an input in the store (Gitea generic package layout). */
export const storeUrl = (store, e) => `${store}/${e.package}/${e.version}/${e.file}`;

const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
const fail = (e, message) => new Error(`::error title=build input ${e.id}::${message}`);

/**
 * Gets input `e` for `platformKey` from the private store into `dest`, checking size and SHA-256. Two retries with backoff
 * on a 5xx or a network error; a 4xx is final. Throws an Error whose message is the one `::error::` annotation.
 * Returns `{ tier, bytes, ms }`.
 */
export async function ensureInput(e, platformKey, dest, { store = DEFAULT_STORE, token, backoffMs = 1000, timeoutMs = 120000 } = {}) {
  const p = e.platforms.find((x) => x.key === platformKey);
  if (!p) throw fail(e, `${e.id} ${e.version} has no pin for ${platformKey}. Next: add a [[input.platform]] to build-inputs.toml.`);
  const url = storeUrl(store, e);
  const mirror = `run \`./dev inputs mirror ${e.id}\` (until it exists: download ${p.url} once, check it against build-inputs.toml and upload it to ${new URL(url).pathname}) on a workstation, then re-run this job`;
  if (!token) {
    throw fail(e, `could not get ${e.id} ${e.version} (${e.file}): BUILD_INPUTS_TOKEN is not set, so the private store cannot be read. Next: add the read-only token as the repository Actions secret BUILD_INPUTS_TOKEN and pass it to this step.`);
  }
  const started = Date.now();
  let last = '';
  for (let attempt = 0; attempt <= 2; attempt++) {
    if (attempt > 0) await sleep(backoffMs * 2 ** (attempt - 1));
    let res;
    try {
      res = await fetch(url, { headers: { authorization: `token ${token}` }, signal: AbortSignal.timeout(timeoutMs) });
    } catch (err) {
      const cause = err?.cause?.code ?? err?.cause?.message ?? err?.name ?? String(err);
      last = err?.name === 'TimeoutError' ? `no answer within ${timeoutMs} ms from ${new URL(url).host}` : `could not connect to ${new URL(url).host} (${cause})`;
      continue;
    }
    if (res.status === 401 || res.status === 403) {
      throw fail(e, `could not get ${e.id} ${e.version} (${e.file}): the private store answered HTTP ${res.status} for ${e.package}/${e.version}; the token in BUILD_INPUTS_TOKEN is refused or lacks read:package. Next: replace the secret with a read-only token of an account that reads the paddock-build-inputs organisation.`);
    }
    if (res.status === 404) {
      throw fail(e, `could not get ${e.id} ${e.version} (${e.file}): the private store answered HTTP 404 for ${e.package}/${e.version}. Next: ${mirror}.`);
    }
    if (!res.ok) {
      last = `the private store answered HTTP ${res.status} for ${e.package}/${e.version}`;
      if (res.status >= 500) continue;
      throw fail(e, `could not get ${e.id} ${e.version} (${e.file}): ${last}. Next: check the store at ${new URL(url).host}.`);
    }
    const bytes = Buffer.from(await res.arrayBuffer());
    if (bytes.length !== p.size) {
      throw fail(e, `${e.id} ${e.version} (${e.file}) size mismatch: expected ${p.size} bytes, got ${bytes.length}. Next: the stored file is not the pinned one; re-upload it from ${p.url} after checking it against build-inputs.toml.`);
    }
    const got = createHash('sha256').update(bytes).digest('hex');
    if (got !== p.sha256) {
      throw fail(e, `${e.id} ${e.version} (${e.file}) SHA-256 mismatch: expected ${p.sha256}, got ${got}. Next: the stored file is not the pinned one; re-upload it from ${p.url} after checking it against build-inputs.toml.`);
    }
    mkdirSync(join(dest, '..'), { recursive: true });
    const part = `${dest}.part-${process.pid}`;
    writeFileSync(part, bytes);
    renameSync(part, dest);
    return { tier: 'private-store', bytes: bytes.length, ms: Date.now() - started };
  }
  throw fail(e, `could not get ${e.id} ${e.version} (${e.file}): ${last}, after 3 tries. Next: check that ${new URL(url).host} is reachable from this runner (the store is not a publisher; the dead-proxy run exempts it), then re-run this job.`);
}

/** The folder Tauri's bundler keeps its tools in: `dirs::cache_dir()/tauri` (tauri-bundler 2.10.3, ADR 0062 Context). */
export function tauriToolsDir({ platform = process.platform, env = process.env } = {}) {
  if (platform === 'win32') return join(env.LOCALAPPDATA ?? join(homedir(), 'AppData', 'Local'), 'tauri');
  if (platform === 'darwin') return join(env.HOME ?? homedir(), 'Library', 'Caches', 'tauri');
  return join(env.XDG_CACHE_HOME || join(env.HOME ?? homedir(), '.cache'), 'tauri');
}

/** Unpacks a zip with the system bsdtar (Windows 10+ and macOS read zips; Git Bash's GNU tar does not). */
function extractZip(zipPath, into, e, env) {
  const tar = process.platform === 'win32' ? join(env.SystemRoot ?? 'C:\\Windows', 'System32', 'tar.exe') : 'tar';
  const x = spawnSync(tar, ['-xf', zipPath, '-C', into], { encoding: 'utf8' });
  if (x.status !== 0) throw fail(e, `could not extract ${e.file}: ${tar} exited ${x.status}: ${(x.stderr ?? '').slice(-300)}. Next: check that tar.exe is in System32.`);
}

const byId = (inventory, id) => {
  const e = inventory.find((x) => x.id === id);
  if (!e) throw new Error(`::error title=build input ${id}::${id} is not in build-inputs.toml. Next: add the entry.`);
  return e;
};
const line = (e, r) => `${e.id} ${e.version} ${r.tier} ${r.bytes} bytes ${r.ms} ms`;

/**
 * Seeds what `tauri build` would download. Linux: AppRun, linuxdeploy and the AppImage plugin as fresh copies in Tauri's
 * tools folder (Tauri patches linuxdeploy in place, so a reused copy no longer matches its hash), and the type-2 runtime
 * beside them for `LDAI_RUNTIME_FILE`. Windows: the NSIS toolset in `<tools>/NSIS`. macOS: nothing.
 * Returns `{ env, summary }`.
 */
export async function seedTauri({ platform = process.platform, env = process.env, inventory, store, token, backoffMs } = {}) {
  const opts = { store, token, backoffMs };
  const dir = tauriToolsDir({ platform, env });
  const out = { env: {}, summary: [] };
  if (platform === 'linux') {
    mkdirSync(dir, { recursive: true });
    const plan = [
      ['apprun', 'AppRun-x86_64'],
      ['linuxdeploy', 'linuxdeploy-07333c6-x86_64.AppImage'],
      ['linuxdeploy-plugin-appimage', 'linuxdeploy-plugin-appimage.AppImage'],
      ['appimage-type2-runtime', join('inputs', 'runtime-x86_64')],
    ];
    for (const [id, name] of plan) {
      const e = byId(inventory, id);
      const dest = join(dir, name);
      rmSync(dest, { force: true });
      const r = await ensureInput(e, 'linux-x86_64', dest, opts);
      chmodSync(dest, 0o755);
      out.summary.push(line(e, r));
      if (id === 'appimage-type2-runtime') out.env.LDAI_RUNTIME_FILE = dest;
    }
  } else if (platform === 'win32') {
    const nsisDir = join(dir, 'NSIS');
    const zip = byId(inventory, 'nsis');
    const utils = byId(inventory, 'nsis-tauri-utils');
    const zipPin = zip.platforms.find((x) => x.key === 'windows-x86_64')?.sha256;
    const dllPin = utils.platforms.find((x) => x.key === 'windows-x86_64')?.sha256;
    const marker = join(nsisDir, '.paddock-build-input');
    const stamp = `nsis ${zip.version} ${zipPin}\nnsis-tauri-utils ${utils.version} ${dllPin}\n`;
    // A folder this reader built from the pinned, verified files carries the marker and is kept (a concurrent job may be running
    // makensis from it). Any other folder (Tauri's own, unverified, filled from GitHub) is replaced by a fresh one: extract into a
    // new folder, add the verified DLL, then swap it in with renames. The old folder is renamed aside and removed, never edited.
    if (existsSync(marker) && readFileSync(marker, 'utf8') === stamp) {
      out.summary.push(`${zip.id} ${zip.version} kept-folder 0 bytes 0 ms`);
      return out;
    }
    const work = join(dir, `NSIS.new-${process.pid}`);
    rmSync(work, { recursive: true, force: true });
    mkdirSync(work, { recursive: true });
    const zipPath = join(work, zip.file);
    const r = await ensureInput(zip, 'windows-x86_64', zipPath, opts);
    out.summary.push(line(zip, r));
    const unpacked = join(work, 'unpacked');
    mkdirSync(unpacked);
    extractZip(zipPath, unpacked, zip, env);
    const tops = readdirSync(unpacked);
    if (tops.length !== 1 || !statSync(join(unpacked, tops[0])).isDirectory()) {
      throw fail(zip, `${zip.file} should hold one top folder, found: ${tops.join(', ') || 'nothing'}. Next: the pinned zip is not the one Tauri expects; re-read the entry.`);
    }
    const fresh = join(unpacked, tops[0]);
    const dll = join(fresh, 'Plugins', 'x86-unicode', 'additional', 'nsis_tauri_utils.dll');
    const d = await ensureInput(utils, 'windows-x86_64', dll, opts);
    out.summary.push(line(utils, d));
    for (const needed of ['NSIS.exe', 'makensis.exe']) {
      if (!existsSync(join(fresh, needed))) throw fail(zip, `the unpacked ${zip.file} has no ${needed}. Next: the pinned zip is not the one Tauri expects.`);
    }
    writeFileSync(join(fresh, '.paddock-build-input'), stamp);
    const aside = join(dir, `NSIS.old-${process.pid}`);
    rmSync(aside, { recursive: true, force: true });
    try {
      if (existsSync(nsisDir)) renameSync(nsisDir, aside);
      renameSync(fresh, nsisDir);
    } catch (err) {
      throw fail(zip, `could not swap the verified NSIS folder into ${nsisDir}: ${err.code ?? err.message}. Next: another job on this runner is probably running makensis from it; re-run when it has finished.`);
    }
    rmSync(aside, { recursive: true, force: true });
    rmSync(work, { recursive: true, force: true });
  }
  return out;
}

/** The first file named `name` under `dir` (an unpacked release archive), or null. */
function findFile(dir, name) {
  for (const f of readdirSync(dir)) {
    const p = join(dir, f);
    if (statSync(p).isDirectory()) {
      const hit = findFile(p, name);
      if (hit) return hit;
    } else if (f === name || f === `${name}.exe`) return p;
  }
  return null;
}

/**
 * Installs a prebuilt tool (`tool` kind: an archive with one executable, `bin`) from the store into `binDir`, verified, and
 * returns the `PATH` entry to add. Replaces `cargo install` / taiki-e/install-action on release paths (ADR 0062 WP-1b).
 */
export async function installTool({ id, inventory, binDir, platformKey = 'linux-x86_64', store, token, backoffMs }) {
  const e = byId(inventory, id);
  const work = mkdtempSync(join(tmpdir(), `build-input-${id}-`));
  const archive = join(work, e.file);
  const r = await ensureInput(e, platformKey, archive, { store, token, backoffMs });
  const x = spawnSync('tar', ['-xf', archive, '-C', work], { encoding: 'utf8' });
  if (x.status !== 0) throw fail(e, `could not unpack ${e.file}: tar exited ${x.status}: ${(x.stderr ?? '').slice(-300)}. Next: check that tar (and xz for .tar.xz) is installed.`);
  const bin = findFile(work, e.bin ?? id);
  if (!bin) throw fail(e, `${e.file} holds no executable named ${e.bin ?? id}. Next: fix \`bin\` in build-inputs.toml.`);
  mkdirSync(binDir, { recursive: true });
  const dest = join(binDir, basename(bin));
  copyFileSync(bin, dest);
  chmodSync(dest, 0o755);
  rmSync(work, { recursive: true, force: true });
  return { dest, summary: line(e, r) };
}

/** The last `Downloading https://...` URL in a build log, or null (ADR 0062 section 5: a failed build names the fetch). */
export function lastDownload(log) {
  const urls = [...log.matchAll(/Downloading(?:\s+\[[^\]]*\])?\s+(https?:\/\/\S+)/g)].map((m) => m[1]);
  return urls.length ? urls[urls.length - 1] : null;
}

async function main(argv) {
  const [cmd, arg] = argv;
  if (cmd === 'seed-tauri') {
    const inventory = readInventory(readFileSync(fileURLToPath(new URL('../../build-inputs.toml', import.meta.url)), 'utf8'));
    const store = process.env.BUILD_INPUTS_STORE || DEFAULT_STORE;
    const { env, summary } = await seedTauri({ inventory, store, token: process.env.BUILD_INPUTS_TOKEN });
    for (const s of summary) console.error(`build input: ${s}`);
    if (process.env.GITHUB_STEP_SUMMARY && summary.length) {
      appendFileSync(process.env.GITHUB_STEP_SUMMARY, `### Build inputs\n\n${summary.map((s) => `- ${s}`).join('\n')}\n`);
    }
    for (const [k, v] of Object.entries(env)) console.log(`export ${k}='${v.replace(/'/g, "'\\''")}'`);
    return 0;
  }
  if (cmd === 'install-tool') {
    const inventory = readInventory(readFileSync(fileURLToPath(new URL('../../build-inputs.toml', import.meta.url)), 'utf8'));
    const store = process.env.BUILD_INPUTS_STORE || DEFAULT_STORE;
    const binDir = process.env.BUILD_INPUTS_BIN || join(process.env.RUNNER_TEMP || tmpdir(), 'build-inputs-bin');
    const { summary } = await installTool({ id: arg, inventory, binDir, store, token: process.env.BUILD_INPUTS_TOKEN });
    console.error(`build input: ${summary}`);
    if (process.env.GITHUB_STEP_SUMMARY) appendFileSync(process.env.GITHUB_STEP_SUMMARY, `- build input: ${summary}\n`);
    if (process.env.GITHUB_PATH) appendFileSync(process.env.GITHUB_PATH, `${binDir}\n`);
    console.log(`export PATH='${binDir.replace(/'/g, "'\\''")}':"$PATH"`);
    return 0;
  }
  if (cmd === 'scan') {
    const url = lastDownload(readFileSync(arg, 'utf8'));
    if (url) console.log(url);
    return url ? 0 : 1;
  }
  console.error('usage: build-inputs-reader.mjs seed-tauri | install-tool <id> | scan <logfile>');
  return 2;
}

if (isMain(import.meta.url)) {
  main(process.argv.slice(2)).then(
    (c) => { process.exitCode = c; },
    (err) => {
      console.error(String(err.message ?? err).startsWith('::error') ? err.message : `::error title=build inputs::${err.message ?? err}`);
      process.exitCode = 1;
    },
  );
}
