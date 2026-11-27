import assert from 'node:assert/strict';
import { test } from 'node:test';
import { execFileSync } from 'node:child_process';
import { writeFileSync } from 'node:fs';
import { join } from 'node:path';
import { makeApp } from './helpers.mjs';
import check, { absPathSites, addedLines } from '../src/checks/C171-abs-path-literals.mjs';

const lines = (text, file = 'tools/x/tests/it/a.rs') => absPathSites(text, file).map((s) => s.line);

test('C171: a Unix absolute literal passed to Path::new, PathBuf::from or a known validator is flagged', () => {
  assert.deepEqual(lines('fn t() {\n    let p = PathBuf::from("/srv/dev-targets/wt");\n}\n'), [2]);
  assert.deepEqual(lines('fn t() {\n    assert!(Path::new("/a/b").is_absolute());\n}\n'), [2]);
  assert.deepEqual(lines('fn t() {\n    assert_eq!(dev_targets_of(Some("/srv/dt/"), Some("/x/wt")), None);\n}\n'), [2]);
  assert.deepEqual(lines('fn t() {\n    let p = std::path::PathBuf::from(\n        "/dev-targets-x",\n    );\n}\n'), [3]);
});

test('C171: relative paths, text that is not a path constructor, and non-test code are fine', () => {
  assert.deepEqual(lines('fn t() {\n    let p = PathBuf::from("wt/a");\n    let s = String::from("/srv");\n    let u = "/srv/x";\n    let r = Path::new("rel");\n}\n'), []);
  assert.deepEqual(lines('fn t() {\n    // PathBuf::from("/srv/x")\n    let s = "PathBuf::from(\\"/srv\\")";\n}\n'), [], 'comments and strings');
  assert.deepEqual(lines('fn p() { let a = PathBuf::from("/usr/bin"); }\n', 'crates/a/src/lib.rs'), [], 'production code');
});

test('C171: a literal given to a helper that makes it a path of this OS is not flagged', () => {
  assert.deepEqual(lines('fn t() {\n    let p = PathBuf::from(abs("/srv/dt"));\n    let g = dev_targets_of(Some(abs("/srv")), None);\n}\n'), []);
  assert.deepEqual(lines('fn t() {\n    let g = dev_targets_of(None, Some(&"/srv/dt/wt"));\n}\n'), [2], 'a reference and Some() are still direct');
});

test('C171: test items of a src file are read; a unix-only item, a unix-only file and a waiver are not', () => {
  assert.deepEqual(lines('fn p() {}\n#[cfg(test)]\nmod tests {\n    #[test]\n    fn t() { let p = PathBuf::from("/a/b"); }\n}\n', 'crates/a/src/lib.rs'), [5]);
  assert.deepEqual(lines('#[cfg(unix)]\n#[test]\nfn t() { let p = Path::new("/dev/null"); }\n'), [], 'cfg(unix) item');
  assert.deepEqual(lines('#[cfg(not(windows))]\n#[test]\nfn t() { let p = Path::new("/dev/null"); }\n'), [], 'cfg(not(windows)) item');
  assert.deepEqual(lines('#![cfg(unix)]\nfn t() { let p = Path::new("/dev/null"); }\n'), [], 'a unix-only file');
  assert.deepEqual(lines('fn t() {\n    // conformance: abs-path — the validator is given text from a Unix tool\n    let p = Path::new("/a");\n}\n'), [], 'waiver above');
  assert.deepEqual(lines('#[cfg(windows)]\n#[test]\nfn t() { let p = Path::new("/a"); }\n'), [1 + 2], 'a windows-only item is still flagged');
});

const diffAdding = (file, ...lines) => `diff --git a/${file} b/${file}\n--- a/${file}\n+++ b/${file}\n${lines.map((n) => `@@ -${n},0 +${n} @@\n+x\n`).join('')}`;
const fakeGit = (diff) => (args) => (args[0] === 'diff' ? diff : args[0] === 'merge-base' ? 'base111\n' : 'head222\n');
const run = (files, diff, extra = {}) => check.run({ files: () => Object.keys(files), read: (f) => files[f], options: { git: fakeGit(diff), ...extra } });
const PLANTED = 'fn t() {\n    let p = PathBuf::from("/planted/abs");\n}\n';

test('C171: addedLines reads the new line numbers of a unified diff', () => {
  const d = 'diff --git a/a.rs b/a.rs\n--- a/a.rs\n+++ b/a.rs\n@@ -3,0 +4,2 @@\n+x\n+y\n@@ -9 +11 @@\n-o\n+n\ndiff --git a/gone.rs b/gone.rs\n--- a/gone.rs\n+++ /dev/null\n@@ -1,2 +0,0 @@\n';
  const m = addedLines(d);
  assert.deepEqual([...m.get('a.rs')], [4, 5, 11]);
  assert.equal(m.has('gone.rs'), false);
});

test('C171: a literal on a line the change adds blocks, with the paddock-testkit::path hint', () => {
  const out = run({ 'tools/x/tests/it/planted.rs': PLANTED }, diffAdding('tools/x/tests/it/planted.rs', 2));
  assert.equal(out.length, 1, 'the planted literal is seen (positive control)');
  assert.equal(out[0].severity, 'error');
  assert.match(out[0].fix, /paddock-testkit::path/);
  assert.match(out[0].message, /this line is new/);
});

