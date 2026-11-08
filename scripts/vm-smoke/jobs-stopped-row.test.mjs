import assert from 'node:assert/strict';
import { test } from 'node:test';
import { JOBS_STOPPED_TITLE, jobsStoppedStep } from './jobs-stopped-row.mjs';

const BAD = 'The jobs could not be read';
// A scripted world: the clock advances 1 s per sleep; the panel text is a function of the time since SIGCONT (null: not yet resumed).
function world({ errorFrom = 1, clearAfterCont = 3, flashAt = null, stopOk = true, openOk = true } = {}) {
  let t = 0, contAt = null; const log = [];
  const d = {
    quitApp: async () => log.push('quit'),
    killAgentAndStopNew: async () => (stopOk ? { pid: 777, start: 'Sun Nov  8 10:00:00 2026' } : null),
    launchApp: async () => log.push('launch'),
    openJobs: async () => openOk,
    jobsText: async () => {
      if (contAt === null) return t >= errorFrom ? BAD : '';
      const since = t - contAt;
      if (flashAt !== null && since === flashAt) return BAD;
      return since < clearAfterCont ? BAD : '';
    },
    cont: async () => { contAt = t; log.push('cont'); },
    guestClock: async () => `10:00:${String(t).padStart(2, '0')}`,
  };
  return { d, c: { now: () => t * 1000, sleep: async (ms) => { t += ms / 1000; } }, log };
}

test('PASS: the error shows while the agent is stopped and clears by itself within the limit', async () => {
  const w = world({});
  const r = await jobsStoppedStep(w.d, w.c, { holdMs: 10_000 });
  assert.equal(r.title, JOBS_STOPPED_TITLE);
  assert.equal(r.status, 'pass', r.summary);
  assert.ok(w.log.includes('cont'));
});

test('FAIL: the error stays after the agent answers', async () => {
  const w = world({ clearAfterCont: 999 });
  assert.equal((await jobsStoppedStep(w.d, w.c, { holdMs: 10_000, clearMaxMs: 10_000 })).status, 'fail');
});

test('FAIL: the error flashes back after it cleared', async () => {
  const w = world({ flashAt: 5 });
  assert.equal((await jobsStoppedStep(w.d, w.c, { holdMs: 10_000 })).status, 'fail');
});

test('check broken (ER-33): the error state was never reached, so the clearing proves nothing', async () => {
  const w = world({ errorFrom: 9999 });
  const r = await jobsStoppedStep(w.d, w.c, { holdMs: 10_000 });
  assert.notEqual(r.status, 'pass');
  assert.notEqual(r.status, 'fail');
  assert.ok(w.log.includes('cont'), 'the agent is always continued');
});

test('check broken when the new agent could not be stopped; the agent is never left stopped', async () => {
  const w = world({ stopOk: false });
  const r = await jobsStoppedStep(w.d, w.c, { holdMs: 10_000 });
  assert.equal(r.status, 'check-broken');
});
