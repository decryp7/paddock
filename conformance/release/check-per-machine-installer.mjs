#!/usr/bin/env node
// The artifact gate of a per-machine installer (family ADR 0053 section 8, ADR 0039): the built NSIS installer must ask for
// administrator level, contain paddock-setup.exe, and carry the include's version in its version resource, so a build whose hooks
// never ran the include (or whose bundle step did not stage the program) cannot be released. On Windows the embedded program
// is also run with `self-test`, which must name the app's identifier and say `stop_all=present`.
//
//   node conformance/release/check-per-machine-installer.mjs --dir <cargo profile folder holding bundle/nsis> --conf <tauri.conf.json>
//        [--include crates/paddock-setup/nsis/paddock-per-machine.nsh]
//
// Needs 7-Zip (it opens NSIS installers): on PATH or in C:\\Program Files\\7-Zip; without it the gate fails, it never passes unchecked.
import { spawnSync } from 'node:child_process';
import { mkdtempSync, readFileSync, readdirSync, rmSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { isMain } from '../src/util/is-main.mjs';

/** Whole-installer checks are on bytes: the manifest is stored as plain XML. */
export function hasAdministratorManifest(bytes) {
  return bytes.includes('requireAdministrator');
}

/** The `PaddockInclude` value of the version resource (UTF-16: key, two NULs, value), or null. */
export function includeVersionOf(bytes) {
  const key = Buffer.from('PaddockInclude', 'utf16le');
  const at = bytes.indexOf(key);
  if (at < 0) return null;
  let i = at + key.length;
  while (i + 1 < bytes.length && bytes[i] === 0 && bytes[i + 1] === 0) i += 2;
  let end = i;
  while (end + 1 < bytes.length && !(bytes[end] === 0 && bytes[end + 1] === 0)) end += 2;
  return bytes.subarray(i, end).toString('utf16le');
}

/** The file names inside the archive from `7z l -slt` (the archive's own line is skipped). */
export function listedFiles(text) {
  return text
    .split('\n')
    .map((l) => /^Path = (.+?)\r?$/.exec(l)?.[1])
    .filter((p) => p !== undefined)
    .slice(1);
}

/** The NSIS installers a build wrote: `*-setup.exe` directly in `<dir>/bundle/nsis` (nowhere else under the folder). */
export function findInstallers(dir) {
  try {
    return readdirSync(join(dir, 'bundle', 'nsis'))
      .filter((n) => /-setup\.exe$/i.test(n))
      .map((n) => join(dir, 'bundle', 'nsis', n))
      .sort();
  } catch {
    return [];
  }
}

const SEVEN_ZIP_FALLBACK = 'C:\\Program Files\\7-Zip\\7z.exe';

/** The 7z to use: on PATH, then the standard install folder; otherwise one error that says how to get it. */
export function find7z({ probe = (cmd, args) => spawnSync(cmd, args, { encoding: 'utf8', timeout: 20000 }) } = {}) {
  for (const cmd of ['7z', '7zz', SEVEN_ZIP_FALLBACK]) {
    const r = probe(cmd, ['i']);
    if (!r.error && r.status === 0) return { cmd };
  }
  return { error: '7-Zip (7z) was not found on PATH or in C:\\Program Files\\7-Zip: the gate opens the NSIS installer with it. Install 7-Zip on the runner (winget install 7zip.7zip, or the CI image: apt install 7zip), then run the job again.' };
}

/** The trusted comment of a minisign signature file's text (the file is base64 of the four-line signature), or null. */
export function trustedCommentOf(text) {
  const t = Buffer.from(String(text).trim(), 'base64').toString('utf8');
  return /^trusted comment: (.*)$/m.exec(t)?.[1]?.trimEnd() ?? null;
}

/** paddock-setup accepts an update only when the signed trusted comment names `<productName>_<version>_<arch>-setup.exe`. */
export function checkSignatureName({ sig, file, product, version }) {
  const comment = trustedCommentOf(sig);
  if (comment === null) return [`${file}.sig is unreadable (no trusted comment)`];
  const m = /^(.+)_(.+)_([^_]+)-setup\.exe$/.exec(file);
  if (!m || m[1] !== product || m[2] !== version) return [`the file name ${file} is not ${product}_${version}_<arch>-setup.exe`];
  const named = /(?:^|\t)file:(\S+)/.exec(comment)?.[1];
  if (named === undefined) return [`${file}.sig's trusted comment has no file: name (${comment})`];
  return named === file ? [] : [`${file}.sig's trusted comment names ${named}, expected ${file}: every update would be refused`];
}

/** The Subsystem of a PE file's optional header (2 GUI, 3 console); throws a message that says what is wrong. */
export function subsystemOf(bytes) {
  const b = Buffer.from(bytes);
  if (b.length < 0x40 || b.readUInt16LE(0) !== 0x5a4d) throw new Error('not a Windows program (no MZ header)');
  const pe = b.readUInt32LE(0x3c);
  if (pe + 24 + 70 > b.length || b.readUInt32LE(pe) !== 0x4550) throw new Error('not a Windows program (no PE header)');
  return b.readUInt16LE(pe + 24 + 68);
}

/** paddock-setup.exe is started by the installer on every user's PC: a console-subsystem build opens a command prompt. */
export function checkSetupSubsystem({ bytes, name }) {
  if (!bytes) return [`${name}: paddock-setup.exe could not be extracted to check its subsystem`];
  try {
    const s = subsystemOf(bytes);
    return s === 2 ? [] : [`${name}: paddock-setup.exe has PE subsystem ${s}, expected 2 (GUI): a console program shows a command prompt window during install, update and uninstall`];
  } catch (e) {
    return [`${name}: paddock-setup.exe is ${e.message}`];
  }
}

/** One string of a version resource (UTF-16: key, NULs, value), or null. */
export function resourceStringOf(bytes, key) {
  const b = Buffer.from(bytes);
  const k = Buffer.from(key, 'utf16le');
  for (let at = b.indexOf(k); at >= 0; at = b.indexOf(k, at + 2)) {
    // A key is preceded by its block header; skip a hit that is inside another string (the key must be followed by its NUL).
    if (b[at + k.length] !== 0 || b[at + k.length + 1] !== 0) continue;
    let i = at + k.length;
    while (i + 1 < b.length && b[i] === 0 && b[i + 1] === 0) i += 2;
    let end = i;
    while (end + 1 < b.length && !(b[end] === 0 && b[end + 1] === 0)) end += 2;
    return b.subarray(i, end).toString('utf16le');
  }
  return null;
}

/** What a build account leaves in a Windows program: its home folder and the cargo registry path (`--remap-path-prefix` removes both). */
const BUILD_PATH_LEAKS = [/[A-Za-z]:\\Users\\[^\\\0]+/, /\.cargo[\\/]registry/];

/**
 * paddock-setup.exe is judged by Defender's machine-learning model, which reads the file's metadata (Dyno 1.1.1 WP-4, WP-7): a full version resource (every
 * field), an `asInvoker` manifest with `supportedOS`, its own version (the crate's, so the bytes do not move with an app release) and no build
 * account path in it.
 */
export function checkSetupDescribed({ bytes, name, expectVersion, product }) {
  if (!bytes) return [`${name}: paddock-setup.exe could not be extracted to check its version resource`];
  const f = [];
  const want = { FileVersion: expectVersion, ProductVersion: expectVersion, OriginalFilename: 'paddock-setup.exe', InternalName: 'paddock-setup', ProductName: product, FileDescription: `${product} update` };
  for (const key of ['CompanyName', 'LegalCopyright', ...Object.keys(want)]) {
    const v = resourceStringOf(bytes, key);
    if (v === null || v === '') f.push(`${name}: paddock-setup.exe has no ${key} in its version resource (crates/paddock-setup/src/versionres.rs)`);
    else if (key in want && want[key] !== undefined && v !== want[key]) f.push(`${name}: paddock-setup.exe ${key} is "${v}", expected "${want[key]}"${key.endsWith('Version') ? ': it carries the crate version, not the app version' : ''}`);
  }
  const text = Buffer.from(bytes).toString('latin1');
  if (!text.includes('level="asInvoker"')) f.push(`${name}: paddock-setup.exe has no asInvoker manifest`);
  if (!text.includes('<supportedOS')) f.push(`${name}: paddock-setup.exe manifest has no supportedOS`);
  for (const re of BUILD_PATH_LEAKS) {
    const m = re.exec(text);
    if (m) f.push(`${name}: paddock-setup.exe contains a build path (${m[0]}): build it through scripts/stage-paddock-setup.mjs, which remaps paths`);
  }
  return f;
}

const NSIS_MIN = [3, 12];

/** The NSIS version that made an installer, from the stub's default branding string ("Nullsoft Install System v3.13", ANSI or
 * UTF-16), or null when the string is not there (an app that sets its own BrandingText). */
export function nsisVersionOf(bytes) {
  const b = Buffer.from(bytes);
  for (const enc of ['latin1', 'utf16le']) {
    const m = /Nullsoft Install System v(\d+\.\d+(?:\.\d+)?)/.exec(b.toString(enc));
    if (m) return m[1];
  }
  return null;
}

/** NSIS before 3.12 can run elevated code out of a $PLUGINSDIR a standard user could prepare (3.11: restricted $PLUGINSDIR; 3.12: low-IL
 * temp when elevated; both per NSIS's own changelog). The Tauri bundler picks the NSIS version, so the built installer is read. */
export function checkNsisVersion({ bytes, name }) {
  const v = nsisVersionOf(bytes);
  if (v === null) return [`${name}: the NSIS version was not measured (no "Nullsoft Install System vX.Y" string in the installer), so the NSIS 3.12 minimum is not shown; do not release until it is`];
  const [maj, min] = v.split('.').map(Number);
  return maj > NSIS_MIN[0] || (maj === NSIS_MIN[0] && min >= NSIS_MIN[1]) ? [] : [`${name} was made by NSIS ${v}, below ${NSIS_MIN.join('.')}: a $PLUGINSDIR privilege escalation fixed in 3.11 and 3.12 (NSIS changelog) is present. Use a Tauri CLI whose bundler downloads NSIS ${NSIS_MIN.join('.')} or later`];
}

/** What the embedded program's `self-test` must say: the app's identifier, and `stop_all=present` (a build without the stopper
 * would fail `stop-all` at install time). */
export function checkSelfTest({ stdout, status, identifier, name }) {
  const out = typeof stdout === 'string' ? stdout : '';
  if (status !== 0) return [`${name}: paddock-setup.exe self-test did not run or failed (exit ${status ?? 'none'})`];
  const f = [];
  if (!out.includes(identifier)) f.push(`${name}: paddock-setup.exe self-test does not name ${identifier} (output: ${out.trim()})`);
  if (!/^stop_all=present\s*$/m.test(out)) f.push(`${name}: paddock-setup.exe self-test does not say stop_all=present: this build cannot stop other sessions' programs`);
  return f;
}

/** The `version` of a crate's `[package]` table, or null. */
export function crateVersion(toml) {
  const pkg = /^\[package\]\s*$([\s\S]*?)(?=^\[|(?![\s\S]))/m.exec(toml)?.[1] ?? '';
  return /^version\s*=\s*"([^"]+)"/m.exec(pkg)?.[1] ?? null;
}