test('C171: an existing literal only warns, and so does a new one in another file', () => {
  const files = { 'tools/x/tests/it/planted.rs': PLANTED };
  assert.equal(run(files, diffAdding('tools/x/tests/it/planted.rs', 7))[0].severity, 'warn', 'another line of the file was added');
  assert.equal(run(files, diffAdding('tools/x/tests/it/other.rs', 2))[0].severity, 'warn', 'another file was changed');
  assert.equal(run(files, '')[0].severity, 'warn', 'no change at all');
});

test('C171: a waiver passes a new literal, and a relative one is quiet', () => {
  const waived = 'fn t() {\n    // conformance: abs-path — Unix text from a Unix tool, never a validator\n    let p = PathBuf::from("/planted/abs");\n}\n';
  assert.equal(run({ 'tools/x/tests/it/w.rs': waived }, diffAdding('tools/x/tests/it/w.rs', 2, 3)).length, 0);
  assert.equal(run({ 'tools/x/tests/it/p.rs': 'fn t() { let p = PathBuf::from("rel/x"); }\n' }, diffAdding('tools/x/tests/it/p.rs', 1)).length, 0);
});

test('C171: when the base cannot be read a site is a warning, and in CI a not-measured error no waiver covers', () => {
  const broken = { git: () => { throw new Error('fatal: no merge base'); } };
  const files = { 'tools/x/tests/it/planted.rs': PLANTED };
  const local = check.run({ files: () => Object.keys(files), read: (f) => files[f], options: { ...broken, ci: false } });
  assert.ok(local.every((f) => f.severity === 'warn'), JSON.stringify(local));
  const ci = check.run({ files: () => Object.keys(files), read: (f) => files[f], options: { ...broken, ci: true } });
  assert.ok(ci.some((f) => f.severity === 'error' && f.unwaivable === true && f.outcome === 'not-measured'), JSON.stringify(ci));
});

test('C171: the check is a gate (new lines block) with id C171', () => {
  assert.equal(check.mode, 'gate');
  assert.equal(check.id, 'C171');
});

test('C171: a waiver sits on the statement: rustfmt may move the literal below the comment (the throttle move, bf651d875), and it does not reach another statement', () => {
  const spread = 'fn t() {\n    // conformance: abs-path — the expected macOS default of a modelled host, compared as text\n    assert_eq!(\n        c.root,\n        PathBuf::from("/Users/u/x")\n    );\n}\n';
  assert.deepEqual(lines(spread), [], 'the comment above the statement covers its literal');
  assert.deepEqual(lines(spread.replace('    // conformance: abs-path — the expected macOS default of a modelled host, compared as text\n    assert_eq!(', '    // conformance: abs-path — the expected macOS default of a modelled host, compared as text\n    let a = 1;\n    assert_eq!(')), [6], 'a waiver on an earlier statement does not');
  assert.deepEqual(lines('fn t() {\n    // conformance: abs-path — a reason that is long enough\n    f(g(1), PathBuf::from("/a/b"), [1, 2]);\n}\n'), [], 'a waiver above a one-line statement, as before');
});

// ---- a moved file's old sites are not "added lines": the check reads git's rename detection (ER-21) ----
const sh = (root, ...args) => execFileSync('git', ['-c', 'user.name=t', '-c', 'user.email=t@example.invalid', '-c', 'commit.gpgsign=false', ...args], { cwd: root, stdio: 'ignore' });
const BODY = Array.from({ length: 30 }, (_, i) => `    let v${i} = ${i};\n`).join('');
const OLD_RS = `fn t() {\n${BODY}    let p = PathBuf::from("/planted/abs");\n}\n`;
function movedRepo(newText) {
  const root = makeApp({ 'tools/x/tests/it/old.rs': OLD_RS });
  sh(root, 'init', '-q', '-b', 'main');
  sh(root, 'add', '.');
  sh(root, 'commit', '-q', '-m', 'base');
  sh(root, 'checkout', '-q', '-b', 'topic');
  sh(root, 'mv', 'tools/x/tests/it/old.rs', 'tools/x/tests/it/new.rs');
  if (newText) writeFileSync(join(root, 'tools/x/tests/it/new.rs'), newText);
  sh(root, 'commit', '-q', '-a', '-m', 'move');
  return root;
}
const runReal = (root) => check.run({ root, files: () => ['tools/x/tests/it/new.rs'], read: (f) => execFileSync('cat', [join(root, f)], { encoding: 'utf8' }), options: { baseRef: 'main', ci: false } });

test('C171: a pure move keeps its old sites warnings; a moved file that gains a literal still blocks on that line', () => {
  const moved = runReal(movedRepo(null));
  assert.deepEqual(moved.map((f) => f.severity), ['warn'], 'the move adds no line');
  const plus = runReal(movedRepo(OLD_RS.replace('}\n', '    let q = Path::new("/new/abs");\n}\n')));
  assert.deepEqual(plus.map((f) => f.severity).sort(), ['error', 'warn'], 'the new line blocks, the old site still warns');
  assert.match(plus.find((f) => f.severity === 'error').message, /this line is new/);
});
