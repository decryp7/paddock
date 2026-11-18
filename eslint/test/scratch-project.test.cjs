'use strict';
// The scratch helper (scripts/test-helpers/scratch-project.cjs, ER-45): what a test plants stays in the copy, the copy is removed, and a signal mid-run removes it too.
const assert = require('node:assert/strict');
const { spawn, execFileSync } = require('node:child_process');
const { existsSync, mkdirSync, readdirSync, realpathSync, writeFileSync } = require('node:fs');
const { tmpdir } = require('node:os');
const { join, resolve } = require('node:path');
const { test } = require('node:test');
const { scratchProject } = require('../../scripts/test-helpers/scratch-project.cjs');

const REPO = resolve(__dirname, '..', '..');
const status = () => execFileSync('git', ['status', '--porcelain'], { cwd: REPO, encoding: 'utf8' });

test('scratchProject: the layout is there, a planted file stays in the copy, cleanup removes it', async () => {
  const before = status();
  const scratch = scratchProject(REPO, 'apps/dyno/app');
  try {
    assert.ok(existsSync(join(scratch.dir, 'e2e')), 'the project is copied');
    assert.ok(existsSync(join(scratch.root, 'eslint', 'src', 'index.cjs')) && existsSync(join(scratch.root, 'node_modules')), 'eslint/ and node_modules are linked');
    assert.ok(existsSync(join(scratch.root, 'ui', 'projects')), 'an app gets ui for the kit its specs import');
    assert.equal(existsSync(join(scratch.dir, 'dist')), false, 'dist is not copied');
    writeFileSync(join(scratch.dir, 'e2e', 'zz-planted.spec.ts'), 'export {};\n');
    writeFileSync(join(scratch.dir, 'eslint-suppressions.json'), '{}\n');
    assert.equal(status(), before, 'a planted file did not reach the real repository');
    const r = await scratch.run('node', ['-e', "process.stdout.write(require('node:process').cwd())"]);
    assert.equal(r.status, 0);
    assert.equal(realpathSync(r.out), realpathSync(scratch.dir), 'run starts in the copy of the project');
  } finally {
    scratch.cleanup();
  }
  assert.equal(existsSync(scratch.root), false, 'cleanup removed the copy');
  scratch.cleanup();
  assert.equal(status(), before);
});

test('scratchProject: SIGINT mid-run removes the copy and kills what it started (the reviewer\'s probe: the old control left planted files in the worktree)', async () => {
  const before = status();
  const script = `
    const { scratchProject } = require(${JSON.stringify(join(REPO, 'scripts/test-helpers/scratch-project.cjs'))});
    const s = scratchProject(${JSON.stringify(REPO)}, 'ui');
    require('node:fs').writeFileSync(require('node:path').join(s.dir, 'e2e', 'zz-planted.spec.ts'), 'export {};\\n');
    console.log('ROOT ' + s.root);
    s.run('sleep', ['60']).then(() => console.log('finished'));
  `;
  const child = spawn(process.execPath, ['-e', script], { stdio: ['ignore', 'pipe', 'inherit'] });
  let out = '';
  const root = await new Promise((res, rej) => {
    child.stdout.on('data', (d) => {
      out += d;
      const m = /ROOT (\S+)/.exec(out);
      if (m) res(m[1]);
    });
    child.on('exit', () => rej(new Error(`the child ended before it printed its root: ${out}`)));
  });
  assert.ok(existsSync(join(root, 'ui', 'e2e', 'zz-planted.spec.ts')), 'the copy exists mid-run and holds the planted file');
  const exited = new Promise((res) => child.on('exit', (code, signal) => res({ code, signal })));
  child.kill('SIGINT');
  const { code } = await exited;
  assert.equal(code, 130, 'the handler cleans up and exits with 130');
  assert.equal(existsSync(root), false, 'the copy is gone');
  assert.equal(status(), before, 'git status is unchanged');
  assert.deepEqual(readdirSync(tmpdir()).filter((n) => n.startsWith('paddock-scratch-') && join(tmpdir(), n) === root), []);
});

test('scratchProject: the copy of a process that is gone (an interrupted run that could not clean up) is removed by the next call, a live one is not', () => {
  const stale = join(tmpdir(), 'paddock-scratch-2147483646-stale');
  const live = join(tmpdir(), `paddock-scratch-${process.pid}-live`);
  mkdirSync(stale, { recursive: true });
  mkdirSync(live, { recursive: true });
  const scratch = scratchProject(REPO, 'ui');
  try {
    assert.equal(existsSync(stale), false, 'the folder of a dead process is removed');
    assert.equal(existsSync(live), true, 'the folder of a live process is kept');
  } finally {
    scratch.cleanup();
    require('node:fs').rmSync(live, { recursive: true, force: true });
  }
});
