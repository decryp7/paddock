import test from 'node:test';
import assert from 'node:assert/strict';
import { hasMenuBar, dynoItems, judgeMenuBar } from './mac-menubar.mjs';

const BAR = 'Window Server\t120\t24\tMenubar\t0,0,1920x24\nControl Center\t300\t25\tnet.decryptology.dyno.menubar.cpu\t100,0,40x24\n';
const NOBAR = 'AccessibilityUIServer\t99\t0\t\t0,0,10x10\n';

test('the Menubar window is found, and only as a Window Server row', () => {
  assert.equal(hasMenuBar(BAR), true);
  assert.equal(hasMenuBar(NOBAR), false);
  assert.equal(hasMenuBar('Other\t1\t0\tMenubar\t0,0,1x1\n'), false);
  assert.equal(dynoItems(BAR).length, 1);
});
test('no menu bar is NOT MEASURED, never a fail', () => {
  const j = judgeMenuBar({ menubar: false, items: 0, dynoRunning: true, crashReports: [] });
  assert.equal(j.status, 'not-measured');
  assert.match(j.summary, /not measured: guest has no menu bar/);
});
test('a menu bar without Dyno items fails; with items passes', () => {
  assert.equal(judgeMenuBar({ menubar: true, items: 0, dynoRunning: true, crashReports: [] }).status, 'fail');
  assert.equal(judgeMenuBar({ menubar: true, items: 4, dynoRunning: true, crashReports: [] }).status, 'pass');
});
test('Dyno gone with a crash report fails even without a menu bar', () => {
  const j = judgeMenuBar({ menubar: false, items: 0, dynoRunning: false, crashReports: ['dyno-app_2026-11-05.ips'] });
  assert.equal(j.status, 'fail');
  assert.match(j.summary, /crash/);
});
test('a crash report while Dyno runs is recorded, not failed', () => {
  const j = judgeMenuBar({ menubar: true, items: 4, dynoRunning: true, crashReports: ['dyno-app.ips'] });
  assert.equal(j.status, 'pass');
  assert.match(j.checks[1].detail, /crash report\(s\) exist/);
});
