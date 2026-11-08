import assert from 'node:assert/strict';
import { test } from 'node:test';
import { selectRows } from './win-rows.mjs';

test('rows are found by name and by number, once, in phases', () => {
  const s = selectRows('10,11,16,21,42,38,39');
  assert.deepEqual(s.ui, ['tray-menu', 'disk-space', 'exploit']); // the table's order, not the order typed
  assert.deepEqual(s.after, ['hung-page', 'install-rows']);
  assert.equal(s.helper, true);
  assert.equal(s.reboot, false);
  assert.ok(s.manifest.includes('tray-countdown') && s.manifest.includes('kill-installer'));
});
test('the helper is needed only by the rows that read through it', () => {
  assert.equal(selectRows('tray-menu,disk-space').helper, false);
  assert.equal(selectRows('29').helper, true);
});
test('the swap row needs the older installer', () => assert.equal(selectRows('40').oldInstaller, true));
test('a suite part passes through when no row is named', () => assert.deepEqual([selectRows('start').suitePart, selectRows('start').rows], ['start', []]));
test('rows mixed with a suite part, or restart-tray mixed with rows, are refused', () => {
  assert.throws(() => selectRows('10,start'), /mixes rows/);
  assert.throws(() => selectRows('restart-tray,10'), /alone/);
});
test('no --only selects nothing', () => assert.deepEqual(selectRows(undefined).rows, []));
import { parseArgs } from './run-win.mjs';
test('run-win: rows need --vnc, the swap needs the older installer, restart-tray is the restart phase', () => {
  const base = ['--app', 'dyno', '--installer', '/x/setup.exe'];
  assert.throws(() => parseArgs([...base, '--only', '10']), /need --vnc/);
  assert.throws(() => parseArgs([...base, '--vnc', '--only', '40']), /old-installer/);
  assert.equal(parseArgs([...base, '--vnc', '--only', 'restart-tray']).reboot, true);
  assert.equal(parseArgs([...base, '--vnc', '--only', '10,16,21']).sel.helper, true);
  assert.equal(parseArgs([...base, '--only', 'start']).sel.rows.length, 0);
});
