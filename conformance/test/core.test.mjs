import assert from 'node:assert/strict';
import { readFileSync, writeFileSync } from 'node:fs';
import { join } from 'node:path';
import { test } from 'node:test';
import { main } from '../src/cli.mjs';
import { formatMarkdown } from '../src/report.mjs';
import { runChecks } from '../src/runner.mjs';
import { parseJobs, parseSteps } from '../src/util/workflows.mjs';
import { applyWaivers, parseWaivers } from '../src/util/waivers.mjs';
import { pathMatches } from '../src/util/fs.mjs';
import { makeApp } from './helpers.mjs';

const WF = `name: x
on: push
jobs:
  test:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
      - name: Tests
        env:
          A: "1"
        run: |
          cargo test
          echo done
  bundle:
    needs: [test]
    steps:
      - run: echo b
  release:
    needs:
      - test
      - bundle
    steps:
      - name: Publish
        run: node scripts/release/publish.mjs
`;

test('workflow reader: jobs with needs in all three spellings, steps with env and run blocks', () => {
  const jobs = parseJobs(WF);
  assert.deepEqual(jobs.map((j) => [j.id, j.needs]), [['test', []], ['bundle', ['test']], ['release', ['test', 'bundle']]]);
  const steps = parseSteps(WF);
  assert.equal(steps.length, 4);
  assert.equal(steps[0].uses, 'actions/checkout@v4');
  assert.deepEqual(steps[1].env, { A: '"1"' });
  assert.equal(steps[1].run, 'cargo test\necho done');
  assert.equal(steps[1].line, 8);
});

test('waivers: parse, match by prefix, suppress a finding', () => {
  const { waivers, problems } = parseWaivers('# c\n[[waiver]]\ncheck = "C16"\npath = "app/mock/*"\nreason = "parser registry"\nexpires = "2099-01-01"\n');
  assert.deepEqual(problems, []);
  const finding = { check: 'C16', severity: 'error', file: 'app/mock/a.ts', message: 'm' };
  const r = applyWaivers({ findings: [finding, { ...finding, file: 'app/other.ts' }], waivers, pathMatches });
  assert.equal(r.waived.length, 1);
  assert.equal(r.waived[0].waiver.reason, 'parser registry');
  assert.equal(r.findings.length, 1);
  assert.equal(pathMatches('a/*', 'a/b/c'), true);
  assert.equal(pathMatches('a/b', 'a/bc'), false);
});

test('waivers: an expired waiver fails the build and waives nothing; a missing reason fails too', () => {
  const finding = { check: 'C16', severity: 'error', file: 'a.ts', message: 'm' };
  const old = parseWaivers('[[waiver]]\ncheck = "C16"\npath = "a.ts"\nreason = "later"\nexpires = "2026-01-01"\n');
  const r = applyWaivers({ findings: [finding], ...old, now: new Date('2026-10-24T00:00:00Z'), pathMatches });
  assert.deepEqual(r.findings.map((f) => f.check).sort(), ['C16', 'W001']);
  assert.equal(r.waived.length, 0);
  const noReason = parseWaivers('[[waiver]]\ncheck = "C16"\npath = "a.ts"\nreason = ""\nexpires = "2099-01-01"\n');
  assert.deepEqual(applyWaivers({ findings: [finding], ...noReason, pathMatches }).findings.map((f) => f.check).sort(), ['C16', 'W002']);
  const junk = parseWaivers('check = "C16"\n');
  assert.equal(applyWaivers({ findings: [], ...junk, pathMatches }).findings[0].check, 'W002');
});

const app = {
  'app/src-tauri/tauri.conf.json': JSON.stringify({ version: '1.0.0' }),
  'app/src/a.spec.ts': "expect(appInfo.version).toBe('1.0.0');\n",
};

