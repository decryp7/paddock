import { test } from 'node:test';
import assert from 'node:assert/strict';
import { spawnSync } from 'node:child_process';
import { readFileSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';
import { baselineGrowth } from '../conformance/src/util/baseline.mjs';
import { ALLOW_FILE, MAX_DAYS, advisories, judge, measure, parseAllow } from './npm-audit.mjs';

const here = dirname(fileURLToPath(import.meta.url));
// The cases the Rust reader (tools/paddock-dev/src/audit/tests.rs) answers too: one file, so the two cannot drift (rule 55).
const fixtures = JSON.parse(readFileSync(join(here, 'npm-audit-fixtures.json'), 'utf8'));
const textOf = (c) => c.raw ?? JSON.stringify(c.report ?? c.doc);
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


for (const c of fixtures.reports) {
  test(`report fixture: ${c.name}`, () => {
    const m = measure(textOf(c));
    if (c.notMeasured !== undefined) {
      assert.ok(m.notMeasured?.includes(c.notMeasured), `not measured with "${c.notMeasured}", got ${JSON.stringify(m)}`);
    } else {
      assert.equal(m.notMeasured, undefined);
      assert.deepEqual(advisories(m.report).map((a) => a.id), c.ids);
    }
  });
}

for (const c of fixtures.allow) {
  test(`allow fixture: ${c.name}`, () => {
    if (c.error !== undefined) assert.throws(() => parseAllow(textOf(c)), (e) => e.message.includes(c.error), `refused with "${c.error}"`);
    else assert.deepEqual(parseAllow(textOf(c)).map((e) => e.id), c.ids);
  });
}

test('the fixtures name the cases the issue lists: missing key, error object, zero dependencies, unknown severity, the 90-day cap', () => {
  const names = fixtures.reports.map((c) => c.name).join('|') + fixtures.allow.map((c) => c.name).join('|');
  for (const word of ['no vulnerabilities key', 'error object', 'zero dependencies', 'unknown severity', '91 days']) assert.ok(names.includes(word), word);
  assert.equal(MAX_DAYS, 90);
});

const run = (args, { input, env } = {}) =>
  spawnSync(process.execPath, [join(here, 'npm-audit.mjs'), ...args], { encoding: 'utf8', input, env: env ?? process.env });

test('the command: every "did not look" report exits 6 with one ::error:: line, on the --report path', () => {
  for (const c of fixtures.reports.filter((x) => x.notMeasured !== undefined)) {
    const r = run(['--report', '-', '--today', '2026-11-20'], { input: textOf(c) });
    assert.equal(r.status, 6, `${c.name}: ${r.stdout}${r.stderr}`);
    const errors = (r.stdout + r.stderr).split('\n').filter((l) => l.startsWith('::error::'));
    assert.equal(errors.length, 1, c.name);
    assert.ok(errors[0].includes(c.notMeasured) && errors[0].includes('Next step'), errors[0]);
  }
});

test('the command: a clean report passes (the positive control of the not-measured cases)', () => {
  const clean = fixtures.reports.find((c) => c.ids?.length === 0);
  const r = run(['--report', '-', '--today', '2026-11-20'], { input: textOf(clean) });
  assert.equal(r.status, 0, r.stdout + r.stderr);
});

test('the command: a finding exits 1 with one ::error:: per finding', () => {
  const r = run(['--report', '-', '--today', '2026-11-20'], { input: JSON.stringify(report('high', 'GHSA-zzzz-zzzz-zzzz')) });
  assert.equal(r.status, 1, r.stdout);
  assert.match(r.stdout, /::error::npm-audit: GHSA-zzzz-zzzz-zzzz high in picomatch/);
});

test('the command: npm that cannot start is exit 6 with an ::error::, not a crash', () => {
  const r = run([], { env: { ...process.env, PATH: '' } });
  assert.equal(r.status, 6, r.stdout + r.stderr);
  assert.match(r.stderr, /::error::npm-audit: npm could not run/);
  assert.doesNotMatch(r.stderr, /TypeError/);
});

test('the command: a report file that cannot be read is exit 6 with an ::error::', () => {
  const r = run(['--report', join(here, 'no-such-report.json')]);
  assert.equal(r.status, 6);
  assert.match(r.stderr, /::error::npm-audit: the report .* cannot be read/);
});

// ER-46: the list grows only with the user's approval. The comparison is the conformance kit's own `baselineGrowth` (the one C53, C115
// and the allow lists use), against the merge base and not origin/main's tip. Growth is a new id, a later review day or a changed
// approval day, so renewing an exception is growth too. A base that cannot be read is not measured, never an empty list.
const keyOf = (e) => `${e.id}\u0000${e.approved}\u0000${e.reviewBy}`;
const ALLOW_PATH = 'scripts/npm-audit-allow.json';
function growth(currentText, git) {
  return baselineGrowth({
    root: here,
    path: ALLOW_PATH,
    current: parseAllow(currentText),
    keyOf,
    entriesOf: (json) => parseAllow(JSON.stringify(json)),
    git,
    baseRef: 'origin/main',
  });
}
const listText = (...es) => JSON.stringify({ allow: es });
const fakeGit = (baseText) => (args) => {
  if (args[0] === 'rev-parse' && args[1] === 'HEAD') return 'headsha\n';
  if (args[0] === 'merge-base') return 'basesha\n';
  if (args[0] === 'show') {
    if (baseText === null) throw Object.assign(new Error('fatal: path does not exist in basesha'), { stderr: 'fatal: path does not exist in basesha' });
    return baseText;
  }
  throw new Error(`unexpected git ${args.join(' ')}`);
};

test('ER-46 control: the growth comparison sees a new id, a later review day and a changed approval day', () => {
  const base = listText(entry());
  assert.deepEqual(growth(base, fakeGit(base)).added, [], 'the same list is no growth');
  assert.equal(growth(listText(entry(), entry({ id: 'GHSA-zzzz-zzzz-zzzz' })), fakeGit(base)).added.length, 1, 'a new id');
  assert.equal(growth(listText(entry({ reviewBy: '2027-01-10' })), fakeGit(base)).added.length, 1, 'a later review day');
  assert.equal(growth(listText(entry({ approved: '2026-11-19' })), fakeGit(base)).added.length, 1, 'a changed approval day');
  assert.deepEqual(growth(listText(), fakeGit(base)).added, [], 'removing an entry is shrinking');
  assert.equal(growth(listText(entry({ approvedBy: 'user' })), fakeGit(listText(entry({ approvedBy: 'someone' })))).added.length, 0, 'the approver string is not the signal');
});

test('ER-46 control: a base that cannot be found is not measured, never an empty list', () => {
  const noBase = () => {
    throw new Error('fatal: Not a valid object name origin/main');
  };
  const g = growth(listText(entry()), noBase);
  assert.equal(g.kind, 'no-base');
  assert.ok(g.reason);
  assert.equal(g.added, undefined);
});

test('ER-46: the list on this branch has no entry the merge base lacks (growth needs the user, see conformance/src/util/allow-list.mjs)', () => {
  const g = growth(readFileSync(ALLOW_FILE, 'utf8'));
  assert.equal(g.reason, undefined, `not measured: ${g.reason}. Next step: fetch the history and origin/main (fetch-depth 0); a base that cannot be read never passes (ADR 0089).`);
  assert.deepEqual(g.added.map((e) => e.id), [], `${ALLOW_PATH} grew against the merge base ${g.base?.slice(0, 9)}: a new exception, a later review day or a changed approval day needs the user's approval (ER-46); remove it or ask.`);
});
