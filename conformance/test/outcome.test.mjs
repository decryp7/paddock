import test from 'node:test';
import assert from 'node:assert/strict';
import { runChecks } from '../src/runner.mjs';
import { error, warn, unwaivable } from '../src/finding.mjs';
import { exitStatus, EXIT } from '../src/outcome.mjs';
import { main } from '../src/cli.mjs';
import { makeApp } from './helpers.mjs';

const waiver = (id) => `[[waiver]]\ncheck = "${id}"\npath = "*"\nreason = "plant"\nexpires = "2099-01-01"\n`;
const run = (check, extra = {}) => runChecks({ root: makeApp({ a: 'x', 'paddock.waivers.toml': waiver('C00'), ...extra }), app: 'demo', registry: [{ id: 'C00', title: 't', rule: 1, run: check }] });

test('a throw of any shape is a crash finding, never a rejected run (P2-1)', async () => {
  for (const thrown of [undefined, null, 'boom', 42, {}, { message: 'kaput' }]) {
    const r = await run(() => { throw thrown; });
    const f = r.findings.find((x) => x.check === 'C00');
    assert.ok(f, `thrown ${String(thrown)}`);
    assert.match(f.message, /the check crashed: /);
    assert.equal(f.unwaivable, true);
    assert.equal(f.outcome, 'check-broken');
  }
  assert.match((await run(() => { throw undefined; })).findings[0].message, /undefined/);
  assert.match((await run(() => { throw { message: 'kaput' }; })).findings[0].message, /kaput/);
});

test('a malformed finding is an unwaivable check-broken finding (P2-2, P2-3)', async () => {
  const bad = [[null], [42], ['text'], [{ check: 'C00', severity: 'info', message: 'm' }], [{ check: 'C00', severity: 'error' }], [{ check: 'C00', severity: 'warn', message: 7 }]];
  for (const result of bad) {
    const r = await run(() => result);
    const f = r.findings.find((x) => x.check === 'C00');
    assert.ok(f, JSON.stringify(result));
    assert.equal(f.unwaivable, true, JSON.stringify(result));
    assert.equal(f.outcome, 'check-broken');
    assert.match(f.message, /check broken: .*invalid finding/);
    assert.equal(r.waived.length, 0);
  }
  const ok = await run(() => [error('C00', 1, 'a', 1, 'm', 'f'), warn('C00', 1, 'a', 1, 'm', 'f')]);
  assert.equal(ok.waived.length, 2, 'valid findings are still waivable');
});

test('a waiver for a crashed or incomplete check is not reported as stale (P2-4)', async () => {
  for (const check of [() => { throw new Error('x'); }, () => undefined]) {
    const r = await run(check);
    assert.equal(r.findings.some((f) => f.check === 'W003'), false);
  }
  const clean = await run(() => []);
  assert.equal(clean.findings.some((f) => f.check === 'W003'), true, 'the probe still sees a stale waiver on a clean run');
});

test('exit status: 7 broken, 1 findings, 6 not measured, 0 clean, whatever the waivers say (ADR 0089 section 6)', async () => {
  assert.deepEqual(EXIT, { clean: 0, findings: 1, notMeasured: 6, checkBroken: 7 });
  assert.equal(exitStatus(await run(() => { throw new Error('x'); })), 7);
  assert.equal(exitStatus(await run(() => undefined)), 6);
  assert.equal(exitStatus(await run(() => [])), 0);
  assert.equal(exitStatus(await run(() => [error('C00', 1, 'a', 1, 'm', 'f')])), 0, 'a waived finding is waived');
  assert.equal(exitStatus(await runChecks({ root: makeApp({ a: 'x' }), app: 'demo', registry: [{ id: 'C00', title: 't', rule: 1, run: () => [error('C00', 1, 'a', 1, 'm', 'f')] }] })), 1);
});

test('exit status over every combination of findings (property)', () => {
  const kinds = [
    ['ok', []],
    ['err', [error('C1', 1, 'a', 1, 'm', 'f')]],
    ['warn', [warn('C1', 1, 'a', 1, 'm', 'f')]],
    ['nm', [unwaivable(error('C2', 1, '.', 1, 'not measured: x', 'f'), 'not-measured')]],
    ['broken', [unwaivable(error('C3', 1, '.', 1, 'check broken: x', 'f'))]],
  ];
  let combos = 0;
  const go = (names, findings, left) => {
    combos += 1;
    const has = (n) => names.includes(n);
    const want = has('broken') ? 7 : has('err') ? 1 : has('nm') ? 6 : 0;
    assert.equal(exitStatus({ findings, errors: findings.filter((f) => f.severity === 'error').length, warnings: 0 }), want, names.join());
    if (!left) return;
    for (const [n, f] of kinds) go([...names, n], [...findings, ...f], left - 1);
  };
  go([], [], 4);
  const warnOnly = { findings: [warn('C1', 1, 'a', 1, 'm', 'f')] };
  assert.equal(exitStatus(warnOnly), 0, 'warnings are report-only');
  assert.equal(exitStatus(warnOnly, new Set(['C1'])), 1, '--strict makes them fail');
  assert.equal(exitStatus({ findings: [...warnOnly.findings, ...kinds[3][1]] }), 6, 'a warning never masks a missing measurement');
  assert.equal(combos, 1 + 5 + 25 + 125 + 625);
});

test('main(): a crashing check exits 7 and an incomplete one 6, whatever the waivers say (CLI level)', async () => {
  const root = makeApp({ a: 'x', 'paddock.waivers.toml': waiver('C00') });
  const go = (check) => main(['--root', root, '--app', 'demo'], { log: () => {}, err: () => {}, registry: [{ id: 'C00', title: 't', rule: 1, run: check }] });
  assert.equal(await go(() => { throw new Error('x'); }), 7);
  assert.equal(await go(() => undefined), 6);
  assert.equal(await go(() => []), 0);
  assert.equal(await go(() => [error('C00', 1, 'a', 1, 'm', 'f')]), 0, 'waived');
  const open = makeApp({ a: 'x' });
  assert.equal(await main(['--root', open, '--app', 'demo'], { log: () => {}, err: () => {}, registry: [{ id: 'C00', title: 't', rule: 1, run: () => [error('C00', 1, 'a', 1, 'm', 'f')] }] }), 1);
});
