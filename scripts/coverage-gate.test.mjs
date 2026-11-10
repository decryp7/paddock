import assert from 'node:assert/strict';
import { test } from 'node:test';
import { SECURITY, crateOf, gate, measure } from './coverage-gate.mjs';

const sources = {
  'a.rs': ['fn a() {}', '#[::tauri::command]', 'pub async fn cmd() {', '    body();', '}'].join('\n'),
  'b.rs': ['#[tauri::command]', 'pub async fn other() {}', '#[derive(Debug)]'].join('\n'),
};
const read = (p) => sources[p] ?? null;

test('only the tauri command attribute lines are left out of the count', () => {
  const lcov = ['SF:a.rs', 'DA:1,1', 'DA:2,0', 'DA:3,1', 'DA:4,0', 'end_of_record', 'SF:b.rs', 'DA:1,0', 'DA:2,1', 'DA:3,0', 'end_of_record'].join('\n');
  // a.rs:2 and b.rs:1 are attributes; the uncovered body (a.rs:4) and the derive (b.rs:3) still count.
  assert.deepEqual(measure(lcov, read), { hit: 3, total: 5, skipped: 2 });
});

test('a file that cannot be read keeps all its lines', () => {
  assert.deepEqual(measure('SF:gone.rs\nDA:1,0\nDA:2,1\nend_of_record', read), { hit: 1, total: 2, skipped: 0 });
});

test('crateOf names the folder after crates/ or the first folder', () => {
  assert.equal(crateOf('/work/paddock/crates/paddock-proc/src/lib.rs'), 'paddock-proc');
  assert.equal(crateOf('paddock-ipc/src/a.rs'), 'paddock-ipc');
  assert.equal(crateOf('lib.rs'), '(other)');
});

const lcov = [
  'SF:x/crates/paddock-a/src/a.rs', 'DA:1,1', 'DA:2,0', 'DA:3,0', 'end_of_record',
  'SF:x/crates/paddock-b/src/b.rs', 'DA:1,1', 'DA:2,1', 'end_of_record',
].join('\n');

test('a failing gate prints one ::error:: line and the table, worst crate first', () => {
  const { code, lines } = gate(lcov, 99, () => null, []);
  assert.equal(code, 1);
  const errors = lines.filter((l) => l.startsWith('::error::'));
  assert.equal(errors.length, 1);
  assert.match(errors[0], /60\.00% is below the minimum 99%: 2 lines uncovered, 2 more must be covered/);
  const rows = lines[1].split('\n');
  assert.match(rows[0], /^crate\s+lines\s+covered\s+missing\s+%$/);
  assert.match(rows[1], /^paddock-a\s+3\s+1\s+2\s+33\.33$/);
  assert.match(rows[2], /^paddock-b\s+2\s+2\s+0\s+100\.00$/);
});

test('a passing gate prints the table too, without an error line', () => {
  const { code, lines } = gate(lcov, 50, () => null, []);
  assert.equal(code, 0);
  assert.equal(lines.some((l) => l.includes('::error::')), false);
  assert.match(lines[1], /paddock-a/);
});

test('an empty lcov fails instead of dividing by zero', () => {
  const { code, lines } = gate('', 99, () => null);
  assert.equal(code, 1);
  assert.ok(lines.some((l) => l.startsWith('::error::')));
});

test('the default floor is 90% over everything', () => {
  const ok = ['SF:x/crates/paddock-a/src/a.rs', ...Array.from({ length: 10 }, (_, i) => `DA:${i + 1},${i === 0 ? 0 : 1}`), 'end_of_record'].join('\n');
  assert.equal(gate(ok, undefined, () => null).code, 0);
  const bad = ['SF:x/crates/paddock-a/src/a.rs', ...Array.from({ length: 10 }, (_, i) => `DA:${i + 1},${i < 2 ? 0 : 1}`), 'end_of_record'].join('\n');
  assert.equal(gate(bad, undefined, () => null).code, 1);
});

test('security code keeps 99% even when the aggregate passes', () => {
  const secure = (misses) =>
    ['SF:x/crates/paddock-env/src/private.rs', ...Array.from({ length: 10 }, (_, i) => `DA:${i + 1},${i < misses ? 0 : 1}`), 'end_of_record',
      'SF:x/crates/paddock-a/src/a.rs', ...Array.from({ length: 990 }, (_, i) => `DA:${i + 1},1`), 'end_of_record'].join('\n');
  assert.equal(gate(secure(0), undefined, () => null).code, 0);
  const { code, lines } = gate(secure(1), undefined, () => null);
  assert.equal(code, 1);
  assert.equal(lines.filter((l) => l.startsWith('::error::security code line coverage 90.00%')).length, 1);
});

test('the security list names the three security areas', () => {
  const hit = (p) => SECURITY.some((r) => r.test(p));
  assert.equal(hit('/w/crates/paddock-env/src/private.rs'), true);
  assert.equal(hit('crates/paddock-service/src/privileged/install.rs'), true);
  assert.equal(hit('crates/paddock-ipc/src/trust.rs'), true);
  assert.equal(hit('crates/paddock-env/src/dirs.rs'), false);
});
