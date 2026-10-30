#!/usr/bin/env node
// Installs the pinned Chrome for Testing build into a folder, so tests never launch an installed Chrome/Edge/Chromium
// (rule 7, check C15; plan X10, generalised from Debrief's install-test-browser.mjs). The archive is verified against its
// SHA-256 before it is extracted. The pin is a `paddock-runtime` catalogue (paddock-chromium's chromium.toml, the build the apps
// download), or still the app's own: a JSON file { version, base, sums: { platform: sha256 } } or, as in Debrief, the Rust
// source that holds PINNED_VERSION / BASE / a table of ("platform", "sha256") pairs.
//
//   node install-test-browser.mjs --catalogue chromium.toml|--pin app/chromium-pin.json|--rust-source runtime.rs
//        [--dir <folder>] [--env-prefix DEBRIEF]
//
// Prints `CHROMIUM_TEST_PATH=<executable>`; with $GITHUB_ENV set it appends that line and `<PREFIX>_REQUIRE_TEST_BROWSER=1`
// (a missing browser then fails the tests instead of skipping them).
import { createHash } from 'node:crypto';
import { execFileSync } from 'node:child_process';
import { appendFileSync, existsSync, mkdirSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from 'node:fs';
import { homedir, tmpdir } from 'node:os';
import { join, win32 } from 'node:path';
import { CFT_PLATFORMS } from './pin-runtime.mjs';
import { entryFor, platformKey, readCatalogue } from './runtime-catalogue.mjs';
import { isMain } from '../src/util/is-main.mjs';

/** The pin from a Rust source (`PINNED_VERSION: &str = "..."`, `const BASE: &str = "..."`, `("linux64", "<sha256>")` pairs). */
export function parseRustPin(rs) {
  const version = rs.match(/PINNED_VERSION: &str = "([^"]+)"/)?.[1];
  const base = rs.match(/const BASE: &str = "([^"]+)"/)?.[1];
  const sums = {};
  for (const m of rs.matchAll(/\("([a-z0-9-]+)",\s*"([0-9a-f]{64})"\)/g)) sums[m[1]] = m[2];
  if (!version || !base || !Object.keys(sums).length) throw new Error('could not read the Chromium pin from the Rust source');
  return { version, base, sums };
}

