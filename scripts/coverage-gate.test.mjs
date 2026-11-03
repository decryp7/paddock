import assert from 'node:assert/strict';
import { mkdirSync, mkdtempSync, rmSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { test } from 'node:test';
import { check, crateOf, floor2, measure, measureByCrate } from './coverage-gate.mjs';
import { ignoreRegex, parseRegister, validateRegister } from './coverage-register.mjs';

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

test('lines are counted per crate, from the path under the root or after the last /crates/', () => {
  assert.equal(crateOf('/x/crates/paddock-a/src/lib.rs', '/x/crates'), 'paddock-a');
  assert.equal(crateOf('/ci/crates/work/crates/paddock-b/src/lib.rs'), 'paddock-b');
  assert.equal(crateOf('/elsewhere/lib.rs', '/x/crates'), null);
  const lcov = 'SF:/x/crates/a/src/l.rs\nDA:1,1\nDA:2,0\nend_of_record\nSF:/x/crates/b/src/l.rs\nDA:1,1\nend_of_record';
  const m = measureByCrate(lcov, () => null, '/x/crates');
  assert.deepEqual([...m], [['a', { hit: 1, total: 2, skipped: 0 }], ['b', { hit: 1, total: 1, skipped: 0 }]]);
});

test('a recorded figure is cut, never rounded up', () => {
  assert.equal(floor2(97.129), 97.12);
  assert.equal(floor2(97.5), 97.5);
});

// A crates folder on disk: crates/<name>/src/lib.rs for each name, plus extra files.
function tree(names, files = []) {
  const root = mkdtempSync(join(tmpdir(), 'covgate-'));
  for (const n of names) mkdirSync(join(root, n, 'src'), { recursive: true });
  for (const f of files) {
    mkdirSync(join(root, f, '..'), { recursive: true });
    writeFileSync(join(root, f), '');
  }
  return root;
}
/** lcov for crates given as { name: [hit, total] } under `root`. */
function lcovFor(root, figures) {
  return Object.entries(figures)
    .map(([name, [hit, total]]) => {
      const da = Array.from({ length: total }, (_, i) => `DA:${i + 1},${i < hit ? 1 : 0}`);
      return [`SF:${root}/${name}/src/lib.rs`, ...da, 'end_of_record'].join('\n');
    })
    .join('\n');
}
const run = (root, figures, register, now = new Date('2026-11-04T00:00:00Z')) =>
  check({ lcovText: lcovFor(root, figures), registerText: register, root, now, read: () => null });
const entry = (name, minimum, extra = 'reason = "native"\nreview = "2027-02-01"') => `[[crate]]\nname = "${name}"\nminimum = ${minimum}\n${extra}\n`;

test('every crate at 99 % passes with an empty register', () => {
  const root = tree(['a', 'b']);
  const r = run(root, { a: [99, 100], b: [1000, 1000] }, '');
  assert.equal(r.exit, 0);
  assert.match(r.summary, /\| a \| 99\.00 % \| 99 % \|/);
  rmSync(root, { recursive: true });
});

test('a crate under 99 % without an entry fails with one error naming crate, figure, minimum and next step', () => {
  const root = tree(['a', 'b']);
  const r = run(root, { a: [98, 100], b: [100, 100] }, '');
  assert.equal(r.exit, 1);
  const errors = r.out.filter((l) => l.startsWith('::error::'));
  assert.equal(errors.length, 1);
  assert.match(errors[0], /crate a: 98\.00 % is under its minimum 99 %.*Next step/);
  rmSync(root, { recursive: true });
});

test('a registered crate passes at its minimum and fails when it drops, even above 90 %', () => {
  const root = tree(['a']);
  const reg = entry('a', 95.5);
  assert.equal(run(root, { a: [191, 200] }, reg).exit, 0);
  const drop = run(root, { a: [190, 200] }, reg);
  assert.equal(drop.exit, 1);
  assert.match(drop.out[0], /crate a: 95\.00 % is under its minimum 95\.5 % \(registered: native\).*ratchets up/);
  rmSync(root, { recursive: true });
});

test('a crate under the 90 % floor fails even with a reason and a minimum under 90', () => {
  const root = tree(['a']);
  const r = run(root, { a: [89, 100] }, entry('a', 85));
  assert.equal(r.exit, 1);
  assert.ok(r.out.some((l) => /hard floor of 90 %/.test(l)));
  assert.ok(r.out.some((l) => /minimum 85 is under the hard floor/.test(l)));
  rmSync(root, { recursive: true });
});

test('a crate above its minimum or at 99 % only warns', () => {
  const root = tree(['a', 'b']);
  const r = run(root, { a: [96, 100], b: [99, 100] }, entry('a', 95) + entry('b', 95));
  assert.ok(r.out.some((l) => l.startsWith('::warning::crate a: 96.00 % is above its recorded minimum 95 %')));
  assert.ok(r.out.some((l) => l.startsWith('::warning::crate b') && /remove its \[\[crate\]\] entry/.test(l)));
  rmSync(root, { recursive: true });
});

test('an entry without a reason or a review date, or with a stale one', () => {
  const root = tree(['a']);
  const noReason = run(root, { a: [95, 100] }, '[[crate]]\nname = "a"\nminimum = 95\nreview = "2027-02-01"\n');
  assert.match(noReason.out.join('\n'), /needs a reason/);
  const noDate = run(root, { a: [95, 100] }, '[[crate]]\nname = "a"\nminimum = 95\nreason = "r"\n');
  assert.match(noDate.out.join('\n'), /needs review/);
  const stale = run(root, { a: [95, 100] }, entry('a', 95, 'reason = "r"\nreview = "2026-10-01"'));
  assert.equal(stale.exit, 0);
  assert.ok(stale.out.some((l) => l.startsWith('::warning::') && /review date 2026-10-01 has passed/.test(l)));
  rmSync(root, { recursive: true });
});

test('an excluded file needs a reason and must exist; a crate entry needs the crate', () => {
  const root = tree(['a'], ['a/src/native.rs']);
  const reg = ['[[exclude]]', 'path = "a/src/native.rs"', '', '[[exclude]]', 'path = "a/src/gone.rs"', 'reason = "x"', '', entry('missing', 95)].join('\n');
  const r = run(root, { a: [100, 100] }, reg);
  const text = r.out.join('\n');
  assert.equal(r.exit, 1);
  assert.match(text, /excluded file a\/src\/native\.rs needs a reason/);
  assert.match(text, /excluded file a\/src\/gone\.rs does not exist/);
  assert.match(text, /crate missing does not exist/);
  rmSync(root, { recursive: true });
});

test('an unreadable register line is an error, not a guess', () => {
  const root = tree(['a']);
  assert.match(run(root, { a: [100, 100] }, 'nonsense').out.join('\n'), /cannot read "nonsense"/);
  rmSync(root, { recursive: true });
});

test('the ignore regex is generated from the excluded files and matches exactly them', () => {
  const { excludes } = parseRegister('[[exclude]]\npath = "p/src/native.rs"\nreason = "r"\n[[exclude]]\npath = "q/src/bin/t.rs"\nreason = "r"\n');
  const re = new RegExp(ignoreRegex(excludes));
  assert.ok(re.test('/w/crates/p/src/native.rs'));
  assert.ok(re.test('/w/crates/q/src/bin/t.rs'));
  assert.ok(!re.test('/w/crates/p/src/nativeXrs'));
  assert.ok(!re.test('/w/crates/p/src/native.rs.bak'));
  assert.equal(ignoreRegex([]), '^$');
});

test('validateRegister takes the clock and filesystem as arguments', () => {
  const reg = parseRegister(entry('a', 95));
  const ok = validateRegister(reg, '/r', new Date('2026-11-04'), () => true, () => ['a']);
  assert.deepEqual(ok, { errors: [], warnings: [] });
});

test('a figure is shown cut like the minimum, so a drop within one hundredth never reads as equal', () => {
  const root = tree(['a']);
  const r = run(root, { a: [98759, 100000] }, entry('a', 98.76));
  assert.equal(r.exit, 1);
  assert.match(r.out[0], /98\.75 % is under its minimum 98\.76 %/);
  rmSync(root, { recursive: true });
});
