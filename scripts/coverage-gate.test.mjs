import assert from 'node:assert/strict';
import { test } from 'node:test';
import { measure } from './coverage-gate.mjs';

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
