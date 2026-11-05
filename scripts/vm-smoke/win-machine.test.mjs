import test from 'node:test';
import assert from 'node:assert/strict';
import { exitCodeOf, semverOf, judgeUninstall } from './win-machine.mjs';

test('exitCodeOf reads the last EXIT line', () => {
  assert.equal(exitCodeOf('not newer\nEXIT=27\n'), 27);
  assert.equal(exitCodeOf('no exit line'), null);
});
test('semverOf takes the first three numbers', () => {
  assert.equal(semverOf('1.1.0.0\r\n'), '1.1.0');
  assert.equal(semverOf('x'), null);
});
test('judgeUninstall fails on a left folder and passes on a clean machine', () => {
  const clean = { folder: false, folderFiles: '', uninstallKey: false, service: false, processes: 0, runValues: '' };
  assert.ok(judgeUninstall(clean).every((c) => c.ok));
  const bad = judgeUninstall({ ...clean, folder: true, folderFiles: 'dyno-app.exe', runValues: 'Dyno' });
  assert.equal(bad.filter((c) => !c.ok).length, 2);
});
