import assert from 'node:assert/strict';
import { test } from 'node:test';
import { judgeSwap, upgradeSwapStep, versionOf } from './upgrade-swap-row.mjs';

const base = { oldVersion: '1.1.0', helperWasOn: true, oldAgentBuild16: 'aaaa', expectedBuild16: 'bbbb', agentBuild: 'bbbb', tookMs: 9000, agentStarts: ['9@2000'], extra: [] };
const bad = (c) => c.filter((x) => !x.ok);
test('the new agent build within the wait, one agent after the boundary: pass', () => assert.equal(bad(judgeSwap(base)).length, 0));
test('the old build still in agent.json fails; two agents after the boundary is a restart loop', () => {
  assert.equal(bad(judgeSwap({ ...base, agentBuild: 'aaaa' })).length, 1);
  assert.match(bad(judgeSwap({ ...base, agentStarts: ['9@2000', '10@3000'] }))[0].detail, /restart loop/);
});
test('ER-33: same agent file, no old reading or no helper before are broken checks, never passes or product fails', () => {
  for (const f of [{ ...base, oldAgentBuild16: 'bbbb' }, { ...base, oldVersion: null }, { ...base, helperWasOn: false }, { ...base, expectedBuild16: null }]) assert.ok(judgeSwap(f).every((c) => c.ok || c.kind === 'check'));
  assert.ok(bad(judgeSwap({ ...base, oldAgentBuild16: 'bbbb' })).length > 0);
});
test('OS specific extra checks are part of the verdict', () => assert.equal(bad(judgeSwap({ ...base, extra: [{ ok: false, detail: 'the chip is missing' }] })).length, 1));
test('the step waits up to 60 s for the swap, watches for a loop and an install that fails is NOT MEASURED', async () => {
  let t = 0, built = 'aaaa';
  const p = {
    now: () => t, sleep: async (ms) => { t += ms; if (t > 6000) built = 'bbbb'; },
    installOld: async () => {}, startOldWithHelper: async () => ({ oldVersion: '1.1.0', helperWasOn: true }), oldAgentBuild16: async () => 'aaaa', expectedAgentBuild16: async () => 'bbbb',
    beginInstall: async () => 1000, installNew: async () => {}, openApp: async () => {}, agentBuild16: async () => built, agentProcs: async () => [{ pid: 5, startMs: 2000 }], extraChecks: async () => [],
  };
  const r = await upgradeSwapStep(p);
  assert.equal(r.status, 'pass');
  const r2 = await upgradeSwapStep({ ...p, installNew: async () => { throw new Error('dpkg: error processing archive (rc=1)'); } });
  assert.equal(r2.status, 'not-measured');
  assert.match(r2.summary, /dpkg/);
});
test('versionOf', () => assert.equal(versionOf('Dyno 1.1.0 (build x)'), '1.1.0'));
