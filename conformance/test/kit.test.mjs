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
  const note = (who) => `echo "${who} $* home=\${PADDOCK_DEV_HOME:-} target=\${PADDOCK_DEV_TARGET_DIR:-} dl=\${PADDOCK_DEV_DOWNLOAD:-} jobs=\${CARGO_BUILD_JOBS:-} pinned=\${PADDOCK_DEV:-} wrapper=\${RUSTC_WRAPPER:-}" >> '${log}'`;
  exe(join(root, 'dev'), `${note('dev')}\ncase "$*" in "--print-binary") echo "${root}/pinned-tool" ;; esac`);
  exe(join(root, 'pinned-tool'), note('pinned'));
  exe(join(root, 'conformance', 'ci', 'job-isolation.sh'), `${note('job-isolation.sh')}\ncase "$*" in "path tools") echo "${root}/ci-home/tools" ;; esac`);
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
      env: { PATH: `${join(root, 'bin')}:/usr/bin:/bin`, HOME: root, RUNNER_OS: 'macOS', GITHUB_ACTIONS: 'true', ...env },
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
    assert.match(r.log, /^dev ci build-jobs 3 .* jobs=4 /m, r.log);
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

test('isolation acquire: the tool is fetched once, under the CI home\'s tools folder found in shell, and pinned; release runs the pinned tool', () => {
  const c = checkout();
  try {
    let r = c.run(['isolation', 'acquire'], { GITHUB_TOKEN: 't', RUSTC_WRAPPER: 'sccache' });
    assert.equal(r.status, 0, r.stderr);
    const lines = r.log.trim().split('\n');
    assert.match(lines[0], /^job-isolation\.sh path tools /, 'the folder is found without the tool');
    assert.match(lines[1], new RegExp(`^dev --print-binary home=${c.root}/ci-home/tools/paddock-dev target=${c.root}/ci-home/tools/paddock-dev-target `), 'the first and only ./dev call has its home under the CI home');
    assert.match(lines[2], new RegExp(`^pinned ci isolation acquire .*pinned=${c.root}/pinned-tool`), 'acquire runs the tool it pins, with PADDOCK_DEV set');
    assert.equal(lines.length, 3, r.log);
    assert.ok(lines.every((l) => !/wrapper=sccache/.test(l)), 'the job\'s compiler wrapper never reaches ./dev or the tool');
    r = c.run(['isolation', 'release'], { PADDOCK_DEV: join(c.root, 'pinned-tool'), GITHUB_TOKEN: 't' });
    assert.match(r.log, /^pinned ci isolation release /, 'release runs the pinned tool: no build after the job\'s temp folder is gone');
    assert.doesNotMatch(r.log, /^dev /m);
    r = c.run(['isolation', 'release'], { GITHUB_TOKEN: 't', RUSTC_WRAPPER: 'sccache' });
    assert.match(r.log, /^dev ci isolation release home=.*paddock-dev target=.* wrapper=\n/m, 'without a pin release still builds under the CI home, without the wrapper');
  } finally {
    c.done();
  }
});

test('build-jobs and value commands on a host also keep the tool under the CI home, never the runner\'s', () => {
  const c = checkout();
  try {
    let r = c.run(['build-jobs'], { GITHUB_TOKEN: 't' });
    assert.match(r.log, new RegExp(`^job-isolation\\.sh path tools`));
    assert.match(r.log, new RegExp(`dev ci build-jobs  ?home=${c.root}/ci-home/tools/paddock-dev `), r.log);
    r = c.run(['isolation', 'path', 'target', 'a'], { GITHUB_TOKEN: 't' });
    assert.match(r.log, new RegExp(`^dev ci isolation path target a home=${c.root}/ci-home/tools/paddock-dev `, 'm'), r.log);
  } finally {
    c.done();
  }
});

test('a person\'s machine (no GITHUB_ACTIONS) never gets the CI home\'s tools folder: the tool stays where ./dev puts it', () => {
  const c = checkout();
  try {
    const r = c.run(['build-jobs'], { GITHUB_ACTIONS: '', GITHUB_TOKEN: 't', RUNNER_OS: 'macOS' });
    assert.doesNotMatch(r.log, /job-isolation\.sh path tools/, r.log);
    assert.match(r.log, /^dev ci build-jobs  ?home= target= /m, r.log);
  } finally {
    c.done();
  }
});

