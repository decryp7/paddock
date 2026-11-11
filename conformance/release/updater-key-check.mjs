#!/usr/bin/env node
// Every updater signature a build wrote verifies against the one public key the build carries (ADR 0085 D7, rule 78). Tauri
// signs each updater artifact with minisign (tauri-cli 2.12.3 src/bundle.rs sign_updaters) and writes `<file>.sig`: the
// base64 of a minisign signature file. The public key (TAURI_UPDATER_PUBKEY, plugins.updater.pubkey) is the base64 of a
// minisign public key file. A signature from another key (a throw-away key on a tag, a key that does not match the secret's
// public half) would install nowhere: this check fails the build instead of the user's update.
//
//   node updater-key-check.mjs --pubkey <base64 key> --dir <folder>      every *.sig under <folder>, recursively
//
// Exit 0 when at least one signature was found and all verify; 1 otherwise, with one ::error:: per finding.
import { createHash, createPublicKey, verify } from 'node:crypto';
import { existsSync, readdirSync, readFileSync, statSync } from 'node:fs';
import { join } from 'node:path';
import { isMain } from '../src/util/is-main.mjs';

/** The second line of a base64-wrapped minisign file, decoded: the raw key or signature bytes. */
function payload(b64, what) {
  const text = Buffer.from(String(b64).trim(), 'base64').toString('utf8');
  const line = text.split('\n')[1];
  if (!line) throw new Error(`${what} is not a minisign file (no second line)`);
  return { raw: Buffer.from(line.trim(), 'base64'), text };
}

/** { id, key } of a minisign public key: its 8-byte key id (hex, as minisign prints it) and the Ed25519 key object. */
export function parsePublicKey(b64) {
  const { raw } = payload(b64, 'the public key');
  if (raw.length !== 42 || raw.subarray(0, 2).toString() !== 'Ed') throw new Error('the public key is not an Ed25519 minisign key');
  const der = Buffer.concat([Buffer.from('302a300506032b6570032100', 'hex'), raw.subarray(10)]);
  return { id: Buffer.from(raw.subarray(2, 10)).reverse().toString('hex').toUpperCase(), key: createPublicKey({ key: der, format: 'der', type: 'spki' }) };
}

/** Whether `sigB64` (a Tauri .sig file's content) signs `data` with `pub`; { ok, id, reason }. */
export function verifySignature(pub, sigB64, data) {
  let raw;
  try {
    ({ raw } = payload(sigB64, 'the signature'));
  } catch (e) {
    return { ok: false, id: null, reason: e.message };
  }
  if (raw.length !== 74) return { ok: false, id: null, reason: `a signature of ${raw.length} bytes, not 74` };
  const alg = raw.subarray(0, 2).toString();
  const id = Buffer.from(raw.subarray(2, 10)).reverse().toString('hex').toUpperCase();
  if (id !== pub.id) return { ok: false, id, reason: `signed by key ${id}, not ${pub.id}` };
  // `ED`: the BLAKE2b-512 of the file is signed (minisign's prehashed mode, what Tauri writes); `Ed`: the file itself.
  const message = alg === 'ED' ? createHash('blake2b512').update(data).digest() : alg === 'Ed' ? data : null;
  if (!message) return { ok: false, id, reason: `unknown signature algorithm ${JSON.stringify(alg)}` };
  return verify(null, message, pub.key, raw.subarray(10)) ? { ok: true, id } : { ok: false, id, reason: 'the signature does not verify' };
}

function sigsUnder(dir, out = []) {
  for (const e of readdirSync(dir, { withFileTypes: true })) {
    const p = join(dir, e.name);
    if (e.isDirectory()) sigsUnder(p, out);
    else if (e.name.endsWith('.sig')) out.push(p);
  }
  return out;
}

/** The findings for every .sig under `dir` (empty: all verify). */
export function checkDir(pubkey, dir) {
  let pub;
  try {
    pub = parsePublicKey(pubkey);
  } catch (e) {
    return { pub: null, checked: 0, findings: [`the updater public key cannot be read: ${e.message}`] };
  }
  if (!existsSync(dir) || !statSync(dir).isDirectory()) return { pub, checked: 0, findings: [`check broken: ${dir} is not a folder`] };
  const sigs = sigsUnder(dir).sort();
  if (!sigs.length) return { pub, checked: 0, findings: [`check broken: no .sig file under ${dir}, so nothing was compared with the key`] };
  const findings = [];
  for (const s of sigs) {
    const file = s.slice(0, -'.sig'.length);
    if (!existsSync(file)) {
      findings.push(`${s}: the signed file ${file} is missing`);
      continue;
    }
    const r = verifySignature(pub, readFileSync(s, 'utf8'), readFileSync(file));
    if (!r.ok) findings.push(`${s}: ${r.reason}`);
  }
  return { pub, checked: sigs.length, findings };
}

export function main(argv, log = console.log) {
  const opt = (n) => {
    const i = argv.indexOf(n);
    return i < 0 ? undefined : argv[i + 1];
  };
  const pubkey = opt('--pubkey');
  const dir = opt('--dir');
  if (!pubkey || !dir) {
    log('::error::usage: updater-key-check.mjs --pubkey <base64 key> --dir <folder>');
    return 2;
  }
  const r = checkDir(pubkey, dir);
  for (const f of r.findings) {
    log(`::error::Updater signature: ${f}. Next step: the build was signed with another key than the public key it carries; check TAURI_SIGNING_PRIVATE_KEY and TAURI_UPDATER_PUBKEY are one pair (ADR 0085 D7).`);
  }
  if (!r.findings.length) log(`updater-key-check: ${r.checked} signature(s) verify against key ${r.pub.id}.`);
  return r.findings.length ? 1 : 0;
}

if (isMain(import.meta.url)) process.exit(main(process.argv.slice(2)));
