// conformance/ci/kit.sh, the bootstrap shim that runs a kit step in Rust or, on the release path, through its shell script
// (docs/plans/ci-in-rust.md, WP-1): which implementation each situation gets, and that acquire and release pin the tool under the
// CI home. Fakes stand in for ./dev, the two scripts and rustc, in a scratch checkout (never the real runner or a real tool).
import assert from 'node:assert/strict';
import { spawnSync } from 'node:child_process';
import { chmodSync, copyFileSync, mkdirSync, mkdtempSync, readFileSync, realpathSync, rmSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { test } from 'node:test';
import { PADDOCK } from './helpers.mjs';

const exe = (p, body) => {
  writeFileSync(p, `#!/bin/sh\n${body}\n`);
  chmodSync(p, 0o755);
};

/** A scratch checkout with kit.sh, a fake ./dev (it logs its arguments and the variables that steer it), and fake scripts. */
function checkout({ tools = true, rustc = true } = {}) {
  const root = realpathSync(mkdtempSync(join(tmpdir(), 'kit-')));
  mkdirSync(join(root, 'conformance', 'ci'), { recursive: true });
  mkdirSync(join(root, 'bin'));
  copyFileSync(join(PADDOCK, 'conformance', 'ci', 'kit.sh'), join(root, 'conformance', 'ci', 'kit.sh'));
  const log = join(root, 'log');
  const note = (who) => `echo "${who} $* home=\${PADDOCK_DEV_HOME:-} target=\${PADDOCK_DEV_TARGET_DIR:-} dl=\${PADDOCK_DEV_DOWNLOAD:-} jobs=\${CARGO_BUILD_JOBS:-} pinned=\${PADDOCK_DEV:-}" >> '${log}'`;
  exe(join(root, 'dev'), `${note('dev')}\ncase "$*" in "ci isolation path tools") echo "${root}/ci-home/tools" ;; "--print-binary") echo "${root}/pinned-tool" ;; esac`);
  exe(join(root, 'pinned-tool'), note('pinned'));
  exe(join(root, 'conformance', 'ci', 'job-isolation.sh'), note('job-isolation.sh'));
  exe(join(root, 'conformance', 'ci', 'build-jobs.sh'), note('build-jobs.sh'));
  if (rustc) exe(join(root, 'bin', 'rustc'), 'exit 0');
  if (tools) {
    mkdirSync(join(root, 'tools', 'paddock-dev'), { recursive: true });
    writeFileSync(join(root, 'tools', 'paddock-dev', 'Cargo.toml'), '');
  }
  const git = spawnSync('git', ['init', '-q'], { cwd: root });
  assert.equal(git.status, 0);
  const run = (args, env = {}) => {
    writeFileSync(log, '');
    const r = spawnSync('sh', [join(root, 'conformance', 'ci', 'kit.sh'), ...args], {
      cwd: root,
      env: { PATH: `${join(root, 'bin')}:/usr/bin:/bin`, HOME: root, RUNNER_OS: 'macOS', ...env },
      encoding: 'utf8',
    });
    return { ...r, log: readFileSync(log, 'utf8') };
  };
  return { root, run, done: () => rmSync(root, { recursive: true, force: true }) };
}

test('the kit picks Rust off the release path and the shell on it', () => {
  const c = checkout();
  try {
    assert.equal(c.run(['which']).stdout.trim(), 'dev', 'a branch run');
    assert.equal(c.run(['which'], { GITHUB_REF: 'refs/heads/main' }).stdout.trim(), 'dev');
    assert.equal(c.run(['which'], { GITHUB_REF: 'refs/tags/debrief-v1.2.0' }).stdout.trim(), 'shell', 'a tag run');
    assert.equal(c.run(['which'], { PADDOCK_DEV_DOWNLOAD: '0' }).stdout.trim(), 'shell', 'release and rule 54 jobs never download the tool');
  } finally {
    c.done();
  }
});

test('where ./dev cannot run the shell runs: no tool in the checkout, no rustc', () => {
  for (const missing of [{ tools: false }, { rustc: false }]) {
    const c = checkout(missing);
    try {
      // PATH without rustc: /usr/bin and /bin have none on the machines that run these tests.
      assert.equal(c.run(['which']).stdout.trim(), 'shell', JSON.stringify(missing));
    } finally {
      c.done();
    }
  }
});

test('another repository using the actions (its checkout has no tools/paddock-dev) keeps the shell; a kept clone of this one gets Rust', () => {
  const c = checkout();
  const other = realpathSync(mkdtempSync(join(tmpdir(), 'kit-other-')));
  const run = (cwd) =>
    spawnSync('sh', [join(c.root, 'conformance', 'ci', 'kit.sh'), 'which'], {
      cwd,
      env: { PATH: `${join(c.root, 'bin')}:/usr/bin:/bin`, HOME: other },
      encoding: 'utf8',
    }).stdout.trim();
  try {
    spawnSync('git', ['init', '-q'], { cwd: other });
    assert.equal(run(other), 'shell');
    // The kept clone of a host job: the actions come from the bootstrap checkout, the steps run in another tree with the tool.
    mkdirSync(join(other, 'tools', 'paddock-dev'), { recursive: true });
    writeFileSync(join(other, 'tools', 'paddock-dev', 'Cargo.toml'), '');
    assert.equal(run(other), 'dev');
  } finally {
    c.done();
    rmSync(other, { recursive: true, force: true });
  }
});

test('build-jobs: Rust with a core limit for the tool\'s own first build, the script on the release path', () => {
  const c = checkout();
  try {
    let r = c.run(['build-jobs', '3'], { GITHUB_TOKEN: 't' });
    assert.match(r.log, /^dev ci build-jobs 3 .* jobs=4 /, r.log);
    r = c.run(['build-jobs'], { CARGO_BUILD_JOBS: '7', GITHUB_TOKEN: 't' });
    assert.match(r.log, /jobs=7 /, 'a value set earlier in the job is kept for the tool\'s build');
    r = c.run(['build-jobs', '2'], { PADDOCK_DEV_DOWNLOAD: '0' });
    assert.match(r.log, /^build-jobs\.sh 2 /, r.log);
    r = c.run(['build-jobs'], { GITHUB_REF: 'refs/tags/x-v1' });
    assert.match(r.log, /^build-jobs\.sh /, r.log);
  } finally {
    c.done();
  }
});

test('a job without the read credential builds the tool from source and never downloads a stored binary', () => {
  const c = checkout();
  try {
    assert.match(c.run(['build-jobs']).log, /dl=0 /);
    assert.match(c.run(['build-jobs'], { GITHUB_TOKEN: 't' }).log, /dl= /, 'with a credential the default stays');
    assert.match(c.run(['build-jobs'], { PADDOCK_DEV_TOKEN: 't' }).log, /dl= /);
  } finally {
    c.done();
  }
});

test('isolation acquire: the tool is fetched under the CI home\'s tools folder and pinned; release runs the pinned tool', () => {
  const c = checkout();
  try {
    let r = c.run(['isolation', 'acquire'], { GITHUB_TOKEN: 't' });
    assert.equal(r.status, 0, r.stderr);
    const lines = r.log.trim().split('\n');
    assert.match(lines[0], /^dev ci isolation path tools /, 'the first call finds the tools folder');
    assert.match(lines[1], new RegExp(`^dev --print-binary home=${c.root}/ci-home/tools/paddock-dev target=${c.root}/ci-home/tools/paddock-dev-target `), 'the tool lives under the CI home');
    assert.match(lines[2], new RegExp(`^pinned ci isolation acquire .*pinned=${c.root}/pinned-tool`), 'acquire runs the tool it pins, with PADDOCK_DEV set');
    r = c.run(['isolation', 'release'], { PADDOCK_DEV: join(c.root, 'pinned-tool'), GITHUB_TOKEN: 't' });
    assert.match(r.log, /^pinned ci isolation release /, 'release runs the pinned tool: no build after the job\'s temp folder is gone');
    assert.doesNotMatch(r.log, /^dev /m);
    r = c.run(['isolation', 'release'], { GITHUB_TOKEN: 't' });
    assert.match(r.log, /^dev ci isolation path tools/m, 'without a pin release still builds under the CI home');
    assert.match(r.log, /^dev ci isolation release home=.*paddock-dev target=/m);
  } finally {
    c.done();
  }
});

test('isolation in a Linux container takes no slot and fetches no tool; value commands use the pinned tool or the wrapper', () => {
  const c = checkout();
  try {
    assert.equal(c.run(['isolation', 'acquire'], { RUNNER_OS: 'Linux' }).log, '');
    assert.equal(c.run(['isolation', 'release'], { RUNNER_OS: 'Linux' }).log, '');
    let r = c.run(['isolation', 'path', 'target', 'app'], { RUNNER_OS: 'Linux', GITHUB_TOKEN: 't' });
    assert.match(r.log, /^dev ci isolation path target app /, r.log);
    r = c.run(['isolation', 'port', 'webkit'], { PADDOCK_DEV: join(c.root, 'pinned-tool') });
    assert.match(r.log, /^pinned ci isolation port webkit /, r.log);
  } finally {
    c.done();
  }
});

test('isolation on the release path runs job-isolation.sh with the arguments it was given', () => {
  const c = checkout();
  try {
    const r = c.run(['isolation', 'path', 'npm'], { PADDOCK_DEV_DOWNLOAD: '0' });
    assert.match(r.log, /^job-isolation\.sh path npm /, r.log);
    assert.equal(c.run(['isolation', 'acquire'], { GITHUB_REF: 'refs/tags/v1.0.0' }).log.startsWith('job-isolation.sh acquire'), true);
  } finally {
    c.done();
  }
});

test('an unknown step is a usage error', () => {
  const c = checkout();
  try {
    assert.equal(c.run(['nothing']).status, 2);
  } finally {
    c.done();
  }
});
