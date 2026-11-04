import assert from 'node:assert/strict';
import { execFileSync, spawnSync } from 'node:child_process';
import { mkdirSync, mkdtempSync, rmSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { test } from 'node:test';
import { fileURLToPath } from 'node:url';
import { check, crateOf, floor2, measure, measureByCrate, suggest } from './coverage-gate.mjs';
import { ignoreRegex, addedTrailers, parseRegister, ratchetErrors, validateRegister, workspaceMembers } from './coverage-register.mjs';

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
  assert.equal(crateOf('/elsewhere/crates/paddock-c/src/lib.rs', '/x/crates'), null, 'a path outside the root is no crate of it');
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
  writeFileSync(join(root, 'Cargo.toml'), `[workspace]\nmembers = [\n${names.map((n) => `    "${n}",`).join('\n')}\n]\n`);
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
const run = (root, figures, register, extra = {}, now = new Date('2026-11-04T00:00:00Z')) =>
  check({ lcovText: lcovFor(root, figures), registerText: register, root, now, read: () => null, ...extra });
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

test('a figure more than 0.05 above its minimum is an error (raise it); a crate at 99 % only warns to drop its entry', () => {
  const root = tree(['a', 'b', 'c']);
  const r = run(root, { a: [98, 100], b: [99, 100], c: [9503, 10000] }, entry('a', 95) + entry('b', 95) + entry('c', 95));
  assert.equal(r.exit, 1);
  assert.ok(r.out.some((l) => l.startsWith('::error::crate a: 98.00 %') && /raise the minimum to 98\.00/.test(l)));
  assert.ok(r.out.some((l) => l.startsWith('::warning::crate b') && /remove its \[\[crate\]\] entry/.test(l)));
  assert.ok(!r.out.some((l) => l.startsWith('::error::crate c')), 'within 0.05 above the minimum passes');
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
  assert.match(text, /crate missing is not a member of the workspace/);
  assert.equal(r.exit, 1);
  assert.match(text, /excluded file a\/src\/native\.rs needs a reason/);
  assert.match(text, /excluded file a\/src\/gone\.rs does not exist/);
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
  const ok = validateRegister(reg, '/r', new Date('2026-11-04'), { exists: () => true, members: ['a'] });
  assert.deepEqual(ok, { errors: [], warnings: [] });
});

test('a figure is shown cut like the minimum, so a drop within one hundredth never reads as equal', () => {
  const root = tree(['a']);
  const r = run(root, { a: [98759, 100000] }, entry('a', 98.76));
  assert.equal(r.exit, 1);
  assert.match(r.out[0], /98\.75 % is under its minimum 98\.76 %/);
  rmSync(root, { recursive: true });
});

test('a # inside a quoted value stays; a comment after it goes; an unknown key is an error', () => {
  const reg = parseRegister('[[crate]]\nname = "a"  # the crate\nreason = "issue #12 stays"\nbogus = "x"\n');
  assert.equal(reg.crates[0].reason, 'issue #12 stays');
  assert.equal(reg.crates[0].name, 'a');
  assert.match(reg.problems.join('\n'), /unknown key "bogus" in \[\[crate\]\]/);
});

test('workspace members are read from the Cargo.toml', () => {
  assert.deepEqual(workspaceMembers('[workspace]\nresolver = "3"\nmembers = [\n  "a", # one\n  "b-c",\n]\n'), ['a', 'b-c']);
  assert.equal(workspaceMembers('[package]\n'), null);
});

// The ratchet against the base register.
const reg = (text) => parseRegister(text);
const base = entry('a', 95) + '[[exclude]]\npath = "a/src/n.rs"\nreason = "r"\n';

test('ratchet: a lowered minimum, a new crate entry and a new exclude each fail without the trailer', () => {
  const none = new Set();
  assert.deepEqual(ratchetErrors(reg(base), reg(base), none), []);
  assert.match(ratchetErrors(reg(base), reg(entry('a', 94.9) + '[[exclude]]\npath = "a/src/n.rs"\nreason = "r"\n'), none)[0], /minimum lowered from 95 % to 94.9 %/);
  assert.match(ratchetErrors(reg(base), reg(base + entry('b', 96)), none)[0], /crate b: a new \[\[crate\]\] entry/);
  assert.match(ratchetErrors(reg(base), reg(base + '[[exclude]]\npath = "b/src/n.rs"\nreason = "r"\n'), none)[0], /file b\/src\/n\.rs: a new \[\[exclude\]\] entry/);
  assert.deepEqual(ratchetErrors(reg(base), reg(entry('a', 99.5).replace('99.5', '96')), none), [], 'a raise is fine');
});

test('ratchet: a trailer allows only a new crate or a new exclude, never a lower minimum; no base register is the seed', () => {
  const approved = addedTrailers([
    'Coverage-lowered: b because the adapter needs a desktop approved: user',
    'coverage-lowered: x lower-case key approved: user',
    'Coverage-lowered: c reason approved: agent',
    'Coverage-lowered: a reason approved: user',
  ]);
  assert.deepEqual([...approved].sort(), ['a', 'b']);
  assert.deepEqual(ratchetErrors(reg(base), reg(base + entry('b', 96)), approved), []);
  assert.equal(ratchetErrors(reg(base), reg(base + entry('c', 96)), approved).length, 1);
  const lowered = ratchetErrors(reg(base), reg(entry('a', 94) + '[[exclude]]\npath = "a/src/n.rs"\nreason = "r"\n'), approved);
  assert.match(lowered[0], /minimum lowered from 95 % to 94 %.*no trailer allows a lower one/, 'the trailer for a does not exempt a lowered minimum');
  assert.deepEqual(ratchetErrors(null, reg(base), new Set()), []);
});

test('a TODO reason fails validation', () => {
  const root = tree(['a']);
  assert.match(run(root, { a: [95, 100] }, entry('a', 95, 'reason = "TODO: why these lines cannot be covered"\nreview = "2027-02-01"')).out.join('\n'), /placeholder reason/);
  rmSync(root, { recursive: true });
});

test('the raise check allows max(0.05 %, 2 lines): a crate of 100 lines may be 2 lines above its minimum, one of 100000 only 0.05 %', () => {
  const root = tree(['a', 'b']);
  assert.equal(run(root, { a: [97, 100] }, entry('a', 95)).exit, 0);
  const r = run(root, { a: [98, 100], b: [96000, 100000] }, entry('a', 95) + entry('b', 95));
  assert.ok(r.out.some((l) => l.startsWith('::error::crate a: 98.00 %')));
  assert.ok(r.out.some((l) => l.startsWith('::error::crate b: 96.00 %')));
  assert.equal(run(root, { a: [95, 100], b: [95040, 100000] }, entry('a', 95) + entry('b', 95)).exit, 0);
  assert.equal(run(root, { a: [94, 100] }, entry('a', 95)).exit, 1, 'under the minimum stays exact');
  rmSync(root, { recursive: true });
});

test('suggest prints the whole register, measured, and is stable on its own output', () => {
  const root = tree(['a', 'b', 'c']);
  const text = `# header\n\n${entry('a', 90, 'reason = "kept"\nreview = "2027-02-01"')}\n[[exclude]]\npath = "a/src/n.rs"\nreason = "r"\n`;
  const m = measureByCrate(lcovFor(root, { a: [9503, 10000], b: [100, 100], c: [80, 100] }), () => null, root);
  const out = suggest(m, text);
  assert.match(out, /^# header\n\n\[\[crate\]\]\nname = "a"\nminimum = 95\.03\nreason = "kept"\nreview = "2027-02-01"\n\n\[\[crate\]\]\nname = "c"\nminimum = 80\.00\nreason = "TODO/);
  assert.match(out, /\[\[exclude\]\]\npath = "a\/src\/n\.rs"\nreason = "r"\n$/);
  assert.ok(!out.includes('name = "b"'));
  assert.equal(suggest(m, out), out);
  rmSync(root, { recursive: true });
});

test('suggest keeps a registered crate that has no measured lines, and says so', () => {
  const root = tree(['a', 'z']);
  const m = measureByCrate(lcovFor(root, { a: [90, 100] }), () => null, root);
  const notes = [];
  const out = suggest(m, entry('a', 90) + entry('z', 93.5), notes);
  assert.match(out, /name = "z"\nminimum = 93\.50/);
  assert.ok(notes.some((n) => /crate z has no measured lines/.test(n)));
  assert.equal(suggest(m, out), out);
  rmSync(root, { recursive: true });
});

test('the ratchet CLI reads the base register and the trailer block from git, and fails closed', () => {
  const root = tree(['a', 'b']);
  const git = (...a) => execFileSync('git', ['-c', 'user.name=t', '-c', 'user.email=t@example.invalid', ...a], { cwd: root, encoding: 'utf8' });
  git('init', '-q');
  writeFileSync(join(root, 'x.txt'), '');
  git('add', '.');
  git('commit', '-q', '-m', 'chore: seed');
  const seed = git('rev-parse', 'HEAD').trim();
  writeFileSync(join(root, 'coverage.toml'), entry('a', 95));
  git('add', '.');
  git('commit', '-q', '-m', 'ci: register');
  const gate = fileURLToPath(new URL('./coverage-gate.mjs', import.meta.url));
  const env = { ...process.env, GITHUB_EVENT_NAME: '', GITHUB_REF: '' };
  const cli = (...extra) => spawnSync('node', [gate, 'ratchet', join(root, 'coverage.toml'), ...extra], { encoding: 'utf8', env });
  assert.match(cli('--base', seed).stdout, /initial seed/, 'a base whose tree has no register is the seed');
  assert.equal(cli('--base', seed).status, 0);
  const registered = git('rev-parse', 'HEAD').trim();
  writeFileSync(join(root, 'coverage.toml'), entry('a', 94) + entry('b', 96));
  git('commit', '-q', '-a', '-m', 'ci: lower\n\nCoverage-lowered: a moved approved: user\nCoverage-lowered: b new approved: user');
  const lowered = cli('--base', registered);
  assert.equal(lowered.status, 1);
  assert.match(lowered.stdout, /::error::crate a: minimum lowered from 95 % to 94 %/);
  assert.ok(!/crate b/.test(lowered.stdout), 'the trailer in the trailer block allows the new crate b');
  assert.match(cli('--base', 'nonexistent').stdout, /::error::git cat-file -e failed/);
  assert.equal(cli('--base', 'nonexistent').status, 1);
  assert.match(cli().stdout, /::notice::coverage ratchet not checked: no base/);
  const strict = spawnSync('node', [gate, 'ratchet', join(root, 'coverage.toml')], { encoding: 'utf8', env: { ...env, GITHUB_EVENT_NAME: 'pull_request' } });
  assert.match(strict.stdout, /::warning::coverage ratchet not checked: no base revision on a pull request/);
  rmSync(root, { recursive: true });
});
