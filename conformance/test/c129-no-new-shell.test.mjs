import assert from 'node:assert/strict';
import { test } from 'node:test';
import { execFileSync } from 'node:child_process';
import { chmodSync, readFileSync, writeFileSync } from 'node:fs';
import { join } from 'node:path';
import { fileURLToPath } from 'node:url';
import { globRegExp, inlineRun, isShell, lineCount, parseBaseline, parseExceptions } from '../src/checks/C129-no-new-shell.mjs';
import { findingsOf as run, makeApp } from './helpers.mjs';

/** The findings of C129 over a fixture, without the note that a fixture has no merge base to compare the baseline with. */
const findingsOf = async (id, root, opts) => (await run(id, root, opts)).filter((f) => !/shrink-only rule is not enforced here/.test(f.message));

const BASELINE = 'conformance/shell-baseline.json';
const EXCEPTIONS = 'conformance/shell-exceptions.json';
const TOOL = fileURLToPath(new URL('../bin/paddock-shell-baseline.mjs', import.meta.url));
const baseline = (files = []) => JSON.stringify({ files: files.map(([path, lines]) => ({ path, lines })) });
const exceptions = (list = []) => JSON.stringify({ exceptions: list.map(([path, id = 'E1']) => ({ path, id, reason: 'it must start first', owner: 'plan' })) });
const tree = (extra = {}, { base = [], exc = [] } = {}) => makeApp({ [BASELINE]: baseline(base), [EXCEPTIONS]: exceptions(exc), ...extra });
const where = (found) => found.map((f) => [f.file, f.message.split(/[:(]/)[0].trim()]);
const regenerate = `node conformance/bin/paddock-shell-baseline.mjs --init`;

test('C129 bad: a new shell file of every kind is a warning at line 1 (ER-33: the probe sees a planted file)', async () => {
  const files = {};
  for (const name of ['a.sh', 'b.bash', 'c.zsh', 'd.ps1', 'e.psm1', 'f.py', 'g.cmd', 'h.bat', 'dir/I.SH']) files[name] = 'echo hi\n';
  files.run = '#!/usr/bin/env bash\necho hi\n';
  files['bin/tool'] = '#!/usr/bin/env -S python3 -I\nprint(1)\n';
  files['bin/pwsh-tool'] = '#!/usr/bin/pwsh\nWrite-Host 1\n';
  const found = await findingsOf('C129', tree(files));
  assert.deepEqual(
    found.map((f) => f.file).sort(),
    ['a.sh', 'b.bash', 'bin/pwsh-tool', 'bin/tool', 'c.zsh', 'd.ps1', 'dir/I.SH', 'e.psm1', 'f.py', 'g.cmd', 'h.bat', 'run'],
  );
  assert.ok(found.every((f) => f.severity === 'warn' && f.line === 1), 'report-only until its first green run (ER-41)');
  assert.match(found[0].message, /new shell file \(1 lines\)/);
});

test('C129 good: a node script, a text file, an extensionless file without a shell shebang, a binary', async () => {
  const found = await findingsOf('C129', tree({ 'a.mjs': '#!/usr/bin/env node\n', 'LICENSE': 'MIT\n', 'Dockerfile': 'FROM x\n', 'bin/node-tool': '#!/usr/bin/env node\n', 'blob': '#!/bin/sh\0\n', 'x/y.txt': '#!/bin/sh\n' }));
  assert.deepEqual(found, []);
});

test('C129 the baseline: a listed file passes, a grown one is a finding, a shrunk one is fine', async () => {
  const files = { 'a.sh': 'x\n'.repeat(5), 'b.sh': 'x\n'.repeat(7), 'c.sh': 'x\n'.repeat(2) };
  const found = await findingsOf('C129', tree(files, { base: [['a.sh', 5], ['b.sh', 6], ['c.sh', 9]] }));
  assert.deepEqual(where(found), [['b.sh', 'shell file grew from 6 to 7 lines']]);
});

test('C129 exceptions: a path and a pattern are skipped, an exception that names no file is stale', async () => {
  const files = { dev: '#!/bin/sh\n', 'fixtures/x/a.sh': 'x\n', 'scripts/vm-win/one.ps1': 'x\n', 'scripts/vm-win/sub/two.ps1': 'x\n' };
  const exc = [['dev'], ['fixtures/', 'E6'], ['scripts/vm-win/*.ps1', 'E5'], ['gone/*.sh', 'E5']];
  const found = await findingsOf('C129', tree(files, { exc }));
  assert.deepEqual(where(found), [['scripts/vm-win/sub/two.ps1', 'new shell file'], [EXCEPTIONS, 'stale exception']]);
});

test('C129 the baseline only shrinks: an entry whose file is gone is stale, and the finding names the command', async () => {
  const found = await findingsOf('C129', tree({ 'a.sh': 'x\n' }, { base: [['a.sh', 1], ['gone.sh', 3]] }));
  assert.deepEqual(where(found.filter((f) => f.file === BASELINE && /stale/.test(f.message))), [[BASELINE, 'stale baseline entry']]);
  assert.match(found.find((f) => /stale/.test(f.message)).fix, /paddock-shell-baseline\.mjs --shrink/);
});

test('C129 a missing, malformed or incomplete list makes the check broken: one finding, nothing else', async () => {
  for (const [path, text] of [
    [BASELINE, null], [BASELINE, 'nope'], [BASELINE, '{"files": [{"path": "a.sh"}]}'], [BASELINE, '{}'],
    [EXCEPTIONS, null], [EXCEPTIONS, 'nope'], [EXCEPTIONS, '{"exceptions": [{"path": "a", "id": "E9", "reason": "long enough", "owner": "x"}]}'],
    [EXCEPTIONS, '{"exceptions": [{"path": "a", "id": "E1", "reason": "short", "owner": "x"}]}'],
  ]) {
    const files = { 'a.sh': 'x\n', [BASELINE]: baseline(), [EXCEPTIONS]: exceptions() };
    if (text === null) delete files[path];
    else files[path] = text;
    const found = await findingsOf('C129', makeApp(files));
    assert.deepEqual(found.map((f) => [f.file, f.outcome]), [[path, 'check-broken']], `${path} ${text}`);
  }
});

test('C129 a file that cannot be read is not measured, at that file', { skip: process.getuid?.() === 0 && 'root reads a 000 file' }, async () => {
  const root = tree({ 'scripts/a.sh': 'x\n' }, { base: [['scripts/a.sh', 1]] });
  chmodSync(join(root, 'scripts/a.sh'), 0o000);
  try {
    const found = await findingsOf('C129', root);
    assert.deepEqual(found.map((f) => [f.file, f.severity, f.outcome]), [['scripts/a.sh', 'error', 'not-measured']]);
  } finally {
    chmodSync(join(root, 'scripts/a.sh'), 0o644);
  }
});

test('C129 callers: a script a workflow, ci contract or lefthook names that the tree lacks is a warning, with the three ways to resolve a path', async () => {
  const files = {
    '.gitea/workflows/x.yml': 'jobs:\n  a:\n    steps:\n      - run: scripts/gone.sh\n      - run: bash ./scripts/here.sh\n      - run: scripts/in-app.sh\n      - run: scripts/$NAME.sh\n      - run: ls scripts/*.sh\n      # scripts/commented.sh\n      - run: echo conformance/ci/gone.sh.bak\n',
    'scripts/here.sh': 'x\n',
    'apps/dyno/scripts/in-app.sh': 'x\n',
    'apps/dyno/paddock.ci.json': '{"prepare": ["scripts/in-app.sh", "scripts/nope.ps1"]}',
    'lefthook.yml': 'pre-push:\n  run: ./scripts/here.sh\n',
  };
  const found = (await findingsOf('C129', tree(files, { base: [['scripts/here.sh', 1], ['apps/dyno/scripts/in-app.sh', 1]] }))).filter((f) => /names the script/.test(f.message));
  assert.deepEqual(found.map((f) => [f.file, f.line, f.message.match(/`(.*)`/)[1]]), [['.gitea/workflows/x.yml', 4, 'scripts/gone.sh'], ['apps/dyno/paddock.ci.json', 1, 'scripts/nope.ps1']]);
});

test('C129 inline run lines: block, folded and one-line values, comments and blank lines not counted', () => {
  const yaml = ['jobs:', '  a:', '    steps:', '      - run: |', '          echo a', '', '          # note', '          echo b', '      - run: echo c', '        run-extra: no', '      - name: x', '        run: >-', '          one', '          two', '      # run: echo skipped', '      - uses: y'].join('\n');
  assert.deepEqual(inlineRun(yaml), { blocks: 3, lines: 5 });
  assert.deepEqual(inlineRun('a: 1\n'), { blocks: 0, lines: 0 });
});

test('C129 helpers: line counts, shebang kinds, patterns, list parsers', () => {
  assert.deepEqual(['', 'a', 'a\n', 'a\nb', 'a\n\n'].map(lineCount), [0, 1, 1, 2, 2]);
  assert.equal(isShell('x/run', () => '#!/bin/sh\n'), true);
  assert.equal(isShell('x/run', () => '#!/usr/bin/env python3\n'), true);
  assert.equal(isShell('x/run', () => '#!/usr/bin/env node\n'), false);
  assert.equal(isShell('x/run', () => '# !/bin/sh\n'), false);
  assert.equal(isShell('x/run.txt', () => '#!/bin/sh\n'), false);
  assert.equal(isShell('x/.gitignore', () => '#!/bin/sh\n'), true, 'a dotfile has no extension');
  assert.equal(globRegExp('scripts/vm-win/*.ps1').test('scripts/vm-win/a.ps1'), true);
  assert.equal(globRegExp('scripts/vm-win/*.ps1').test('scripts/vm-win/x/a.ps1'), false);
  assert.equal(globRegExp('tools/golden/').test('tools/golden/a/b.sh'), true);
  assert.deepEqual(parseBaseline(baseline([['a.sh', 2]])), [{ path: 'a.sh', lines: 2 }]);
  assert.equal(parseExceptions(exceptions([['dev', 'E1']]))[0].re.test('dev'), true);
  assert.throws(() => parseBaseline('{"files": [{"path": "a", "lines": -1}]}'), /whole number/);
});

const git = (cwd, ...args) => execFileSync('git', ['-c', 'user.name=t', '-c', 'user.email=t@example.com', '-c', 'commit.gpgsign=false', ...args], { cwd, encoding: 'utf8' });
test('C129 the baseline against its merge base: an added entry and a raised count are findings, a lowered one is not', async () => {
  const files = { 'a.sh': 'x\n'.repeat(4), 'b.sh': 'x\n'.repeat(8), 'c.sh': 'x\n'.repeat(2), 'd.sh': 'x\n' };
  const root = tree(files, { base: [['a.sh', 4], ['b.sh', 5], ['c.sh', 3]] });
  git(root, 'init', '-q', '-b', 'main');
  git(root, 'add', '-A');
  git(root, 'commit', '-q', '-m', 'base');
  git(root, 'checkout', '-q', '-b', 'work');
  // b raised to 8, c lowered to 2, d added
  writeFileSync(join(root, BASELINE), baseline([['a.sh', 4], ['b.sh', 8], ['c.sh', 2], ['d.sh', 1]]));
  git(root, 'commit', '-q', '-am', 'edit');
  process.env.PADDOCK_BASE_REF = 'main';
  try {
    const found = await run('C129', root);
    assert.deepEqual(found.map((f) => f.message), ['the baseline raised b.sh from 5 to 8 lines; it may only shrink', found[1].message]);
    assert.match(found[1].message, /the baseline grew: 1 entry is not in the baseline at the merge base/);
  } finally {
    delete process.env.PADDOCK_BASE_REF;
  }
});

test('C129 the regeneration tool: --shrink removes stale entries and lowers counts, never adds or raises; --init writes from scratch', () => {
  const root = tree({ 'a.sh': 'x\n'.repeat(3), 'b.sh': 'x\n'.repeat(9), 'new.sh': 'x\n', dev: '#!/bin/sh\n' }, { base: [['a.sh', 5], ['b.sh', 4], ['gone.sh', 2]], exc: [['dev']] });
  const run = (...args) => execFileSync('node', [TOOL, '--root', root, ...args], { encoding: 'utf8' });
  assert.match(run(), /3 shell files/);
  run('--shrink');
  assert.deepEqual(parseBaseline(readFileSync(join(root, BASELINE), 'utf8')), [{ path: 'a.sh', lines: 3 }, { path: 'b.sh', lines: 4 }]);
  run('--init');
  assert.deepEqual(parseBaseline(readFileSync(join(root, BASELINE), 'utf8')).map((e) => e.path), ['a.sh', 'b.sh', 'new.sh']);
  assert.ok(readFileSync(join(root, BASELINE), 'utf8').split('\n').filter((l) => l.includes('"path"')).length === 3, 'one entry per line');
  assert.ok(regenerate.includes('--init'));
});
