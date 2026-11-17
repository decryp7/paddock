import { test } from 'node:test';
import assert from 'node:assert/strict';
import { execFileSync, spawnSync } from 'node:child_process';
import { mkdtempSync, readFileSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';
import { ALLOW_FILE, advisories, judge, parseAllow } from './npm-audit.mjs';

const here = dirname(fileURLToPath(import.meta.url));
const ghsa = 'GHSA-aaaa-bbbb-cccc';
// The shape `npm audit --json` gives (npm 11): the advisory sits in the vulnerable package's `via`, its dependents name it by package.
const report = (severity = 'high', id = ghsa) => ({
  vulnerabilities: {
    picomatch: { severity, via: [{ source: 1, name: 'picomatch', severity, title: 'ReDoS', url: `https://github.com/advisories/${id}` }] },
    fdir: { severity, via: ['picomatch'] },
  },
});
const entry = (over = {}) => ({ id: ghsa, reason: 'build tool only', approvedBy: 'user', approved: '2026-11-18', reviewBy: '2026-12-18', ...over });

test('a high advisory blocks unless the list allows it, and only until its review day', () => {
  assert.deepEqual(judge(report(), [], '2026-11-20').blocked.map((a) => a.id), [ghsa]);
  const before = judge(report(), [entry()], '2026-12-17');
  assert.deepEqual([before.blocked.length, before.allowed.length, before.expired.length], [0, 1, 0]);
  const on = judge(report(), [entry()], '2026-12-18');
  assert.deepEqual([on.blocked.length, on.expired.length], [1, 1], 'on the review day the exception stops counting');
});

test('moderate and low advisories are not findings; dependents add no advisory of their own', () => {
  assert.deepEqual(advisories(report('moderate')), []);
  assert.equal(advisories(report('critical')).length, 1);
});

test('an entry that matches nothing is reported as unused, not as a failure', () => {
  const v = judge({ vulnerabilities: {} }, [entry()], '2026-11-20');
  assert.deepEqual([v.blocked.length, v.unused.length], [0, 1]);
});

test('the list refuses entries without a reason, an approver, a review day after approval, or a GHSA id', () => {
  const doc = (e) => JSON.stringify({ allow: [e] });
  assert.throws(() => parseAllow(doc(entry({ reason: '' }))), /no reason/);
  assert.throws(() => parseAllow(doc(entry({ approvedBy: undefined }))), /no approvedBy/);
  assert.throws(() => parseAllow(doc(entry({ reviewBy: '2026-11-18' }))), /review day/);
  assert.throws(() => parseAllow(doc(entry({ id: 'CVE-1' }))), /not a GHSA id/);
  assert.throws(() => parseAllow(JSON.stringify({ allow: [entry(), entry()] })), /listed twice/);
});

test('the command: exit 1 with one ::error:: per finding, 0 once allowed, 1 again when the exception expired', () => {
  const dir = mkdtempSync(join(tmpdir(), 'npm-audit-'));
  const file = join(dir, 'r.json');
  writeFileSync(file, JSON.stringify(report('high', 'GHSA-zzzz-zzzz-zzzz')));
  const run = (today) => spawnSync(process.execPath, [join(here, 'npm-audit.mjs'), '--report', file, '--today', today], { encoding: 'utf8' });
  const red = run('2026-11-20');
  assert.equal(red.status, 1, red.stdout);
  assert.match(red.stdout, /::error::npm-audit: GHSA-zzzz-zzzz-zzzz high in picomatch/);
  // The real list's own entry: the report of today's advisory passes until its review day, then fails.
  const real = parseAllow(readFileSync(ALLOW_FILE, 'utf8'))[0];
  writeFileSync(file, JSON.stringify(report('high', real.id)));
  assert.equal(run(real.approved).status, 0);
  const late = run(real.reviewBy);
  assert.equal(late.status, 1);
  assert.match(late.stdout, /expired on/);
});

test('ER-46: an entry the merge base does not have was approved by the user', () => {
  const base = (() => {
    try {
      return execFileSync('git', ['show', 'origin/main:scripts/npm-audit-allow.json'], { cwd: here, encoding: 'utf8', stdio: ['ignore', 'pipe', 'ignore'] });
    } catch {
      return JSON.stringify({ allow: [] }); // the list is new on this branch
    }
  })();
  const before = new Set(parseAllow(base).map((e) => e.id));
  for (const e of parseAllow(readFileSync(ALLOW_FILE, 'utf8')).filter((x) => !before.has(x.id))) {
    assert.equal(e.approvedBy, 'user', `${e.id} is new against origin/main: only the user approves a new exception (ER-46)`);
  }
});
