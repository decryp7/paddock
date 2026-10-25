// The composite actions and reusable workflows are YAML the runner only reads on CI, so their wiring is checked here: every
// run step names its shell, every script and local action they reference exists, every input they use or pass is declared.
// (The scripts themselves are tested in ci-scripts.test.mjs, tools/test and the release tests.)
import assert from 'node:assert/strict';
import { spawnSync } from 'node:child_process';
import { chmodSync, copyFileSync, existsSync, lstatSync, mkdirSync, mkdtempSync, readdirSync, readFileSync, realpathSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { test } from 'node:test';
import { parseSteps } from '../src/util/workflows.mjs';
import { PADDOCK } from './helpers.mjs';

const ACTIONS = join(PADDOCK, '.gitea', 'actions');
const WORKFLOWS = join(PADDOCK, '.gitea', 'workflows');
const names = readdirSync(ACTIONS, { withFileTypes: true }).filter((e) => e.isDirectory()).map((e) => e.name).sort();
const read = (p) => readFileSync(p, 'utf8');

/** Declared input names of an action.yml / a workflow_call block: keys at the first indent under `inputs:`. */
function declaredInputs(text) {
  const lines = text.split('\n');
  const at = lines.findIndex((l) => /^\s*inputs:\s*$/.test(l));
  if (at < 0) return [];
  const base = lines[at].length - lines[at].trimStart().length;
  const out = [];
  for (let i = at + 1; i < lines.length; i++) {
    const l = lines[i];
    if (l.trim() === '' || l.trim().startsWith('#')) continue;
    const indent = l.length - l.trimStart().length;
    if (indent <= base) break;
    if (indent === base + 2) out.push(l.trim().replace(/:.*$/, ''));
  }
  return out;
}

/** Keys of a step's `with:` block. */
function withKeys(stepText) {
  const lines = stepText.split('\n');
  const at = lines.findIndex((l) => /^\s*with:\s*$/.test(l));
  if (at < 0) return [];
  const base = lines[at].length - lines[at].trimStart().length;
  const keys = [];
  for (let i = at + 1; i < lines.length; i++) {
    const l = lines[i];
    if (l.trim() === '') continue;
    const indent = l.length - l.trimStart().length;
    if (indent <= base) break;
    if (indent === base + 2) keys.push(l.trim().replace(/:.*$/, ''));
  }
  return keys;
}

test('every action is a composite action with a name, a description and the README row', () => {
  assert.ok(names.length >= 15, names.join());
  const readme = read(join(ACTIONS, 'README.md'));
  for (const n of names) {
    const text = read(join(ACTIONS, n, 'action.yml'));
    assert.match(text, new RegExp(`^name: ${n}$`, 'm'), `${n}: name`);
    assert.match(text, /^description: ./m, `${n}: description`);
    assert.match(text, /^runs:\n {2}using: composite$/m, `${n}: composite`);
    assert.ok(readme.includes(`| \`${n}\``) || readme.includes(`\`${n}\`, `) || readme.includes(`, \`${n}\``), `${n} is in the README`);
  }
});

test('every run step of an action names its shell; every script it runs exists; every input it reads is declared', () => {
  for (const n of names) {
    const text = read(join(ACTIONS, n, 'action.yml'));
    for (const step of parseSteps(text)) {
      if (step.run) assert.match(step.text, /^\s*(?:-\s+)?(?:.*\n)*?\s*shell: (bash|pwsh)\s*$/m, `${n}: a run step has no shell (line ${step.line})`);
    }
    for (const m of text.matchAll(/\$\{\{ github\.action_path \}\}\/\.\.\/\.\.\/\.\.\/([\w./-]+)/g)) {
      assert.ok(existsSync(join(PADDOCK, m[1])), `${n}: ${m[1]} does not exist`);
    }
    const declared = declaredInputs(text);
    for (const m of text.matchAll(/inputs\.([\w-]+)/g)) assert.ok(declared.includes(m[1]), `${n}: input ${m[1]} is used but not declared`);
  }
});

test('the scripts the actions run are executable programs of this package', () => {
  for (const f of ['plan.mjs', 'build-env.mjs', 'release-notes.mjs', 'install-test-browser.mjs', 'tauri-bundle.sh']) assert.ok(existsSync(join(PADDOCK, 'conformance/ci', f)), f);
  for (const f of ['host-cleanup.sh']) assert.ok(existsSync(join(PADDOCK, 'tools', f)), f);
});

const workflows = readdirSync(WORKFLOWS).filter((f) => f.startsWith('app-') && f.endsWith('.yml')).sort();

test('the reusable workflows are workflow_call workflows that use only actions, inputs and secrets that exist', () => {
  assert.deepEqual(workflows, ['app-e2e.yml', 'app-fuzz.yml', 'app-licences.yml', 'app-mutants.yml', 'app-never-panics.yml', 'app-release.yml']);
  for (const w of workflows) {
    const text = read(join(WORKFLOWS, w));
    assert.match(text, /^on:\n {2}workflow_call:/m, `${w}: workflow_call`);
    const declared = declaredInputs(text.slice(text.indexOf('workflow_call:')));
    assert.ok(declared.includes('paddock-ref'), `${w}: paddock-ref is an input`);
    for (const m of text.matchAll(/\$\{\{ inputs\.([\w-]+) \}\}/g)) assert.ok(declared.includes(m[1]), `${w}: input ${m[1]} is used but not declared`);
    for (const m of text.matchAll(/\$\{\{ secrets\.([A-Z_]+) \}\}/g)) assert.match(text, new RegExp(`^ {6}${m[1]}:`, 'm'), `${w}: secret ${m[1]} is not declared`);
    for (const step of parseSteps(text)) {
      const local = /^\.\/\.paddock\/\.gitea\/actions\/([\w-]+)$/.exec(step.uses ?? '');
      if (!local) continue;
      assert.ok(names.includes(local[1]), `${w}: action ${local[1]} does not exist`);
      const actionInputs = declaredInputs(read(join(ACTIONS, local[1], 'action.yml')));
      for (const key of withKeys(step.text)) assert.ok(actionInputs.includes(key), `${w}: ${local[1]} has no input ${key}`);
    }
    // Every job that uses an action of Paddock checks Paddock out first, at the ref the caller names.
    assert.match(text, /repository: decryp7\/paddock\n\s+ref: \$\{\{ inputs\.paddock-ref \}\}\n\s+path: \.paddock/, `${w}: checks out paddock at paddock-ref`);
  }
});

test('the release workflow cannot publish from anything but a v* tag, and signs only with the secrets it declares', () => {
  const text = read(join(WORKFLOWS, 'app-release.yml'));
  assert.match(text, /release:\n(?:.*\n)*?\s+if: startsWith\(github\.ref, 'refs\/tags\/v'\)/);
  const action = read(join(ACTIONS, 'publish-release', 'action.yml'));
  assert.match(action, /refs\/tags\/v\*/);
  assert.match(action, /releases are published from a v\* tag only \(rule 9\)/);
});

test('the shared actions follow the repo rules: no AI attribution text, Gitea-compatible artifact actions', () => {
  for (const f of [...names.map((n) => join(ACTIONS, n, 'action.yml')), ...workflows.map((w) => join(WORKFLOWS, w))]) {
    const text = read(f);
    assert.doesNotMatch(text, /Co-Authored-By|Generated with/i, f);
    assert.doesNotMatch(text, /actions\/(?:upload|download)-artifact@v4/, `${f}: Gitea supports v3 only`);
  }
});

test('setup-rust declares the sccache backend inputs: local disk by default, Redis only when asked for', () => {
  const text = read(join(ACTIONS, 'setup-rust', 'action.yml'));
  const declared = declaredInputs(text);
  for (const i of ['sccache-backend', 'sccache-redis-endpoint', 'sccache-redis-db']) assert.ok(declared.includes(i), `${i} is declared`);
  assert.match(text, /sccache-backend:\n(?:.*\n)*?\s+default: local/);
  assert.match(text, /BACKEND: \$\{\{ inputs\.sccache-backend \}\}/);
  assert.match(text, /REDIS_ENDPOINT: \$\{\{ inputs\.sccache-redis-endpoint \}\}/);
  assert.match(text, /sccache-backend must be auto, redis or local/);
  assert.match(text, /sccache-backend is redis but sccache-redis-endpoint is empty/);
  const ci = read(join(WORKFLOWS, 'ci.yml'));
  assert.doesNotMatch(ci, /sccache-redis|SCCACHE_REDIS|sccache-backend/, 'Paddock CI no longer uses the shared Redis backend');
});

test('the sccache step uses the local disk: the runner home on a host, /sccache in a container, a cache that never fails a build', () => {
  const host = runSccacheStep({});
  assert.match(host, /^SCCACHE_DIR=.*\/\.paddock-ci\/sccache$/m);
  assert.match(host, /^SCCACHE_CACHE_SIZE=20G$/m);
  assert.match(host, /^SCCACHE_SERVER_PORT=4227$/m);
  assert.match(host, /^SCCACHE_IGNORE_SERVER_IO_ERROR=1$/m);
  assert.match(host, /^CARGO_INCREMENTAL=0$/m);
  assert.doesNotMatch(host, /REDIS/);
  const linux = runSccacheStep({ RUNNER_OS: 'Linux', RUNNER_TEMP: join(tmpdir(), 'paddock-nomount') });
  assert.match(linux, /^SCCACHE_DIR=.*paddock-nomount\/sccache$/m, 'no /sccache mount: a folder of the container');
  const redis = runSccacheStep({ BACKEND: 'auto', REDIS_ENDPOINT: 'redis://h:6379', REDIS_DB: '' });
  assert.match(redis, /^SCCACHE_REDIS_ENDPOINT=redis:\/\/h:6379$/m);
  assert.match(redis, /^SCCACHE_REDIS_DB=5$/m);
  assert.doesNotMatch(redis, /SCCACHE_DIR/);
});

test('a sccache server that cannot start or answer leaves RUSTC_WRAPPER unset and the step green, so the job builds without the cache', () => {
  for (const shim of ['[ "$1" = --start-server ] && exit 1', '[ "$1" = --show-stats ] && exit 1']) {
    const r = runSccacheStep({}, shim);
    assert.doesNotMatch(r.env, /RUSTC_WRAPPER/, shim);
    assert.doesNotMatch(r.env, /CARGO_INCREMENTAL/, 'incremental builds stay on without the cache');
    assert.match(r.stdout, /::warning::the sccache server did not start/);
  }
});

test('setup-rust installs a pinned, checksum-verified sccache when the runner has none, for every supported OS and architecture', () => {
  const text = read(join(ACTIONS, 'setup-rust', 'action.yml'));
  const install = text.slice(text.indexOf('- name: Install sccache'), text.indexOf('- name: sccache\n'));
  assert.match(install, /command -v sccache[^\n]*exit 0/);
  assert.match(install, /VERSION=\d+\.\d+\.\d+/);
  for (const t of ['x86_64-unknown-linux-musl', 'aarch64-unknown-linux-musl', 'aarch64-apple-darwin', 'x86_64-pc-windows-msvc']) {
    assert.match(install, new RegExp(`T=${t};\\s+EXT=(tar\\.gz|zip);\\s+SUM=[0-9a-f]{64}`), `${t}: pinned with a SHA-256`);
  }
  assert.match(install, /checksum mismatch/);
  assert.match(install, /\$GITHUB_PATH/);
  assert.match(install, /RUNNER_TOOL_CACHE/);
});

test('the sccache-stats action prints the statistics and tolerates a missing sccache; every Rust job of Paddock CI uses both on every OS', () => {
  const stats = read(join(ACTIONS, 'sccache-stats', 'action.yml'));
  assert.match(stats, /sccache --show-stats/);
  assert.match(stats, /command -v sccache/);
  const ci = read(join(WORKFLOWS, 'ci.yml'));
  // The jobs that build Rust, by name (a new job that builds Rust joins this list; a count would hide which one).
  const jobsUsing = (action) => {
    const found = [];
    let job = '';
    for (const line of ci.split('\n')) {
      const j = /^ {2}([a-z0-9-]+):\s*$/.exec(line);
      if (j) job = j[1];
      if (line.includes(`uses: ./.gitea/actions/${action}`)) found.push(job);
    }
    return found;
  };
  const rustJobs = ['crates', 'crate', 'shell-demo-app', 'workbench-demo-app'];
  assert.deepEqual(jobsUsing('setup-rust'), rustJobs, 'setup-rust in every job that builds Rust');
  assert.deepEqual(jobsUsing('sccache-stats'), rustJobs, 'stats after each');
  assert.doesNotMatch(ci, /setup-rust\n\s+if: runner\.os/, 'no OS leg is left out');
  assert.doesNotMatch(ci, /dtolnay\/rust-toolchain/, 'one toolchain setup');
});

/** The first plain scalar in `text` (outside block scalars and comments) that a strict YAML parser (Gitea's go-yaml) rejects: `key: a: b`. */
export function strictPlainScalarProblem(text) {
  const lines = text.split('\n');
  let blockIndent = -1;
  for (let i = 0; i < lines.length; i++) {
    const line = lines[i];
    if (line.trim() === '') continue;
    const indent = line.length - line.trimStart().length;
    if (blockIndent >= 0) {
      if (indent > blockIndent) continue;
      blockIndent = -1;
    }
    if (line.trim().startsWith('#')) continue;
    const m = /^(\s*(?:- )*)([A-Za-z0-9_.-]+):(?:\s+(.*))?$/.exec(line);
    if (!m) continue;
    const value = (m[3] ?? '').trim();
    if (/^[|>][+-]?\d*(\s+#.*)?$/.test(value)) {
      blockIndent = indent + m[1].trimStart().length; // the block's lines are indented deeper than the key itself
      continue;
    }
    if (value === '' || /^["'[{&*!%@`]/.test(value)) continue;
    if (/:(\s|$)/.test(value)) return `line ${i + 1}: unquoted ": " in a plain value (quote it): ${line.trim().slice(0, 100)}`;
  }
  return null;
}

test('every action and workflow parses under a strict YAML reader (no ": " inside an unquoted value)', () => {
  const files = [
    ...names.map((n) => join(ACTIONS, n, 'action.yml')),
    ...readdirSync(WORKFLOWS).filter((f) => /\.ya?ml$/.test(f)).map((f) => join(WORKFLOWS, f)),
  ];
  assert.ok(files.length >= 20);
  for (const f of files) assert.equal(strictPlainScalarProblem(read(f)), null, f);
});

test('the strict reader check catches the sccache-stats mistake and leaves block scalars alone', () => {
  assert.match(strictPlainScalarProblem('name: x\ndescription: use `if: always()` here\n'), /line 2/);
  assert.equal(strictPlainScalarProblem('description: "use `if: always()` here"\n'), null);
  assert.equal(strictPlainScalarProblem('steps:\n  - run: |\n      echo "a: b"\n      x: y\n  - name: n\n'), null);
  assert.equal(strictPlainScalarProblem('on:\n  push:\nenv:\n  A: https://x/y\n'), null);
});

// sccache puts every CARGO_* variable into its cache key. The host runners' build folder differs per checkout, so CARGO_TARGET_DIR
// alone made every key unique (2% hits on macOS, 0% on Windows, 95-99% in containers). Run the real step with a shim sccache.
function runSccacheStep(env, shim = 'true') {
  const dir = mkdtempSync(join(tmpdir(), 'paddock-sccache-'));
  const action = join(ACTIONS, 'setup-rust');
  const text = read(join(action, 'action.yml'));
  const at = text.indexOf('- name: sccache\n');
  const block = text.slice(at, text.indexOf('- name: Linux dependencies', at));
  const body = block.slice(block.indexOf('run: |\n') + 'run: |\n'.length).split('\n').map((l) => l.slice(8)).join('\n');
  mkdirSync(join(dir, 'bin'));
  writeFileSync(join(dir, 'bin', 'sccache'), `#!/bin/sh\n[ "$1" = --version ] && echo sccache 0.19.0\n${shim}\nexit 0\n`);
  chmodSync(join(dir, 'bin', 'sccache'), 0o755);
  writeFileSync(join(dir, 'step.sh'), body);
  const ghEnv = join(dir, 'github-env');
  writeFileSync(ghEnv, '');
  const r = spawnSync('bash', ['-e', '-o', 'pipefail', join(dir, 'step.sh')], {
    env: { PATH: `${join(dir, 'bin')}:/usr/bin:/bin`, HOME: dir, RUNNER_OS: 'macOS', GITHUB_ENV: ghEnv, BACKEND: 'auto', REDIS_ENDPOINT: '', REDIS_DB: '', ACTION_DIR: action, ...env },
    encoding: 'utf8',
  });
  assert.equal(r.status, 0, r.stderr + r.stdout);
  const written = readFileSync(ghEnv, 'utf8');
  return shim === 'true' ? written : { env: written, stdout: r.stdout };
}

test('a job with its own CARGO_TARGET_DIR runs rustc through a wrapper that keeps the folder out of the sccache key', () => {
  const withTarget = runSccacheStep({ CARGO_TARGET_DIR: '/work/abc/.paddock-ci-target' });
  assert.match(withTarget, new RegExp(`^RUSTC_WRAPPER=${join(ACTIONS, 'setup-rust', 'rustc-wrapper.sh').replace(/[.*+?^${}()|[\]\\]/g, '\\$&')}$`, 'm'));
  assert.match(runSccacheStep({ CARGO_BUILD_TARGET_DIR: '/x' }), /^RUSTC_WRAPPER=.*rustc-wrapper\.sh$/m);
  assert.match(runSccacheStep({}), /^RUSTC_WRAPPER=sccache$/m, 'no build folder variable: sccache directly');
});

test('the rustc wrapper hands sccache the compile without the build folder variables and with everything else', () => {
  const dir = mkdtempSync(join(tmpdir(), 'paddock-wrapper-'));
  mkdirSync(join(dir, 'bin'));
  writeFileSync(join(dir, 'bin', 'sccache'), '#!/bin/sh\necho "args=$*"\necho "target=${CARGO_TARGET_DIR-unset} build=${CARGO_BUILD_TARGET_DIR-unset} manifest=${CARGO_MANIFEST_DIR-unset}"\nexit 7\n');
  chmodSync(join(dir, 'bin', 'sccache'), 0o755);
  const wrapper = join(dir, 'rustc-wrapper.sh');
  copyFileSync(join(ACTIONS, 'setup-rust', 'rustc-wrapper.sh'), wrapper);
  chmodSync(wrapper, 0o755);
  const r = spawnSync(wrapper, ['rustc', '--crate-name', 'a b'], {
    env: { PATH: `${join(dir, 'bin')}:/usr/bin:/bin`, CARGO_TARGET_DIR: '/t', CARGO_BUILD_TARGET_DIR: '/t', CARGO_MANIFEST_DIR: '/m' },
    encoding: 'utf8',
  });
  assert.equal(r.status, 7, 'the exit status of sccache is the exit status of the wrapper');
  assert.match(r.stdout, /args=rustc --crate-name a b/);
  assert.match(r.stdout, /target=unset build=unset manifest=\/m/);
  const cmd = read(join(ACTIONS, 'setup-rust', 'rustc-wrapper.cmd'));
  assert.match(cmd, /set "CARGO_TARGET_DIR="/);
  assert.match(cmd, /sccache\.exe %\*\r?\nexit \/b %ERRORLEVEL%/);
});

function runStableWorkspace(os, root, over = {}) {
  const text = read(join(ACTIONS, 'stable-workspace', 'action.yml'));
  const body = text.slice(text.indexOf('run: |\n') + 'run: |\n'.length).split('\n').map((l) => l.slice(8)).join('\n');
  const script = join(root, 'step.sh');
  writeFileSync(script, body);
  return spawnSync('bash', ['-e', '-o', 'pipefail', script], {
    env: { PATH: '/usr/bin:/bin', HOME: join(root, 'home'), RUNNER_OS: os, GITHUB_WORKSPACE: join(root, 'work', 'abc', 'hostexecutor'), NAME: 'decryp7/paddock-crate', ...over },
    encoding: 'utf8',
    cwd: join(root, 'work', 'abc', 'hostexecutor'),
  });
}

function stableFixture() {
  const root = realpathSync(mkdtempSync(join(tmpdir(), 'paddock-stable-')));
  mkdirSync(join(root, 'home'));
  mkdirSync(join(root, 'work', 'abc', 'hostexecutor'), { recursive: true });
  writeFileSync(join(root, 'work', 'abc', 'hostexecutor', 'Cargo.toml'), 'x');
  return root;
}

test('stable-workspace gives every run of a host job the same physical checkout path, however the runner named the work folder', () => {
  const root = stableFixture();
  const r = runStableWorkspace('macOS', root);
  assert.equal(r.status, 0, r.stderr);
  const link = join(root, 'work', 'abc', 'hostexecutor');
  const stable = join(root, 'home', '.paddock-ci', 'src', 'decryp7_paddock-crate');
  assert.ok(lstatSync(link).isSymbolicLink());
  assert.equal(realpathSync(link), stable, 'the old path resolves to the stable folder (what cargo and rustc see as their working folder)');
  assert.equal(readFileSync(join(link, 'Cargo.toml'), 'utf8'), 'x');
  assert.equal(runStableWorkspace('macOS', root).status, 0, 'a second run on the linked workspace is a no-op');
  // The next job: a new hashed work folder with a fresh checkout replaces the previous stable folder.
  mkdirSync(join(root, 'work', 'def', 'hostexecutor'), { recursive: true });
  writeFileSync(join(root, 'work', 'def', 'hostexecutor', 'Cargo.toml'), 'y');
  const again = runStableWorkspace('macOS', root, { GITHUB_WORKSPACE: join(root, 'work', 'def', 'hostexecutor') });
  assert.equal(again.status, 0, again.stderr);
  assert.equal(realpathSync(join(root, 'work', 'def', 'hostexecutor')), stable, 'same physical path as the job before');
  assert.equal(readFileSync(join(stable, 'Cargo.toml'), 'utf8'), 'y');
});

test('stable-workspace leaves Linux containers alone and never fails a job it cannot help', () => {
  const root = stableFixture();
  assert.equal(runStableWorkspace('Linux', root).status, 0);
  assert.ok(!lstatSync(join(root, 'work', 'abc', 'hostexecutor')).isSymbolicLink());
  // The stable root cannot be created (a file is in the way): warn and build in place.
  writeFileSync(join(root, 'blocked'), '');
  const r = runStableWorkspace('macOS', root, { PADDOCK_CI_HOME: join(root, 'blocked') });
  assert.equal(r.status, 0, r.stderr);
  assert.match(r.stdout, /::warning::stable-workspace: could not move/);
  assert.ok(!lstatSync(join(root, 'work', 'abc', 'hostexecutor')).isSymbolicLink(), 'the checkout stays where it was');
  assert.equal(readFileSync(join(root, 'work', 'abc', 'hostexecutor', 'Cargo.toml'), 'utf8'), 'x');
});

test('CI moves the checkout to a stable path on the host runners only, after Git bash is on PATH on Windows', () => {
  const ci = read(join(WORKFLOWS, 'ci.yml'));
  const at = ci.indexOf('uses: ./.gitea/actions/stable-workspace');
  assert.ok(at > 0, 'the crate job uses stable-workspace');
  assert.match(ci.slice(at, at + 120), /\n\s+if: runner\.os != 'Linux'/, 'host runners only; on Windows `shell: bash` before Git bash is on PATH resolves to WSL');
  const gitBash = ci.indexOf('Git bash on PATH');
  assert.ok(gitBash > 0 && gitBash < at, 'Git bash is on PATH before any later step needs bash');
  assert.equal(ci.split('uses: ./.gitea/actions/stable-workspace').length - 1, 1);
});