test('PADDOCK_CI_KIT=shell is the fallback switch: every step runs its script, and the choice and its reason are logged', () => {
  const c = checkout();
  try {
    let r = c.run(['build-jobs', '2'], { PADDOCK_CI_KIT: 'shell' });
    assert.match(r.log, /^build-jobs\.sh 2 /, r.log);
    assert.match(r.stderr, /^kit: shell \(PADDOCK_CI_KIT=shell\)$/m, r.stderr);
    r = c.run(['isolation', 'acquire'], { PADDOCK_CI_KIT: 'shell' });
    assert.match(r.log, /^job-isolation\.sh acquire /, r.log);
    assert.equal(c.run(['which'], { PADDOCK_CI_KIT: 'shell' }).stdout.trim(), 'shell');
    // Every reason is named.
    assert.match(c.run(['which'], { GITHUB_REF: 'refs/tags/v1' }).stderr, /kit: shell \(a tag run\)/);
    assert.match(c.run(['which'], { PADDOCK_DEV_DOWNLOAD: '0' }).stderr, /kit: shell \(PADDOCK_DEV_DOWNLOAD=0/);
    assert.match(c.run(['which']).stderr, /kit: dev \(/);
  } finally {
    c.done();
  }
});

test('a checkout whose ./dev wrapper is missing, or with no rustc, keeps the shell and says why', () => {
  const c = checkout();
  try {
    rmSync(join(c.root, 'dev'));
    const r = c.run(['which']);
    assert.equal(r.stdout.trim(), 'shell');
    assert.match(r.stderr, /no \.\/dev wrapper/);
  } finally {
    c.done();
  }
  const d = checkout({ rustc: false });
  try {
    assert.match(d.run(['which']).stderr, /kit: shell \(no rustc on PATH\)/);
  } finally {
    d.done();
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

// ---- host steps (docs/plans/ci-in-rust.md, WP-4): `kit.sh host <step> <args>` ----

/** The checkout of `checkout()` plus fake host scripts that log their name, arguments and the inputs they read from the environment. */
function hostCheckout() {
  const c = checkout();
  const log = join(c.root, 'log');
  const note = (who) => `echo "${who} $* name=\${NAME:-} angular=\${ANGULAR_DIR-unset}" >> '${log}'`;
  mkdirSync(join(c.root, 'conformance', 'ci', 'host'), { recursive: true });
  mkdirSync(join(c.root, 'tools'), { recursive: true });
  for (const step of ['stable-workspace', 'node-caches', 'host-tools', 'sccache-stats']) exe(join(c.root, 'conformance', 'ci', 'host', `${step}.sh`), note(`host/${step}.sh`));
  exe(join(c.root, 'conformance', 'ci', 'build-folder.sh'), note('build-folder.sh'));
  for (const s of ['kept-clone.sh', 'guardian-as-user.sh']) exe(join(c.root, 'conformance', 'ci', s), note(s));
  for (const s of ['prune-build-folders.sh', 'prune-ci-build-folder.sh', 'host-cleanup.sh']) exe(join(c.root, 'tools', s), note(s));
  return c;
}

test('kit.sh host runs a host step as ./dev ci <step> off the release path, with the action inputs as options', () => {
  const c = hostCheckout();
  try {
    // The tool's own folder is asked of job-isolation.sh first on a host (tools_home): that call is not the step.
    const dev = (args, env = {}) =>
      c.run(['host', ...args], env).log.trim().split('\n').filter((l) => !l.startsWith('job-isolation.sh ')).map((l) => l.replace(/ home=.*$/, ''));
    assert.deepEqual(dev(['stable-workspace', 'org/repo-job']), ['dev ci stable-workspace --name org/repo-job']);
    assert.deepEqual(dev(['node-caches', 'ui', 'e2e-1']), ['dev ci node-caches --angular-dir ui --name e2e-1']);
    assert.deepEqual(dev(['node-caches', '', 'e2e-1']), ['dev ci node-caches --angular-dir  --name e2e-1'], 'an empty angular dir keeps only npm');
    assert.deepEqual(dev(['host-tools']), ['dev ci host-tools']);
    assert.deepEqual(dev(['kept-clone', 'org/repo-job', 'lean', 'false']), ['dev ci kept-clone --name org/repo-job --profile lean']);
    assert.deepEqual(dev(['kept-clone', 'org/repo-job', 'bundle', 'true']), ['dev ci kept-clone --name org/repo-job --profile bundle --fresh'], 'a tag run asks for a new clone');
    assert.deepEqual(dev(['guardian-as-user'], { RUNNER_OS: 'Linux' }), ['dev ci guardian-as-user']);
    assert.deepEqual(dev(['build-folder', '/ws', 'app-job']), ['dev ci build-folder /ws app-job']);
    assert.deepEqual(dev(['prune-build-folder', '20', 'false']), ['dev ci prune-build-folder --max-gb 20']);
    assert.deepEqual(dev(['prune-build-folder', '5', 'true']), ['dev ci prune-build-folder --max-gb 5 --drop-bundles']);
    assert.deepEqual(dev(['prune-folder', '/x', '7']), ['dev ci prune-folder /x 7']);
    assert.deepEqual(dev(['host-cleanup', '--id', 'pitwall', '--process-pattern', '{build}/x']), ['dev ci host-cleanup --id pitwall --process-pattern {build}/x']);
    // The tool the job pinned is used as it is.
    assert.deepEqual(dev(['host-tools'], { PADDOCK_DEV: join(c.root, 'pinned-tool') }), ['pinned ci host-tools']);
  } finally {
    c.done();
  }
});

test('kit.sh host runs the shell a step had on a tag run, a run that never downloads the tool, and PADDOCK_CI_KIT=shell', () => {
  const c = hostCheckout();
  try {
    for (const env of [{ GITHUB_REF: 'refs/tags/pitwall-v1.1.0' }, { PADDOCK_DEV_DOWNLOAD: '0' }, { PADDOCK_CI_KIT: 'shell' }]) {
      const sh = (args, more = {}) => c.run(['host', ...args], { ...env, ...more }).log.trim();
      assert.equal(sh(['stable-workspace', 'org/repo-job']), 'host/stable-workspace.sh  name=org/repo-job angular=unset', JSON.stringify(env));
      assert.equal(sh(['node-caches', 'ui', 'e2e-1']), 'host/node-caches.sh  name=e2e-1 angular=ui');
      assert.equal(sh(['node-caches', '', 'e2e-1']), 'host/node-caches.sh  name=e2e-1 angular=');
      assert.match(sh(['host-tools']), /^host\/host-tools\.sh /);
      assert.equal(sh(['kept-clone', 'org/repo-job', 'lean', 'true']), 'kept-clone.sh  name=org/repo-job angular=unset', 'NAME, PROFILE and FRESH are the script\'s inputs');
      assert.match(sh(['guardian-as-user'], { RUNNER_OS: 'Linux' }), /^guardian-as-user\.sh /);
      assert.match(sh(['build-folder', '/ws', 'app-job']), /^build-folder\.sh \/ws app-job /);
      assert.match(sh(['prune-folder', '/x', '7']), /^prune-build-folders\.sh \/x 7 /);
      assert.match(sh(['host-cleanup', '--id', 'x']), /^host-cleanup\.sh --id x /);
      assert.match(sh(['prune-build-folder', '5', 'true'], { PADDOCK_BUILD_FOLDER: '/ci/target-x-s1' }), /^prune-ci-build-folder\.sh \/ci\/target-x-s1 5 --drop-bundles /);
      assert.match(sh(['prune-build-folder', '20', 'false'], { PADDOCK_BUILD_FOLDER: '/ci/target-x-s1' }), /^prune-ci-build-folder\.sh \/ci\/target-x-s1 20 name=/m);
    }
  } finally {
    c.done();
  }
});

test('kit.sh host fetches no tool for what has nothing to do: a Linux container for stable-workspace, no sccache for sccache-stats', () => {
  const c = hostCheckout();
  try {
    assert.equal(c.run(['host', 'stable-workspace', 'x'], { RUNNER_OS: 'Linux' }).log, '', 'neither the tool nor the script ran');
    assert.equal(c.run(['host', 'kept-clone', 'x', 'lean', 'false'], { RUNNER_OS: 'Linux' }).log, '', 'a Linux container has a fixed workspace');
    assert.equal(c.run(['host', 'sccache-stats']).log, '', 'no sccache on PATH');
    exe(join(c.root, 'bin', 'sccache'), 'exit 0');
    assert.match(c.run(['host', 'sccache-stats']).log, /^dev ci sccache-stats/m, 'with sccache the step runs');
    assert.equal(c.run(['host', 'bogus']).status, 2, 'an unknown step is a usage error');
    assert.equal(c.run(['host']).status, 2);
  } finally {
    c.done();
  }
});

test('a failed setup step (build-jobs, isolation acquire) marks the job aborted in GITHUB_ENV and says so; a good one leaves it alone', () => {
  const c = checkout();
  try {
    const env = join(c.root, 'github-env');
    writeFileSync(env, '');
    // The wrapper could not build the tool (run 8027: crate on Windows went on to run its tests without isolation).
    exe(join(c.root, 'dev'), 'exit 1');
    for (const args of [['build-jobs'], ['isolation', 'acquire']]) {
      writeFileSync(env, '');
      const r = c.run(args, { GITHUB_ENV: env });
      assert.equal(r.status, 1, `${args}: ${r.stderr}`);
      assert.match(r.stderr, /::error::A job setup step failed \(exit 1/, `${args}: ${r.stderr}`);
      assert.equal(readFileSync(env, 'utf8'), 'PADDOCK_JOB_ABORTED=1\n', `${args} marks the job`);
    }
    exe(join(c.root, 'dev'), 'case "$*" in "--print-binary") echo "$0-none" ;; esac');
    writeFileSync(env, '');
    const ok = c.run(['build-jobs'], { GITHUB_ENV: env });
    assert.equal(ok.status, 0, ok.stderr);
    assert.equal(readFileSync(env, 'utf8'), '', 'a good setup step marks nothing');
  } finally {
    c.done();
  }
});

test('on a CI host with rustup the pinned toolchain is ensured before the first step runs, and not without rustup or off CI', () => {
  const c = checkout();
  try {
    // dev logs its arguments; the kit must call `--ensure-toolchain` first (run 8034: build-jobs ran before the pinned Rust was installed).
    exe(join(c.root, 'bin', 'rustup'), 'exit 0');
    let r = c.run(['build-jobs'], { GITHUB_TOKEN: 't' });
    const lines = r.log.trim().split('\n');
    assert.match(lines[0], /^dev --ensure-toolchain /, r.log);
    assert.match(r.log, /^dev ci build-jobs /m, r.log);
    // A failed install is a warning, not the end: the step runs and fails with its own message if Rust is missing.
    exe(join(c.root, 'dev'), 'echo "dev $*" >> "' + join(c.root, 'log') + '"; case "$1" in --ensure-toolchain) exit 2 ;; esac');
    r = c.run(['which']);
    assert.match(r.stderr, /::warning::kit\.sh: the pinned Rust toolchain could not be installed/, r.stderr);
    // No rustup, or not a CI job: ./dev is not asked.
    rmSync(join(c.root, 'bin', 'rustup'));
    assert.doesNotMatch(c.run(['which']).log, /ensure-toolchain/);
    exe(join(c.root, 'bin', 'rustup'), 'exit 0');
    assert.doesNotMatch(c.run(['which'], { GITHUB_ACTIONS: '' }).log, /ensure-toolchain/);
  } finally {
    c.done();
  }
});

test('a rustup shim that cannot run is not rustc: the shell kit is chosen (run 8034 review)', () => {
  const c = checkout();
  try {
    exe(join(c.root, 'bin', 'rustc'), 'echo "error: toolchain is not installed" >&2; exit 1');
    const r = c.run(['which']);
    assert.equal(r.stdout.trim(), 'shell');
    assert.match(r.stderr, /rustc does not run in this checkout/);
    exe(join(c.root, 'bin', 'rustc'), 'exit 0');
    assert.equal(c.run(['which']).stdout.trim(), 'dev', 'a rustc that runs keeps the Rust kit');
  } finally {
    c.done();
  }
});

test('after a failed toolchain install the job is marked, later steps neither retry nor use ./dev, and cleanup still runs in shell', () => {
  const c = checkout();
  try {
    const env = join(c.root, 'github-env');
    writeFileSync(env, '');
    exe(join(c.root, 'bin', 'rustup'), 'exit 0');
    exe(join(c.root, 'bin', 'rustc'), 'exit 1');
    exe(join(c.root, 'dev'), `echo "dev $*" >> "${join(c.root, 'log')}"; case "$1" in --ensure-toolchain) exit 2 ;; esac`);
    let r = c.run(['build-jobs'], { GITHUB_ENV: env, GITHUB_TOKEN: 't' });
    assert.equal(r.status, 0, r.stderr);
    assert.equal(readFileSync(env, 'utf8'), 'PADDOCK_TOOLCHAIN_FAILED=1\nPADDOCK_CI_KIT=shell\n');
    assert.match(r.log, /^dev --ensure-toolchain/m);
    assert.match(r.log, /^build-jobs\.sh /m, 'the step itself ran in shell');
    // The next steps of the job see the variables the runner took from GITHUB_ENV.
    const later = { PADDOCK_TOOLCHAIN_FAILED: '1', PADDOCK_CI_KIT: 'shell', GITHUB_ENV: env };
    for (const args of [['build-jobs'], ['isolation', 'release'], ['host', 'sccache-stats'], ['host', 'host-cleanup', '--id', 'x']]) {
      r = c.run(args, later);
      assert.doesNotMatch(r.log, /ensure-toolchain|^dev/m, `${args}: no second install attempt, no ./dev: ${r.log}`);
    }
  } finally {
    c.done();
  }
});