/** Validates a pin object. */
export function checkPin(pin) {
  if (!pin || typeof pin.version !== 'string' || !/^https:\/\//.test(pin.base ?? '') || typeof pin.sums !== 'object') throw new Error('the pin needs version, an https base and sums');
  for (const [k, v] of Object.entries(pin.sums)) if (!/^[0-9a-f]{64}$/.test(v)) throw new Error(`the checksum for ${k} is not a SHA-256`);
  return pin;
}

/** Chrome for Testing platform id of a Node platform/arch, or throws when there is no pinned build. */
export function platformId(platform = process.platform, arch = process.arch) {
  const id = { 'linux/x64': 'linux64', 'darwin/arm64': 'mac-arm64', 'darwin/x64': 'mac-x64', 'win32/x64': 'win64' }[`${platform}/${arch}`];
  if (!id) throw new Error(`no pinned Chromium for ${platform}/${arch}`);
  return id;
}

/** Download URL of a platform's archive. */
export const urlFor = ({ version, base }, id) => `${base}/${version}/${id}/chrome-${id}.zip`;

/** The browser executable inside the extracted folder. */
export function executableIn(dir, id) {
  const root = join(dir, `chrome-${id}`);
  if (id === 'win64') return join(root, 'chrome.exe');
  if (id.startsWith('mac')) return join(root, 'Google Chrome for Testing.app', 'Contents', 'MacOS', 'Google Chrome for Testing');
  return join(root, 'chrome');
}

/**
 * Installs the pinned build into `dir` unless that version is already there, and lets app containers read it either way
 * (`grantCommand`). `fetchBytes(url)`, `extract(zip, dir)` and `grant(dir)` are injectable. Returns the executable path.
 */
export async function install({ pin, dir, id = platformId(), fetchBytes = defaultFetch, extract = defaultExtract, grant = defaultGrant }) {
  const sha = pin.sums[id];
  if (!sha) throw new Error(`no checksum for ${id} in the pin`);
  return installBuild({ version: pin.version, url: urlFor(pin, id), sha, id, dir, fetchBytes, extract, grant });
}

/**
 * Installs the build a `paddock-runtime` catalogue pins for `key` (this machine's by default) into `dir`. Returns the executable.
 */
export async function installFromCatalogue({ text, dir, key = platformKey(), fetchBytes = defaultFetch, extract = defaultExtract, grant = defaultGrant }) {
  const cat = readCatalogue(text);
  const e = entryFor(cat, key);
  const id = CFT_PLATFORMS[key];
  if (!id) throw new Error(`Chrome for Testing has no build for ${key}`);
  if (!/^[0-9a-f]{64}$/.test(e.sha256 ?? '')) throw new Error(`the checksum for ${key} is not a SHA-256`);
  return installBuild({ version: cat.version, url: e.url, sha: e.sha256, id, dir, fetchBytes, extract, grant });
}

async function installBuild({ version, url, sha, id, dir, fetchBytes, extract, grant }) {
  const exe = executableIn(dir, id);
  const stamp = join(dir, '.pinned-version');
  if (existsSync(stamp) && readFileSync(stamp, 'utf8').trim() === version && existsSync(exe)) {
    grant(dir);
    return exe;
  }
  const bytes = await fetchBytes(url);
  const got = createHash('sha256').update(bytes).digest('hex');
  if (got !== sha) throw new Error(`checksum mismatch for ${url}: ${got}`);
  const work = mkdtempSync(join(tmpdir(), 'paddock-cft-'));
  try {
    const zip = join(work, 'cft.zip');
    writeFileSync(zip, bytes);
    rmSync(dir, { recursive: true, force: true });
    mkdirSync(dir, { recursive: true });
    extract(zip, dir);
    writeFileSync(stamp, version);
  } finally {
    rmSync(work, { recursive: true, force: true });
  }
  if (!existsSync(exe)) throw new Error(`the archive did not contain ${exe}`);
  grant(dir);
  return exe;
}

/**
 * The program and arguments that let Chrome's app-container sandboxes read and run the browser in `dir` (Windows only;
 * `null` elsewhere). Chrome checks that an app container can read its program before it starts a sandboxed helper such
 * as the network service; a folder under the runner's user profile grants app containers nothing, so on Windows CI the
 * check failed in every browser ("Sandbox cannot access executable", runs 7149 and 7154) and Chrome started the network
 * service again unsandboxed. Grants read and execute, inherited, to ALL APPLICATION PACKAGES (S-1-15-2-1) and ALL
 * RESTRICTED APPLICATION PACKAGES (S-1-15-2-2), as Program Files has them. Idempotent.
 */
export function grantCommand(dir, platform = process.platform, env = process.env) {
  if (platform !== 'win32') return null;
  const icacls = win32.join(env.SystemRoot ?? 'C:\\Windows', 'System32', 'icacls.exe');
  return [icacls, [dir, '/grant', '*S-1-15-2-1:(OI)(CI)(RX)', '/grant', '*S-1-15-2-2:(OI)(CI)(RX)', '/Q']];
}

/**
 * Runs `grantCommand` for `dir`. The grant is defence in depth (Chromium builds start without the testing config that
 * turns the network-service sandbox on, so CI run 7170's browsers never sandboxed it), so a failed `icacls` warns on
 * stderr and the install carries on. `command`, `run` and `warn` are injectable.
 */
export function defaultGrant(dir, { command = grantCommand(dir), run = (cmd, args) => execFileSync(cmd, args, { timeout: 5 * 60 * 1000 }), warn = (m) => console.warn(m) } = {}) {
  if (!command) return;
  try {
    run(command[0], command[1]);
  } catch (e) {
    warn(`install-test-browser: warning: could not let app containers read ${dir} (${e.message}); the browser still works, but a sandboxed helper of it may not start`);
  }
}

async function defaultFetch(url) {
  const res = await fetch(url, { signal: AbortSignal.timeout(10 * 60 * 1000) });
  if (!res.ok) throw new Error(`${url}: ${res.status}`);
  return Buffer.from(await res.arrayBuffer());
}

/**
 * The program and arguments that unpack `zip` into `dir`. On Windows it is the system's own `tar.exe` (bsdtar, which reads zip)
 * by its full path: Git for Windows puts GNU tar first on PATH, which reads `C:` as a remote host and cannot read zip.
 */
export function extractCommand(zip, dir, platform = process.platform, env = process.env) {
  if (platform === 'win32') return [win32.join(env.SystemRoot ?? 'C:\\Windows', 'System32', 'tar.exe'), ['-xf', zip, '-C', dir]];
  return ['unzip', ['-q', zip, '-d', dir]];
}

function defaultExtract(zip, dir) {
  const [cmd, args] = extractCommand(zip, dir);
  execFileSync(cmd, args, { timeout: 5 * 60 * 1000 });
}

/** Parses the command line. */
export function parseArgs(argv) {
  const o = {};
  for (let i = 0; i < argv.length; i++) {
    const k = argv[i];
    if (!['--catalogue', '--pin', '--rust-source', '--dir', '--env-prefix'].includes(k)) throw new Error(`unknown option ${k}`);
    if (argv[i + 1] === undefined) throw new Error(`${k} needs a value`);
    o[k.slice(2)] = argv[++i];
  }
  if ([o.catalogue, o.pin, o['rust-source']].filter(Boolean).length !== 1) throw new Error('give exactly one of --catalogue <toml>, --pin <json> and --rust-source <file>');
  if (o['env-prefix'] && !/^[A-Z][A-Z0-9_]*$/.test(o['env-prefix'])) throw new Error('--env-prefix must be upper-case letters, digits and underscores');
  return o;
}

async function main(argv, env = process.env) {
  const o = parseArgs(argv);
  const dir = o.dir ?? env.PADDOCK_TEST_BROWSER_DIR ?? join(homedir(), '.cache', 'paddock-test-browser');
  const exe = o.catalogue
    ? await installFromCatalogue({ text: readFileSync(o.catalogue, 'utf8'), dir })
    : await install({ pin: checkPin(o.pin ? JSON.parse(readFileSync(o.pin, 'utf8')) : parseRustPin(readFileSync(o['rust-source'], 'utf8'))), dir });
  const lines = [`CHROMIUM_TEST_PATH=${exe}`, ...(o['env-prefix'] ? [`${o['env-prefix']}_REQUIRE_TEST_BROWSER=1`] : [])];
  console.log(lines.join('\n'));
  if (env.GITHUB_ENV) appendFileSync(env.GITHUB_ENV, `${lines.join('\n')}\n`);
}

if (isMain(import.meta.url)) {
  main(process.argv.slice(2)).catch((e) => {
    console.error(`install-test-browser: ${e.message}`);
    process.exit(1);
  });
}
