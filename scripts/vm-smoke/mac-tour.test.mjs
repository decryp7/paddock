import assert from 'node:assert/strict';
import { test } from 'node:test';
import { MacTour } from './mac-tour.mjs';

const windows = 'Window Server\t189\t24\tMenubar\t0,0,1024x30\nControl Center\t433\t25\tnet.decryptology.dyno.menubar.number\t746,0,53x30\nControl Center\t433\t25\tnet.decryptology.dyno.menubar.number-2\t693,0,12x30\nControl Center\t433\t25\tClock\t872,0,152x30\n';
const guest = { exec: async () => ({ code: 0, stdout: windows, stderr: '' }), now: () => 0, sleep: async () => {} };
const gui = { app: 'Dyno', shot: async () => '/tmp/ev/01-menu-bar.png' };

test('menu bar: counts Dyno items and fails a sliver', async () => {
  const s = await new MacTour(guest, gui, {}).menuBar();
  assert.equal(s.status, 'fail');
  assert.match(s.summary, /20 points/);
  assert.match(s.checks[0].detail, /2 Dyno menu-bar item/);
});
test('menu bar: none is a failure', async () => {
  const s = await new MacTour({ ...guest, exec: async () => ({ code: 0, stdout: 'Dock\t1\t20\tDock\t0,0,10x10\n', stderr: '' }) }, gui, {}).menuBar();
  assert.equal(s.status, 'fail');
});
