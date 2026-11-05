import assert from 'node:assert/strict';
import { test } from 'node:test';
import { EXIT, buildStep, classifyInstallFailure, deriveVerdict, exitCodeFor, makeResult, stepClass } from './verdict.mjs';
import { renderResult } from './judge.mjs';
import { requiredSteps } from './manifest.mjs';

const REQ = [{ id: 'a', match: /^A$/ }, { id: 'b', match: /^B$/ }];
const st = (title, status, extra = {}) => ({ title, status, summary: 's', checks: [], ...extra });

test('all required steps passing is PASS', () => {
  const v = deriveVerdict({ steps: [st('A', 'pass'), st('B', 'pass')], required: REQ });
  assert.equal(v.verdict, 'PASS');
  assert.equal(exitCodeFor(v), EXIT.PASS);
});
test('dead state: only NOT MEASURED and SKIP steps is INCOMPLETE, never PASS', () => {
  const v = deriveVerdict({ steps: [st('A', 'not-measured'), st('B', 'skip')], required: REQ });
  assert.equal(v.verdict, 'INCOMPLETE');
  assert.deepEqual(v.missing.map((m) => m.id), ['a', 'b']);
  assert.equal(exitCodeFor(v), EXIT.INCOMPLETE);
  assert.notEqual(EXIT.INCOMPLETE, EXIT.FAIL);
  assert.notEqual(EXIT.INCOMPLETE, EXIT.ERROR);
});
test('dead state: zero steps is INCOMPLETE', () => {
  assert.equal(deriveVerdict({ steps: [], required: REQ }).verdict, 'INCOMPLETE');
});
test('dead state: no declared manifest is INCOMPLETE (a run cannot pass against nothing)', () => {
  assert.equal(deriveVerdict({ steps: [st('A', 'pass')], required: [] }).verdict, 'INCOMPLETE');
});
test('a required step that never ran is listed as missing', () => {
  const v = deriveVerdict({ steps: [st('A', 'pass')], required: REQ });
  assert.equal(v.verdict, 'INCOMPLETE');
  assert.match(v.missing[0].why, /did not run/);
  assert.equal(v.missing[0].id, 'b');
});
test('a product FAIL wins over a missing step', () => {
  const v = deriveVerdict({ steps: [st('A', 'fail')], required: REQ });
  assert.equal(v.verdict, 'FAIL');
  assert.equal(exitCodeFor(v), EXIT.FAIL);
});
test('a failure of the check itself is not a product FAIL: it is INCOMPLETE', () => {
  const broken = makeResult('A', [{ ok: false, kind: 'check', detail: 'the control did not work' }]);
  assert.equal(broken.status, 'check-broken');
  assert.equal(stepClass(broken), 'check');
  const v = deriveVerdict({ steps: [broken, st('B', 'pass')], required: REQ });
  assert.equal(v.verdict, 'INCOMPLETE');
  assert.equal(v.failed.length, 0);
  assert.equal(v.broken.length, 1);
});
test('a failing check of the product and a broken control in one step is a product FAIL', () => {
  const r = makeResult('A', [{ ok: false, kind: 'check', detail: 'control' }, { ok: false, detail: 'product wrong' }]);
  assert.equal(r.status, 'fail');
});
test('makeResult: info rows never fail and a literal ok:true row does not count as a pass', () => {
  const r = makeResult('A', [{ ok: true, info: true, detail: 'fyi' }]);
  assert.equal(r.status, 'not-measured');
  assert.equal(makeResult('A', [{ ok: true, info: true, detail: 'fyi' }, { ok: true, detail: 'real' }]).status, 'pass');
});
test('partial profile: requires the steps asked for, still never an empty pass', () => {
  assert.equal(deriveVerdict({ steps: [st('A', 'pass')], required: [REQ[0]], partial: true }).verdict, 'PASS');
  assert.equal(deriveVerdict({ steps: [], required: [], partial: true }).verdict, 'INCOMPLETE');
});
test('renderResult prints the derived verdict, the missing list and never PASS for dead input', () => {
  const dead = renderResult({ app: 'D', steps: [st('A', 'not-measured'), st('B', 'skip')], required: REQ });
  assert.match(dead, /Result: \*\*INCOMPLETE\*\*/);
  assert.match(dead, /Missing: a \(/);
  assert.doesNotMatch(dead, /\*\*PASS\*\*/);
  assert.match(renderResult({ app: 'D', steps: [], required: REQ }), /Result: \*\*INCOMPLETE\*\*/);
  assert.match(renderResult({ app: 'D', steps: [st('A', 'pass'), st('B', 'pass')], required: REQ }), /Result: \*\*PASS\*\*/);
  assert.match(renderResult({ app: 'D', steps: [st('A', 'fail')], required: REQ }), /Result: \*\*FAIL\*\*/);
});
test('renderResult header carries the installer hash, its source and the build binding', () => {
  const md = renderResult({ app: 'D', steps: [st('A', 'pass')], required: [REQ[0]], installerSha256: 'ab'.repeat(32), source: 'CI run 77', expectedCommit: '0123456789ab' });
  assert.match(md, /Installer sha256: `abab/);
  assert.match(md, /Source: CI run 77/);
  assert.match(md, /Expected build commit: `0123456789ab`/);
});
test('buildStep: expected commit absent from the binary fails; unknown inputs are not measured', () => {
  assert.equal(buildStep({ expected: '0123456789ab', found: true }).status, 'pass');
  assert.equal(buildStep({ expected: '0123456789ab', found: false }).status, 'fail');
  assert.equal(buildStep({ expected: null, found: null }).status, 'not-measured');
  assert.equal(buildStep({ expected: '0123456789ab', found: null }).status, 'not-measured');
  assert.equal(buildStep({ expected: 'xyz', found: true }).status, 'not-measured');
});
test('the manifest names every suite and OS and keeps the coverage that chain7550 dropped', () => {
  const ids = (o) => requiredSteps(o).map((r) => r.id);
  assert.ok(ids({ os: 'linux', reboot: true }).some((i) => /reboot/.test(i)));
  assert.ok(ids({ os: 'linux', reboot: false, release: true }).some((i) => /reboot/.test(i)), 'a release run needs the reboot steps even without the flag');
  assert.ok(!ids({ os: 'linux', reboot: false, partial: true }).some((i) => /reboot/.test(i)));
  assert.ok(ids({ os: 'macos', suite: 'start' }).length >= 8);
  assert.ok(ids({ os: 'macos', suite: 'power' }).some((i) => /keep-awake/.test(i)));
  assert.ok(ids({ os: 'windows', ui: true, reboot: false }).some((i) => /uninstall/.test(i)));
  assert.ok(ids({ os: 'windows', reboot: true }).some((i) => /autostart/.test(i)));
  for (const os of ['windows', 'linux', 'macos']) assert.ok(ids({ os, suite: 'start' }).includes('build-binding'), `${os} build binding`);
});
test('a release run without --ui or --reboot-check is INCOMPLETE listing them (3 steps no longer print PASS)', () => {
  const steps = ['Install (per-machine NSIS, silent, UAC over VNC)', 'Cold start, one instance', 'Second launch while running'].map((t) => st(t, 'pass'));
  const v = deriveVerdict({ steps, required: requiredSteps({ os: 'windows', ui: false, reboot: false, release: true }) });
  assert.equal(v.verdict, 'INCOMPLETE');
  assert.ok(v.missing.some((m) => /uninstall/.test(m.id)));
});

test('dead state: a failed install stops the run with one class; later steps (a vacuous "Second launch" pass) are dropped and reported not measured', () => {
  const install = { title: 'Install (per-machine NSIS, silent, UAC over VNC)', status: 'fail', summary: 'exit 2', checks: [{ ok: false, kind: 'check', detail: 'precondition: a per-user Dyno is in this snapshot' }, { ok: false, detail: 'dyno-app.exe present' }] };
  const steps = classifyInstallFailure([install, st('Second launch while running', 'pass')]);
  assert.equal(steps.length, 1);
  assert.equal(steps[0].status, 'check-broken');
  const v = deriveVerdict({ steps, required: requiredSteps({ os: 'windows', ui: true }) });
  assert.equal(v.verdict, 'INCOMPLETE');
  assert.match(v.missing.find((m) => m.id === 'second-launch').why, /^not measured: the install did not succeed \(check/);
  const product = classifyInstallFailure([{ ...install, checks: [{ ok: false, detail: 'installer exit code 5' }] }, st('Cold start', 'fail')]);
  assert.equal(product.length, 1);
  assert.equal(deriveVerdict({ steps: product, required: requiredSteps({ os: 'windows', ui: true }) }).verdict, 'FAIL');
});
