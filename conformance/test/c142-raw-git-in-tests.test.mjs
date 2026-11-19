import assert from 'node:assert/strict';
import { test } from 'node:test';
import C142, { rawGitSites } from '../src/checks/C142-raw-git-in-tests.mjs';

const lines = (code, file = 'conformance/test/a.test.mjs') => rawGitSites(code, file).map((s) => s.line);

test('C142: a raw git write in a test is a site, however the options and arguments are spelled', () => {
  assert.deepEqual(lines("spawnSync('git', ['init', '-q'], { cwd: d });\n"), [1]);
  assert.deepEqual(lines("const g = (...a) => execFileSync('git', ['-c', 'user.name=t', '-c', 'user.email=t@t', 'commit', ...a], { cwd });\n"), [1]);
  assert.deepEqual(lines("execFileSync(\"git\", ['-C', dir, 'config', 'user.name', 't']);\n"), [1]);
  assert.deepEqual(lines("spawn('git', ['checkout', '-b', 'x']);\n"), [1]);
  assert.deepEqual(lines("execSync('git init');\n"), [1]);
  assert.deepEqual(lines("execFileSync('git', a, { cwd });\n"), [1], 'a variable argument list cannot be read: a site');
  assert.deepEqual(lines("execFileSync('git', ['-C', dir, ...a]);\n"), [1], 'a spread after the options: the verb is unknown');
  assert.deepEqual(lines("spawnSync(\n  'git',\n  ['clone', 'a', 'b'],\n);\n"), [1]);
});

test('C142: read-only verbs, the scratch-git helper, comments and text inside strings are not sites', () => {
  assert.deepEqual(lines("spawnSync('git', ['ls-files', '*.mjs'], { cwd: PADDOCK });\n"), []);
  assert.deepEqual(lines("execFileSync('git', ['-c', 'x=y', 'rev-parse', 'HEAD']);\n"), []);
  assert.deepEqual(lines("const r = gitResult(dir, ['init', '-q']); git(dir, 'config', 'a', 'b');\n"), []);
  assert.deepEqual(lines("assert.deepEqual(lines('spawnSync(\"git\", [\"worktree\", \"prune\"]);\\n', 'a.mjs'), [1]);\n"), [], 'a test of a lint names the call in a string');
  assert.deepEqual(lines("// spawnSync('git', ['init']) is what the helper replaces\nconst x = 1;\n"), [], 'a commented-out call');
});

test('C142: a waiver with a reason on the line or the line above removes the site; one without a reason does not', () => {
  assert.deepEqual(lines("// conformance: raw-git — a read-only check-attr over the real tree\nspawnSync('git', a);\n"), []);
  assert.deepEqual(lines("spawnSync('git', a); // conformance: raw-git — reads the real tree\n"), []);
  assert.deepEqual(lines("// conformance: raw-git\nspawnSync('git', a);\n"), [2]);
});

test('C142: only test files are read; the helper itself and other code are not', () => {
  assert.deepEqual(rawGitSites("spawnSync('git', ['init']);\n", 'conformance/test/lib/scratch-git.mjs'), []);
  assert.deepEqual(rawGitSites("spawnSync('git', ['init']);\n", 'scripts/publish.mjs'), []);
  assert.deepEqual(rawGitSites("spawnSync('git', ['init']);\n", 'scripts/publish.test.mjs').length, 1);
  assert.deepEqual(rawGitSites("spawnSync('git', ['init']);\n", 'conformance/test/helpers.mjs').length, 1);
});

test('C142 positive control: the check reports a raw spawn as a warning, and nothing once the helper is used', () => {
  const runOn = (files) => C142.run({ files: () => Object.keys(files), read: (f) => files[f] });
  const bad = runOn({ 'conformance/test/x.test.mjs': "import { spawnSync } from 'node:child_process';\nspawnSync('git', ['init', '-q']);\n" });
  assert.equal(bad.length, 1);
  assert.equal(bad[0].severity, 'warn');
  assert.equal(bad[0].file, 'conformance/test/x.test.mjs');
  assert.equal(bad[0].line, 2);
  assert.deepEqual(runOn({ 'conformance/test/x.test.mjs': "import { gitResult } from './lib/scratch-git.mjs';\ngitResult(dir, ['init', '-q']);\n" }), []);
  assert.equal(C142.mode, 'report-only');
});
