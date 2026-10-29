// The composite actions and reusable workflows are YAML the runner only reads on CI, so their wiring is checked here: every
// run step names its shell, every script and local action they reference exists, every input they use or pass is declared.
// (The scripts themselves are tested in ci-scripts.test.mjs, tools/test and the release tests.)
import assert from 'node:assert/strict';
import { spawnSync } from 'node:child_process';
import { chmodSync, copyFileSync, existsSync, lstatSync, mkdirSync, mkdtempSync, readdirSync, readFileSync, realpathSync, symlinkSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { dirname, join, relative } from 'node:path';
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

test('the release workflow cannot publish from anything but its own v* tag, and signs only with the secrets it declares', () => {
  const text = read(join(WORKFLOWS, 'app-release.yml'));
  assert.match(text, /release:\n(?:.*\n)*?\s+if: startsWith\(github\.ref, format\('refs\/tags\/\{0\}v', inputs\.tag-prefix\)\)/);
  const action = read(join(ACTIONS, 'publish-release', 'action.yml'));
  assert.match(action, /refs\/tags\/\$\{PREFIX\}v/);
  assert.match(action, /releases are published from a \$\{PREFIX\}v\* tag only \(rule 9\)/);
  assert.match(action, /version="\$\{tag#"\$PREFIX"\}"/, 'latest.json gets the version after the prefix');
});

// A host job's work folder is not where the build ran: on Windows stable-workspace keeps a clone at the stable path and the work
// folder stays, so a relative `dist/*` finds nothing and the bundle job fails after building (or uploads the wrong files).
test('in a job that moves the checkout (stable-workspace), every upload-artifact path starts at PADDOCK_WORKSPACE', () => {
  const stable = /uses: \.\/(?:\.paddock\/)?\.gitea\/actions\/stable-workspace/;
  let checked = 0;
  for (const file of readdirSync(WORKFLOWS).filter((x) => /\.ya?ml$/.test(x))) {
    for (const job of parseJobs(read(join(WORKFLOWS, file)))) {
      if (!stable.test(job.text)) continue;
      for (const step of parseSteps(job.text).filter((st) => /actions\/upload-artifact@/.test(st.uses ?? ''))) {
        const path = /^\s+path: (.+)$/m.exec(step.text)?.[1] ?? '';
        // A path that is not a build output (a failure report under the app, uploaded `if: failure()`) is judged by its step.
        if (/if: failure\(\)/.test(step.text)) continue;
        if (/^\$\{\{ runner\.temp \}\}\//.test(path)) continue; // outside the checkout by construction
        assert.match(path, /^\$\{\{ env\.PADDOCK_WORKSPACE \|\| github\.workspace \}\}\//, `${file} ${job.id}: upload-artifact path "${path}" must start with the stable workspace`);
        checked += 1;
      }
    }
  }
  assert.ok(checked >= 1, 'app-release.yml at least');
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

// Run 6460 (job 12206): on the Windows host `rustup toolchain install stable` ended 0.4 s into "syncing channel updates" with
// exit 127 and no message (Git Bash reports 127 for a program Windows ended with an NTSTATUS crash code), the same step that
// passed 17 minutes earlier. Run the real step with a shim rustup.
function runToolchainStep(env, { toolchains = '', components = '', targets = '', installRc = 0 } = {}) {
  const dir = mkdtempSync(join(tmpdir(), 'paddock-rustup-'));
  const text = read(join(ACTIONS, 'setup-rust', 'action.yml'));
  const at = text.indexOf('- name: Rust toolchain\n');
  const block = text.slice(at, text.indexOf('- name: Install sccache', at));
  const body = block.slice(block.indexOf('run: |\n') + 'run: |\n'.length).split('\n').map((l) => l.slice(8)).join('\n');
  mkdirSync(join(dir, 'bin'));
  const calls = join(dir, 'calls');
  const lines = (v) => v.split(' ').filter(Boolean).join('\\n');
  writeFileSync(
    join(dir, 'bin', 'rustup'),
    [
      '#!/bin/sh',
      `echo "$*" >> '${calls}'`,
      'case "$1 $2" in',
      `  "toolchain list") printf '${lines(toolchains)}\\n' ;;`,
      `  "component list") printf '${lines(components)}\\n' ;;`,
      `  "target list") printf '${lines(targets)}\\n' ;;`,
      `  "toolchain install") echo "info: syncing channel updates" >&2; exit ${installRc} ;;`,
      'esac',
      'exit 0',
      '',
    ].join('\n'),
  );
  for (const tool of ['rustc', 'cargo']) writeFileSync(join(dir, 'bin', tool), `#!/bin/sh\necho ${tool} 1.99.1\n`);
  for (const f of ['rustup', 'rustc', 'cargo']) chmodSync(join(dir, 'bin', f), 0o755);
  writeFileSync(join(dir, 'step.sh'), body);
  const r = spawnSync('bash', ['-e', '-o', 'pipefail', join(dir, 'step.sh')], {
    env: { PATH: `${join(dir, 'bin')}:/usr/bin:/bin`, HOME: dir, RUNNER_OS: 'Windows', TOOLCHAIN: 'stable', COMPONENTS: 'clippy rustfmt', TARGETS: '', ...env },
    encoding: 'utf8',
  });
  return { status: r.status, out: r.stdout + r.stderr, calls: existsSync(calls) ? read(calls) : '' };
}

test('a failed toolchain install fails the step with a message that names the cause, never a bare exit code', () => {
  const crashed = runToolchainStep({}, { installRc: 127 });
  assert.equal(crashed.status, 1, crashed.out);
  assert.match(crashed.out, /::error::rustup could not install the stable toolchain \(exit 127/);
  assert.match(crashed.out, /NTSTATUS/, 'on Windows, 127 is a crash of rustup, not a missing command');
  const failed = runToolchainStep({ RUNNER_OS: 'Linux' }, { installRc: 1 });
  assert.equal(failed.status, 1, failed.out);
  assert.match(failed.out, /::error::rustup could not install the stable toolchain \(exit 1\)/);
  assert.doesNotMatch(failed.out, /NTSTATUS/);
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
  const rustJobs = ['crates', 'crate', 'app', 'paddock-dev'];
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

// One mechanism sets a job's build folder in every workflow: the shared build-folder action, which writes it to the Cargo
// workspace's .cargo/ci.toml (a kept folder on a host, target/ in a container). CARGO_TARGET_DIR would put it in sccache's
// key (every CARGO_* variable is hashed), a --config flag does not reach `tauri build`, and app copies drift (rule 43).
test('every job that builds Rust on a host sets its build folder with the build-folder action, in a stable checkout', () => {
  const uses = [];
  // Still on CARGO_TARGET_DIR, to move next: the release and smoke workflows of the reference apps. No other workflow may join them.
  const notYet = ['service-smoke.yml', 'shell-demo-release.yml', 'workbench-demo-release.yml'];
  for (const file of readdirSync(WORKFLOWS).filter((x) => /\.ya?ml$/.test(x))) {
    const text = read(join(WORKFLOWS, file));
    const body = text.split('\n').filter((l) => !/^\s*#/.test(l)).join('\n');
    if (notYet.includes(file)) {
      assert.match(body, /CARGO_TARGET_DIR/, `${file} moved onto the build-folder action: take it off the not-yet list`);
      continue;
    }
    assert.doesNotMatch(body, /PADDOCK_CARGO_TARGET|build\.target-dir|CARGO_(BUILD_)?TARGET_DIR\s*[=:]/, `${file}: the build folder is the build-folder action's, never a flag or a CARGO_* variable`);
    assert.doesNotMatch(body, /scripts\/ci\/build-folder\.sh/, `${file}: the shared action, not an app's copy`);
    for (const job of parseJobs(text)) {
      // app-release.yml reaches the kit through its .paddock checkout.
      const at = job.text.search(/uses: \.\/(?:\.paddock\/)?\.gitea\/actions\/build-folder/);
      if (at < 0) continue;
      const sw = job.text.search(/uses: \.\/(?:\.paddock\/)?\.gitea\/actions\/stable-workspace/);
      assert.ok(sw >= 0 && sw < at, `${file} ${job.id}: the stable checkout path before the build folder`);
      const ws = /workspace: \$\{\{ env\.PADDOCK_WORKSPACE \|\| github\.workspace \}\}\/([^\n]+)\n\s+name: (\S+)/.exec(job.text.slice(at));
      assert.ok(ws, `${file} ${job.id}: the workspace in the stable checkout, and a folder name`);
      uses.push(`${file}:${job.id}`);
      // The kit's jobs (ci.yml's app leg, app-release.yml) take the folder from an input or the matrix: build-folder.sh
      // checks the include at run time, and the app tests below check every app that lists a host OS.
      if (ws[1].startsWith('${{')) continue;
      const conf = read(join(PADDOCK, ws[1], '.cargo', 'config.toml'));
      assert.match(conf, /^include = \[.*\{ path = "ci\.toml", optional = true \}\]$/m, `${ws[1]}: .cargo/config.toml includes the job's ci.toml`);
      const ignored = spawnSync('git', ['check-ignore', '-q', join(ws[1], '.cargo', 'ci.toml')], { cwd: PADDOCK });
      assert.equal(ignored.status, 0, `${ws[1]}/.cargo/ci.toml is git-ignored`);
    }
  }
  for (const want of ['ci.yml:crate', 'ci.yml:app', 'app-release.yml:bundle', 'debrief.yml:rust-test-macos', 'debrief.yml:tauri-smoke', 'debrief.yml:bundle']) {
    assert.ok(uses.includes(want), `${want} sets its build folder with the action (found: ${uses.join(', ')})`);
  }
  const copies = spawnSync('git', ['ls-files', 'apps/*/scripts/ci/build-folder.sh'], { cwd: PADDOCK, encoding: 'utf8' });
  assert.equal(copies.stdout.trim(), '', 'no app keeps its own copy of the build-folder script (rule 43)');
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
  // Paddock's own scripts, the demos' and Debrief's (its host jobs run from a linked checkout); another app's (apps/<app>/)
  // follow the rule when they move onto the shared scripts.
  const files = listed.stdout.split('\n').filter((f) => f && !f.includes('node_modules/') && !/^apps\/(?!shell-demo\/|workbench-demo\/|debrief\/)/.test(f));
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
  // The clone is kept between jobs (2026-10-29: the move was tried after deleting the stable folder, so every Windows job
  // cloned and checked out the whole repository again, a median 49 s of the busiest runner): only the new commit is fetched.
  writeFileSync(join(stable, '.git', 'kept-between-jobs'), '');
  assert.equal(run().status, 0);
  assert.equal(readFileSync(join(stable, 'Cargo.toml'), 'utf8'), 'y');
  assert.equal(git(stable, 'rev-parse', 'HEAD'), git(ws, 'rev-parse', 'HEAD'));
  assert.ok(!existsSync(join(stable, 'left-over.rs')));
  assert.ok(existsSync(join(stable, '.git', 'kept-between-jobs')), 'the kept clone is reused, not deleted and cloned again');
  assert.ok(!existsSync(`${stable}.incoming`), 'no half-moved folder is left behind');
});

import { randomBytes } from 'node:crypto';

// The Windows path as the runner sees it (review of ci/optimise-runs, 2026-10-29): every job gets a fresh depth-1 checkout in a
// work folder that cannot move (act holds it), and the kept clone at the stable path is brought to that commit.
function keptCloneFixture() {
  const root = stableFixture();
  const ws = join(root, 'work', 'abc', 'hostexecutor');
  const origin = join(root, 'origin');
  const env = { PATH: '/usr/bin:/bin', HOME: join(root, 'home'), GIT_CONFIG_NOSYSTEM: '1' };
  const git = (cwd, ...args) => {
    const r = spawnSync('git', ['-c', 'user.name=t', '-c', 'user.email=t@t', '-c', 'init.defaultBranch=main', '-c', 'protocol.file.allow=always', ...args], { cwd, encoding: 'utf8', env });
    assert.equal(r.status, 0, `git ${args.join(' ')}: ${r.stderr}`);
    return r.stdout.trim();
  };
  mkdirSync(origin);
  git(origin, 'init', '-q');
  // 1 MiB that does not compress: a fetch that resends the tree shows up in the clone's size.
  writeFileSync(join(origin, 'big.bin'), randomBytes(1 << 20));
  writeFileSync(join(origin, '.gitignore'), 'dist/\n');
  // More objects than fetch.unpackLimit (100), as in the real tree: a fetch is then kept as a pack, duplicates and all
  // (below the limit git unpacks it and skips the objects it has, which would hide the growth).
  mkdirSync(join(origin, 'src'));
  for (let i = 0; i < 150; i += 1) writeFileSync(join(origin, 'src', `f${i}.rs`), randomBytes(64).toString('hex'));
  const bin = join(root, 'bin');
  mkdirSync(bin);
  writeFileSync(join(bin, 'mv'), '#!/bin/sh\necho "Device or resource busy" >&2\nexit 1\n', { mode: 0o755 });
  let n = 0;
  /** A new commit in origin and a fresh depth-1 checkout of it in the work folder, as actions/checkout leaves it. */
  const checkout = () => {
    n += 1;
    // Two commits per job: the commit the clone has is never the parent of the new one (a direct parent hides the growth:
    // git then counts the clone's shallow boundary as common), as on main between two jobs.
    writeFileSync(join(origin, 'between.txt'), `${n}`);
    git(origin, 'add', '-A');
    git(origin, 'commit', '-qm', `between ${n}`);
    writeFileSync(join(origin, 'Cargo.toml'), `v${n}`);
    git(origin, 'add', '-A');
    git(origin, 'commit', '-qm', `c${n}`);
    // As actions/checkout does it: a new repository, the one commit fetched at depth 1, a detached checkout.
    const sha = git(origin, 'rev-parse', 'HEAD');
    spawnSync('rm', ['-rf', ws]);
    mkdirSync(ws, { recursive: true });
    git(ws, 'init', '-q');
    git(ws, 'fetch', '-q', '--depth=1', `file://${origin}`, sha);
    git(ws, 'checkout', '-q', sha);
    return sha;
  };
  const stable = join(root, 'home', '.paddock-ci', 'src', 'decryp7_paddock-crate');
  // No automatic gc in the clone during the test: on the runner it hides the growth only until it runs inside a job.
  const run = (over = {}) => runStableWorkspace('macOS', root, { PATH: `${bin}:/usr/bin:/bin`, GIT_CONFIG_COUNT: '1', GIT_CONFIG_KEY_0: 'gc.auto', GIT_CONFIG_VALUE_0: '0', ...over });
  /** KiB of objects in a repository (loose and packed). */
  const kib = (repo) => {
    const v = Object.fromEntries(git(repo, 'count-objects', '-v').split('\n').map((l) => l.split(': ')));
    return Number(v.size) + Number(v['size-pack']);
  };
  return { root, ws, stable, bin, git, checkout, run, kib };
}

test('stable-workspace: the kept clone takes only what a new commit adds, so it does not grow by the whole tree each job', () => {
  const f = keptCloneFixture();
  f.checkout();
  assert.equal(f.run().status, 0);
  const tree = Number(f.git(f.stable, 'count-objects', '-v').match(/in-pack: (\d+)/)[1]);
  assert.ok(tree > 150, `the fixture tree has more objects than fetch.unpackLimit (${tree})`);
  let sha = '';
  for (let i = 0; i < 5; i += 1) {
    sha = f.checkout();
    // What the fetch from the work folder sends: pack-objects reports the objects it wrote (trace2). Without the clone's
    // objects in view the work folder (one commit deep) sends the whole tree every job: 366.8 -> 713.8 MiB on the real
    // repository with git 2.56 until a gc runs inside some job (git 2.54 deduplicates on receipt, but still sends it all).
    const trace = join(f.root, `trace-${i}.json`);
    const r = f.run({ GIT_TRACE2_EVENT: trace });
    assert.equal(r.status, 0, r.stderr + r.stdout);
    const wrote = [...readFileSync(trace, 'utf8').matchAll(/"key":"write_pack_file\/wrote","value":"(\d+)"/g)].map((m) => Number(m[1]));
    assert.ok(wrote.length > 0, 'the fetch was traced');
    assert.ok(Math.max(...wrote) < 20, `job ${i + 2}: the fetch sends what the new commit adds, not the tree of ${tree} objects (sent ${wrote})`);
  }
  assert.equal(f.git(f.stable, 'rev-parse', 'HEAD'), sha);
  assert.equal(readFileSync(join(f.stable, 'Cargo.toml'), 'utf8'), 'v6');
  assert.ok(!existsSync(join(f.ws, '.git', 'objects', 'info', 'alternates')), 'the checkout does not keep borrowing the clone\'s objects');
});

test('stable-workspace: ignored files of an earlier job (an embedded dist/, binaries) are gone from the kept clone', () => {
  const f = keptCloneFixture();
  f.checkout();
  assert.equal(f.run().status, 0);
  mkdirSync(join(f.stable, 'dist'));
  writeFileSync(join(f.stable, 'dist', 'index.html'), 'from another branch');
  f.checkout();
  assert.equal(f.run().status, 0);
  assert.ok(!existsSync(join(f.stable, 'dist')), 'generate_context! must never embed an earlier job\'s dist/');
});

test('stable-workspace: the checkout never keeps an alternate into the kept clones, even when the fetch is killed', () => {
  const f = keptCloneFixture();
  f.checkout();
  assert.equal(f.run().status, 0);
  f.checkout();
  // A stale line from an earlier, killed run (under the kept folders' root) and an unrelated one the step must keep.
  const alt = join(f.ws, '.git', 'objects', 'info', 'alternates');
  const other = join(f.root, 'other.git');
  f.git(f.root, 'init', '-q', '--bare', other);
  const stale = join(f.root, 'home', '.paddock-ci', 'src', 'gone', '.git', 'objects');
  writeFileSync(alt, `${stale}\n${join(other, 'objects')}\n`);
  assert.equal(f.run().status, 0);
  assert.equal(readFileSync(alt, 'utf8'), `${join(other, 'objects')}\n`, 'the stale line is gone, the unrelated one stays');
  // Every fetch into the clone is killed (TERM to the shell running it), on both attempts: the trap still takes the
  // borrowed alternate back out.
  writeFileSync(alt, '');
  f.checkout();
  const realGit = spawnSync('sh', ['-c', 'command -v git'], { encoding: 'utf8', env: { PATH: '/usr/bin:/bin' } }).stdout.trim();
  writeFileSync(join(f.bin, 'git'), `#!/bin/sh\ncase " $* " in *" fetch "*) kill -TERM $PPID; sleep 1; exit 1 ;; esac\nexec ${realGit} "$@"\n`, { mode: 0o755 });
  const killed = f.run();
  assert.equal(killed.status, 0, killed.stderr + killed.stdout);
  const left = existsSync(join(f.ws, '.git', 'objects', 'info', 'alternates')) ? readFileSync(join(f.ws, '.git', 'objects', 'info', 'alternates'), 'utf8') : '';
  assert.doesNotMatch(left, /\.paddock-ci/, 'no alternate into the kept clones is left behind');
});

test('stable-workspace: the kept clone holds no loose objects and at most 21 packs, however many small jobs it serves', () => {
  const f = keptCloneFixture();
  f.checkout();
  assert.equal(f.run().status, 0);
  for (let i = 0; i < 23; i += 1) {
    f.checkout();
    const r = f.run();
    assert.equal(r.status, 0, r.stderr + r.stdout);
    const v = Object.fromEntries(f.git(f.stable, 'count-objects', '-v').split('\n').map((l) => l.split(': ')));
    // A fetch below fetch.unpackLimit (100 objects) is written as loose objects, which no pack count bounds.
    assert.equal(Number(v.count), 0, `job ${i + 2}: ${v.count} loose objects`);
    assert.ok(Number(v.packs) <= 21, `job ${i + 2}: ${v.packs} packs`);
  }
});

test('stable-workspace: a lock left in the kept clone by a killed job does not break it for good: a fresh clone is made', () => {
  const f = keptCloneFixture();
  f.checkout();
  assert.equal(f.run().status, 0);
  writeFileSync(join(f.stable, '.git', 'index.lock'), '');
  const sha = f.checkout();
  const r = f.run();
  assert.equal(r.status, 0, r.stderr + r.stdout);
  assert.equal(f.git(f.stable, 'rev-parse', 'HEAD'), sha);
  assert.equal(readFileSync(join(f.stable, 'Cargo.toml'), 'utf8'), 'v2');
  assert.match(readFileSync(join(f.root, 'github-env'), 'utf8'), new RegExp(`^PADDOCK_WORKSPACE=${f.stable}$`, 'm'), 'later steps build from the stable folder');
});

test('stable-workspace: a checkout that moved aside but can neither replace the stable folder nor move back fails the job with what to do', () => {
  const root = stableFixture();
  const ws = join(root, 'work', 'abc', 'hostexecutor');
  const stable = join(root, 'home', '.paddock-ci', 'src', 'decryp7_paddock-crate');
  mkdirSync(stable, { recursive: true });
  writeFileSync(join(stable, 'Cargo.toml'), 'old');
  const bin = join(root, 'bin');
  mkdirSync(bin);
  // `rm -rf <stable>` fails (a file held open); the first `mv` (checkout aside) works, later ones fail.
  writeFileSync(join(bin, 'rm'), `#!/bin/sh\nfor a in "$@"; do [ "$a" = "${stable}" ] && { echo "Permission denied" >&2; exit 1; }; done\nexec /bin/rm "$@"\n`, { mode: 0o755 });
  writeFileSync(join(bin, 'mv'), `#!/bin/sh\nif [ ! -f "${root}/mv-once" ]; then : > "${root}/mv-once"; exec /bin/mv "$@"; fi\necho "Device or resource busy" >&2\nexit 1\n`, { mode: 0o755 });
  const r = runStableWorkspace('macOS', root, { PATH: `${bin}:/usr/bin:/bin` });
  assert.notEqual(r.status, 0, 'a job whose checkout is not where the runner looks must not go on');
  assert.match(r.stdout, /::error::stable-workspace: .*\.incoming/, 'it says where the checkout is');
  // When the move back works, the job goes on in its own folder.
  const again = stableFixture();
  const stable2 = join(again, 'home', '.paddock-ci', 'src', 'decryp7_paddock-crate');
  mkdirSync(stable2, { recursive: true });
  const bin2 = join(again, 'bin');
  mkdirSync(bin2);
  writeFileSync(join(bin2, 'rm'), `#!/bin/sh\nfor a in "$@"; do [ "$a" = "${stable2}" ] && exit 1; done\nexec /bin/rm "$@"\n`, { mode: 0o755 });
  const ok = runStableWorkspace('macOS', again, { PATH: `${bin2}:/usr/bin:/bin` });
  assert.equal(ok.status, 0, ok.stderr + ok.stdout);
  assert.equal(readFileSync(join(again, 'work', 'abc', 'hostexecutor', 'Cargo.toml'), 'utf8'), 'x', 'the checkout is back where the runner looks');
  assert.ok(!existsSync(`${stable2}.incoming`));
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
  const jobs = parseJobs(ci).filter((j) => j.text.includes('uses: ./.gitea/actions/stable-workspace'));
  assert.deepEqual(jobs.map((j) => j.id), ['crate', 'app', 'paddock-dev'], 'the crate job, the apps\' host legs and the developer tool');
  for (const job of jobs) {
    const at = job.text.indexOf('uses: ./.gitea/actions/stable-workspace');
    assert.match(job.text.slice(at, at + 160), /\n\s+if: runner\.os != 'Linux'/, `${job.id}: host runners only; on Windows \`shell: bash\` before Git bash is on PATH resolves to WSL`);
    const gitBash = job.text.indexOf('Git bash on PATH');
    assert.ok(gitBash > 0 && gitBash < at, `${job.id}: Git bash is on PATH before any later step needs bash`);
    assert.equal(job.text.split('uses: ./.gitea/actions/stable-workspace').length - 1, 1);
  }
});

test('the test browser of a host job lives in a kept folder of the runner user, outside the checkout that stable-workspace moves', () => {
  const ci = read(join(WORKFLOWS, 'ci.yml'));
  const dirs = [...ci.matchAll(/PADDOCK_TEST_BROWSER_DIR=(\S+)/g)].map((m) => m[1]);
  assert.equal(dirs.length, 3, 'one per host OS in the crate job, and the app leg\'s own (per app, under $HOME)');
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

test('paddock-dev has the same nextest ci profile as the crates, and its longer budgets are waivers with a reason', () => {
  const text = read(join(PADDOCK, 'tools', 'paddock-dev', '.config', 'nextest.toml'));
  const ci = text.slice(text.indexOf('[profile.ci]\n') + '[profile.ci]\n'.length);
  const own = ci.slice(0, ci.search(/^\[/m));
  assert.match(own, /^slow-timeout = \{ period = "\d+s", terminate-after = \d+ \}$/m);
  assert.match(own, /^retries = 0$/m, 'a flaky test is fixed, not retried');
  assert.match(text, /\[profile\.ci\.junit\]/);
  const lines = text.split('\n');
  for (const at of lines.flatMap((l, i) => (l.trim() === '[[profile.ci.overrides]]' ? [i] : []))) {
    let j = at - 1;
    while (j >= 0 && lines[j].startsWith('#') && !lines[j].startsWith('# waiver:')) j -= 1;
    assert.ok(j >= 0 && /^# waiver: \S/.test(lines[j]), `the override at line ${at + 1} has no "# waiver:" reason`);
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

// ---- The apps on the shared kit (ci.yml's app leg through paddock.ci.json, <app>-release.yml through app-release.yml) ----
import { parseJobs } from '../src/util/workflows.mjs';
const KIT_APPS = ['pitcrew', 'spotter', 'pitwall', 'dyno'];
const ciContract = (app) => JSON.parse(read(join(PADDOCK, 'apps', app, 'paddock.ci.json')));

/** Runs conformance/ci/build-folder.sh for `os` on a scratch Cargo workspace; returns what it wrote. */
function runBuildFolder(os, { include = true, name = 'spotter-rust' } = {}) {
  const root = realpathSync(mkdtempSync(join(tmpdir(), 'paddock-build-folder-')));
  const ws = join(root, 'ws');
  mkdirSync(join(ws, '.cargo'), { recursive: true });
  writeFileSync(join(ws, '.cargo', 'config.toml'), include ? 'include = [{ path = "ci.toml", optional = true }]\n' : '[alias]\n');
  writeFileSync(join(ws, '.cargo', 'ci.toml'), '# left by an earlier job\n');
  const r = spawnSync('bash', [join(PADDOCK, 'conformance', 'ci', 'build-folder.sh'), ws, name], {
    env: { PATH: '/usr/bin:/bin', HOME: join(root, 'home'), RUNNER_OS: os, GITHUB_ENV: join(root, 'github-env') },
    encoding: 'utf8',
  });
  const env = existsSync(join(root, 'github-env')) ? readFileSync(join(root, 'github-env'), 'utf8') : '';
  const ci = existsSync(join(ws, '.cargo', 'ci.toml')) ? readFileSync(join(ws, '.cargo', 'ci.toml'), 'utf8') : null;
  return { ...r, root, ws, env, ci };
}

test('build-folder keeps a host job\'s build folder outside the checkout as cargo configuration, and target/ in a container', () => {
  const mac = runBuildFolder('macOS');
  assert.equal(mac.status, 0, mac.stderr + mac.stdout);
  const kept = join(mac.root, 'home', '.paddock-ci', 'target-spotter-rust');
  assert.equal(mac.ci.split('\n').slice(1).join('\n'), `[build]\ntarget-dir = '${kept}'\n`, 'cargo reads the folder from the included ci.toml');
  assert.match(mac.env, new RegExp(`^PADDOCK_BUILD_FOLDER=${kept}$`, 'm'));
  assert.ok(existsSync(kept), 'the kept folder exists');
  const linux = runBuildFolder('Linux');
  assert.equal(linux.status, 0, linux.stderr + linux.stdout);
  assert.equal(linux.ci, null, 'no ci.toml in a container (an old one is removed)');
  assert.match(linux.env, new RegExp(`^PADDOCK_BUILD_FOLDER=${linux.ws}/target$`, 'm'));
  const missing = runBuildFolder('macOS', { include: false });
  assert.notEqual(missing.status, 0, 'a workspace whose config does not include ci.toml would silently build elsewhere');
  assert.match(missing.stdout, /must include the job's build folder/);
  const bad = runBuildFolder('macOS', { name: 'a b' });
  assert.notEqual(bad.status, 0);
});

test('every app workspace has a nextest ci profile that stops a slow test, and every longer budget is a waiver with a reason', () => {
  for (const app of KIT_APPS) {
    const file = join(PADDOCK, 'apps', app, '.config', 'nextest.toml');
    const text = read(file);
    const ci = text.slice(text.indexOf('[profile.ci]\n') + '[profile.ci]\n'.length);
    assert.match(ci.slice(0, ci.search(/^\[/m)), /^slow-timeout = \{ period = "30s", terminate-after = 2 \}$/m, `${app}: the 60-second budget`);
    assert.match(ci, /^retries = 0$/m, `${app}: no retries (rule 5)`);
    const lines = text.split('\n');
    for (const [at] of lines.map((l, i) => [i, l]).filter(([, l]) => l.trim() === '[[profile.ci.overrides]]')) {
      let j = at - 1;
      while (j >= 0 && lines[j].startsWith('#') && !lines[j].startsWith('# waiver:')) j -= 1;
      assert.ok(j >= 0 && /^# waiver: \S/.test(lines[j]), `${app}: the override at line ${at + 1} has no "# waiver:" reason`);
    }
  }
});

test('every e2e job of every kit app, ci.yml and debrief.yml runs on a port no other job uses', () => {
  const ports = [];
  for (const app of KIT_APPS) {
    const c = ciContract(app);
    const shards = c.e2eShards ?? 1;
    for (const line of c.e2e ?? []) {
      const range = /PORT=\$\(\((\d+) \+ E2E_INDEX\)\)/.exec(line);
      if (range) for (let i = 1; i <= shards; i += 1) ports.push([`${app} e2e shard ${i}`, Number(range[1]) + i]);
      else if (/PORT=\d+/.test(line)) ports.push([`${app} e2e`, Number(/PORT=(\d+)/.exec(line)[1])]);
      else if (/playwright test/.test(line)) assert.fail(`${app}: "${line}" names no port: another job could hold the default`);
    }
    for (const line of c.e2eWebkit ?? []) {
      const m = /PORT=(\d+)/.exec(line);
      if (m) ports.push([`${app} webkit`, Number(m[1])]);
      else if (/playwright test/.test(line)) assert.fail(`${app}: "${line}" names no port`);
    }
  }
  for (const file of ['ci.yml', 'debrief.yml']) {
    for (const m of read(join(WORKFLOWS, file)).matchAll(/^\s*\w*PORT: (.+)$/gm)) {
      for (const n of m[1].matchAll(/['"](\d{4})['"]/g)) ports.push([`${file} ${m[0].trim()}`, Number(n[1])]);
      for (const n of m[1].matchAll(/"(\d{3})\$\{\{ matrix\.shard \}\}"/g)) for (const sh of [1, 2]) ports.push([`${file} ${m[0].trim()} shard ${sh}`, Number(`${n[1]}${sh}`)]);
    }
  }
  assert.ok(ports.length >= 10, `found the ports (${ports.length})`);
  const seen = new Map();
  for (const [who, port] of ports) {
    assert.ok(!seen.has(port), `port ${port} is used by both "${seen.get(port)}" and "${who}"`);
    seen.set(port, who);
  }
});

test('app-gates tests last-packages apart on Linux only, and a host leg leaves them out of clippy, tests and doc tests', () => {
  const gates = read(join(ACTIONS, 'app-gates', 'action.yml'));
  assert.match(gates, /for pkg in \$LAST_PACKAGES; do exclude\+=\(--exclude "\$pkg"\); last\+=\(-p "\$pkg"\); done/, 'one list feeds the exclusions and the separate run');
  assert.match(gates, /step \$\{wrap\[@\]\+"\$\{wrap\[@\]\}"\} cargo nextest run --workspace \$\{exclude\[@\]\+"\$\{exclude\[@\]\}"\}/, 'the workspace run never includes them');
  assert.match(gates, /if \[ "\$RUNNER_OS" = Linux \] && \[ \$\{#last\[@\]\} -gt 0 \]; then step \$\{wrap\[@\]\+"\$\{wrap\[@\]\}"\} cargo nextest run \$\{last\[@\]\+"\$\{last\[@\]\}"\}/, 'Linux runs them after the rest');
  assert.match(gates, /step cargo clippy --workspace \$\{hostexclude\[@\]\+"\$\{hostexclude\[@\]\}"\}/, 'clippy: excluded on a host only');
  assert.match(gates, /step cargo test --workspace \$\{hostexclude\[@\]\+"\$\{hostexclude\[@\]\}"\} --doc/, 'doc tests: excluded on a host only');
  assert.match(read(join(WORKFLOWS, 'ci.yml')), /rust-checks: \$\{\{ matrix\.rust_checks \}\}/, 'ci.yml hands rustChecks to app-gates');
  assert.match(read(join(WORKFLOWS, 'ci.yml')), /last-packages: \$\{\{ matrix\.last_packages \}\}/, 'ci.yml hands the contract field to app-gates');
});

test('app-gates reports every failed check: the steps after a failed check still run', () => {
  const gates = read(join(ACTIONS, 'app-gates', 'action.yml'));
  for (const name of ['Rust checks', 'Rust (', 'UI (', 'End-to-end tests']) {
    const at = gates.indexOf(`- name: ${name}`);
    assert.ok(at > 0, name);
    assert.match(gates.slice(at, at + 300), /if: \$\{\{ !cancelled\(\) && steps\.prepare\.outcome != 'failure' && /, `${name} runs after an earlier check failed`);
  }
});

// The kit apps: what their old workflow asserted, now asserted on the contract and the thin release caller.
test('a kit app lists the OS legs its old workflow had, keeps its build folder out of the checkout, and releases through app-release.yml', () => {
  for (const app of KIT_APPS) {
    const contract = ciContract(app);
    assert.ok(!contract.workflow, `${app}: a leg of ci.yml, no workflow of its own`);
    assert.deepEqual(contract.os, ['ubuntu-latest', 'windows-latest', 'macos-latest'], `${app}: Rust on every OS`);
    if (contract.os.some((os) => os !== 'ubuntu-latest')) {
      const dir = join(PADDOCK, 'apps', app, contract.cargo);
      assert.match(read(join(dir, '.cargo', 'config.toml')), /^include = \[\{ path = "ci\.toml", optional = true \}\]$/m, `${app}: .cargo/config.toml includes the job's build folder`);
      assert.match(read(join(PADDOCK, 'apps', app, '.gitignore')), /^\.cargo\/ci\.toml$/m, `${app}: the job's ci.toml is never committed`);
    }
    assert.match(read(join(PADDOCK, 'apps', app, 'app', 'angular.json')), /"cache": \{ "enabled": true, "environment": "all" \}/, `${app}: Angular keeps its cache in CI`);
    assert.equal(existsSync(join(WORKFLOWS, `${app}.yml`)), false, `${app}: no copy of the gates beside ci.yml`);
    for (const copy of ['scripts/release', 'scripts/ci/macos-verify-bundle.sh', 'scripts/commit-lint.mjs', 'scripts/export-diagrams.sh', 'scripts/check-index.mjs']) {
      assert.equal(existsSync(join(PADDOCK, 'apps', app, copy)), false, `${app}: ${copy} is the kit's (rule 42/43)`);
    }
    const wf = read(join(WORKFLOWS, `${app}-release.yml`));
    // A release never publishes past skipped gates on a tag: the release job needs green gates; the run without gates has
    // its own installers job (see the event test below).
    assert.match(wf, /^  release:\n(?:    .*\n)*?    if: \$\{\{ needs\.plan\.outputs\.run == 'true' && needs\.gates\.result == 'success' \}\}$/m, `${app}: the release job needs green gates`);
    assert.match(wf, new RegExp(`gates:\\n(?:.*\\n)*?\\s+if: .*startsWith\\(github\\.ref, 'refs/tags/${app}-v'\\)`), `${app}: a tag always runs the gates`);
    assert.match(wf, /cancel-in-progress: true/, `${app}: a newer push supersedes (a tag has its own ref)`);
    // Every sidecar the bundle carries (bundle.externalBin) is started from the built macOS app.
    const tauriConf = JSON.parse(read(join(PADDOCK, 'apps', app, 'app', 'src-tauri', 'tauri.conf.json')));
    // One name:args per line (a block) or a single one.
    const block = /sidecars: \|\n((?:[ ]{8,}\S.*\n)+)/.exec(wf)?.[1] ?? /sidecars: (.+)/.exec(wf)?.[1] ?? '';
    const listed = block.split(/\s+/).filter(Boolean).map((x) => x.split(':')[0]);
    for (const bin of tauriConf.bundle.externalBin ?? []) assert.ok(listed.includes(bin.split('/').pop()), `${app}: ${bin} is started by the macOS verify (sidecars: ${listed})`);
    assert.match(wf, new RegExp(`tags: \\["${app}-v\\*"\\]`), `${app}: its own tag`);
    assert.match(wf, /uses: \.\/\.gitea\/workflows\/app-release\.yml\n\s+with:\n\s+paddock-ref: \$\{\{ github\.sha \}\}/, `${app}: the shared release, from this commit`);
    assert.match(wf, new RegExp(`tag-prefix: ${app}-\\n\\s+latest-tag: ${app}-latest`), `${app}: its tags and the moving release the updater reads`);
    for (const k of ['SIGNING_PRIVATE_KEY', 'SIGNING_PRIVATE_KEY_PASSWORD', 'UPDATER_PUBKEY']) {
      assert.match(wf, new RegExp(`TAURI_${k}: \\$\\{\\{ secrets\\.PADDOCK_TAURI_${k} \\}\\}`), `${app}: the family's ${k} reaches the kit under its Tauri name`);
    }
    assert.doesNotMatch(wf, /TRANSITION_SIGNING_KEY|OLD_REPO|--newest/, `${app}: one key for the family, no bridge release in the old repo`);
    assert.match(wf, /Existing installs/, `${app}: the First launch notes say to reinstall once`);
    const conf = JSON.parse(read(join(PADDOCK, 'apps', app, 'app', 'src-tauri', 'tauri.conf.json')));
    assert.deepEqual(conf.plugins.updater.endpoints, [`https://dev.decryptology.net/decryp7/paddock/releases/download/${app}-latest/latest.json`], `${app}: the updater reads only Paddock's ${app}-latest`);
  }
});

test("Spotter's real-browser tests use the image's Chrome for Testing on Linux: Spotter's pin is the image's", () => {
  const version = (text) => /^version = "([^"]+)"$/m.exec(text)?.[1];
  const spotter = version(read(join(PADDOCK, 'apps', 'spotter', 'crates', 'spotter-render', 'src', 'chromium-manifest.toml')));
  assert.equal(spotter, version(read(join(PADDOCK, 'crates', 'paddock-chromium', 'src', 'chromium.toml'))), 'the image installs paddock-chromium\'s pin');
  assert.match(read(join(PADDOCK, 'ci-image', 'Dockerfile')), new RegExp(`ARG CHROME_VERSION=${spotter.replace(/\./g, '\\.')}\\n`), 'the image carries that build');
  const contract = ciContract('spotter');
  assert.deepEqual(contract.testBrowser, { envPrefix: 'SPOTTER', catalogue: 'apps/spotter/crates/spotter-render/src/chromium-manifest.toml', noSandboxOnLinux: true }, 'the leg installs Spotter\'s pin on the hosts and points SPOTTER_TEST_BROWSER at it');
  assert.ok(existsSync(join(PADDOCK, contract.testBrowser.catalogue)));
  assert.equal(contract.display, true, 'the renderer\'s window is a real off-screen one: a virtual display on Linux');
  const ci = read(join(WORKFLOWS, 'ci.yml'));
  assert.match(ci, /echo "\$\{PREFIX\}_TEST_BROWSER=\$CHROMIUM_TEST_PATH"/, 'the tests launch the pinned build');
  assert.match(ci, /echo "\$\{PREFIX\}_REQUIRE_TEST_BROWSER=1"/, 'a missing browser fails the run');
  assert.match(ci, /test-browser-\$APP/, 'a kept host folder of the app\'s own (two jobs may run on a host at once)');
  assert.match(read(join(ACTIONS, 'app-gates', 'action.yml')), /xvfb-run --auto-servernum .* dbus-run-session --\)/, 'the tests run under a virtual display');
  assert.match(contract.processPatterns.join('\n'), /test-browser-spotter/, 'host cleanup ends browsers started from the app\'s own test-browser folder');
  assert.match(read(join(PADDOCK, 'apps', 'spotter', '.config', 'nextest.toml')), /filter = "package\(spotter-render\) & binary\(real_browser\)"\ntest-group = "real-browser"/, 'real browsers take turns under nextest too');
});

test("a kit app's end-to-end legs and failure report: ci.yml forwards the shard, the webkit input and uploads the report from the stable workspace", () => {
  const ci = read(join(WORKFLOWS, 'ci.yml'));
  assert.match(ci, /e2e-checks: \$\{\{ matrix\.e2e_checks \}\}\n\s+e2e-shard: \$\{\{ matrix\.e2e_shard \}\}/);
  const step = /- uses: actions\/upload-artifact@\S+ # v3\n\s+if: failure\(\) && matrix\.e2e_artifacts != ''\n[\s\S]*?\n\n/.exec(ci)?.[0] ?? '';
  assert.match(step, /path: \$\{\{ env\.PADDOCK_WORKSPACE \|\| github\.workspace \}\}\/\$\{\{ matrix\.e2e_artifacts \}\}/, 'under the stable workspace');
  assert.match(step, /name: \$\{\{ matrix\.app \}\}-\$\{\{ matrix\.e2e_leg \|\| 'ui' \}\}-results-\$\{\{ strategy\.job-index \}\}/, 'one name per leg');
  assert.match(read(join(ACTIONS, 'app-gates', 'action.yml')), /E2E_INDEX="\$\{E2E_SHARD%%\/\*\}"/);
  assert.equal(ciContract('spotter').e2eArtifacts, 'apps/spotter/app/.angular/playwright-results', "the folder the old spotter.yml uploaded");
});

test('a manual ci.yml run can ask for WebKit (workflow_dispatch input), compared as a boolean or a string, and the release dispatch asks for it', () => {
  const ci = read(join(WORKFLOWS, 'ci.yml'));
  const dispatch = /^  workflow_dispatch:\n((?:    .*\n|\n)+)/m.exec(ci)?.[1] ?? '';
  assert.match(dispatch, /^      webkit:\n(?:        .*\n)*?        type: boolean\n        default: false/m, 'workflow_dispatch has a webkit input');
  assert.match(ci, /PLAN_WEBKIT: \$\{\{ \(inputs\.webkit == true \|\| inputs\.webkit == 'true'\) && '1' \|\| '' \}\}/, 'the plan job turns the input into the WebKit leg');
  assert.match(ci, /name: app-\$\{\{ matrix\.app \}\}\$\{\{ matrix\.e2e_leg != '' && format\('-\{0\}', matrix\.e2e_leg\) \|\| '' \}\}/, 'parallel legs of one app never share an Angular cache');
  const gates = /^  gates:\n(?:    .*\n|\n)+/m.exec(read(join(WORKFLOWS, 'spotter-release.yml')))?.[0] ?? '';
  assert.match(gates, /webkit: \$\{\{ github\.event_name == 'workflow_dispatch' \}\}/, "the pre-tag dispatch covers WebKit like the old bundles run");
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

// Debrief's workflow (.gitea/workflows/debrief.yml) on the same speed rules as ci.yml (Paddock lessons, "Test and build speed").
const debriefYml = () => read(join(WORKFLOWS, 'debrief.yml'));
/** A job's block (its lines indented under `  <name>:`) in a workflow. */
const jobBlock = (text, name) => text.match(new RegExp(`^  ${name}:\\n((?:    .*\\n|\\n)+)`, 'm'))?.[1] ?? '';
/** The non-comment lines of a workflow (a comment may name what is not done). */
const code = (text) => text.split('\n').filter((l) => !/^\s*#/.test(l)).join('\n');

test('Debrief builds Rust through sccache in every Rust job, with the build folder as cargo configuration, never CARGO_TARGET_DIR', () => {
  const yml = debriefYml();
  const rustJobs = ['rust-lint', 'rust-test', 'rust-test-macos', 'plugin-sdk', 'tauri-smoke', 'bundle'];
  for (const job of rustJobs) {
    const block = jobBlock(yml, job);
    assert.notEqual(block, '', `the ${job} job exists`);
    assert.match(block, /uses: \.\/\.gitea\/actions\/setup-rust\n(?!\s+if:)/, `${job}: setup-rust (sccache) on every OS`);
    assert.match(block, /uses: \.\/\.gitea\/actions\/sccache-stats\n\s+if: always\(\)/, `${job}: sccache-stats last`);
  }
  const body = code(yml);
  assert.doesNotMatch(body, /CARGO_(BUILD_)?TARGET_DIR/, 'sccache hashes every CARGO_* variable: no build folder variable');
  assert.doesNotMatch(body, /RUSTC_WRAPPER/, 'setup-rust sets sccache itself as the wrapper; never a .cmd script (CI 6365)');
  assert.doesNotMatch(body, /dtolnay\/rust-toolchain|decryp7\/garage:|actions\/setup-node|playwright install/, 'the shared actions and the paddock-ci image');
  // The kept build folder of the host legs is cargo configuration that cargo itself reads (.cargo/ci.toml, included).
  assert.match(read(join(PADDOCK, 'apps', 'debrief', '.cargo', 'config.toml')), /include = \[[^\]]*\{ path = "ci\.toml", optional = true \}/);
  for (const job of ['rust-test-macos', 'tauri-smoke', 'bundle']) assert.match(jobBlock(yml, job), /uses: \.\/\.gitea\/actions\/build-folder\n/, `${job}: the build folder (the shared action)`);
  assert.match(jobBlock(yml, 'tauri-smoke'), /DEBRIEF_E2E_APP=\$PADDOCK_BUILD_FOLDER\/debug\/debrief-app\$ext/, 'the smoke starts the app from the job\'s build folder');
});

test('Debrief\'s host legs build from a stable checkout path, after Git bash is on PATH on Windows', () => {
  const yml = debriefYml();
  for (const job of ['rust-test-macos', 'tauri-smoke', 'bundle']) {
    const block = jobBlock(yml, job);
    const at = block.indexOf('uses: ./.gitea/actions/stable-workspace');
    assert.ok(at > 0, `${job} uses stable-workspace`);
    const gitBash = block.indexOf('Git bash on PATH');
    if (gitBash >= 0) assert.ok(gitBash < at, `${job}: Git bash is on PATH before stable-workspace`);
    assert.ok(at < block.indexOf('uses: ./.gitea/actions/setup-rust'), `${job}: the stable path before the first build`);
    // Every cargo step builds from the stable path (sccache keys a compile on its working folder).
    for (const step of block.split(/\n      - /).filter((st) => /\n\s+run: .*\bcargo (nextest run|test|build|clippy)\b/.test(`\n${st}`) || /^\s*run: .*\bnpm run e2e:tauri:build|npx tauri build/.test(st))) {
      assert.match(step, /working-directory: \$\{\{ env\.PADDOCK_WORKSPACE \|\| github\.workspace \}\}/, `${job}: ${step.split('\n')[0]}`);
    }
  }
});

test('Debrief runs its installers and real-app smoke tests only on debrief-v* tags, the nightly run and bundles: true', () => {
  const yml = debriefYml();
  const on = yml.match(/^on:\n((?:[ \t]+.*\n|\n)+)/m)?.[1] ?? '';
  assert.match(on, /workflow_dispatch:\n\s+inputs:\n\s+bundles:\n(?:\s+.*\n)*?\s+type: boolean\n\s+default: false/, 'a manual run asks for them');
  const full = jobBlock(yml, 'plan').match(/^      full: \$\{\{ (.*) \}\}$/m)?.[1] ?? '';
  assert.notEqual(full, '', 'the plan job says whether this is a full run');
  assert.match(full, /github\.event_name == 'schedule'/);
  assert.match(full, /startsWith\(github\.ref, 'refs\/tags\/debrief-v'\)/);
  assert.doesNotMatch(full, /refs\/heads\/main/, 'a push to main runs the checks only (main lands in batches, 2026-10-29)');
  assert.match(full, /github\.event_name == 'workflow_dispatch' && \(inputs\.bundles == true/);
  assert.doesNotMatch(full, /pull_request/);
  for (const job of ['tauri-smoke', 'bundle']) {
    assert.match(jobBlock(yml, job), /^    if: needs\.plan\.outputs\.run == 'true' && needs\.plan\.outputs\.full == 'true'$/m, `${job}: full runs only`);
  }
  // The release needs them, and a tag run is always a full run.
  assert.match(jobBlock(yml, 'release'), /needs: \[[^\]]*\bbundle\b[^\]]*\btauri-smoke\b/);
});

test('Debrief\'s end-to-end tests run sharded on the image\'s browsers with kept caches, WebKit only on full runs', () => {
  const block = jobBlock(debriefYml(), 'e2e');
  const shards = block.match(/shard: \[([^\]]*)\]/)?.[1].split(',').map((x) => Number(x.trim())) ?? [];
  assert.ok(shards.length >= 2, 'sharded');
  assert.deepEqual(shards, shards.map((_, i) => i + 1));
  assert.match(block, new RegExp(`--shard=\\$\\{\\{ matrix\\.shard \\}\\}/${shards.length} `), 'every shard runs its part');
  assert.match(block, /uses: \.\/\.gitea\/actions\/node-caches\n\s+with:\n\s+angular-dir: apps\/debrief\/app\n\s+name: debrief-e2e-\$\{\{ matrix\.shard \}\}/, 'one Angular cache per shard');
  assert.match(block, /DEBRIEF_E2E_PORT: "435\$\{\{ matrix\.shard \}\}"/, 'one port per shard');
  assert.match(block, /needs\.plan\.outputs\.full != 'true' && '--project=chromium'/, 'branch runs test in Chromium only (rule 3)');
  // The W0 shell baseline is a manual, macOS-only baseline: never in the default projects (CI 6381 failed every screenshot).
  const pw = read(join(PADDOCK, 'apps', 'debrief', 'app', 'playwright.config.ts'));
  assert.match(pw, /name: 'chromium', testIgnore: SHELL_BASELINE/);
  assert.match(pw, /name: 'webkit', testIgnore: \[PROD_CSP, SHELL_BASELINE\]/);
  assert.match(pw, /\.\.\.\(BASELINE\n?\s*\?/, 'its projects exist only with DEBRIEF_SHELL_BASELINE');
});

test('Debrief\'s nextest ci profile stops a hung test, and every longer budget is a waiver with a reason', () => {
  const text = read(join(PADDOCK, 'apps', 'debrief', '.config', 'nextest.toml'));
  const ci = text.slice(text.indexOf('[profile.ci]\n') + '[profile.ci]\n'.length);
  assert.match(ci.slice(0, ci.search(/^\[/m)), /^slow-timeout = \{ period = "30s", terminate-after = [23] \}$/m, 'at most 90 s per test');
  const lines = text.split('\n');
  for (const [at] of lines.entries()) {
    if (lines[at].trim() !== '[[profile.ci.overrides]]') continue;
    const body = lines.slice(at + 1, lines.findIndex((l, k) => k > at && l.startsWith('['))).join('\n');
    if (!/^slow-timeout/m.test(body)) continue;
    let j = at - 1;
    while (j >= 0 && lines[j].startsWith('#') && !lines[j].startsWith('# waiver:')) j -= 1;
    assert.ok(j >= 0 && /^# waiver: \S/.test(lines[j]), `the time budget override at line ${at + 1} has no "# waiver:" reason`);
    assert.match(body, /^filter = "/m, `the override at line ${at + 1} names its tests`);
  }
});


test('ci.yml runs the icon tests and `install.mjs --check` in an icons job that the plan feeds', () => {
  const ci = read(join(WORKFLOWS, 'ci.yml'));
  assert.match(ci, /icons: \$\{\{ steps\.plan\.outputs\.icons \}\}/, 'the plan job exports the icons output');
  const at = ci.indexOf('\n  icons:\n');
  assert.ok(at > 0, 'ci.yml has an icons job');
  const job = ci.slice(at + 1, ci.indexOf('\n  # ', at + 1) > 0 ? ci.indexOf('\n  # ', at + 1) : undefined);
  assert.match(job, /needs\.plan\.outputs\.icons == 'true'/, 'the icons job runs when the plan says icons');
  assert.match(job, /run: npm test/, 'it runs the icon tests');
  assert.match(job, /run: node icons\/tools\/install\.mjs --check/, 'it runs install.mjs --check');
});

// cmd reads `echo CARGO_BUILD_JOBS=8>>"%GITHUB_ENV%"` as a redirect of handle 8, so the variable was never set (the logs showed
// `CARGO_BUILD_JOBS=`). A digit directly before `>>` must be written `(echo NAME=8)>>file`.
test('no cmd echo puts a digit directly before a redirect in a workflow or an action', () => {
  const files = [
    ...readdirSync(WORKFLOWS).filter((f) => /\.ya?ml$/.test(f)).map((f) => join(WORKFLOWS, f)),
    ...readdirSync(ACTIONS, { withFileTypes: true })
      .filter((d) => d.isDirectory() && existsSync(join(ACTIONS, d.name, 'action.yml')))
      .map((d) => join(ACTIONS, d.name, 'action.yml')),
  ];
  const bad = [];
  for (const f of files) {
    read(f).split('\n').forEach((line, i) => {
      if (/^\s*echo\s[^()\n]*\d>>?/.test(line)) bad.push(`${f.slice(PADDOCK.length)}:${i + 1}: ${line.trim()}`);
    });
  }
  assert.deepEqual(bad, [], 'write (echo NAME=8)>>"%GITHUB_ENV%" so cmd does not read 8>> as a handle redirect');
});

test("app-release runs an app's own bundle check on Linux and macOS, after the installers and before the seal check, with the bundle folder", () => {
  const wf = read(join(WORKFLOWS, 'app-release.yml'));
  const at = wf.indexOf("- name: The app's own bundle check");
  assert.ok(at > wf.indexOf('actions/tauri-bundle') && at < wf.indexOf('actions/macos-verify'), 'between the build and the seal check');
  const step = wf.slice(at, wf.indexOf('\n\n', at));
  assert.match(step, /if: inputs\.bundle-check != '' && runner\.os != 'Windows'/, 'Windows has no run paths to check');
  assert.match(step, /run: bash "\$CHECK" "\$PADDOCK_BUILD_FOLDER\/dist\/bundle"/, 'the folder the installers were built in');
  const dyno = read(join(WORKFLOWS, 'dyno-release.yml'));
  const script = /bundle-check: (\S+)/.exec(dyno)?.[1];
  assert.ok(script && existsSync(join(PADDOCK, script)), "Dyno's check script exists");
});

test('every host job that keeps a build folder caps it at the end, with the shared prune action, in its own folder only', () => {
  const ci = read(join(WORKFLOWS, 'ci.yml'));
  const rel = read(join(WORKFLOWS, 'app-release.yml'));
  assert.match(ci, /- uses: \.\/\.gitea\/actions\/prune-build-folder\n\s+if: always\(\) && runner\.os != 'Linux'\n\n/, "ci.yml's app job prunes on the hosts, even after a failure");
  assert.ok(ci.indexOf('actions/prune-build-folder') > ci.indexOf('actions/host-cleanup'), 'after the clean-up of the job');
  assert.match(rel, /- uses: \.\/\.paddock\/\.gitea\/actions\/prune-build-folder\n\s+if: always\(\) && runner\.os != 'Linux'\n\s+with:\n\s+drop-bundles: "true"/, "app-release's bundle job prunes on the hosts after the upload");
  assert.ok(rel.indexOf('actions/prune-build-folder') > rel.indexOf('actions/upload-artifact'), 'only after the installers are uploaded');
  const action = read(join(ACTIONS, 'prune-build-folder', 'action.yml'));
  assert.match(action, /dir="\$\{PADDOCK_BUILD_FOLDER:-\}"/, 'only the job\'s own build folder');
  assert.match(action, /prune-ci-build-folder\.sh" "\$dir" "\$MAX_GB"/, 'through the guard that accepts only target-* in the CI home');
  assert.match(action, /default: "20"/, 'the cap the old Dyno workflow had');
});

// host-cleanup removes $TMPDIR/<prefix>* on a user's Mac: a kit app's prefix must name only what its tests create.
function runtimeTempPrefixes(app) {
  const found = [];
  const walk = (dir) => {
    for (const e of readdirSync(dir, { withFileTypes: true })) {
      if (e.name === 'target' || e.name === 'node_modules' || e.name === 'tests') continue;
      const f = join(dir, e.name);
      if (e.isDirectory()) walk(f);
      else if (e.name.endsWith('.rs') && !/(^|_)tests?\.rs$/.test(e.name)) {
        let text = read(f);
        const at = text.search(/#\[cfg\(test\)\]|#\[cfg\(all\(test[^\]]*\]|^mod tests \{/m);
        if (at >= 0) text = text.slice(0, at); // test modules close the file
        for (const m of text.matchAll(/temp_dir\(\)[\s\S]{0,40}?\.join\(\s*(?:format!\()?"([A-Za-z][\w-]*)/g)) found.push([relative(PADDOCK, f), m[1]]);
        for (const m of text.matchAll(/\.prefix\(\s*(?:&format!\()?"([\w-]+)/g)) found.push([relative(PADDOCK, f), m[1]]);
        for (const m of text.matchAll(/\.join\(format!\("([A-Za-z][\w]*-[\w-]*)/g)) found.push([relative(PADDOCK, f), m[1]]);
      }
    }
  };
  walk(join(PADDOCK, 'apps', app));
  return found;
}

test("a kit app's host-cleanup temp sweep is <app>-it- and is a prefix of no temp folder its product creates at run time", () => {
  assert.match(read(join(WORKFLOWS, 'ci.yml')), /uses: \.\/\.gitea\/actions\/host-cleanup\n(?:.*\n)*?\s+temp-prefix: \$\{\{ matrix\.app \}\}-it-\n/, 'ci.yml sweeps <app>-it-');
  assert.match(read(join(WORKFLOWS, 'app-release.yml')), /actions\/host-cleanup\n(?:.*\n)*?\s+temp-prefix: \$\{\{ inputs\.app-id \}\}-it-\n/, 'app-release.yml sweeps <app>-it-');
  const dyno = runtimeTempPrefixes('dyno').map(([, p]) => p);
  assert.ok(dyno.includes('dyno-bench-') && dyno.some((p) => p === 'dyno-'), `the scan finds Dyno's own scratch folders (${dyno})`);
  for (const app of KIT_APPS) {
    const sweep = `${app}-it-`;
    for (const [file, prefix] of runtimeTempPrefixes(app)) {
      assert.ok(!prefix.startsWith(sweep), `${file}: the product creates ${prefix}*, which the sweep ${sweep}* would delete on a user's machine`);
    }
  }
});

test("app-gates runs every command list through run-commands.sh, which names the failing command and its exit code, stops there and keeps a cd", () => {
  const gates = read(join(ACTIONS, 'app-gates', 'action.yml'));
  assert.doesNotMatch(gates, /bash -euo pipefail -c "\$COMMANDS"/, 'no anonymous bash -c over a command list');
  assert.equal((gates.match(/conformance\/ci\/run-commands\.sh/g) ?? []).length, 5, 'prepare, checks, rust-checks, ui and e2e');
  const dir = realpathSync(mkdtempSync(join(tmpdir(), 'paddock-run-commands-')));
  mkdirSync(join(dir, 'sub'));
  const run = (commands, env = {}) => spawnSync('bash', [join(PADDOCK, 'conformance', 'ci', 'run-commands.sh')], { cwd: dir, env: { PATH: '/usr/bin:/bin', COMMANDS: commands, ...env }, encoding: 'utf8' });
  const bad = run('cd sub\npwd > here\nprintf x\nexit 7\ntouch never');
  assert.equal(bad.status, 7, 'the failing command\'s exit code');
  assert.match(bad.stdout, /::error::command failed \(exit 7\): exit 7 \(in .*sub\)/, 'it names itself');
  assert.equal(existsSync(join(dir, 'sub', 'here')), true, 'a cd holds for the lines after it');
  assert.equal(existsSync(join(dir, 'sub', 'never')), false, 'the lines after a failure do not run');
  const good = run('echo one\nE2E_INDEX=2 bash -c \'echo $((4359 + E2E_INDEX))\'\n\n');
  assert.equal(good.status, 0, good.stdout);
  assert.match(good.stdout, /4361/, 'shell expansion works as in the old bash -c');
  assert.equal(run('cd nowhere').status, 1);
});

// ---- Which jobs run for an event (2026-10-29, docs/research/2026-10-29-ci-runs.md) ----

/** A workflow expression (`${{ ... }}` or bare) evaluated with `github`, `needs` and `inputs`; enough for the job conditions here. */
function evalExpr(expr, { github, needs = {}, inputs = {} }) {
  const js = expr
    .trim()
    .replace(/^\$\{\{\s*/, '')
    .replace(/\s*\}\}$/, '')
    .replace(/([^!=<>])==(?!=)/g, '$1===')
    .replace(/!=(?!=)/g, '!==');
  const fn = new Function('github', 'needs', 'inputs', 'startsWith', 'cancelled', 'always', 'format', `return (${js});`);
  return fn(github, needs, inputs, (a, b) => String(a ?? '').startsWith(b), () => false, () => true, (f, ...a) => f.replace(/\{(\d+)\}/g, (m, i) => a[i]));
}

/**
 * The result of every job of a workflow for one event, as this Gitea decides it: a job whose `needs` include a job that did
 * not succeed is skipped, whatever its `if` says (observed 2026-10-29: in 104 push and 6 nightly runs of every
 * <app>-release.yml the installers were skipped after skipped gates); otherwise its job-level `if` decides. `outputs` are
 * the plan job's outputs; `fail` names jobs that run but fail.
 */
function jobResults(text, { event, ref, inputs = {}, outputs = {}, fail = [] }) {
  const github = { event_name: event, ref, event: { inputs } };
  const result = new Map();
  for (const job of parseJobs(text)) {
    const needs = Object.fromEntries(job.needs.map((n) => [n, { result: result.get(n), outputs: n === 'plan' ? outputs : {} }]));
    const cond = /^ {4}if: (.+)$/m.exec(job.text)?.[1];
    const runs = job.needs.every((n) => result.get(n) === 'success') && (cond === undefined || evalExpr(cond, { github, needs, inputs }));
    result.set(job.id, !runs ? 'skipped' : fail.includes(job.id) ? 'failure' : 'success');
  }
  return result;
}

test("debrief.yml: installers, real-app smoke and WebKit only on tags, the nightly and a manual full run; a push to main gets the gates", () => {
  const wf = read(join(WORKFLOWS, 'debrief.yml'));
  const full = /^ {6}full: (.+)$/m.exec(wf)?.[1];
  assert.ok(full, 'the plan job has a full output');
  const isFull = (github, inputs = {}) => evalExpr(full, { github: { ...github, event: { inputs } }, inputs });
  assert.equal(isFull({ event_name: 'push', ref: 'refs/heads/main' }), false, 'a push to main runs the gates only');
  assert.equal(isFull({ event_name: 'pull_request', ref: 'refs/pull/1/head' }), false);
  assert.equal(isFull({ event_name: 'workflow_dispatch', ref: 'refs/heads/x' }, { bundles: 'false' }), false);
  assert.equal(isFull({ event_name: 'push', ref: 'refs/tags/debrief-v1.2.0' }), true, 'a tag');
  assert.equal(isFull({ event_name: 'schedule', ref: 'refs/heads/main' }), true, 'the nightly');
  assert.equal(isFull({ event_name: 'workflow_dispatch', ref: 'refs/heads/x' }, { bundles: 'true' }), true, 'a manual full run');
  // The full-only jobs and the WebKit project hang on that output.
  for (const job of ['tauri-smoke', 'bundle']) {
    assert.match(parseJobs(wf).find((j) => j.id === job).text, /^ {4}if: needs\.plan\.outputs\.run == 'true' && needs\.plan\.outputs\.full == 'true'$/m, `${job}: full runs only`);
  }
  assert.match(wf, /--shard=\$\{\{ matrix\.shard \}\}\/4 \$\{\{ needs\.plan\.outputs\.full != 'true' && '--project=chromium' \|\| '' \}\}/, 'WebKit only on full runs');
});

test("every kit app's nightly and manual runs build the installers when the plan marks the app; a tag builds them only past green gates", () => {
  for (const app of KIT_APPS) {
    const wf = read(join(WORKFLOWS, `${app}-release.yml`));
    const jobs = parseJobs(wf);
    const callers = jobs.filter((j) => /uses: \.\/\.gitea\/workflows\/app-release\.yml/.test(j.text)).map((j) => j.id);
    const installers = (ev, extra = {}) => {
      const r = jobResults(wf, { outputs: { run: 'true' }, ...ev, ...extra });
      return callers.filter((c) => r.get(c) === 'success');
    };
    const needsGates = (id) => jobs.find((j) => j.id === id).needs.includes('gates');
    const nightly = { event: 'schedule', ref: 'refs/heads/main' };
    const manual = { event: 'workflow_dispatch', ref: 'refs/heads/main', inputs: { gates: 'false' } };
    const manualGates = { event: 'workflow_dispatch', ref: 'refs/heads/main', inputs: { gates: 'true' } };
    const tag = { event: 'push', ref: `refs/tags/${app}-v1.2.3` };
    assert.equal(installers(nightly).length, 1, `${app}: the nightly builds the installers once (it skipped them after the skipped gates)`);
    assert.equal(installers(manual).length, 1, `${app}: a manual run builds the installers once`);
    for (const [what, ev] of [['a tag', tag], ['a manual run with gates', manualGates]]) {
      const run = installers(ev);
      assert.equal(run.length, 1, `${app}: ${what} builds the installers once`);
      assert.ok(needsGates(run[0]), `${app}: ${what} builds them only after the gates`);
      assert.deepEqual(installers(ev, { fail: ['gates'] }), [], `${app}: red gates on ${what} build nothing`);
    }
    for (const ev of [nightly, manual, tag]) assert.deepEqual(installers(ev, { outputs: { run: 'false' } }), [], `${app}: nothing when the plan does not mark ${app}`);
    // The two callers build the same installers: one text from `uses:` to the end of the job.
    const call = (id) => jobs.find((j) => j.id === id).text.replace(/^[\s\S]*?(    uses: )/, '$1').replace(/(\n\s*(#.*)?)+$/, '');
    assert.equal(new Set(callers.map(call)).size, 1, `${app}: every app-release caller passes the same inputs and secrets`);
    // Main lands in batches; a push to main gets ci.yml only, not a release workflow (2026-10-29).
    const push = /^ {2}push:\n((?: {4}.*\n)+)/m.exec(wf)?.[1] ?? '';
    assert.doesNotMatch(push, /branches:/, `${app}: no push-to-main trigger`);
    assert.match(push, new RegExp(`tags: \\["${app}-v\\*"\\]`), `${app}: its tag still triggers`);
  }
});

// Whether a skipped release job came from a skipped need or from an empty plan output (run 6864: the plan logged
// "pitwall: affected (rust, ui)", yet installers were skipped): the output must reach the job, end to end.
test("the release workflows' plan output reaches their jobs: the job maps the plan action's `run`, which the decide step writes", () => {
  const action = read(join(ACTIONS, 'plan', 'action.yml'));
  assert.match(action, /^outputs:\n  run:\n(?:    .*\n)*?    value: \$\{\{ steps\.decide\.outputs\.run \}\}$/m, 'the action exposes the decide step\'s run');
  for (const app of KIT_APPS) {
    const plan = parseJobs(read(join(WORKFLOWS, `${app}-release.yml`))).find((j) => j.id === 'plan').text;
    assert.match(plan, /^    outputs:\n      run: \$\{\{ steps\.plan\.outputs\.run \}\}$/m, `${app}: the job output is the step's`);
    assert.match(plan, /- id: plan\n\s+uses: \.\/\.gitea\/actions\/plan\n\s+with:\n\s+app: (\S+)/, `${app}: the step with id plan is the plan action`);
    assert.equal(/- id: plan\n\s+uses: \.\/\.gitea\/actions\/plan\n\s+with:\n\s+app: (\S+)/.exec(plan)[1], app);
  }
  // The decide step itself, as the nightly runs it for Pitwall: run=true reaches $GITHUB_OUTPUT (and nothing else is
  // written there by the scripts it calls with their own output file).
  const step = action.slice(action.indexOf('- id: decide'));
  const body = step.slice(step.indexOf('run: |\n') + 'run: |\n'.length).split('\n').map((l) => l.slice(8)).join('\n').replaceAll('${{ github.action_path }}', join(ACTIONS, 'plan'));
  const root = realpathSync(mkdtempSync(join(tmpdir(), 'paddock-plan-decide-')));
  const out = join(root, 'output');
  writeFileSync(out, '');
  const r = spawnSync('bash', ['-e', '-o', 'pipefail', '-c', body], {
    cwd: PADDOCK,
    encoding: 'utf8',
    env: { ...process.env, EVENT: 'schedule', REF: 'refs/heads/main', SHA: 'x', MAIN_REF: 'refs/heads/main', GREEN: '', APP: 'pitwall', TAG_PATTERN: 'pitwall-v*', PLAN_TOKEN: '', GITHUB_EVENT_NAME: 'schedule', GITHUB_REF: 'refs/heads/main', GITHUB_OUTPUT: out, GITHUB_STEP_SUMMARY: '' },
  });
  assert.equal(r.status, 0, r.stderr);
  assert.equal(readFileSync(out, 'utf8'), 'run=true\n', 'exactly the output the job maps');
});
