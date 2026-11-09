import { test } from 'node:test';
import assert from 'node:assert/strict';
import { reasonClass, roundStats } from './round-stats.mjs';

test('reasons fall into the closed union, and unknown text stays unclassified', () => {
  assert.equal(reasonClass('the install did not finish (timeout); there is no installed tool to attack'), 'precondition:install');
  assert.equal(reasonClass('Quit and stop measuring could not be triggered: no "Quit and stop measuring" item in the File menu'), 'target-not-found');
  assert.equal(reasonClass('the switch could not be put back to on (it reads unknown)'), 'undo-not-undone');
  assert.equal(reasonClass('dyno mcp could not be run in the guest (exit 127: bash: line 4: timeout: command not found)'), 'kit-error');
  assert.equal(reasonClass('LastBootUpTime did not move (before unreadable, after unreadable)'), 'parse');
  assert.equal(reasonClass('the panel never showed the error within 25 s'), 'probe-timeout');
  assert.equal(reasonClass('something new'), 'unclassified');
});

test('runs are counted per OS with partial runs apart, and an unselected step of a partial run is not a reason', () => {
  const s = roundStats([
    { os: 'windows', partial: false, steps: [{ status: 'pass' }, { status: 'check-broken' }], missing: [{ id: 'a', why: 'the step ended check-broken: no window with a File menu' }, { id: 'b', why: 'the step did not run' }] },
    { os: 'windows', partial: true, steps: [{ status: 'pass' }], missing: [{ id: 'c', why: 'the step did not run' }] },
  ]);
  assert.deepEqual(s.per['windows full'], { runs: 1, steps: 2, status: { pass: 1, 'check-broken': 1 } });
  assert.equal(s.per['windows partial'].runs, 1);
  assert.deepEqual(s.reasons['check-broken'], { 'target-not-found': 1 });
  assert.deepEqual(s.reasons.missing, { 'not-driven': 1, 'not-selected (partial run)': 1 });
});
