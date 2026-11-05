import test from 'node:test';
import assert from 'node:assert/strict';
import { judgeHelperLog } from './mac-helper-log.mjs';

test('a start and a clean stop pass', () => {
  const j = judgeHelperLog('2026-11-05 10:00:00 dyno-helper[12] DynoHelper is starting\n2026-11-05 10:00:09 dyno-helper[12] DynoHelper stopped cleanly after 9000 ms\n');
  assert.ok(j.checks.every((c) => c.ok));
});
test('a start with an exit-code line passes', () => {
  const j = judgeHelperLog('x DynoHelper is starting\nx DynoHelper stopped after 5 ms because it panicked (exit code 1000)\n');
  assert.ok(j.checks.every((c) => c.ok));
});
test('no lines fail both checks', () => {
  assert.equal(judgeHelperLog('nothing here').checks.filter((c) => !c.ok).length, 2);
});
