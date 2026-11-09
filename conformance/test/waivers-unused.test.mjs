import assert from 'node:assert/strict';
import { test } from 'node:test';
import { runChecks } from '../src/runner.mjs';
import { pathMatches } from '../src/util/fs.mjs';
import { applyWaivers, parseWaivers } from '../src/util/waivers.mjs';
import { makeApp } from './helpers.mjs';

const W = (check, path) => `[[waiver]]\ncheck = "${check}"\npath = "${path}"\nreason = "known"\nexpires = "2099-01-01"\n`;

test('waivers: one that waives nothing in a check that ran is a W003 warning; one for a check that did not run is not', () => {
  const finding = { check: 'C16', severity: 'error', file: 'app/mock/a.ts', message: 'm' };
  const parsed = parseWaivers(`${W('C16', 'app/mock/*')}\n${W('C16', 'app/gone.ts')}\n${W('C00', 'x')}`);
  const r = applyWaivers({ findings: [finding], ...parsed, pathMatches, checksRun: ['C16'] });
  assert.equal(r.waived.length, 1);
  assert.deepEqual(r.findings.map((f) => [f.check, f.severity, f.line]), [['W003', 'warn', 7]]);
  assert.match(r.findings[0].message, /C16 app\/gone\.ts waives nothing/);
  // Without the list of checks that ran (an older caller), nothing is said about unused waivers.
  assert.deepEqual(applyWaivers({ findings: [finding], ...parsed, pathMatches }).findings, []);
});

test('runner: an unused waiver is reported as a warning, never as an error', async () => {
  const root = makeApp({ 'src/a.rs': 'fn f() {}\n', 'paddock.waivers.toml': W('C54', 'src/old.rs') });
  const r = await runChecks({ root, app: 'demo', only: ['C54'] });
  assert.equal(r.errors, 0);
  assert.deepEqual(r.findings.map((f) => [f.check, f.severity]), [['W003', 'warn']]);
});
