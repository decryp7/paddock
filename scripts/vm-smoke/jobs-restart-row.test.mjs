import assert from 'node:assert/strict';
import { test } from 'node:test';
import { JOBS_RESTART_TITLE, jobsRestartStep } from './jobs-restart-row.mjs';

const BAD = 'The jobs could not be read';
// A scripted driver: jobsText answers come from `texts` in order (the last one repeats).
function driver({ texts, panel = true, agentBack = true, killed = true }) {
  let i = 0; const calls = [];
  return {
    calls,
    d: {
      openJobs: async () => { calls.push('open'); return panel; },
      jobsText: async () => texts[Math.min(i++, texts.length - 1)],
      killAgent: async () => { calls.push('kill'); return killed; },
      relaunchApp: async () => { calls.push('relaunch'); },
      agentUp: async () => agentBack,
    },
    c: { now: (() => { let t = 0; return () => (t += 1000); })(), sleep: async () => {} },
  };
}

test('PASS: the panel reads fine once the agent is back', async () => {
  const { d, c } = driver({ texts: ['', '', ''] });
  const r = await jobsRestartStep(d, c, { settleMs: 5000 });
  assert.equal(r.title, JOBS_RESTART_TITLE);
  assert.equal(r.status, 'pass');
});

test('FAIL: "could not be read" stays after the agent is back', async () => {
  const { d, c } = driver({ texts: ['', BAD] });
  const r = await jobsRestartStep(d, c, { settleMs: 5000 });
  assert.equal(r.status, 'fail');
});

test('PASS when the message shows briefly and clears', async () => {
  const { d, c } = driver({ texts: ['', BAD, '', '', ''] });
  assert.equal((await jobsRestartStep(d, c, { settleMs: 8000 })).status, 'pass');
});

test('check broken when the Jobs panel cannot be opened', async () => {
  const { d, c } = driver({ texts: [''], panel: false });
  const r = await jobsRestartStep(d, c, { settleMs: 5000 });
  assert.notEqual(r.status, 'pass');
  assert.notEqual(r.status, 'fail');
});

test('check broken when the agent does not come back or was not killed', async () => {
  const y = driver({ texts: [''], agentBack: false });
  assert.notEqual((await jobsRestartStep(y.d, y.c, { settleMs: 3000 })).status, 'pass');
  const x = driver({ texts: [''], killed: false });
  assert.notEqual((await jobsRestartStep(x.d, x.c, { settleMs: 3000 })).status, 'pass');
});
