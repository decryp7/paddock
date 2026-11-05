import assert from 'node:assert/strict';
import { mkdtempSync, readdirSync, readFileSync, rmSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { test } from 'node:test';
import { RELEASE_RUNS, finalizeRun, foundFromGrep, foundFromPs, judgeRelease, psHasCommit, resultFolder, sidecar, withBuildStep } from './report.mjs';
import { EXIT } from './verdict.mjs';

const passing = (label, extra = {}) => ({ label, os: RELEASE_RUNS.find((r) => r.label === label).os, ciRun: '7', installerSha256: `h-${label.split('-')[0]}`, partial: false, verdict: 'PASS', missing: [], failed: [], broken: [], ...extra });
const all = () => RELEASE_RUNS.map((r) => passing(r.label));

test('a release is PASS only with every run present, passing, of one CI run and one installer per OS', () => {
  assert.equal(judgeRelease(all()).verdict, 'PASS');
});
test('dead state: chain7550 without Linux --reboot-check and the macOS power suite is INCOMPLETE and names them', () => {
  const r = judgeRelease(all().filter((s) => !['linux-reboot', 'macos-power', 'macos-start'].includes(s.label)));
  assert.equal(r.verdict, 'INCOMPLETE');
  assert.equal(r.exit, EXIT.INCOMPLETE);
  for (const l of ['linux-reboot', 'macos-power', 'macos-start']) assert.ok(r.problems.some((p) => p.startsWith(`${l}:`)), l);
});
test('dead state: an empty folder is INCOMPLETE', () => assert.equal(judgeRelease([]).verdict, 'INCOMPLETE'));
test('a partial run, an INCOMPLETE run, mixed CI runs and mixed installers all stop a PASS; a FAIL is FAIL', () => {
  assert.equal(judgeRelease([...all().slice(1), passing('windows-ui', { partial: true })]).verdict, 'INCOMPLETE');
  assert.equal(judgeRelease([...all().slice(1), passing('windows-ui', { verdict: 'INCOMPLETE', missing: [{ id: 'x' }] })]).verdict, 'INCOMPLETE');
  assert.equal(judgeRelease([...all().slice(1), passing('windows-ui', { ciRun: '8' })]).verdict, 'INCOMPLETE');
  assert.equal(judgeRelease([...all().slice(1), passing('macos-start', { installerSha256: 'other' })]).verdict, 'INCOMPLETE');
  const f = judgeRelease([...all().slice(1), passing('windows-ui', { verdict: 'FAIL', failed: ['Quit'] })]);
  assert.equal(f.verdict, 'FAIL');
  assert.equal(f.exit, EXIT.FAIL);
});
test('finalizeRun writes md and json into a folder named after the CI run and exits by the verdict', () => {
  const out = mkdtempSync(join(tmpdir(), 'vmreport-'));
  try {
    const run = { app: 'Dyno', steps: [{ title: 'A', status: 'not-measured', summary: 'x', checks: [] }], required: [{ id: 'a', match: /^A$/ }], installerSha256: 'ab'.repeat(32) };
    const r = finalizeRun({ out, run, meta: { label: 'macos-start', os: 'macos', run: '42' }, started: '2026-11-05T10:00:00.000Z' });
    assert.equal(r.exit, EXIT.INCOMPLETE);
    assert.ok(r.file.includes(`${out}/run-42/`));
    assert.deepEqual(readdirSync(join(out, 'run-42')).map((f) => f.split('.').pop()).sort(), ['json', 'md']);
    assert.equal(JSON.parse(readFileSync(r.file.replace(/\.md$/, '.json'), 'utf8')).verdict, 'INCOMPLETE');
    assert.equal(resultFolder('/o', { installerSha256: 'abcdef0123456789' }), '/o/local-abcdef012345');
    assert.equal(sidecar(run, { label: 'l', os: 'macos' }).missing[0].id, 'a');
  } finally { rmSync(out, { recursive: true, force: true }); }
});

test('the build binding probes read found, not found and unreadable apart', () => {
  assert.deepEqual([0, 1, 2, 127].map(foundFromGrep), [true, false, null, null]);
  assert.deepEqual(['True', 'False', 'unreadable', ''].map(foundFromPs), [true, false, null, null]);
  assert.match(psHasCommit('C:\\x\\dyno-app.exe', 'abcdef012345'), /Contains\('abcdef012345'\)/);
  assert.deepEqual(withBuildStep([{ title: 'Install' }, { title: 'Cold' }], { title: 'B' }).map((s) => s.title), ['Install', 'B', 'Cold']);
});
