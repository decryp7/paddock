import test from 'node:test';
import assert from 'node:assert/strict';
import { judgeLoginSwitch, loginSwitchStep, hiddenStartStep, startsHidden, parseBtm } from './login-start-rows.mjs';

const e = (present, count = present ? 1 : 0, command = '"C:\\Program Files\\Dyno\\dyno-app.exe" --hidden') => ({ present, count, command });
test('startsHidden', () => { assert.ok(startsHidden('"C:\\Program Files\\Dyno\\dyno-app.exe" --hidden')); assert.ok(startsHidden('Exec="/usr/bin/dyno-app" "--hidden"'.replace('Exec=', ''))); assert.ok(!startsHidden('"dyno-app.exe"')); });
test('default on, off removes, on restores once: pass', () => assert.ok(judgeLoginSwitch({ defaultEntry: e(true), afterOff: e(false), afterOn: e(true), switchBefore: 'on' }).every((c) => c.ok)));
test('no default entry, a leftover after off, a duplicate after on each fail', () => {
  assert.ok(judgeLoginSwitch({ defaultEntry: e(false), afterOff: e(false), afterOn: e(true), switchBefore: 'on' }).some((c) => !c.ok));
  assert.ok(judgeLoginSwitch({ defaultEntry: e(true), afterOff: e(true), afterOn: e(true), switchBefore: 'on' }).some((c) => !c.ok && /still registered/.test(c.detail)));
  assert.ok(judgeLoginSwitch({ defaultEntry: e(true), afterOff: e(false), afterOn: e(true, 2), switchBefore: 'on' }).some((c) => !c.ok));
});
test('an unreadable switch is a broken check', () => assert.ok(judgeLoginSwitch({ defaultEntry: e(true), afterOff: null, afterOn: null, switchBefore: 'unknown' }).filter((c) => !c.ok).every((c) => c.kind === 'check')));
test('the step flips the fake switch', async () => {
  let on = true; const p = { sleep: async () => {}, readSwitch: async () => (on ? 'on' : 'off'), pressSwitch: async () => { on = !on; return true; }, readEntry: async () => e(on) };
  assert.equal((await loginSwitchStep(p)).status, 'pass');
});
test('the hidden start step', async () => {
  const mk = (wins) => ({ sleep: async () => {}, stop: async () => {}, startEntry: async () => {}, agentUp: async () => true, windows: async () => wins });
  assert.equal((await hiddenStartStep(mk([]))).status, 'pass');
  assert.equal((await hiddenStartStep(mk(['Dyno']))).status, 'fail');
});
test('parseBtm counts enabled Dyno items only', () => {
  const t = '\n #1: UUID: a\n  Name: Dyno\n  Type: app (0x2)\n  Disposition: [enabled, allowed, visible, notified] (0xb)\n  URL: file:///Applications/Dyno.app/\n #2: UUID: b\n  Name: net.decryptology.dyno.agent\n  Type: legacy agent\n  Disposition: [enabled] (0x1)\n #3: UUID: c\n  Name: Other\n  Type: app\n  Disposition: [enabled]';
  assert.equal(parseBtm(t).count, 1);
});

// 2026-11-08 triage of 7d06 row 19: Dyno's Settings switches edit a DRAFT; "Save changes" applies it (settings-page.ts). A probe that never saves sees the entry unchanged.
// The fake: the switch flips the draft, apply() saves it (the entry follows the saved value only), readSwitch reads the saved value after Settings is reopened.
function draftModel({ saveWorks = true, logLines = true } = {}) {
  let saved = true; let draft = true; const log = [];
  return {
    saved: () => saved,
    p: {
      sleep: async () => {},
      readSwitch: async () => (draft ? 'on' : 'off'),
      pressSwitch: async () => { draft = !draft; return true; },
      apply: async () => { if (!saveWorks) return { ok: false, detail: 'no Save changes button' }; saved = draft; if (logLines) log.push(saved ? 'per-user Run value Dyno written' : 'per-user Run value Dyno removed'); return { ok: true, saved: saved ? 'on' : 'off', detail: 'saved' }; },
      readEntry: async () => e(saved),
      witness: async () => log.slice(),
    },
  };
}
test('draft model: press then Save changes removes and restores the entry; the app log line is a second witness', async () => {
  const m = draftModel();
  const r = await loginSwitchStep(m.p);
  assert.equal(r.status, 'pass', r.summary);
  assert.ok(r.checks.some((c) => /per-user Run value Dyno removed/.test(c.detail)), JSON.stringify(r.checks));
  assert.equal(m.saved(), true);
});
test('draft model: a press that is never saved is a broken check (the probe did not apply the change), not a product fail', async () => {
  const m = draftModel({ saveWorks: false });
  const r = await loginSwitchStep(m.p);
  assert.equal(r.status, 'check-broken', r.summary);
  assert.match(r.summary, /not saved|Save changes/);
});
test('draft model: saved off but the entry stays is a product fail', async () => {
  const m = draftModel();
  m.p.readEntry = async () => e(true);
  assert.equal((await loginSwitchStep(m.p)).status, 'fail');
});
