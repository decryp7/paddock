import assert from 'node:assert/strict';
import { test } from 'node:test';
import { MacGui } from './mac-gui.mjs';

// A guest that records what runs. `ax` answers the axdrive calls from a script; time passes only through sleep().
function guestOf(ax) {
  let t = 0; const calls = [];
  return { calls, now: () => t, sleep: async (ms) => { t += ms; }, exec: async (c) => { calls.push(c); return ax(c, t); } };
}
const ok = (stdout = '') => ({ code: 0, stdout, stderr: '' });
const no = () => ({ code: 1, stdout: 'not found', stderr: '' });

test('Settings is opened only after the app is frontmost: an AXPress on the menu item of an inactive app is accepted and does nothing (2026-11-05, row 25)', async () => {
  let active = false, opened = false;
  const g = guestOf((c) => {
    if (/osascript.*activate/.test(c)) { active = true; return ok(); }
    if (/axdrive press .*Settings…/.test(c)) { if (active) opened = true; return ok('pressed'); }
    if (/axdrive has .*\^Settings\$.* AXWindow/.test(c)) return opened ? ok() : no();
    if (/axdrive press .*AXButton/.test(c)) return ok();
    return no();
  });
  const gui = new MacGui(g, { intervalMs: 10 });
  const r = await gui.openSettingsSection('^Dyno$');
  assert.equal(r.ok, true, r.detail);
  assert.ok(g.calls.findIndex((c) => /activate/.test(c)) < g.calls.findIndex((c) => /Settings…/.test(c)), 'activate first');
});

test('addSchedule waits for the "Turn on scheduled wake" banner to go before it presses Add schedule: a press during the re-render opens no form (measured: 0.2 s after turning on, form never opened; 4 s after, it opened)', async () => {
  let banner = true, bannerGoneAt = 3000, formOpen = false; const order = [];
  const g = guestOf((c, t) => {
    if (t >= bannerGoneAt) banner = false;
    if (/axdrive has .*Turn on scheduled wake/.test(c)) return banner ? ok() : no();
    if (/axdrive press .*Turn on scheduled wake/.test(c)) { order.push('turnon'); return ok(); }
    if (/axdrive press .*\^Add schedule\$/.test(c)) { order.push(banner ? 'add-too-early' : 'add'); formOpen = !banner; return ok(); }
    return no();
  });
  const gui = new MacGui(g, { intervalMs: 10 });
  gui.openPower = async () => true;
  gui.fieldIndex = async () => (formOpen ? 0 : -1);
  gui.setField = async () => true;
  gui.texts = async () => [{ role: 'AXStaticText', text: 'vm-check' }];
  const r = await gui.addSchedule('vm-check', '9:30 PM');
  assert.deepEqual(order.slice(0, 2), ['turnon', 'add'], 'the first Add press comes after the banner went');
  assert.equal(r.ok, true, r.detail);
});

test('ensureMainWindow reopens the app when it started hidden and waits for the window; it does nothing when the window is up', async () => {
  const mk = (windowAfterOpen) => { let up = false; const cmds = [];
    const g = { now: () => Date.now(), sleep: async () => {}, exec: async (c) => { cmds.push(c); if (/^open -a/.test(c) && windowAfterOpen) up = true; return { code: /axdrive has/.test(c) ? (up ? 0 : 1) : 0, stdout: '', stderr: '' }; } };
    return { cmds, gui: new MacGui(g, {}) }; };
  const a = mk(true); assert.equal(await a.gui.ensureMainWindow(), true); assert.ok(a.cmds.some((c) => /^open -a '\/Applications\/Dyno\.app'/.test(c)));
  const b = mk(true); b.gui.g.exec = async (c) => { b.cmds.push(c); return { code: 0, stdout: '', stderr: '' }; }; assert.equal(await b.gui.ensureMainWindow(), true); assert.ok(!b.cmds.some((c) => /^open -a/.test(c)));
});
