'use strict';
// A scratch copy of one project of the repository, outside the repository (ER-45: a check or test never writes into the real repository; the class the
// round-4 review found in eslint/test/wiring.test.cjs, which planted files in the real worktree and left them behind on SIGINT, and corrupted a second run's "original").
// Shared by the eslint and conformance tests (CommonJS, so both can import it: `require` from a test, `createRequire` or `import()` from an ES module).
//
//   const scratch = scratchProject(repoRoot, 'apps/dyno/app');     // { root, dir, run(cmd, args), cleanup() }
//   try { writeFileSync(join(scratch.dir, 'e2e/x.ts'), ...); await scratch.run('npm', ['run', 'lint']); } finally { scratch.cleanup(); }
//
// The copy has the same layout under `root` (`root/apps/dyno/app`, `root/ui`): the project folder is copied (`fs.cpSync`) without node_modules, dist, .angular,
// test-results, target and *-snapshots; node_modules, eslint/, tsconfig/, the root package.json and .npmrc are symlinked to the repository's, so relative paths
// out of the project (`../../../eslint/src/index.cjs`, `extends: ../../../tsconfig/..`) and the npm and ng binaries resolve as in the real tree. An app also gets `ui`
// as a link (its specs import the kit from `ui/projects/paddock-e2e`); the ui project itself is copied with `projects` linked. src-tauri of an app is linked
// (a lint script reads it). Everything a test plants goes into the copy only.
//
// Cleanup: `cleanup()` (call it in `finally`), and a process-exit, SIGINT, SIGTERM and SIGHUP handler that kills the programs `run` started (their process groups)
// and removes every copy still alive. A SIGKILL cannot be handled (`node --test` ends a test file that way when it is interrupted): the copy then stays under the OS temp
// folder, never in the repository, and the next `scratchProject` call removes every `paddock-scratch-<pid>-*` folder whose process is gone.
const { spawn } = require('node:child_process');
const { cpSync, existsSync, mkdirSync, mkdtempSync, rmSync, symlinkSync } = require('node:fs');
const { tmpdir } = require('node:os');
const { basename, dirname, join } = require('node:path');
const { readdirSync } = require('node:fs');

const SKIP = new Set(['node_modules', 'dist', '.angular', 'test-results', 'target', '.git']);
const LINKED_AT_ROOT = ['node_modules', 'eslint', 'tsconfig', 'package.json', '.npmrc'];
const LINKED_IN_PROJECT = ['src-tauri', 'projects'];
const PREFIX = 'paddock-scratch-';

/** Removes the scratch folders of processes that no longer exist (an interrupted run that could not clean up). Never throws. */
function sweepStale() {
  let names = [];
  try {
    names = readdirSync(tmpdir());
  } catch {
    return;
  }
  for (const name of names) {
    const m = new RegExp(`^${PREFIX}(\\d+)-`).exec(name);
    if (!m) continue;
    const pid = Number(m[1]);
    let alive = true;
    try {
      process.kill(pid, 0);
    } catch (e) {
      alive = e.code === 'EPERM';
    }
    if (!alive) rmSync(join(tmpdir(), name), { recursive: true, force: true });
  }
}

/** Scratch folders and running programs that a signal or an exit must clean up. */
const live = new Set();
let hooked = false;

function killChildren(scratch) {
  for (const child of scratch.children) {
    try {
      process.kill(-child.pid, 'SIGKILL'); // its own process group: npm, ng and the programs they started
    } catch {
      /* already gone */
    }
  }
  scratch.children.clear();
}

function cleanupAll() {
  for (const scratch of [...live]) scratch.cleanup();
}

function hook() {
  if (hooked) return;
  hooked = true;
  process.on('exit', cleanupAll);
  for (const [signal, code] of [['SIGINT', 130], ['SIGTERM', 143], ['SIGHUP', 129]]) {
    process.on(signal, () => {
      cleanupAll();
      process.exit(code);
    });
  }
}

/**
 * @param {string} repo the repository root
 * @param {string} project the project folder, relative to it (`apps/dyno/app`, `ui`)
 */
function scratchProject(repo, project) {
  hook();
  sweepStale();
  const root = mkdtempSync(join(tmpdir(), `${PREFIX}${process.pid}-`));
  const children = new Set();
  const scratch = {
    root,
    dir: join(root, project),
    children,
    /** Runs a program in the copy of the project; resolves { status, out } and never rejects. */
    run(cmd, args = [], env = {}) {
      return new Promise((resolveRun) => {
        const child = spawn(cmd, args, { cwd: scratch.dir, detached: true, env: { ...process.env, CI: '1', FORCE_COLOR: '0', ...env } });
        children.add(child);
        let out = '';
        child.stdout.on('data', (d) => (out += d));
        child.stderr.on('data', (d) => (out += d));
        child.on('close', (status) => {
          children.delete(child);
          resolveRun({ status, out });
        });
        child.on('error', (e) => {
          children.delete(child);
          resolveRun({ status: -1, out: String(e) });
        });
      });
    },
    /** Kills what `run` started and removes the copy. Safe to call twice. */
    cleanup() {
      killChildren(scratch);
      live.delete(scratch);
      rmSync(root, { recursive: true, force: true, maxRetries: 3 });
    },
  };
  live.add(scratch);
  try {
    for (const name of LINKED_AT_ROOT) if (existsSync(join(repo, name))) symlinkSync(join(repo, name), join(root, name));
    const isApp = project.startsWith('apps/');
    if (isApp) symlinkSync(join(repo, 'ui'), join(root, 'ui'));
    mkdirSync(dirname(scratch.dir), { recursive: true });
    const source = join(repo, project);
    cpSync(source, scratch.dir, {
      recursive: true,
      filter: (src) => !SKIP.has(basename(src)) && !basename(src).endsWith('-snapshots') && !(LINKED_IN_PROJECT.includes(basename(src)) && dirname(src) === source),
    });
    for (const name of LINKED_IN_PROJECT) if (existsSync(join(source, name))) symlinkSync(join(source, name), join(scratch.dir, name));
  } catch (e) {
    scratch.cleanup();
    throw e;
  }
  return scratch;
}

module.exports = { scratchProject };
