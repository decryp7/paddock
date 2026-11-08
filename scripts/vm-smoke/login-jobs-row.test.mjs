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

// 2026-11-08: the Start at login change ends before Cancel can be pressed on a quiet guest. The Jobs-list Cancel is then measured on a job that lasts (a Sustained
// processor benchmark, "Running benchmarks"); the switch is always put back.
function longRig({ loginListed = true, longCancels = true }) {
  const r = rig({ cancelWorks: false, jobAppears: loginListed, start: 'on' });
  let long = 'idle';
  const base = r.d.jobsText;
  r.d.startLongJob = async () => { long = 'running'; return true; };
  r.d.stopLongJob = async () => { long = 'idle'; };
  r.d.jobsText = async () => `${await base()} ${long === 'running' ? 'Running benchmarks Pass 1 of 5 Cancel' : long === 'cancelled' ? 'Running benchmarks Cancelled' : ''}`;
  r.d.pressCancel = async () => { if (long === 'running' && longCancels) { long = 'cancelled'; return true; } return false; };
  return r;
}
test('a quick login job that was listed, with Cancel measured on a long benchmark job, passes and the switch is put back', async () => {
  const r = longRig({});
  const s = await loginJobsRow(r.d, r.clock);
  assert.equal(s.status, 'pass', s.summary);
  assert.match(s.summary, /benchmark/);
  assert.equal(await r.d.readSwitch(), 'on');
});
test('a long job with no Cancel to press stays not measured; a login job never listed stays not measured even when the long job cancels', async () => {
  assert.equal((await loginJobsRow(longRig({ longCancels: true }).d, longRig({}).clock)).status, 'pass');
  const f = longRig({ longCancels: false });
  assert.equal((await loginJobsRow(f.d, f.clock)).status, 'not-measured');
  const n = longRig({ loginListed: false });
  const s = await loginJobsRow(n.d, n.clock);
  assert.equal(s.status, 'not-measured');
  assert.equal(await n.d.readSwitch(), 'on');
});

test('draft model: the job starts only when the change is saved; the row presses Save changes after the switch and on the way back', async () => {
  let t = 0, draft = 'on', saved = 'on', state = 'idle'; const calls = [];
  const clock = { now: () => t, sleep: async (ms) => { t += ms; } };
  const d = {
    readSwitch: async () => draft,
    pressSwitch: async () => { calls.push('press'); draft = draft === 'on' ? 'off' : 'on'; return true; },
    save: async () => { calls.push('save'); if (draft !== saved) { saved = draft; state = 'running'; } return true; },
    apply: async () => { calls.push('apply'); saved = draft; return { ok: true, saved }; },
    openJobs: async () => true,
    jobsText: async () => (state === 'running' ? 'Start Dyno at login Working Cancel' : state === 'cancelled' ? 'Start Dyno at login Cancelled' : ''),
    pressCancel: async () => { if (state === 'running') { state = 'cancelled'; draft = saved = 'on'; return true; } return false; },
  };
  const s = await loginJobsRow(d, clock);
  assert.equal(s.status, 'pass', s.summary);
  assert.deepEqual(calls.slice(0, 2), ['press', 'save']);
  assert.equal(saved, 'on');
});
