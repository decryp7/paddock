#!/usr/bin/env node
// Downloads the pinned Microsoft Visual C++ Redistributable and stages it next to the installer hooks, where
// installer-hooks.nsh embeds it (`File`). Run before the Windows bundle (pitwall-release.yml `windows-pre-bundle`). The pin (url,
// SHA-256, size) is app/src-tauri/windows/vc-redist.lock.json; the download is checked against it and a file already staged with the
// right hash is kept. Fails closed: a wrong hash or size, a failed download or a missing lock stops the build with one ::error::
// that names the cause and the next step, so no installer is built without the runtime its hooks embed.
//
//   node apps/pitwall/scripts/ci/stage-vc-redist.mjs [--root <apps/pitwall>]
import { createHash } from 'node:crypto';
import { existsSync, mkdirSync, readFileSync, renameSync, writeFileSync } from 'node:fs';
import { dirname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
import { isMain } from '../../../../conformance/src/util/is-main.mjs';

const APP = resolve(dirname(fileURLToPath(import.meta.url)), '..', '..');
const HOST = 'https://download.visualstudio.microsoft.com/download/pr/';

const sha256 = (bytes) => createHash('sha256').update(bytes).digest('hex');

/** The pin for `arch`, validated: only Microsoft's download host, a lowercase 64-digit hash that the url's path also carries. */
export function readPin(lock, arch = 'x64') {
  const p = lock?.[arch];
  if (!p || typeof p !== 'object') throw new Error(`vc-redist.lock.json has no "${arch}" entry`);
  if (typeof p.name !== 'string' || !/^vc_redist\.[a-z0-9]+\.exe$/.test(p.name)) throw new Error(`the pinned file name ${JSON.stringify(p.name)} is not vc_redist.<arch>.exe`);
  if (typeof p.url !== 'string' || !p.url.startsWith(HOST)) throw new Error(`the pinned url must start with ${HOST}`);
  if (typeof p.sha256 !== 'string' || !/^[0-9a-f]{64}$/.test(p.sha256)) throw new Error('the pinned sha256 must be 64 lowercase hex digits');
  if (!p.url.toLowerCase().includes(`/${p.sha256}/`)) throw new Error('the pinned url does not carry the pinned sha256 in its path (Microsoft puts it there): the pin was edited by hand');
  if (!Number.isInteger(p.size) || p.size <= 0) throw new Error('the pinned size must be a positive integer');
  return p;
}

/** Throws unless `bytes` are the pinned file. */
export function verify(bytes, pin) {
  if (bytes.length !== pin.size) throw new Error(`${pin.name} is ${bytes.length} bytes, the pin says ${pin.size}`);
  const got = sha256(bytes);
  if (got !== pin.sha256) throw new Error(`${pin.name} has sha256 ${got}, the pin says ${pin.sha256}`);
}

const download = async (url) => {
  const res = await fetch(url);
  if (!res.ok) throw new Error(`GET ${url} answered ${res.status}`);
  return new Uint8Array(await res.arrayBuffer());
};

/** Stages the runtime; returns `{ path, downloaded }`. `fetchBytes` is injectable for the tests. */
export async function stage({ app = APP, fetchBytes = download } = {}) {
  const dir = join(app, 'app', 'src-tauri', 'windows');
  const lockFile = join(dir, 'vc-redist.lock.json');
  if (!existsSync(lockFile)) throw new Error(`${lockFile} is missing`);
  const pin = readPin(JSON.parse(readFileSync(lockFile, 'utf8')));
  const dest = join(dir, pin.name);
  if (existsSync(dest)) {
    try {
      verify(readFileSync(dest), pin);
      console.log(`${pin.name} is already staged (sha256 ${pin.sha256})`);
      return { path: dest, downloaded: false };
    } catch (e) {
      console.log(`the staged ${pin.name} does not match the pin (${e.message}); downloading it again`);
    }
  }
  const t0 = Date.now();
  const bytes = await fetchBytes(pin.url);
  verify(bytes, pin);
  mkdirSync(dir, { recursive: true });
  writeFileSync(`${dest}.part`, bytes);
  renameSync(`${dest}.part`, dest);
  console.log(`staged ${pin.name} (${bytes.length} bytes, sha256 ${pin.sha256}) in ${Date.now() - t0} ms`);
  return { path: dest, downloaded: true };
}

if (isMain(import.meta.url)) {
  const i = process.argv.indexOf('--root');
  stage(i > 0 ? { app: resolve(process.argv[i + 1]) } : {}).catch((e) => {
    console.error(`::error::Cannot stage the Visual C++ runtime for the Pitwall installer: ${e.message}. Check vc-redist.lock.json and the runner's access to download.visualstudio.microsoft.com, then run the bundle step again.`);
    process.exit(1);
  });
}
