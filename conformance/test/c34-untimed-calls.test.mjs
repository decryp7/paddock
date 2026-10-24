import assert from 'node:assert/strict';
import { test } from 'node:test';
import { untimedPolicies } from '../src/checks/C34-untimed-calls.mjs';
import { findingsOf, makeApp, messages } from './helpers.mjs';

const backend = (policy) => `import { withCallTimeout } from '@decryp7/paddock-ui/ipc';\nexport const b = withCallTimeout<AppBackend>(new TauriBackend(), ${policy});\n`;
const app = (policy, config = {}) =>
  makeApp({ 'app/src/app/core/backend.ts': backend(policy), 'paddock.conformance.json': JSON.stringify({ app: 'demo', ...config }) });

test('untimedPolicies reads literal lists, multi-line lists and policies without one', () => {
  assert.deepEqual(untimedPolicies(backend("{ subject: 'S', untimed: ['a', \"b\"] }")), [{ line: 2, untimed: ['a', 'b'] }]);
  assert.deepEqual(untimedPolicies(backend("{ subject: 'S', untimed: [\n 'a',\n 'b',\n ], limits: { c: 5 } }"))[0].untimed, ['a', 'b']);
  assert.deepEqual(untimedPolicies(backend("{ subject: 'S' }"))[0].untimed, []);
  assert.equal(untimedPolicies(backend('{ subject: S, untimed: JOBS }'))[0].untimed, null);
  assert.equal(untimedPolicies(backend('{ subject: S, untimed: [...JOBS, "a"] }'))[0].untimed, null);
  assert.deepEqual(untimedPolicies('const x = 1;'), []);
  assert.deepEqual(untimedPolicies('withCallTimeout(unclosed'), []);
});

test('C34 good: every untimed entry is declared with a reason', async () => {
  const f = await findingsOf('C34', app("{ subject: 'S', untimed: ['startImport', 'chooseFolder'] }", { untimedCommands: { startImport: 'starts a job; events report it', chooseFolder: 'waits for the user in a dialog' } }));
  assert.deepEqual(f, []);
  assert.deepEqual(await findingsOf('C34', app("{ subject: 'S', limits: { slow: 60000 } }")), []);
});

test('C34 bad: undeclared, no reason, stale declaration, not a literal', async () => {
  const undeclared = await findingsOf('C34', app("{ subject: 'S', untimed: ['loadModel'] }"));
  assert.equal(undeclared[0].severity, 'error');
  assert.match(messages(undeclared), /loadModel is untimed but not declared/);
  assert.equal(undeclared[0].file, 'app/src/app/core/backend.ts');
  const noReason = await findingsOf('C34', app("{ subject: 'S', untimed: ['a'] }", { untimedCommands: { a: 'x' } }));
  assert.match(messages(noReason), /needs a reason/);
  const stale = await findingsOf('C34', app("{ subject: 'S' }", { untimedCommands: { gone: 'starts a job once upon a time' } }));
  assert.equal(stale[0].severity, 'warn');
  assert.match(messages(stale), /gone is declared but no withCallTimeout policy lists it/);
  const dynamic = await findingsOf('C34', app('{ subject: S, untimed: JOBS }'));
  assert.equal(dynamic[0].severity, 'warn');
  assert.match(messages(dynamic), /not a literal array/);
});

test('C34 ignores specs and files outside the app', async () => {
  const policy = "{ subject: 'S', untimed: ['x'] }";
  const f = await findingsOf('C34', makeApp({ 'app/src/a.spec.ts': backend(policy), 'crates/x/a.ts': backend(policy), 'paddock.conformance.json': JSON.stringify({ app: 'demo' }) }));
  assert.deepEqual(f, []);
});
