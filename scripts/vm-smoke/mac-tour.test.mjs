import assert from 'node:assert/strict';
import { test } from 'node:test';
import { MacTour } from './mac-tour.mjs';

const windows = 'Window Server\t189\t24\tMenubar\t0,0,1024x30\nControl Center\t433\t25\tnet.decryptology.dyno.menubar.number\t746,0,53x30\nControl Center\t433\t25\tnet.decryptology.dyno.menubar.number-2\t693,0,12x30\nControl Center\t433\t25\tClock\t872,0,152x30\n';
const fake = (tsv, { running = 'yes', crashes = '' } = {}) => ({ exec: async (cmd) => ({ code: 0, stdout: /axdrive windows/.test(cmd) ? tsv : /pgrep/.test(cmd) ? `${running}\n` : crashes, stderr: '' }), now: () => 0, sleep: async () => {} });
const guest = fake(windows);
const gui = { app: 'Dyno', shot: async () => '/tmp/ev/01-menu-bar.png' };

test('menu bar: counts Dyno items and fails a sliver', async () => {
  const s = await new MacTour(guest, gui, {}).menuBar();
  assert.equal(s.status, 'fail');
  assert.match(s.summary, /20 points/);
  assert.match(s.checks.find((c) => /Dyno menu-bar item/.test(c.detail)).detail, /2 Dyno menu-bar item/);
});
test('menu bar: none with a menu bar up is a failure', async () => {
  const s = await new MacTour(fake('Window Server\t1\t24\tMenubar\t0,0,1024x30\nDock\t1\t20\tDock\t0,0,10x10\n'), gui, {}).menuBar();
  assert.equal(s.status, 'fail');
});
test('menu bar: a guest without a menu bar is not measured, with a screenshot as evidence', async () => {
  const s = await new MacTour(fake('Dock\t1\t20\tDock\t0,0,10x10\n'), gui, {}).menuBar();
  assert.equal(s.status, 'not-measured');
  assert.match(s.summary, /not measured: guest has no menu bar/);
  assert.match(s.evidence[0], /01-menu-bar\.png/);
});
