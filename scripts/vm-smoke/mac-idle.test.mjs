import assert from 'node:assert/strict';
import { test } from 'node:test';
import { MacIdleChecks } from './mac-idle.mjs';

// ER-27: when "Dyno started with the rule" fails, the step must say why and show the guest's state, not only "fail".
test('a failed start under the idle rule is reported with its failing checks and the guest state (processes, launchd job, agent log, load)', async () => {
  const cmds = [];
  let t = 0;
  const g = { now: () => t, sleep: async (ms) => { t += ms; }, exec: async (c) => { cmds.push(c); return { code: 0, stdout: `out of ${c.slice(0, 25)}`, stderr: '' }; } };
  const check = { quit: async () => ({}), startAndJudge: async () => ({ status: 'fail', summary: 'not ready within the 90 s guard', checks: [{ ok: false, detail: 'not ready within the 90 s guard' }, { ok: true, detail: 'fine' }] }) };
  const idle = new MacIdleChecks(g, { dyno: async () => ({ stdout: 'Dyno is not running', code: 1 }) }, check, {}); // no UI: the CLI path, every poll runs out its guard on the fake clock
  const s = await idle.run();
  const start = s.checks.find((c) => /Dyno started with the rule/.test(c.detail));
  assert.equal(start.ok, false);
  assert.match(start.detail, /fail: not ready within the 90 s guard/, 'the start judgement says why');
  const ev = s.evidence.join('\n');
  for (const want of [/ps -axo/, /launchctl print gui\/\$\(id -u\)\/net\.decryptology\.dyno\.agent/, /dyno-agent\.\*\.log/, /uptime/]) assert.match(cmds.join('\n'), want);
  assert.match(ev, /guest state after the failed start/);
});

// ER-33: "waits while audio plays" is only tested when audio really plays; the sound file is made before the timed idle window, and a sound that never
// holds the OS's power assertion is a broken check, not a product fail (run 4 of 2026-11-05: no assertion, the rule fired, "the rule did not report waiting").
function audioRig({ assertionAppears, say = true, audioDevice }) {
  let t = 0; const cmds = []; let played = false, mode = 'counting';
  const g = { now: () => t, sleep: async (ms) => { t += ms; }, exec: async (c) => {
    cmds.push({ c, at: t });
    if (/axdrive drag/.test(c) && mode === 'cancelled') mode = 'counting';
    if (/say -o/.test(c)) return { code: say ? 0 : 1, stdout: '', stderr: say ? '' : 'say failed' };
    if (/afplay/.test(c) && /nohup/.test(c)) { played = true; return { code: 0, stdout: '', stderr: '' }; }
    if (/pmset -g assertions/.test(c)) return { code: 0, stdout: played && assertionAppears ? 'pid 230(coreaudiod): PreventUserIdleSystemSleep' : '', stderr: '' };
    return { code: 0, stdout: 'x', stderr: '' }; } };
  const power = { dyno: async (c) => {
    if (c === 'idle cancel') { mode = 'cancelled'; return { code: 0, stdout: 'Watching: idle for 5 s' }; }
    if (/^idle snooze/.test(c)) { mode = 'snoozed'; return { code: 0, stdout: 'Snoozed for now' }; }
    if (played && assertionAppears) return { code: 0, stdout: 'Waiting because audio is playing' };
    return { code: 0, stdout: mode === 'counting' ? 'Counting down to sleep' : mode === 'snoozed' ? 'Snoozed for now' : 'Watching: idle for 5 s' }; } };
  const check = { quit: async () => ({}), startAndJudge: async () => ({ status: 'pass', checks: [], summary: '' }) };
  return { cmds, idle: new MacIdleChecks(g, power, check, { audioDevice: audioDevice ?? true }) };
}
test('the sound is made before the activity that starts the idle clock, and a sound that never holds an assertion is a broken check', async () => {
  const { cmds, idle } = audioRig({ assertionAppears: false });
  await idle.run();
  const s = idle.audioStep;
  const iSay = cmds.findIndex((x) => /say -o/.test(x.c));
  const lastActivity = cmds.map((x, i) => [x, i]).filter(([x]) => /axdrive drag/.test(x.c)).at(-1)?.[1] ?? -1;
  assert.ok(iSay >= 0 && iSay < lastActivity, 'the file is rendered before the last activity event, outside the timed window');
  assert.equal(s.status, 'check-broken');
  assert.ok(s.checks.some((c) => c.kind === 'check' && /never held a power assertion/.test(c.detail)));
});
test('with the assertion present the audio check is judged as before; a sound file that cannot be made is a broken check too', async () => {
  const ok = audioRig({ assertionAppears: true }).idle; await ok.run(); assert.equal(ok.audioStep.status, 'pass');
  const bad = audioRig({ assertionAppears: true, say: false }).idle; await bad.run(); const s = bad.audioStep;
  assert.equal(s.status, 'check-broken');
  assert.ok(s.checks.some((c) => c.kind === 'check' && /sound file/.test(c.detail)));
});
test('no guest audio device: the audio step is not measured with the reason, nothing is played, and the other idle checks still pass', async () => {
  const { cmds, idle } = audioRig({ assertionAppears: true, audioDevice: false });
  const main = await idle.run();
  assert.equal(main.status, 'pass');
  assert.equal(idle.audioStep.status, 'not-measured');
  assert.match(idle.audioStep.summary, /no audio device/);
  assert.ok(!cmds.some((x) => /afplay|say -o/.test(x.c)));
});
