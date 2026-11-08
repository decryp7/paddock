#!/usr/bin/env node
// The live-data policy of ADR 0062 section 3a (WP-1c): data that is meant to change (advisory databases) is not pinned, so a
// release run proves it is fresh instead.
//
//   live-data.mjs advisory-db [--release]     the RustSec pull mirror's last sync: older than 2 days warns, older than 7 days
//                                             fails a release run (a tag, or --release)
//   live-data.mjs npm-audit [--level high]    `npm audit --omit=dev` for the nearest package-lock.json. A release run accepts a
//                                             passing record for the same lockfile hash that is at most 48 hours old, else
//                                             audits live once; a branch run audits live and only warns when npm is unreachable
//   live-data.mjs npm-audit-record [--level high]   (nightly) audits live and stores the record in the private store
//
// Environment: BUILD_INPUTS_TOKEN (read), BUILD_INPUTS_WRITE_TOKEN (record), BUILD_INPUTS_STORE.
import { spawnSync } from 'node:child_process';
import { createHash } from 'node:crypto';
import { existsSync, readFileSync } from 'node:fs';
import { dirname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
import { isMain } from '../src/util/is-main.mjs';

const STORE = 'https://dev.decryptology.net/api/packages/paddock-build-inputs/generic';
const MIRROR_API = 'https://dev.decryptology.net/api/v1/repos/paddock-mirrors/advisory-db';
const DAY = 86400000;

/** Age policy of the advisory mirror: `{ days, level: 'ok' | 'warn' | 'fail', message }`. */
export function advisoryAge({ updated, now = Date.now(), release }) {
  const t = Date.parse(updated);
  if (Number.isNaN(t)) {
    return { days: NaN, level: release ? 'fail' : 'warn', message: `the advisory mirror reports no usable last-sync time (${JSON.stringify(updated)}). Next: open https://dev.decryptology.net/paddock-mirrors/advisory-db and trigger "Synchronize now" in its settings.` };
  }
  const days = (now - t) / DAY;
  const shown = days.toFixed(1);
  const next = 'Next: sync the mirror (https://dev.decryptology.net/paddock-mirrors/advisory-db, Settings, "Synchronize now"; Gitea refreshes it on its own schedule), then re-run.';
  if (release && days > 7) return { days, level: 'fail', message: `the RustSec advisory mirror was last synced ${shown} days ago (a release run allows 7). ${next}` };
  if (days > 2) return { days, level: 'warn', message: `the RustSec advisory mirror was last synced ${shown} days ago (a warning above 2). ${next}` };
  return { days, level: 'ok', message: `the RustSec advisory mirror was synced ${shown} days ago` };
}

export const lockHash = (text) => createHash('sha256').update(text).digest('hex');

/**
 * Whether a stored audit record covers this lockfile now: `{ ok, reason }`. A record counts for the same lockfile hash and
 * audit level, with a passing outcome, at most 48 hours old.
 */
export function recordCovers({ record, hash, level, now = Date.now() }) {
  if (!record) return { ok: false, reason: `no audit record for lockfile ${hash.slice(0, 12)}` };
  if (record.lockfile_sha256 !== hash) return { ok: false, reason: 'the record is for another lockfile' };
  if (record.level !== level) return { ok: false, reason: `the record audited at level ${record.level}, not ${level}` };
  if (record.outcome !== 'pass') return { ok: false, reason: `the record's outcome is ${record.outcome}` };
  const hours = (now - Date.parse(record.time)) / 3600000;
  if (!(hours <= 48)) return { ok: false, reason: `the record is ${Number.isFinite(hours) ? hours.toFixed(1) : '?'} hours old (allowed: 48)` };
  return { ok: true, reason: `a passing record from ${record.time} covers lockfile ${hash.slice(0, 12)}` };
}

const NETWORK = /ENOTFOUND|ECONNREFUSED|ECONNRESET|ETIMEDOUT|EAI_AGAIN|audit endpoint returned an error|network/i;

/** Runs `npm audit --omit=dev --audit-level=<level>`: `{ outcome: 'pass' | 'advisories' | 'unreachable', output }`. */
export function liveAudit({ level, cwd, run = spawnSync }) {
  const r = run('npm', ['audit', '--omit=dev', `--audit-level=${level}`], { cwd, encoding: 'utf8' });
  const output = `${r.stdout ?? ''}${r.stderr ?? ''}`;
  if (r.status === 0) return { outcome: 'pass', output };
  return { outcome: NETWORK.test(output) && !/vulnerabilit/i.test(output) ? 'unreachable' : 'advisories', output };
}

function findLockfile(from) {
  for (let d = resolve(from); ; d = dirname(d)) {
    if (existsSync(join(d, 'package-lock.json'))) return join(d, 'package-lock.json');
    if (dirname(d) === d) return null;
  }
}

const isRelease = (env) => /^refs\/tags\//.test(env.GITHUB_REF ?? '');

async function readRecord(hash, { store, token, fetchImpl = fetch }) {
  if (!token) return null;
  const res = await fetchImpl(`${store}/npm-audit-records/${hash}/record.json`, { headers: { authorization: `token ${token}` } });
  return res.ok ? res.json() : null;
}

export async function npmAudit({ level = 'high', cwd = process.cwd(), env = process.env, now = Date.now(), fetchImpl = fetch, run = spawnSync, log = console }) {
  const lock = findLockfile(cwd);
  if (!lock) {
    log.log('::error title=npm audit::no package-lock.json found above ' + cwd + '. Next: run this from a folder inside the npm workspace.');
    return 1;
  }
  const hash = lockHash(readFileSync(lock));
  const release = isRelease(env);
  if (release) {
    const record = await readRecord(hash, { store: env.BUILD_INPUTS_STORE || STORE, token: env.BUILD_INPUTS_TOKEN, fetchImpl }).catch(() => null);
    const c = recordCovers({ record, hash, level, now });
    log.log(`npm audit: ${c.reason}`);
    if (c.ok) return 0;
  }
  const a = liveAudit({ level, cwd, run });
  if (a.outcome === 'pass') return 0;
  log.log(a.output);
  if (a.outcome === 'advisories') {
    log.log(`::error title=npm audit::npm audit found advisories at level ${level} or above in the production dependencies of ${lock}. Next: update the dependency, or record why it does not apply.`);
    return 1;
  }
  if (release) {
    log.log('::error title=npm audit::the npm audit endpoint cannot be reached and no passing record covers this lockfile. Next: dispatch the nightly workflow (its audit-records job stores the record) or wait for it, then re-run the release.');
    return 1;
  }
  log.log('::warning title=npm audit::the npm audit endpoint cannot be reached; this branch run did not audit the production dependencies (a release run needs a record).');
  return 0;
}

export async function npmAuditRecord({ level = 'high', cwd = process.cwd(), env = process.env, now = Date.now(), fetchImpl = fetch, run = spawnSync, log = console }) {
  const lock = findLockfile(cwd);
  if (!lock) return 1;
  const token = env.BUILD_INPUTS_WRITE_TOKEN;
  if (!token) {
    log.log('::warning title=npm audit record::BUILD_INPUTS_WRITE_TOKEN is not set: the audit record was not stored, so a release run cannot use it (not measured). Next: add the write token as a repository Actions secret.');
    return 0;
  }
  const hash = lockHash(readFileSync(lock));
  const a = liveAudit({ level, cwd, run });
  if (a.outcome === 'unreachable') {
    log.log('::error title=npm audit record::the npm audit endpoint cannot be reached, so no record was stored. Next: re-run the nightly workflow.');
    return 1;
  }
  const count = (a.output.match(/(\d+) (?:low|moderate|high|critical)/g) ?? []).length;
  const record = { lockfile_sha256: hash, time: new Date(now).toISOString(), level, outcome: a.outcome === 'pass' ? 'pass' : 'fail', advisory_count: count };
  const url = `${env.BUILD_INPUTS_STORE || STORE}/npm-audit-records/${hash}/record.json`;
  const headers = { authorization: `token ${token}` };
  await fetchImpl(url, { method: 'DELETE', headers });
  const put = await fetchImpl(url, { method: 'PUT', headers, body: JSON.stringify(record) });
  if (!put.ok) {
    log.log(`::error title=npm audit record::storing the record answered HTTP ${put.status} for npm-audit-records/${hash.slice(0, 12)}. Next: check BUILD_INPUTS_WRITE_TOKEN (write:package on paddock-build-inputs).`);
    return 1;
  }
  log.log(`npm audit record stored for lockfile ${hash.slice(0, 12)}: ${record.outcome}`);
  return a.outcome === 'pass' ? 0 : 1;
}

async function main(argv, env = process.env) {
  const [cmd, ...rest] = argv;
  const level = rest.includes('--level') ? rest[rest.indexOf('--level') + 1] : 'high';
  if (cmd === 'advisory-db') {
    const release = rest.includes('--release') || isRelease(env);
    let updated;
    try {
      const res = await fetch(MIRROR_API, { signal: AbortSignal.timeout(30000) });
      if (!res.ok) throw new Error(`HTTP ${res.status}`);
      updated = (await res.json()).mirror_updated;
    } catch (err) {
      console.log(`::${release ? 'error' : 'warning'} title=advisory mirror::could not read the advisory mirror's last sync from ${MIRROR_API}: ${err.message}. Next: check that dev.decryptology.net is reachable and paddock-mirrors/advisory-db exists.`);
      return release ? 1 : 0;
    }
    const r = advisoryAge({ updated, release });
    console.log(r.level === 'ok' ? r.message : `::${r.level === 'fail' ? 'error' : 'warning'} title=advisory mirror::${r.message}`);
    return r.level === 'fail' ? 1 : 0;
  }
  if (cmd === 'npm-audit') return npmAudit({ level });
  if (cmd === 'npm-audit-record') return npmAuditRecord({ level });
  console.error('usage: live-data.mjs advisory-db [--release] | npm-audit [--level L] | npm-audit-record [--level L]');
  return 2;
}

if (isMain(import.meta.url)) {
  main(process.argv.slice(2)).then((c) => process.exit(c));
}
