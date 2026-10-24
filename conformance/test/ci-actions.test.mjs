// The composite actions and reusable workflows are YAML the runner only reads on CI, so their wiring is checked here: every
// run step names its shell, every script and local action they reference exists, every input they use or pass is declared.
// (The scripts themselves are tested in ci-scripts.test.mjs, tools/test and the release tests.)
import assert from 'node:assert/strict';
import { existsSync, readdirSync, readFileSync } from 'node:fs';
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
  assert.deepEqual(workflows, ['app-e2e.yml', 'app-licences.yml', 'app-release.yml']);
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

test('setup-rust wires the shared sccache backend: inputs from Gitea variables, Redis on every OS, a cache that never fails a build', () => {
  const text = read(join(ACTIONS, 'setup-rust', 'action.yml'));
  const declared = declaredInputs(text);
  for (const i of ['sccache-backend', 'sccache-redis-endpoint', 'sccache-redis-db']) assert.ok(declared.includes(i), `${i} is declared`);
  assert.match(text, /default: \$\{\{ vars\.SCCACHE_REDIS_ENDPOINT \}\}/);
  assert.match(text, /default: \$\{\{ vars\.SCCACHE_REDIS_DB \}\}/);
  assert.match(text, /sccache-backend:\n(?:.*\n)*?\s+default: auto/);
  assert.match(text, /BACKEND: \$\{\{ inputs\.sccache-backend \}\}/);
  assert.match(text, /REDIS_ENDPOINT: \$\{\{ inputs\.sccache-redis-endpoint \}\}/);
  assert.match(text, /REDIS_DB: \$\{\{ inputs\.sccache-redis-db \}\}/);
  for (const v of ['RUSTC_WRAPPER=sccache', 'CARGO_INCREMENTAL=0', 'SCCACHE_IGNORE_SERVER_IO_ERROR=1', 'SCCACHE_REDIS_ENDPOINT=', 'SCCACHE_REDIS_DB=']) {
    assert.ok(text.includes(v), `${v} is exported`);
  }
  // Redis on every OS unless the backend is local or no endpoint is set (then local disk); the database is never part of the URL.
  assert.match(text, /\[ "\$BACKEND" != local \] && \[ -n "\$REDIS_ENDPOINT" \]/);
  assert.doesNotMatch(text, /RUNNER_OS" = "Linux" \] && \[ -n/);
  assert.match(text, /sccache: local disk/);
  assert.match(text, /sccache-backend must be auto, redis or local/);
  assert.match(text, /sccache-backend is redis but sccache-redis-endpoint is empty/);
  assert.doesNotMatch(text, /SCCACHE_REDIS_ENDPOINT=\$REDIS_ENDPOINT\//);
});

test('CI runs its own sccache server on a distinct port, started with the job env and stopped only on that port', () => {
  const setup = read(join(ACTIONS, 'setup-rust', 'action.yml'));
  const stats = read(join(ACTIONS, 'sccache-stats', 'action.yml'));
  assert.match(setup, /echo "SCCACHE_SERVER_PORT=4227"/);
  assert.match(setup, /export SCCACHE_SERVER_PORT=4227\n(?:.*\n)*?.*sccache --start-server/);
  assert.ok(setup.indexOf('SCCACHE_REDIS_ENDPOINT=$REDIS_ENDPOINT') < setup.indexOf('sccache --start-server'), 'the server starts after the backend env is set');
  assert.doesNotMatch(setup + stats, /SCCACHE_SERVER_PORT=4226/);
  assert.match(stats, /if \[ -n "\$SCCACHE_SERVER_PORT" \]; then sccache --stop-server/);
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
  assert.equal((ci.match(/uses: \.\/\.gitea\/actions\/setup-rust/g) ?? []).length, 3, 'setup-rust in the crates, crate (matrix) and shell demo jobs');
  assert.equal((ci.match(/uses: \.\/\.gitea\/actions\/sccache-stats/g) ?? []).length, 3, 'stats after each');
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
