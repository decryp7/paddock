import assert from 'node:assert/strict';
import { execFileSync } from 'node:child_process';
import { test } from 'node:test';
import { sitesOf, summaryTable } from './e2e-ready-report.mjs';

test('sitesOf keeps the sites of the ready-reads rule only', () => {
  const results = [{ filePath: '/r/a.ts', messages: [{ ruleId: 'paddock/e2e-ready-reads', line: 4, message: 'x' }, { ruleId: 'other', line: 9, message: 'y' }] }, { filePath: '/r/b.ts', messages: [] }];
  assert.deepEqual(sitesOf(results), [{ file: '/r/a.ts', line: 4, message: 'x' }]);
});

test('summaryTable lists each project and the total', () => {
  const t = summaryTable({ 'apps/debrief/app': 3, ui: 2 });
  assert.match(t, /\| apps\/debrief\/app \| 3 \|/);
  assert.match(t, /\| \*\*total \(measured projects\)\*\* \| \*\*5\*\* \|/);
  assert.match(t, /projects not measured\*\* \| \*\*0\*\*/);
});

test('summaryTable shows a project eslint failed in as not measured, never as zero sites (ER-33)', () => {
  const t = summaryTable({ ui: 2 }, ['apps/dyno/app', 'apps/pitwall/app']);
  assert.match(t, /\| apps\/dyno\/app \| not measured \|/);
  assert.match(t, /\| apps\/pitwall\/app \| not measured \|/);
  assert.match(t, /projects not measured\*\* \| \*\*2\*\*/);
  assert.doesNotMatch(t, /apps\/dyno\/app \| 0/);
});

test('the script measures a project whose plugin is registered only inside the e2e block (a --rule override cannot find it: "could not find plugin paddock")', () => {
  // The committed baseline and gate are not this test's subject (the gate is blocking): it runs with its own, report-only.
  const { out } = gate({}, null);
  assert.match(out, /scripts\/fixtures\/e2e-ready\/e2e\/bad\.ts:6/, out);
  assert.match(out, /\| scripts\/fixtures\/e2e-ready \| 1 \|/);
  assert.match(out, /projects not measured\*\* \| \*\*0\*\*/);
});

