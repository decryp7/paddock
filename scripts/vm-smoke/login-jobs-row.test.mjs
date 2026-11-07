import assert from 'node:assert/strict';
import { test } from 'node:test';
import { loginJobsRow } from './login-jobs-row.mjs';

function rig({ jobAppears = true, cancelWorks = true, start = 'off' }) {
  let t = 0, sw = start, state = 'idle', presses = 0;
  const clock = { now: () => t, sleep: async (ms) => { t += ms; } };
  const d = {
    readSwitch: async () => sw,
    pressSwitch: async () => { presses++; sw = sw === 'on' ? 'off' : 'on'; if (presses === 1 && jobAppears) state = 'running'; return true; },
    openJobs: async () => true,
    jobsText: async () => (state === 'running' ? 'Start Dyno at login Working Cancel' : state === 'cancelled' ? 'Start Dyno at login Cancelled' : ''),
    pressCancel: async () => { if (state === 'running' && cancelWorks) { state = 'cancelled'; return true; } return false; },
  };
  return { d, clock, presses: () => presses };
}
test('the job shows, Cancel ends it as Cancelled, a second change is accepted and the switch is put back', async () => {
  const r = rig({});
  const s = await loginJobsRow(r.d, r.clock);
  assert.equal(s.status, 'pass', s.summary);
  assert.ok(r.presses() >= 2);
});
test('ER-33: a change that is never listed is NOT MEASURED with the reason, never a pass', async () => {
  const r = rig({ jobAppears: false });
  const s = await loginJobsRow(r.d, r.clock);
  assert.equal(s.status, 'not-measured');
  assert.match(s.summary, /finished before the Jobs list showed it/);
});
test('a listed job whose Cancel does nothing is not measured; an unreadable switch is a broken check', async () => {
  const r = rig({ cancelWorks: false });
  assert.equal((await loginJobsRow(r.d, r.clock)).status, 'not-measured');
  const r2 = rig({}); r2.d.readSwitch = async () => 'unknown';
  assert.equal((await loginJobsRow(r2.d, r2.clock)).status, 'check-broken');
});
