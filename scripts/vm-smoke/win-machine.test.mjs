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
  const clean = { folder: 'no', folderFiles: '', uninstallKey: 'no', service: 'no', processes: 0, runMachine: 'no', runUser: 'no' };
  assert.ok(judgeUninstall(clean).every((c) => c.ok));
  const bad = judgeUninstall({ ...clean, folder: 'yes', folderFiles: 'dyno-app.exe', runUser: 'yes' });
  assert.equal(bad.filter((c) => !c.ok).length, 2);
  assert.ok(judgeUninstall({ ...clean, folder: 'unreadable' }).some((c) => !c.ok && c.kind === 'check'), 'a dead SSH is not "removed"');
});