test('runner: a waiver in the app removes the finding; an expired one brings it back with W001', async () => {
  const live = makeApp({ ...app, 'paddock.waivers.toml': '[[waiver]]\ncheck = "C16"\npath = "app/src/*"\nreason = "known"\nexpires = "2099-01-01"\n' });
  const r = await runChecks({ root: live, app: 'demo', only: ['C16'] });
  assert.equal(r.errors, 0);
  assert.equal(r.waived.length, 1);
  const dead = makeApp({ ...app, 'paddock.waivers.toml': '[[waiver]]\ncheck = "C16"\npath = "app/src/*"\nreason = "known"\nexpires = "2020-01-01"\n' });
  const d = await runChecks({ root: dead, app: 'demo', only: ['C16'] });
  assert.deepEqual(d.findings.map((f) => f.check).sort(), ['C16', 'W001']);
});

test('runner: a check that throws is an error finding, unknown ids are refused, the app name is required', async () => {
  const boom = { id: 'C99', title: 't', rule: 1, run() { throw new Error('kaput'); } };
  const r = await runChecks({ root: makeApp({ 'a': 'x' }), app: 'demo', registry: [boom] });
  assert.match(r.findings[0].message, /the check crashed: kaput/);
  await assert.rejects(runChecks({ root: makeApp({ a: 'x' }), app: 'demo', only: ['C77'] }), /unknown check/);
  await assert.rejects(runChecks({ root: makeApp({ a: 'x' }) }), /app name is needed/);
});

test('report: markdown table with the fix, and the passed form', async () => {
  const r = await runChecks({ root: makeApp(app), app: 'demo', only: ['C16'] });
  const md = formatMarkdown(r);
  assert.match(md, /^## Conformance: demo/);
  assert.match(md, /\| error \| C16 \| 6 \| `app\/src\/a\.spec\.ts:1` \|/);
  assert.match(formatMarkdown({ app: 'x', checks: ['C03'], findings: [], waived: [], errors: 0, warnings: 0 }), /All checks passed/);
});

test('cli: exit status, json, out file, job summary file, only, list, usage', async () => {
  const root = makeApp(app);
  const out = join(root, 'conformance.json');
  const summaryEnv = join(root, 'step-summary.md');
  writeFileSync(summaryEnv, '');
  const lines = [];
  const io = { log: (l) => lines.push(l), err: (l) => lines.push(l), env: { GITHUB_STEP_SUMMARY: summaryEnv } };
  assert.equal(await main(['--app', 'demo', '--root', root, '--only', 'c16', '--json', '--out', out], io), 1);
  assert.equal(JSON.parse(readFileSync(out, 'utf8')).errors, 1);
  assert.match(readFileSync(summaryEnv, 'utf8'), /Conformance: demo/);
  assert.equal(JSON.parse(lines[0]).app, 'demo');
  const clean = makeApp({ 'app/src-tauri/tauri.conf.json': JSON.stringify({ version: '1.0.0' }) });
  assert.equal(await main(['--app', 'demo', '--root', clean, '--only', 'C16'], io), 0);
  assert.equal(await main(['list'], io), 0);
  assert.equal(await main([], io), 2);
  assert.equal(await main(['--bogus'], io), 2);
  assert.equal(await main(['--app', 'demo', '--only', 'C77', '--root', clean], io), 2);
  assert.equal(await main(['run', 'echo'], io), 2);
});

test('cli run: the hermetic wrapper passes the exit status through', async () => {
  const io = { log() {}, err() {}, env: {} };
  assert.equal(await main(['run', '--', process.execPath, '-e', 'process.exit(0)'], io), 0);
  assert.equal(await main(['run', '--', process.execPath, '-e', 'process.exit(3)'], io), 3);
});

test('a waiver of a check that ran and matched nothing is a W003 warning; one of a check that did not run is silent', async () => {
  const root = makeApp({ 'paddock.waivers.toml': '[[waiver]]\ncheck = "C46"\npath = "apps/gone/*"\nreason = "was needed"\nexpires = "2099-01-01"\n' });
  const r = await runChecks({ root, app: 'demo', only: ['C46'] });
  assert.deepEqual(r.findings.map((f) => [f.check, f.severity]), [['W003', 'warn']]);
  const quiet = await runChecks({ root, app: 'demo', only: ['C06'] });
  assert.ok(!quiet.findings.some((f) => f.check === 'W003'));
});
