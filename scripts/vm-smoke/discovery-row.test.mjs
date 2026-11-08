import test from 'node:test';
import assert from 'node:assert/strict';
import { judgeDiscovery, discoveryStep, pidOf, TITLE_DISCOVERY } from './discovery-row.mjs';

const ok = { livePid: 10, startBefore: '10@a', startAfter: '10@a', deleted: { rewritten: true, ms: 5000, pid: 10 }, foreign: { rewritten: true, ms: 12000, pid: 10 }, statusOk: true };
test('pidOf', () => { assert.equal(pidOf('{"pid": 7}'), 7); assert.equal(pidOf('nope'), null); });
test('rewritten in time with the live pid passes', () => assert.ok(judgeDiscovery(ok).every((c) => c.ok)));
test('too slow, wrong pid, restart and a dead status each fail', () => {
  assert.ok(judgeDiscovery({ ...ok, deleted: { rewritten: true, ms: 50000, pid: 10 } }).some((c) => !c.ok));
  assert.ok(judgeDiscovery({ ...ok, foreign: { rewritten: true, ms: 1000, pid: 99 } }).some((c) => !c.ok));
  assert.ok(judgeDiscovery({ ...ok, startAfter: '11@b' }).some((c) => !c.ok));
  assert.ok(judgeDiscovery({ ...ok, statusOk: false }).some((c) => !c.ok));
});
test('no live pid is a broken check', () => assert.ok(judgeDiscovery({ livePid: null }).every((c) => c.kind === 'check')));
test('the step against a fake agent that rewrites the file', async () => {
  let file = '{"pid": 10, "token": "x"}', t = 0, pending = null;
  const p = {
    now: () => t, sleep: async (ms) => { t += ms; if (pending && t >= pending.at) { file = '{"pid": 10, "token": "x"}'; pending = null; } },
    readFile: async () => file, deleteFile: async () => { file = ''; pending = { at: t + 6000 }; }, writeFile: async (x) => { file = x; pending = { at: t + 6000 }; },
    agent: async () => ({ pid: 10, start: 'a' }), statusOk: async () => true,
  };
  const r = await discoveryStep(p);
  assert.equal(r.title, TITLE_DISCOVERY); assert.equal(r.status, 'pass');
});
test('an agent that never rewrites fails', async () => {
  let file = '{"pid": 10}', t = 0;
  const p = { now: () => t, sleep: async (ms) => { t += ms; }, readFile: async () => file, deleteFile: async () => { file = ''; }, writeFile: async (x) => { file = x; }, agent: async () => ({ pid: 10, start: 'a' }), statusOk: async () => true };
  assert.equal((await discoveryStep(p)).status, 'fail');
});