// The gate (baseline that only shrinks, ER-46; blocking switch, ER-41).
import { mkdtempSync, readFileSync, rmSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { baselineGrowth, compareToBaseline, countsByFile, findings } from './e2e-ready-report.mjs';

const FIXTURE_FILE = 'scripts/fixtures/e2e-ready/e2e/bad.ts';
/** Runs the script over the fixture with a temp baseline and gate; returns { code, out, baseline }. */
function gate(baseline, blockingSinceRun, extra = [], waivers = {}, project = 'scripts/fixtures/e2e-ready') {
  const dir = mkdtempSync(join(tmpdir(), 'ready-gate-'));
  try {
    const b = join(dir, 'baseline.json');
    const g = join(dir, 'gate.json');
    const w = join(dir, 'waivers.json');
    writeFileSync(w, JSON.stringify(waivers));
    writeFileSync(b, JSON.stringify(baseline));
    writeFileSync(g, JSON.stringify({ blockingSinceRun }));
    let code = 0;
    let out = '';
    try {
      out = execFileSync('node', ['scripts/e2e-ready-report.mjs', project, '--baseline', b, '--waivers', w, '--gate', g, '--base', 'HEAD', ...extra], { encoding: 'utf8', cwd: new URL('..', import.meta.url) });
    } catch (e) {
      code = e.status;
      out = e.stdout.toString();
    }
    return { code, out, baseline: JSON.parse(readFileSync(b, 'utf8')), waivers: JSON.parse(readFileSync(w, 'utf8')) };
  } finally {
    rmSync(dir, { recursive: true, force: true });
  }
}

test('countsByFile counts per file with the root stripped, sorted', () => {
  assert.deepEqual(countsByFile([{ file: '/r/b.ts' }, { file: '/r/a.ts' }, { file: '/r/b.ts' }], '/r'), { 'a.ts': 1, 'b.ts': 2 });
});

test('compareToBaseline: more sites than recorded (or an unrecorded file) is added, fewer is fixed', () => {
  assert.deepEqual(compareToBaseline({ 'a.ts': 2, 'c.ts': 1 }, { 'a.ts': 1, 'b.ts': 1 }), {
    added: [{ file: 'a.ts', now: 2, recorded: 1 }, { file: 'c.ts', now: 1, recorded: 0 }],
    fixed: [{ file: 'b.ts', now: 0, recorded: 1 }],
  });
});

test('baselineGrowth: a grown or new entry against the merge base is growth; a shrunk or removed one is not (ER-46)', () => {
  assert.deepEqual(baselineGrowth({ 'a.ts': 2, 'b.ts': 1 }, { 'a.ts': 3, 'b.ts': 0, 'c.ts': 1 }), [{ file: 'a.ts', base: 2, head: 3 }, { file: 'c.ts', base: 0, head: 1 }]);
  assert.deepEqual(baselineGrowth({ 'a.ts': 2 }, { 'a.ts': 1 }), []);
});

test('findings: growth asks for the user\'s approval, a new site names the helper, an unmeasured project is an error (ER-33)', () => {
  const lines = findings({ added: [{ file: 'a.ts', now: 2, recorded: 1 }], fixed: [], grown: [{ file: 'b.ts', base: 1, head: 2 }], unmeasured: ['ui'] });
  assert.match(lines.join('\n'), /a\.ts::e2e ready-reads: 1 new read.*ready helper/);
  assert.match(lines.join('\n'), /b\.ts::.*needs the user's approval/);
  assert.match(lines.join('\n'), /ui not measured/);
});

test('positive control: a new unready read (an empty baseline) is reported, and blocks once blockingSinceRun is set', () => {
  const report = gate({}, null);
  assert.equal(report.code, 0, report.out);
  assert.match(report.out, new RegExp(`::error file=${FIXTURE_FILE}::.*1 new read`));
  assert.match(report.out, /report-only \(ER-41\)/);
  const blocking = gate({}, 8300);
  assert.equal(blocking.code, 1, blocking.out);
  assert.match(blocking.out, /blocking since run 8300/);
});

test('the recorded site passes; a baseline above the count (a fixed site) asks to shrink it', () => {
  const same = gate({ [FIXTURE_FILE]: 1 }, 8300);
  assert.equal(same.code, 0, same.out);
  assert.match(same.out, /0 finding/);
  const stale = gate({ [FIXTURE_FILE]: 2 }, 8300);
  assert.equal(stale.code, 1, stale.out);
  assert.match(stale.out, /--update/);
});

test('--update shrinks the baseline and refuses growth', () => {
  const shrunk = gate({ [FIXTURE_FILE]: 3, 'gone.ts': 1 }, null, ['--update']);
  assert.equal(shrunk.code, 0, shrunk.out);
  assert.deepEqual(shrunk.baseline, { [FIXTURE_FILE]: 1 });
  const grew = gate({}, null, ['--update']);
  assert.equal(grew.code, 1, grew.out);
  assert.match(grew.out, /only shrinks/);
  assert.deepEqual(grew.baseline, {});
});

test('a project eslint cannot measure fails the gate (ER-33)', () => {
  const dir = mkdtempSync(join(tmpdir(), 'ready-gate-'));
  try {
    writeFileSync(join(dir, 'b.json'), '{}');
    writeFileSync(join(dir, 'w.json'), '{}');
    writeFileSync(join(dir, 'g.json'), '{"blockingSinceRun":1}');
    assert.throws(
      () => execFileSync('node', ['scripts/e2e-ready-report.mjs', 'scripts/fixtures/no-such-project', '--baseline', join(dir, 'b.json'), '--waivers', join(dir, 'w.json'), '--gate', join(dir, 'g.json'), '--base', 'HEAD'], { cwd: new URL('..', import.meta.url), stdio: 'pipe' }),
      (e) => e.status === 1 && /no-such-project not measured/.test(e.stdout.toString()),
    );
  } finally {
    rmSync(dir, { recursive: true, force: true });
  }
});

test('the committed baseline is a map of positive counts for e2e files', () => {
  const b = JSON.parse(readFileSync(new URL('./e2e-ready-baseline.json', import.meta.url), 'utf8'));
  for (const [f, n] of Object.entries(b)) {
    assert.match(f, /e2e\//, f);
    assert.ok(Number.isInteger(n) && n > 0, f);
  }
});

const WAIVED = 'scripts/fixtures/e2e-ready-waived';
const WAIVED_FILE = `${WAIVED}/e2e/waived.ts`;

test('a waiver silences the site but is counted: an unrecorded waiver needs the user\'s approval (positive control, ER-46)', () => {
  const none = gate({}, 8300, [], {}, WAIVED);
  assert.equal(none.code, 1, none.out);
  assert.match(none.out, new RegExp(`::error file=${WAIVED_FILE}::.*1 new waiver.*needs the user's approval`));
  assert.doesNotMatch(none.out, /new read/, 'the waived read is not a site');
  const recorded = gate({}, 8300, [], { [WAIVED_FILE]: 1 }, WAIVED);
  assert.equal(recorded.code, 0, recorded.out);
});

test('removing a waiver asks to shrink the list; --update shrinks it and refuses a new waiver', () => {
  const stale = gate({}, 8300, [], { [WAIVED_FILE]: 1, 'gone.e2e.ts': 2 }, WAIVED);
  assert.equal(stale.code, 1, stale.out);
  assert.match(stale.out, /gone\.e2e\.ts::.*2 waiver\(s\) removed/);
  const shrunk = gate({}, null, ['--update'], { [WAIVED_FILE]: 1, 'gone.e2e.ts': 2 }, WAIVED);
  assert.equal(shrunk.code, 0, shrunk.out);
  assert.deepEqual(shrunk.waivers, { [WAIVED_FILE]: 1 });
  const grew = gate({}, null, ['--update'], {}, WAIVED);
  assert.equal(grew.code, 1, grew.out);
  assert.deepEqual(grew.waivers, {});
});

test('findings: a waiver list that grew against the merge base needs the user\'s approval', () => {
  const lines = findings({ added: [], fixed: [], grown: [], unmeasured: [], waiverGrown: [{ file: 'a.ts', base: 0, head: 1 }] });
  assert.match(lines.join('\n'), /a\.ts::e2e ready-reads waivers grew against the merge base.*needs the user's approval/);
});

test('the committed waiver list is a map of positive counts for e2e files', () => {
  const w = JSON.parse(readFileSync(new URL('./e2e-ready-waivers.json', import.meta.url), 'utf8'));
  for (const [f, n] of Object.entries(w)) {
    assert.match(f, /e2e\//, f);
    assert.ok(Number.isInteger(n) && n > 0, f);
  }
});
