import assert from 'node:assert/strict';
import { execFileSync } from 'node:child_process';
import { readFileSync } from 'node:fs';
import { test } from 'node:test';
import { mkdtempSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { fileURLToPath } from 'node:url';
import { advisoryAge, lockHash, npmAudit, npmAuditRecord, recordCovers } from '../ci/live-data.mjs';

// ADR 0062 section 3a (L1): no `cargo deny` site reaches github.com for the RustSec advisory database.
const root = fileURLToPath(new URL('../..', import.meta.url));
const MIRROR = 'https://dev.decryptology.net/paddock-mirrors/advisory-db.git';
const denyFiles = execFileSync('git', ['ls-files'], { cwd: root, encoding: 'utf8' })
  .split('\n')
  .filter((f) => /(^|\/)deny\.toml$/.test(f));

test('there are twelve deny.toml files (a new one needs the mirror too)', () => {
  assert.equal(denyFiles.length, 12, denyFiles.join(', '));
});

for (const f of denyFiles) {
  test(`${f} reads the advisory database from the Paddock mirror, not GitHub`, () => {
    const text = readFileSync(`${root}/${f}`, 'utf8');
    const advisories = text.split(/^\[advisories\]\s*$/m)[1]?.split(/^\[/m)[0] ?? '';
    assert.match(advisories, new RegExp(`^db-urls\\s*=\\s*\\["${MIRROR.replace(/[.]/g, '\\.')}"\\]\\s*$`, 'm'), 'db-urls in [advisories]');
    assert.doesNotMatch(text, /github\.com\/rustsec/);
  });
}

const NOW = Date.parse('2026-11-10T00:00:00Z');
const ago = (d) => new Date(NOW - d * 86400000).toISOString();

test('the advisory mirror: fresh is ok, over 2 days warns, over 7 days fails only a release run', () => {
  assert.equal(advisoryAge({ updated: ago(1), now: NOW, release: true }).level, 'ok');
  assert.equal(advisoryAge({ updated: ago(3), now: NOW, release: true }).level, 'warn');
  const old = advisoryAge({ updated: ago(8), now: NOW, release: true });
  assert.equal(old.level, 'fail');
  assert.match(old.message, /8\.0 days old.*allows 7.*Synchronize now/);
  assert.equal(advisoryAge({ updated: ago(8), now: NOW, release: false }).level, 'warn');
});

test('an advisory mirror with no usable time is broken, not fresh (ER-33)', () => {
  assert.equal(advisoryAge({ updated: undefined, now: NOW, release: true }).level, 'fail');
});

test('an audit record covers only the same lockfile, level, outcome and at most 48 hours', () => {
  const rec = { lockfile_sha256: 'abc', level: 'high', outcome: 'pass', time: ago(1) };
  assert.equal(recordCovers({ record: rec, hash: 'abc', level: 'high', now: NOW }).ok, true);
  assert.equal(recordCovers({ record: rec, hash: 'xyz', level: 'high', now: NOW }).ok, false);
  assert.equal(recordCovers({ record: rec, hash: 'abc', level: 'low', now: NOW }).ok, false);
  assert.equal(recordCovers({ record: { ...rec, outcome: 'fail' }, hash: 'abc', level: 'high', now: NOW }).ok, false);
  assert.match(recordCovers({ record: { ...rec, time: ago(3) }, hash: 'abc', level: 'high', now: NOW }).reason, /72\.0 hours old/);
  assert.equal(recordCovers({ record: null, hash: 'abc', level: 'high', now: NOW }).ok, false);
});

const lockDir = () => {
  const d = mkdtempSync(join(tmpdir(), 'audit-'));
  writeFileSync(join(d, 'package-lock.json'), '{"lockfileVersion":3}');
  return d;
};
const quiet = () => ({ lines: [], log(x) { this.lines.push(x); } });
const npmResult = (status, text) => () => ({ status, stdout: text, stderr: '' });
const storeWith = (record) => async () => ({ ok: !!record, status: record ? 200 : 404, json: async () => record });

test('release run: a fresh passing record is accepted without auditing', async () => {
  const cwd = lockDir();
  const hash = lockHash('{"lockfileVersion":3}');
  const rec = { lockfile_sha256: hash, level: 'high', outcome: 'pass', time: ago(1) };
  const log = quiet();
  const code = await npmAudit({ cwd, env: { GITHUB_REF: 'refs/tags/x-v1', BUILD_INPUTS_TOKEN: 't' }, now: NOW, fetchImpl: storeWith(rec), run: () => assert.fail('no live audit'), log });
  assert.equal(code, 0);
});

test('release run: no record and an unreachable endpoint fails with the next step', async () => {
  const log = quiet();
  const code = await npmAudit({ cwd: lockDir(), env: { GITHUB_REF: 'refs/tags/x-v1', BUILD_INPUTS_TOKEN: 't' }, now: NOW, fetchImpl: storeWith(null), run: npmResult(1, 'npm error code ENOTFOUND\naudit endpoint'), log });
  assert.equal(code, 1);
  assert.match(log.lines.join('\n'), /::error title=npm audit::.*cannot be reached.*nightly/);
});

test('branch run: an unreachable endpoint only warns; advisories fail', async () => {
  const log = quiet();
  assert.equal(await npmAudit({ cwd: lockDir(), env: {}, run: npmResult(1, 'ECONNREFUSED'), log }), 0);
  assert.match(log.lines.join('\n'), /::warning title=npm audit::/);
  const log2 = quiet();
  assert.equal(await npmAudit({ cwd: lockDir(), env: {}, run: npmResult(1, '3 high severity vulnerabilities'), log: log2 }), 1);
  assert.match(log2.lines.join('\n'), /::error title=npm audit::.*advisories/);
});

test('the nightly record is stored under the lockfile hash, and a missing write token is reported as not measured', async () => {
  const calls = [];
  const fetchImpl = async (url, init) => (calls.push([init.method, url, init.body]), { ok: true, status: 201 });
  const cwd = lockDir();
  const log = quiet();
  assert.equal(await npmAuditRecord({ cwd, env: { BUILD_INPUTS_WRITE_TOKEN: 'w' }, now: NOW, fetchImpl, run: npmResult(0, 'found 0 vulnerabilities'), log }), 0);
  const hash = lockHash('{"lockfileVersion":3}');
  assert.deepEqual(calls.map((c) => c[0]), ['DELETE', 'PUT']);
  assert.ok(calls[1][1].endsWith(`/npm-audit-records/${hash}/record.json`));
  assert.equal(JSON.parse(calls[1][2]).outcome, 'pass');
  const log2 = quiet();
  assert.equal(await npmAuditRecord({ cwd, env: {}, log: log2 }), 0);
  assert.match(log2.lines.join('\n'), /not measured/);
});
