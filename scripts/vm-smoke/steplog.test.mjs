import assert from 'node:assert/strict';
import { test } from 'node:test';
import { makeSteps } from './steplog.mjs';

test('a step logs "start <label>" before it runs and its status with the elapsed seconds after', async () => {
  const lines = [];
  const steps = [];
  let t = 0;
  const { step } = makeSteps(steps, (l) => lines.push(l), () => t);
  const s = await step('cold start', async () => {
    lines.push('(running)');
    t = 12_400;
    return { status: 'pass', title: 'Cold start', summary: 'ok' };
  });
  assert.deepEqual(lines, ['start cold start', '(running)', 'PASS  Cold start: ok (12 s)']);
  assert.deepEqual(steps, [s]);
});

test('a step that fails is still recorded and logged', async () => {
  const lines = [];
  const steps = [];
  const { step } = makeSteps(steps, (l) => lines.push(l), () => 0);
  await step('x', () => ({ status: 'fail', title: 'X', summary: 'bad' }));
  assert.equal(steps.length, 1);
  assert.equal(lines[1], 'FAIL  X: bad (0 s)');
});
