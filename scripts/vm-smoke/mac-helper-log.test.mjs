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

test('the step asks the log only for the helper\'s own lines, so the start line cannot be pushed out of a fixed tail by noise', async () => {
  const { macHelperLogStep } = await import('./mac-helper-log.mjs');
  let cmd = '';
  const r = await macHelperLogStep({ exec: async (c) => { cmd = c; return { code: 0, stdout: '2026-11-05 15:43:16 Df dyno-helper[1246:27c4] DynoHelper is starting\n2026-11-05 15:56:33 Df dyno-helper[1246:27c4] DynoHelper stopped cleanly after 776012 ms\n', stderr: '' }; } });
  assert.equal(r.status, 'pass');
  assert.match(cmd, /--predicate 'eventMessage CONTAINS "DynoHelper"'/);
  assert.doesNotMatch(cmd, /process == "dyno-helper"|tail -80/, 'no process-wide predicate and no tail that noise can push the start line out of');
});
