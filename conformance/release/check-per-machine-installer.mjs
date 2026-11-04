#!/usr/bin/env node
// The artifact gate of a per-machine installer (family ADR 0053 section 8, ADR 0039): the built NSIS installer must ask for
// administrator level, contain paddock-setup.exe, and carry the include's version in its version resource, so a build whose hooks
// never ran the include (or whose bundle step did not stage the program) cannot be released. On Windows the embedded program
// is also run with `self-test`, which must name the app's identifier.
//
//   node conformance/release/check-per-machine-installer.mjs --dir <cargo profile folder holding bundle/nsis> --conf <tauri.conf.json>
//        [--include crates/paddock-setup/nsis/paddock-per-machine.nsh]
//
// Needs 7z (it opens NSIS installers); without it the gate fails, it never passes unchecked.
import { spawnSync } from 'node:child_process';
import { mkdtempSync, readFileSync, readdirSync, statSync } from 'node:fs';
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

/** NSIS installers under `dir` (any depth). */
export function findInstallers(dir) {
  const out = [];
  const walk = (d) => {
    let names;
    try {
      names = readdirSync(d);
    } catch {
      return;
    }
    for (const n of names) {
      const p = join(d, n);
      if (statSync(p).isDirectory()) walk(p);
      else if (/-setup\.exe$/i.test(n)) out.push(p);
    }
  };
  walk(dir);
  return out.sort();
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
  const installers = findInstallers(o.dir);
  if (installers.length === 0) {
    console.error(`::error::no *-setup.exe under ${o.dir}: nothing to check`);
    return 1;
  }
  let failed = false;
  for (const path of installers) {
    const name = path.split(/[\\/]/).pop();
    const listing = spawnSync('7z', ['l', '-slt', path], { encoding: 'utf8' });
    const files = listing.status === 0 ? listedFiles(listing.stdout) : null;
    const problems = [];
    try {
      problems.push(...checkSignatureName({ sig: readFileSync(`${path}.sig`, 'utf8'), file: name, product: conf.productName, version: conf.version }));
    } catch {
      if (!o['no-sig']) problems.push(`${name}.sig is missing: the release is signed by the bundle step`);
    }
    problems.push(...checkInstaller({ bytes: readFileSync(path), files, expectVersion, name }));
    if (process.platform === 'win32' && files) {
      const tmp = mkdtempSync(join(tmpdir(), 'pm-gate-'));
      const ex = spawnSync('7z', ['e', '-y', `-o${tmp}`, path, 'paddock-setup.exe'], { encoding: 'utf8' });
      const run = ex.status === 0 ? spawnSync(join(tmp, 'paddock-setup.exe'), ['self-test'], { encoding: 'utf8' }) : null;
      if (!run || run.status !== 0 || !run.stdout.includes(conf.identifier)) problems.push(`${name}: paddock-setup.exe self-test does not name ${conf.identifier} (output: ${run?.stdout?.trim() ?? 'did not run'})`);
    }
    for (const p of problems) console.error(`::error::${p}`);
    if (problems.length === 0) console.log(`${name}: administrator level, paddock-setup.exe inside, include version ${expectVersion}`);
    failed ||= problems.length > 0;
  }
  return failed ? 1 : 0;
}

if (isMain(import.meta.url)) process.exit(main(process.argv.slice(2)));
