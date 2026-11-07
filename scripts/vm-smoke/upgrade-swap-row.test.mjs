import assert from 'node:assert/strict';
import { test } from 'node:test';
import { judgeSwap, upgradeSwapStep, versionOf } from './upgrade-swap-row.mjs';

const base = { oldVersion: '1.0.0', newVersion: '1.1.0', helperWasOn: true, agentVersion: '1.1.0', helperVersion: '1.1.0', helperReadable: true, tookMs: 8000, waited: 8000, installedAtMs: 1000, procs: [{ name: 'dyno-agent', pid: 9, exe: '/x', startMs: 2000 }] };
const bad = (c) => c.filter((x) => !x.ok);
test('a swapped agent and helper with only new processes pass', () => assert.equal(bad(judgeSwap(base)).length, 0));
test('the old agent still answering, the old helper, or a process from before the install each fail', () => {
  assert.equal(bad(judgeSwap({ ...base, agentVersion: '1.0.0' })).length, 1);
  assert.equal(bad(judgeSwap({ ...base, helperVersion: '1.0.0' })).length, 1);
  assert.match(bad(judgeSwap({ ...base, procs: [{ name: 'dyno-agent', pid: 5, exe: '/old', startMs: 500 }] }))[0].detail, /still run/);
});
test('ER-33: same versions, no old reading or no helper before are broken checks, not passes or product fails', () => {
  for (const f of [{ ...base, oldVersion: '1.1.0' }, { ...base, oldVersion: null }, { ...base, helperWasOn: false }]) assert.ok(judgeSwap(f).every((c) => c.ok || c.kind === 'check'));
  assert.ok(bad(judgeSwap({ ...base, oldVersion: '1.1.0' })).length > 0);
});
test('an unreadable helper version is a broken check for that part only', () => assert.ok(bad(judgeSwap({ ...base, helperReadable: false, helperVersion: null })).every((c) => c.kind === 'check')));
test('the step waits up to 60 s for the swap and an old build that cannot be installed is NOT MEASURED', async () => {
  let t = 0, polls = 0;
  const p = { now: () => t, sleep: async (ms) => { t += ms; }, installOld: async () => {}, startOldWithHelper: async () => ({ oldVersion: '1.0.0', helperWasOn: true }), newVersion: async () => '1.1.0', installNew: async () => 100, openApp: async () => {},
    agentVersion: async () => (++polls > 3 ? '1.1.0' : '1.0.0'), helperVersion: async () => ({ version: '1.1.0', readable: true }), procs: async () => [] };
  assert.equal((await upgradeSwapStep(p)).status, 'pass');
  assert.equal((await upgradeSwapStep({ ...p, agentVersion: async () => '1.0.0' })).status, 'fail');
  assert.ok(t >= 60_000);
  const s = await upgradeSwapStep({ ...p, installOld: async () => { throw new Error('the older build cannot be installed in this VM'); } });
  assert.equal(s.status, 'not-measured');
  assert.match(s.summary, /cannot be installed/);
  assert.equal(versionOf('dyno-agent 1.1.0 (a19d8d288)'), '1.1.0');
});
