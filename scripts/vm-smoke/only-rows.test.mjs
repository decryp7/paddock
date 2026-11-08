import assert from 'node:assert/strict';
import { test } from 'node:test';
import { requiredSteps } from './manifest.mjs';
import { LINUX_ROWS, firstBrokenPrecondition, onlyRequired, parseOnly, stepRow } from './only-rows.mjs';

test('--only takes row numbers, sorted and without duplicates', () => {
  assert.deepEqual(parseOnly('23,7,10,7,19'), [7, 10, 19, 23]);
  assert.deepEqual(parseOnly(' 7 , 10 '), [7, 10]);
});
test('--only refuses an empty list, a word, zero and a row that does not exist', () => {
  for (const bad of ['', ' ', 'x', '7,x', '0', '27', '-1', '1.5']) assert.throws(() => parseOnly(bad), /--only/, bad);
});
test('dead state: every manifest row of a full Linux release run has a row number, so --only cannot silently lose one', () => {
  const ids = new Set(Object.values(LINUX_ROWS).flat());
  for (const r of requiredSteps({ os: 'linux', reboot: true, release: true })) assert.ok(ids.has(r.id), `manifest row ${r.id} has no --only row number`);
});
test('the numbers are the report table: 7 and 10 are the restart rows, 19 the login job, 23 Quit and stop', () => {
  assert.deepEqual(LINUX_ROWS[7], ['reboot-before']);
  assert.deepEqual(LINUX_ROWS[10], ['reboot-card']);
  assert.deepEqual(LINUX_ROWS[19], ['login-jobs']);
  assert.deepEqual(LINUX_ROWS[23], ['quit-stop']);
  assert.deepEqual(LINUX_ROWS[15], ['reading:wol-state']);
  assert.deepEqual(LINUX_ROWS[16], ['reading:lifespan-smart']);
});
test('only the chosen rows are required, nothing is added and nothing is left out', () => {
  const full = requiredSteps({ os: 'linux', reboot: true, release: false, partial: true });
  assert.deepEqual(onlyRequired(full, [7, 10, 19, 23]).map((r) => r.id).sort(), ['login-jobs', 'quit-stop', 'reboot-before', 'reboot-card']);
  assert.deepEqual(onlyRequired(full, [3]).map((r) => r.id), ['cold-start']);
});
test('a step title is mapped back to the row it belongs to', () => {
  const full = requiredSteps({ os: 'linux', reboot: true, release: false, partial: true });
  assert.equal(stepRow('Restart check: card and a helper reading after the reboot', full), 10);
  assert.equal(stepRow('Row 5a: Start at login change shows in the Jobs list with Cancel', full), 19);
  assert.equal(stepRow('Something nobody registered', full), null);
});
test('preconditions are checked in a fixed order and the first broken one is named', () => {
  const ok = { installed: true, commitMatches: true, helperActive: true, autostartEntry: true, windowReadable: true };
  assert.equal(firstBrokenPrecondition(ok, [7, 10, 19, 23]), null);
  assert.equal(firstBrokenPrecondition({ ...ok, helperActive: false }, [7]), 'the administrator helper is active (dyno-helper.service)');
  assert.equal(firstBrokenPrecondition({ ...ok, autostartEntry: false }, [10]), 'Launch at login is on: ~/.config/autostart/Dyno.desktop exists');
  assert.equal(firstBrokenPrecondition({ ...ok, windowReadable: false }, [19]), 'the Dyno window is readable over AT-SPI');
  assert.equal(firstBrokenPrecondition({ ...ok, commitMatches: false, helperActive: false }, [7]), 'the installed build is the expected commit');
});
test('a precondition is only demanded by the rows that rely on it', () => {
  const ok = { installed: true, commitMatches: true, helperActive: true, autostartEntry: true, windowReadable: true };
  assert.equal(firstBrokenPrecondition({ ...ok, helperActive: false }, [19]), null, 'row 19 does not need the helper');
  assert.equal(firstBrokenPrecondition({ ...ok, autostartEntry: false }, [19]), null, 'row 19 switches the entry itself');
  assert.equal(firstBrokenPrecondition({ ...ok, autostartEntry: false }, [7, 10]), null, 'row 7 switches it on itself and checks it');
  assert.equal(firstBrokenPrecondition({ ...ok, autostartEntry: false }, [23]), null, 'row 23 switches it on itself and checks it');
});

test('run-linux.mjs --only makes a partial run, and the restart rows switch the reboot check on', async () => {
  const { parseArgs } = await import('./run-linux.mjs');
  const base = ['--app', 'dyno', '--installer', 'x.deb'];
  const a = parseArgs([...base, '--only', '19,7']);
  assert.deepEqual(a.only, [7, 19]);
  assert.equal(a.partial, true);
  assert.equal(a.reboot, true);
  const b = parseArgs([...base, '--only', '19']);
  assert.equal(b.reboot, undefined);
  assert.throws(() => parseArgs([...base, '--only']), /needs a value/);
  assert.throws(() => parseArgs([...base, '--only', '99']), /no such row/);
});

test('the stale "jobs could not be read" heading is found in the Jobs list evidence, not in the log lines after it', async () => {
  const { staleJobsReadError } = await import('./only-rows.mjs');
  assert.equal(staleJobsReadError(['jobs list: heading  The jobs could not be read | list item  X', '--- Dyno log lines about jobs (every Dyno log in the guest)']), true);
  assert.equal(staleJobsReadError(['jobs list: list item  X', '--- Dyno log lines about jobs (every Dyno log in the guest)', 'The jobs could not be read']), false);
  assert.equal(staleJobsReadError([]), false);
});