/** What is wrong with one installer, one sentence each with the next step. */
export function checkInstaller({ bytes, files, expectVersion, name }) {
  const f = [];
  if (!hasAdministratorManifest(bytes)) f.push(`${name} does not request administrator level (installMode must be perMachine)`);
  if (files === null) f.push(`${name} could not be listed (7z): the gate cannot see whether paddock-setup.exe is inside`);
  else if (!files.some((p) => /(^|[\\/])paddock-setup\.exe$/i.test(p))) f.push(`${name} does not contain paddock-setup.exe: stage it before the bundle (scripts/stage-paddock-setup.mjs)`);
  const v = includeVersionOf(bytes);
  if (v !== expectVersion) f.push(`${name} carries include version ${v ?? 'none'}, expected ${expectVersion}: its hooks do not include the current paddock-per-machine.nsh`);
  return f;
}

function main(argv) {
  const o = {};
  for (let i = 0; i < argv.length; i++) {
    const k = argv[i].replace(/^--/, '');
    if (k === 'no-sig') o[k] = true;
    else o[k] = argv[++i];
  }
  if (!o.dir || !o.conf) {
    console.error('usage: check-per-machine-installer.mjs --dir <folder> --conf <tauri.conf.json> [--include <nsh>]');
    return 2;
  }
  const includePath = o.include ?? 'crates/paddock-setup/nsis/paddock-per-machine.nsh';
  const expectVersion = /PADDOCK_INCLUDE_VERSION "([^"]+)"/.exec(readFileSync(includePath, 'utf8'))?.[1];
  const conf = JSON.parse(readFileSync(o.conf, 'utf8'));
  // paddock-setup has its own version (the crate's), not the app's.
  const setupVersion = crateVersion(readFileSync(o.setupManifest ?? 'crates/paddock-setup/Cargo.toml', 'utf8'));
  const installers = findInstallers(o.dir);
  if (installers.length === 0) {
    console.error(`::error::no *-setup.exe under ${o.dir}: nothing to check`);
    return 1;
  }
  const sevenZip = find7z();
  if (sevenZip.error) {
    console.error(`::error::${sevenZip.error}`);
    return 1;
  }
  let failed = false;
  for (const path of installers) {
    const name = path.split(/[\\/]/).pop();
    const listing = spawnSync(sevenZip.cmd, ['l', '-slt', path], { encoding: 'utf8', timeout: 120000 });
    const files = listing.status === 0 ? listedFiles(listing.stdout) : null;
    const problems = [];
    try {
      problems.push(...checkSignatureName({ sig: readFileSync(`${path}.sig`, 'utf8'), file: name, product: conf.productName, version: conf.version }));
    } catch {
      if (!o['no-sig']) problems.push(`${name}.sig is missing: the release is signed by the bundle step`);
    }
    problems.push(...checkNsisVersion({ bytes: readFileSync(path), name }));
    problems.push(...checkInstaller({ bytes: readFileSync(path), files, expectVersion, name }));
    if (files) {
      const tmp = mkdtempSync(join(tmpdir(), 'pm-gate-'));
      try {
        const ex = spawnSync(sevenZip.cmd, ['e', '-y', `-o${tmp}`, path, 'paddock-setup.exe'], { encoding: 'utf8', timeout: 120000 });
        const exe = join(tmp, 'paddock-setup.exe');
        problems.push(...checkSetupSubsystem({ bytes: ex.status === 0 ? readFileSync(exe) : null, name }));
        problems.push(...checkSetupDescribed({ bytes: ex.status === 0 ? readFileSync(exe) : null, name, expectVersion: setupVersion, product: conf.productName }));
        // Running it needs Windows. Its output is read through pipes: a GUI-subsystem program still writes to inherited handles.
        if (process.platform === 'win32' && ex.status === 0) {
          const run = spawnSync(exe, ['self-test'], { encoding: 'utf8', timeout: 60000 });
          problems.push(...checkSelfTest({ stdout: run.stdout, status: run.status, identifier: conf.identifier, name }));
        }
      } finally {
        rmSync(tmp, { recursive: true, force: true });
      }
    }
    for (const p of problems) console.error(`::error::${p}`);
    if (problems.length === 0) console.log(`${name}: administrator level, paddock-setup.exe inside, include version ${expectVersion}`);
    failed ||= problems.length > 0;
  }
  return failed ? 1 : 0;
}

if (isMain(import.meta.url)) {
  try {
    process.exit(main(process.argv.slice(2)));
  } catch (e) {
    console.error(`::error::the per-machine installer gate failed to run: ${e.message}. Check --dir, --conf and --include, then run it again.`);
    process.exit(1);
  }
}
