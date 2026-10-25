// The composite actions and reusable workflows are YAML the runner only reads on CI, so their wiring is checked here: every
// run step names its shell, every script and local action they reference exists, every input they use or pass is declared.
// (The scripts themselves are tested in ci-scripts.test.mjs, tools/test and the release tests.)
import assert from 'node:assert/strict';
import { spawnSync } from 'node:child_process';
import { chmodSync, copyFileSync, existsSync, lstatSync, mkdirSync, mkdtempSync, readdirSync, readFileSync, realpathSync, symlinkSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { dirname, join } from 'node:path';
import { pathToFileURL } from 'node:url';
import { test } from 'node:test';
import { executableIn, platformId } from '../ci/install-test-browser.mjs';
import { isMain } from '../src/util/is-main.mjs';
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
  // The mount path is injected: the test never looks at the runner's real /sccache.
  const nomount = join(tmpdir(), 'paddock-nomount');
  const linux = runSccacheStep({ RUNNER_OS: 'Linux', RUNNER_TEMP: nomount, PADDOCK_SCCACHE_MOUNT: join(nomount, 'absent-mount') });
  assert.match(linux, /^SCCACHE_DIR=.*paddock-nomount\/sccache$/m, 'no mount: a folder of the container');
  const mount = mkdtempSync(join(tmpdir(), 'paddock-mount-'));
  const mounted = runSccacheStep({ RUNNER_OS: 'Linux', RUNNER_TEMP: nomount, PADDOCK_SCCACHE_MOUNT: mount });
  assert.match(mounted, new RegExp(`^SCCACHE_DIR=${mount.replace(/[.*+?^${}()|[\]\\]/g, '\\$&')}$`, 'm'), 'a writable mount is used');
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
  const rustJobs = ['crates', 'crate', 'app'];
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

// Run 6365: a `rustc-wrapper.cmd` between cargo and sccache broke every Windows build ("The command line is too long" for
// windows-sys): Windows starts a .cmd through cmd.exe, whose command line is 8191 characters at most.
test('no script sits between cargo and rustc: RUSTC_WRAPPER is sccache itself, whatever build folder the job has', () => {
  for (const env of [{}, { CARGO_TARGET_DIR: '/work/abc/.paddock-ci-target' }, { CARGO_BUILD_TARGET_DIR: '/x' }]) {
    assert.match(runSccacheStep(env), /^RUSTC_WRAPPER=sccache$/m, JSON.stringify(env));
  }
  const scripts = [];
  const walk = (dir) => {
    for (const e of readdirSync(dir, { withFileTypes: true })) {
      if (e.isDirectory()) walk(join(dir, e.name));
      else if (/\.(cmd|bat)$/i.test(e.name)) scripts.push(join(dir, e.name));
    }
  };
  walk(ACTIONS);
  assert.deepEqual(scripts, [], 'no cmd.exe script in the shared actions');
  const files = [...names.map((n) => join(ACTIONS, n, 'action.yml')), ...readdirSync(WORKFLOWS).filter((f) => /\.ya?ml$/.test(f)).map((f) => join(WORKFLOWS, f))];
  for (const f of files) {
    // sccache itself, the action's $WRAPPER, or an expression that picks sccache or nothing (`${{ ... && 'sccache' || '' }}`).
    for (const m of read(f).matchAll(/RUSTC_WRAPPER\s*[=:]\s*("?)(\$\{\{[^}]*\}\}|[^"\s>]*)/g)) {
      const v = m[2];
      const expr = /^\$\{\{/.test(v) && [...v.matchAll(/'([^']*)'/g)].every((q) => /^(sccache|ubuntu-latest|macos-latest|windows-latest)?$/.test(q[1]));
      assert.ok(expr || /^(sccache|\$WRAPPER)?$/.test(v), `${f}: RUSTC_WRAPPER=${v}; a wrapper script fails on Windows`);
    }
  }
});

// The crate job's host legs keep their build folder outside the checkout. CARGO_TARGET_DIR would put it in sccache's key (every
// CARGO_* variable is hashed), so it is cargo configuration, passed on every cargo command of the job.
test('the crate job sets its build folder as cargo configuration on every cargo step, never with CARGO_TARGET_DIR', () => {
  const ci = read(join(WORKFLOWS, 'ci.yml'));
  const job = ci.match(/^  crate:\n((?:    .*\n|\n)+)/m)?.[1] ?? '';
  assert.notEqual(job, '', 'the crate job exists');
  assert.doesNotMatch(job, /CARGO_(BUILD_)?TARGET_DIR=/, 'no step sets the build folder variable');
  const set = [...job.matchAll(/PADDOCK_CARGO_TARGET=(\S+?)'?(?:"|>>)/g)].map((m) => m[1]);
  assert.equal(set.length, 2, 'one per host OS');
  for (const v of set) {
    assert.match(v, /^--config=build\.target-dir='.*\.paddock-ci[\\/]target-/, v);
    assert.doesNotMatch(v, /GITHUB_WORKSPACE|\.\.|\s/, `${v}: outside the checkout, and no spaces (the variable is expanded unquoted)`);
  }
  const cargo = job.split('\n').filter((l) => /^\s*(run: )?cargo (clippy|nextest|test|build|check|llvm-cov)\b/.test(l) && !/--version/.test(l));
  assert.ok(cargo.length >= 6, `${cargo.length} cargo commands`);
  for (const l of cargo) assert.match(l, /cargo (clippy|nextest run|test|build|check|llvm-cov) \$PADDOCK_CARGO_TARGET /, l.trim());
});

// Run 6365: after stable-workspace linked the macOS work folder, `node <work folder>/.../install-test-browser.mjs` exited 0 and
// printed nothing. Node gives the main module's import.meta.url its real path, so `import.meta.url === pathToFileURL(argv[1])`
// was false and main() never ran; the real-browser test then failed with "CHROMIUM_TEST_PATH names no Chrome for Testing".
test('the test-browser installer runs, prints and exports the browser when it is started through a linked folder', () => {
  const root = mkdtempSync(join(tmpdir(), 'paddock-linked-'));
  symlinkSync(PADDOCK, join(root, 'work'), 'dir');
  const dir = join(root, 'browser');
  const id = platformId();
  const exe = executableIn(dir, id);
  mkdirSync(dirname(exe), { recursive: true });
  writeFileSync(exe, '');
  writeFileSync(join(dir, '.pinned-version'), '1.2.3');
  writeFileSync(join(root, 'pin.json'), JSON.stringify({ version: '1.2.3', base: 'https://example.invalid', sums: { [id]: 'a'.repeat(64) } }));
  const ghEnv = join(root, 'github-env');
  writeFileSync(ghEnv, '');
  // The path as the action builds it: through the link and with `..` segments.
  const script = join(root, 'work', '.gitea', 'actions', 'install-test-browser', '..', '..', '..', 'conformance', 'ci', 'install-test-browser.mjs');
  const r = spawnSync(process.execPath, [script, '--pin', join(root, 'pin.json'), '--dir', dir, '--env-prefix', 'PADDOCK'], {
    env: { PATH: process.env.PATH, GITHUB_ENV: ghEnv },
    encoding: 'utf8',
  });
  assert.equal(r.status, 0, r.stderr);
  assert.match(r.stdout, new RegExp(`^CHROMIUM_TEST_PATH=${exe.replace(/[.*+?^${}()|[\]\\]/g, '\\$&')}$`, 'm'));
  assert.match(read(ghEnv), /^CHROMIUM_TEST_PATH=.+\nPADDOCK_REQUIRE_TEST_BROWSER=1\n$/);
});

test('install-test-browser fails when the installer printed no CHROMIUM_TEST_PATH', () => {
  const text = read(join(ACTIONS, 'install-test-browser', 'action.yml'));
  const body = text.slice(text.indexOf('run: |\n') + 'run: |\n'.length).split('\n').map((l) => l.slice(8)).join('\n');
  const root = mkdtempSync(join(tmpdir(), 'paddock-itb-'));
  mkdirSync(join(root, 'bin'));
  const run = (out) => {
    writeFileSync(join(root, 'bin', 'node'), `#!/bin/sh\n${out}\nexit 0\n`);
    chmodSync(join(root, 'bin', 'node'), 0o755);
    writeFileSync(join(root, 'step.sh'), body);
    return spawnSync('bash', ['-e', '-o', 'pipefail', join(root, 'step.sh')], {
      env: { PATH: `${join(root, 'bin')}:/usr/bin:/bin`, CATALOGUE: 'c.toml', PIN: '', RUST_SOURCE: '', PREFIX: 'PADDOCK', DIR: '/d', INSTALLER: '/i.mjs' },
      encoding: 'utf8',
    });
  };
  const silent = run('true');
  assert.notEqual(silent.status, 0, 'an installer that printed nothing fails the step');
  assert.match(silent.stdout, /::error::install-test-browser printed no CHROMIUM_TEST_PATH/);
  const ok = run('echo CHROMIUM_TEST_PATH=/b/chrome; echo PADDOCK_REQUIRE_TEST_BROWSER=1');
  assert.equal(ok.status, 0, ok.stdout + ok.stderr);
  assert.match(ok.stdout, /^CHROMIUM_TEST_PATH=\/b\/chrome$/m, 'the installer output is shown');
});

// The same mistake in any script: an entry point compared without resolving links does nothing when it is reached through one
// (a linked work folder, node_modules). Every script decides with isMain.
test('every Node script decides it is the main module with isMain, which resolves links', () => {
  const listed = spawnSync('git', ['ls-files', '*.mjs', '*.js'], { cwd: PADDOCK, encoding: 'utf8' });
  assert.equal(listed.status, 0, listed.stderr);
  // Paddock's own scripts and the demos'; an app's (apps/<app>/) follow the rule when they move onto the shared scripts.
  const files = listed.stdout.split('\n').filter((f) => f && !f.includes('node_modules/') && !/^apps\/(?!shell-demo\/|workbench-demo\/)/.test(f));
  assert.ok(files.length > 50, `${files.length} scripts`);
  const naive = /import\.meta\.url\s*===|===\s*import\.meta\.url|fileURLToPath\(import\.meta\.url\)\s*===|===\s*fileURLToPath\(import\.meta\.url\)(?!\))|pathToFileURL\(process\.argv\[1\]/;
  const bad = files.filter((f) => naive.test(read(join(PADDOCK, f))) && !f.endsWith('ci-actions.test.mjs'));
  assert.deepEqual(bad, [], 'compare real paths: isMain(import.meta.url) from conformance/src/util/is-main.mjs');
  const root = mkdtempSync(join(tmpdir(), 'paddock-ismain-'));
  writeFileSync(join(root, 'a.mjs'), '');
  symlinkSync(join(root, 'a.mjs'), join(root, 'b.mjs'));
  const url = pathToFileURL(realpathSync(join(root, 'a.mjs'))).href;
  assert.equal(isMain(url, join(root, 'b.mjs')), true, 'through a link');
  assert.equal(isMain(url, join(root, 'x', '..', 'a.mjs')), true, 'with .. segments');
  assert.equal(isMain(url, join(root, 'missing.mjs')), false);
  assert.equal(isMain(url, undefined), false);
  assert.equal(isMain(url, ''), false);
});

function runStableWorkspace(os, root, over = {}) {
  const text = read(join(ACTIONS, 'stable-workspace', 'action.yml'));
  const body = text.slice(text.indexOf('run: |\n') + 'run: |\n'.length).split('\n').map((l) => l.slice(8)).join('\n');
  const script = join(root, 'step.sh');
  writeFileSync(script, body);
  return spawnSync('bash', ['-e', '-o', 'pipefail', script], {
    env: { PATH: '/usr/bin:/bin', HOME: join(root, 'home'), RUNNER_OS: os, GITHUB_WORKSPACE: join(root, 'work', 'abc', 'hostexecutor'), GITHUB_ENV: join(root, 'github-env'), NAME: 'decryp7/paddock-crate', ...over },
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
  assert.match(readFileSync(join(root, 'github-env'), 'utf8'), new RegExp(`^PADDOCK_WORKSPACE=${stable}$`, 'm'), 'later steps build from the stable folder');
  assert.equal(runStableWorkspace('macOS', root).status, 0, 'a second run on the linked workspace is a no-op');
  // The next job: a new hashed work folder with a fresh checkout replaces the previous stable folder.
  mkdirSync(join(root, 'work', 'def', 'hostexecutor'), { recursive: true });
  writeFileSync(join(root, 'work', 'def', 'hostexecutor', 'Cargo.toml'), 'y');
  const again = runStableWorkspace('macOS', root, { GITHUB_WORKSPACE: join(root, 'work', 'def', 'hostexecutor') });
  assert.equal(again.status, 0, again.stderr);
  assert.equal(realpathSync(join(root, 'work', 'def', 'hostexecutor')), stable, 'same physical path as the job before');
  assert.equal(readFileSync(join(stable, 'Cargo.toml'), 'utf8'), 'y');
});

test('stable-workspace keeps a clone at the stable path at the checkout\'s commit where the work folder cannot move (Windows)', () => {
  const root = stableFixture();
  const ws = join(root, 'work', 'abc', 'hostexecutor');
  const git = (cwd, ...args) => {
    const r = spawnSync('git', ['-c', 'user.name=t', '-c', 'user.email=t@t', '-c', 'init.defaultBranch=main', ...args], { cwd, encoding: 'utf8', env: { PATH: '/usr/bin:/bin', HOME: join(root, 'home') } });
    assert.equal(r.status, 0, r.stderr);
    return r.stdout.trim();
  };
  git(ws, 'init', '-q');
  git(ws, 'add', 'Cargo.toml');
  git(ws, 'commit', '-qm', 'one');
  // `mv` fails as it does on the Windows runner, which keeps a handle on the work folder.
  const bin = join(root, 'bin');
  mkdirSync(bin);
  writeFileSync(join(bin, 'mv'), '#!/bin/sh\necho "Device or resource busy" >&2\nexit 1\n', { mode: 0o755 });
  const run = () => runStableWorkspace('macOS', root, { PATH: `${bin}:/usr/bin:/bin` });
  const stable = join(root, 'home', '.paddock-ci', 'src', 'decryp7_paddock-crate');
  const r = run();
  assert.equal(r.status, 0, r.stderr);
  assert.ok(!lstatSync(ws).isSymbolicLink(), 'the checkout stays where it was');
  assert.equal(readFileSync(join(stable, 'Cargo.toml'), 'utf8'), 'x');
  assert.equal(git(stable, 'rev-parse', 'HEAD'), git(ws, 'rev-parse', 'HEAD'));
  assert.match(readFileSync(join(root, 'github-env'), 'utf8'), new RegExp(`^PADDOCK_WORKSPACE=${stable}$`, 'm'));
  // The next run: a new commit, and a file the last build left in the clone is gone.
  writeFileSync(join(ws, 'Cargo.toml'), 'y');
  git(ws, 'commit', '-qam', 'two');
  writeFileSync(join(stable, 'left-over.rs'), '');
  assert.equal(run().status, 0);
  assert.equal(readFileSync(join(stable, 'Cargo.toml'), 'utf8'), 'y');
  assert.equal(git(stable, 'rev-parse', 'HEAD'), git(ws, 'rev-parse', 'HEAD'));
  assert.ok(!existsSync(join(stable, 'left-over.rs')));
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
  assert.ok(!existsSync(join(root, 'github-env')) || !/PADDOCK_WORKSPACE/.test(readFileSync(join(root, 'github-env'), 'utf8')), 'later steps build in the work folder');
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

test('the test browser of a host job lives in a kept folder of the runner user, outside the checkout that stable-workspace moves', () => {
  const ci = read(join(WORKFLOWS, 'ci.yml'));
  const dirs = [...ci.matchAll(/PADDOCK_TEST_BROWSER_DIR=(\S+)/g)].map((m) => m[1]);
  assert.equal(dirs.length, 2, 'one per host OS');
  for (const d of dirs) {
    assert.doesNotMatch(d, /GITHUB_WORKSPACE|\.\./, `${d}: a path relative to the checkout dies when the checkout moves`);
    assert.match(d, /\.paddock-ci[\\/]test-browser/, d);
  }
  assert.ok(ci.indexOf('PADDOCK_TEST_BROWSER_DIR') < ci.indexOf('uses: ./.gitea/actions/install-test-browser'), 'the folder is chosen before the browser is installed');
});

test('the WebKit job runs on the Mac runner for pushes to main and v* tags only, and runs the three WebKit projects', () => {
  const ci = read(join(WORKFLOWS, 'ci.yml'));
  const on = ci.match(/^on:\n((?:[ \t]+.*\n|\n)+)/m)?.[1] ?? '';
  assert.match(on, /push:\n\s+branches: \[main\]\n\s+tags: \["v\*"\]/, 'a push is only main or a v* tag');
  const job = ci.match(/^  e2e-webkit:\n((?:    .*\n|\n)+)/m)?.[1] ?? '';
  assert.notEqual(job, '', 'the e2e-webkit job exists');
  assert.match(job, /runs-on: macos-latest/);
  const cond = job.match(/^    if: (.*)$/m)?.[1] ?? '';
  assert.match(cond, /github\.event_name == 'push'/, 'never a pull request or a manual run');
  assert.match(cond, /github\.ref == 'refs\/heads\/main'/);
  assert.match(cond, /startsWith\(github\.ref, 'refs\/tags\/v'\)/);
  assert.doesNotMatch(cond, /pull_request|workflow_dispatch/);
  for (const project of ['gallery-webkit', 'shell-demo-webkit', 'workbench-demo-webkit']) assert.match(job, new RegExp(`--project=${project}`));
  assert.match(job, /PADDOCK_WEBKIT: "1"/, 'the projects exist only with PADDOCK_WEBKIT');
});

test('the nextest ci profile fails a test over its time budget, and every longer budget is a waiver with a reason', () => {
  const text = read(join(PADDOCK, 'crates', '.config', 'nextest.toml'));
  const ci = text.slice(text.indexOf('[profile.ci]\n') + '[profile.ci]\n'.length);
  const own = ci.slice(0, ci.search(/^\[/m));
  assert.match(own, /^slow-timeout = \{ period = "\d+s", terminate-after = \d+ \}$/m, 'the ci profile stops a slow test');
  const lines = text.split('\n');
  const overrides = lines.flatMap((l, i) => (l.trim() === '[[profile.ci.overrides]]' ? [i] : []));
  for (const at of overrides) {
    let j = at - 1;
    while (j >= 0 && lines[j].startsWith('#') && !lines[j].startsWith('# waiver:')) j -= 1;
    assert.ok(j >= 0 && /^# waiver: \S/.test(lines[j]), `the override at line ${at + 1} of nextest.toml has no "# waiver:" reason`);
    const body = lines.slice(at + 1, lines.findIndex((l, k) => k > at && l.startsWith('['))).join('\n');
    assert.match(body, /^filter = "/m, `the override at line ${at + 1} names its tests`);
  }
});

/** Runs host-tools' script with a PATH of shims; `nextest` is the version the shim `cargo-nextest` reports. */
function runHostTools(os, { kept = null, onPath = null } = {}) {
  const root = realpathSync(mkdtempSync(join(tmpdir(), 'paddock-host-tools-')));
  const text = read(join(ACTIONS, 'host-tools', 'action.yml'));
  const body = text.slice(text.indexOf('run: |\n') + 'run: |\n'.length).split('\n').map((l) => l.slice(8)).join('\n');
  const version = /^VERSION=(\S+)$/m.exec(body)[1];
  const shim = (dir, v) => {
    mkdirSync(dir, { recursive: true });
    writeFileSync(join(dir, 'cargo-nextest'), `#!/bin/sh\necho "cargo-nextest ${v}"\n`, { mode: 0o755 });
  };
  const bin = join(root, 'bin');
  mkdirSync(bin);
  // Any download fails the test: a kept or image copy must not be fetched again.
  writeFileSync(join(bin, 'curl'), '#!/bin/sh\necho "curl was called" >&2\nexit 7\n', { mode: 0o755 });
  if (kept) shim(join(root, 'home', '.paddock-ci', 'tools', 'cargo-nextest', version), kept === 'pin' ? version : kept);
  if (onPath) shim(bin, onPath === 'pin' ? version : onPath);
  writeFileSync(join(root, 'step.sh'), body);
  const r = spawnSync('bash', ['-e', '-o', 'pipefail', join(root, 'step.sh')], {
    env: { PATH: `${bin}:/usr/bin:/bin`, HOME: join(root, 'home'), RUNNER_OS: os, GITHUB_PATH: join(root, 'github-path') },
    encoding: 'utf8',
  });
  return { ...r, root, version };
}

test('host-tools keeps the pinned cargo-nextest on a host and checks the version on every runner', () => {
  const kept = runHostTools('macOS', { kept: 'pin' });
  assert.equal(kept.status, 0, kept.stderr + kept.stdout);
  assert.match(kept.stdout, /kept in .*tools\/cargo-nextest\//, 'no download when the pinned version is there');
  assert.match(readFileSync(join(kept.root, 'github-path'), 'utf8'), new RegExp(`tools/cargo-nextest/${kept.version.replace(/\./g, '\\.')}$`, 'm'));
  const linux = runHostTools('Linux', { onPath: 'pin' });
  assert.equal(linux.status, 0, linux.stderr + linux.stdout);
  const other = runHostTools('Linux', { onPath: '0.0.1' });
  assert.notEqual(other.status, 0);
  assert.match(other.stdout, /cargo-nextest is 0\.0\.1 on this runner, the pin is/);
  const missing = runHostTools('macOS');
  assert.notEqual(missing.status, 0, 'a missing copy is downloaded (the curl shim fails here)');
  assert.match(missing.stderr, /curl was called/);
});

test('the host-tools pin is a release version with SHA-256 sums', () => {
  const pin = /^\s+VERSION=(\S+)$/m.exec(read(join(ACTIONS, 'host-tools', 'action.yml')))[1];
  assert.match(pin, /^\d+\.\d+\.\d+$/);
  for (const sum of read(join(ACTIONS, 'host-tools', 'action.yml')).matchAll(/SUM=(\S+);/g)) assert.match(sum[1], /^[0-9a-f]{64}$/);
});

function runNodeCaches(os, env = {}) {
  const root = realpathSync(mkdtempSync(join(tmpdir(), 'paddock-node-caches-')));
  const text = read(join(ACTIONS, 'node-caches', 'action.yml'));
  const body = text.slice(text.indexOf('run: |\n') + 'run: |\n'.length).split('\n').map((l) => l.slice(8)).join('\n');
  mkdirSync(join(root, 'ws', 'ui', '.angular', 'cache'), { recursive: true });
  writeFileSync(join(root, 'ws', 'ui', '.angular', 'cache', 'stale'), '');
  writeFileSync(join(root, 'step.sh'), body);
  const r = spawnSync('bash', ['-e', '-o', 'pipefail', join(root, 'step.sh')], {
    env: { PATH: '/usr/bin:/bin', HOME: join(root, 'home'), RUNNER_OS: os, GITHUB_WORKSPACE: join(root, 'ws'), GITHUB_ENV: join(root, 'github-env'), ANGULAR_DIR: 'ui', NAME: 'e2e-1', PADDOCK_CI_MOUNT: join(root, 'no-mount'), ...env },
    encoding: 'utf8',
  });
  assert.equal(r.status, 0, r.stderr + r.stdout);
  const written = existsSync(join(root, 'github-env')) ? readFileSync(join(root, 'github-env'), 'utf8') : '';
  return { root, stdout: r.stdout, written };
}

test('node-caches keeps the npm and Angular caches on the host or the container mount, never failing a job', () => {
  const host = runNodeCaches('macOS');
  const kept = join(host.root, 'home', '.paddock-ci');
  assert.match(host.written, new RegExp(`^npm_config_cache=${kept}/npm$`, 'm'));
  assert.equal(realpathSync(join(host.root, 'ws', 'ui', '.angular', 'cache')), join(kept, 'angular', 'e2e-1'), 'one Angular cache per job and shard');
  const mount = join(host.root, 'mount');
  mkdirSync(mount);
  const linux = runNodeCaches('Linux', { PADDOCK_CI_MOUNT: mount });
  assert.match(linux.written, /^npm_config_cache=.*\/mount\/npm$/m);
  const bare = runNodeCaches('Linux');
  assert.equal(bare.written, '');
  assert.match(bare.stdout, /::notice::no \/sccache mount/);
  assert.match(read(join(PADDOCK, 'ui', 'angular.json')), /"cache": \{ "enabled": true, "environment": "all" \}/, 'Angular keeps its cache in CI too');
});
