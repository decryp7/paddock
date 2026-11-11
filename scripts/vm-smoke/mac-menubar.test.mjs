import test from 'node:test';
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { appleItems, confirmMenuBar, dynoItems, hasMenuBar, judgeMenuBar, menuBarFacts } from './mac-menubar.mjs';

const WINDOWS = readFileSync(new URL('./fixtures/mac-axdrive-windows.tsv', import.meta.url), 'utf8');
const TREE = readFileSync(new URL('./fixtures/mac-axdrive-tree-menubar.txt', import.meta.url), 'utf8');
const OWNERS_ONLY = 'Terminal\t90\t0\tadmin — -zsh — 120×30\t0,0,10x10\nDock\t60\t20\tDock\t0,0,1024x40\n';

test('the real captures: the bar is confirmed by the Apple items, never by a "Menubar" title; Dyno items are counted from the windows', () => {
  assert.deepEqual(appleItems(TREE), ['clock', 'controlcenter']);
  assert.equal(hasMenuBar(TREE), true);
  assert.equal(hasMenuBar(''), false);
  assert.equal(hasMenuBar('AXMenuBarItem  \n'), false);
  assert.equal(dynoItems(WINDOWS).length, 4);
  assert.equal(hasMenuBar(WINDOWS), false, 'Dyno windows alone do not prove the bar');
  assert.equal(hasMenuBar('Window Server\t120\t24\tMenubar\t0,0,1920x24\n'), false, 'the invented Menubar window proves nothing');
});
test('dead state: no known item visible is NOT MEASURED, never a pass, even if Dyno windows are listed', () => {
  const j = judgeMenuBar({ menubar: hasMenuBar(''), items: dynoItems(WINDOWS).length, dynoRunning: true, crashReports: [] });
  assert.equal(j.status, 'not-measured');
  assert.match(j.summary, /not measured/);
});
test('a bar that is up without Dyno items fails; with the real Dyno rows passes', () => {
  assert.equal(judgeMenuBar({ menubar: true, controls: appleItems(TREE), items: 0, dynoRunning: true, crashReports: [] }).status, 'fail');
  const ok = judgeMenuBar({ menubar: true, controls: appleItems(TREE), items: dynoItems(WINDOWS).length, dynoRunning: true, crashReports: [] });
  assert.equal(ok.status, 'pass');
  assert.equal(ok.checks[0].info, true, 'the bar line is information, not a pass');
});
test('Dyno gone with a crash report fails even without a menu bar; a crash while Dyno runs is recorded, not failed', () => {
  const j = judgeMenuBar({ menubar: false, items: 0, dynoRunning: false, crashReports: ['dyno-app_2026-11-05.ips'] });
  assert.equal(j.status, 'fail');
  assert.match(j.summary, /crash/);
  const run = judgeMenuBar({ menubar: true, items: 4, dynoRunning: true, crashReports: ['dyno-app.ips'] });
  assert.equal(run.status, 'pass');
  assert.match(run.checks[1].detail, /crash report\(s\) exist/);
});
test('the facts and the confirmation read the guest: Finder first, then poll until an Apple item shows', async () => {
  const cmds = [];
  let reads = 0;
  const g = { exec: async (cmd) => { cmds.push(cmd); return { code: 0, stdout: /axdrive windows/.test(cmd) ? WINDOWS : /axdrive tree/.test(cmd) ? (++reads > 2 ? TREE : OWNERS_ONLY) : /pgrep/.test(cmd) ? 'yes\n' : '', stderr: '' }; } };
  const f = await menuBarFacts(g);
  assert.equal(f.menubar, false);
  const c = await confirmMenuBar(g, { sleep: async () => {} });
  assert.equal(c.menubar, true);
  assert.ok(c.waited >= 1);
  assert.equal(cmds.find((x) => /open -a Finder/.test(x)) !== undefined, true);
  assert.ok(cmds.findIndex((x) => /open -a Finder/.test(x)) < cmds.findLastIndex((x) => /axdrive tree/.test(x)));
  assert.equal((await confirmMenuBar({ exec: async (c2) => ({ code: 0, stdout: /axdrive windows/.test(c2) ? WINDOWS : '', stderr: '' }) }, { tries: 2, sleep: async () => {} })).menubar, false);
});
