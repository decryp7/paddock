import assert from 'node:assert/strict';
import { test } from 'node:test';
import { e2ePort } from './env.ts';

test('the port comes from the environment, else the local default', () => {
  assert.equal(e2ePort('PW_PORT', 4300, { PW_PORT: '4601' }), 4601);
  assert.equal(e2ePort('PW_PORT', 4300, {}), 4300);
  assert.equal(e2ePort('PW_PORT', 4300, { PW_PORT: '' }), 4300);
});

test('CI must name its port, and a port must be one', () => {
  assert.throws(() => e2ePort('PW_PORT', 4300, { CI: 'true' }), /CI: set PW_PORT/);
  assert.equal(e2ePort('PW_PORT', 4300, { CI: 'true', PW_PORT: '4700' }), 4700);
  for (const bad of ['x', '0', '70000', '43.5']) assert.throws(() => e2ePort('PW_PORT', 4300, { PW_PORT: bad }), /is not a port/);
});
