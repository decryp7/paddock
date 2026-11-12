import assert from 'node:assert/strict';
import { readdirSync } from 'node:fs';
import { spawnSync } from 'node:child_process';
import { join } from 'node:path';
import { test } from 'node:test';
import { calls, run, scratch, shim, write } from './helpers.mjs';

const git = (cwd, ...a) => spawnSync('git', ['-c', 'user.name=t', '-c', 'user.email=t@t', ...a], { cwd, encoding: 'utf8' });

/** A repo with ci-image/VERSION and crates a and b on `main`, then `changed` edited on a branch. */
function repo(changed) {
  const root = scratch('paddock-repo-');
  git(root, 'init', '-q', '-b', 'main');
  write(root, {
    'ci-image/VERSION': '7\n',
    'crates/Cargo.toml': '[workspace]\n',
    '.cargo/config.toml': '#\n',
    'crates/paddock-a/Cargo.toml': '[package]\nname = "paddock-a"\n',
    'crates/paddock-a/src/lib.rs': '//',
    'apps/x/file.rs': '//',
  });
  git(root, 'add', '.');
  git(root, 'commit', '-qm', 'init');
  git(root, 'checkout', '-qb', 'work');
  for (const f of changed) write(root, { [f]: '// changed' });
  git(root, 'commit', '-qam', 'change');
  return root;
}
/** docker: `info` succeeds unless NO_DOCKER_DAEMON; every call is logged; nothing is started. */
function bin() {
  const b = scratch('paddock-bin-');
  shim(b, 'docker', 'case "$1" in info) [ -z "$NO_DOCKER_DAEMON" ];; image) exit 0;; esac');
  return b;
}
const go = (b, root, args = [], env = {}) =>
  run('linux-clippy-touched.sh', ['--base', 'main', '--root', root, ...args], { bin: b, cwd: root, env: { DOCKER: join(b, 'docker'), ...env } });

test("a crate change runs CI's exact workspace command with --locked in the pinned CI image, capped, versions first", () => {
  const b = bin();
  const r = go(b, repo(['crates/paddock-a/src/lib.rs', 'apps/x/file.rs']));
  assert.equal(r.status, 0, r.out);
  const last = calls(b).at(-1);
  assert.match(last, /^docker run --rm .*--cpus 4 --memory 12g --platform linux\/amd64 /);
  assert.ok(last.includes(' dev.decryptology.net/decryp7/paddock-ci:v7 sh -c '), last);
  assert.match(last, /rustc --version && cargo clippy --version && cd crates && cargo clippy --workspace --all-targets --all-features --locked -- -D warnings$/);
  assert.ok(!calls(b).some((l) => l.startsWith('docker build')), 'a ready image gets no layer of ours');
});

test('a workspace-level file (crates/Cargo.toml) and .cargo/config.toml each run the same workspace command', () => {
  for (const f of ['crates/Cargo.toml', '.cargo/config.toml']) {
    const b = bin();
    const r = go(b, repo([f]));
    assert.equal(r.status, 0, r.out);
    assert.match(calls(b).at(-1), /cargo clippy --workspace --all-targets --all-features --locked -- -D warnings$/, f);
  }
});

test('--image replaces the CI image and drops the platform; PADDOCK_HEAVY prefixes the container run', () => {
  const b = bin();
  shim(b, 'heavy.sh', 'exec "$@"');
  go(b, repo(['crates/paddock-a/src/lib.rs']), ['--image', 'rust:1.99.1-bookworm'], { PADDOCK_HEAVY: 'heavy.sh' });
  const log = calls(b);
  assert.match(log.find((l) => l.startsWith('heavy.sh')), /^heavy.sh bash .*linux-container.sh --id paddock /);
  assert.ok(log.at(-1).includes(' rust:1.99.1-bookworm sh -c '));
  assert.doesNotMatch(log.at(-1), /--platform/);
});

test('the diff starts at the merge base: a change main got meanwhile does not count', () => {
  const b = bin();
  const root = repo(['apps/x/file.rs']);
  git(root, 'checkout', '-q', 'main');
  write(root, { 'crates/paddock-a/src/lib.rs': '// main moved' });
  git(root, 'commit', '-qam', 'main moves');
  git(root, 'checkout', '-q', 'work');
  const r = go(b, root);
  assert.equal(r.status, 0, r.out);
  assert.match(r.out, /nothing to check/);
  assert.deepEqual(calls(b), []);
});

test('nothing under crates/ or .cargo/ changed: nothing runs, not even docker', () => {
  const b = bin();
  const r = go(b, repo(['apps/x/file.rs']));
  assert.equal(r.status, 0, r.out);
  assert.match(r.out, /nothing to check/);
  assert.deepEqual(calls(b), []);
});

test('no Docker daemon: NOT MEASURED, exit 3, the agent is told not to start Docker, no container started', () => {
  const b = bin();
  const r = go(b, repo(['crates/paddock-a/src/lib.rs']), [], { NO_DOCKER_DAEMON: '1' });
  assert.equal(r.status, 3, r.out);
  assert.match(r.out, /::warning::.*NOT MEASURED.*do not start or restart it \(ER-14\)/);
  assert.doesNotMatch(r.out, /CI will run/);
  assert.ok(!calls(b).some((l) => l.startsWith('docker run')));
});

test('no docker binary at all: the same, exit 3', () => {
  const root = repo(['crates/paddock-a/src/lib.rs']);
  const r = run('linux-clippy-touched.sh', ['--base', 'main', '--root', root], { cwd: root, env: { DOCKER: '/nonexistent/docker' } });
  assert.equal(r.status, 3, r.out);
  assert.match(r.out, /NOT MEASURED/);
});

test("Clippy's own failure code is passed through", () => {
  const b = scratch('paddock-bin-');
  shim(b, 'docker', 'case "$1" in info) exit 0;; image) exit 0;; run) exit 101;; esac');
  assert.equal(go(b, repo(['crates/paddock-a/src/lib.rs'])).status, 101);
});

test('a bad base is an error with the next step; an unknown option is a usage error', () => {
  const b = bin();
  const root = repo(['crates/paddock-a/src/lib.rs']);
  const bad = run('linux-clippy-touched.sh', ['--base', 'nope', '--root', root], { bin: b, cwd: root, env: { DOCKER: join(b, 'docker') } });
  assert.equal(bad.status, 1);
  assert.match(bad.out, /::error::.*git fetch/);
  assert.equal(go(b, root, ['--wat']).status, 2);
});

test('the temporary error file is removed after a container run, not only on an early exit', () => {
  const b = bin();
  const tmp = scratch('paddock-tmp-');
  // macOS mktemp ignores TMPDIR without a template, so a shim makes the file where the test can see it.
  shim(b, 'mktemp', `f="${tmp}/err.$$"; : >"$f"; echo "$f"`);
  const r = go(b, repo(['crates/paddock-a/src/lib.rs']));
  assert.equal(r.status, 0, r.out);
  assert.deepEqual(readdirSync(tmp), []);
});

test('docker exit 125 (the run could not start) names the cause and the next step, and keeps the code', () => {
  const b = scratch('paddock-bin-');
  shim(b, 'docker', 'case "$1" in info) exit 0;; image) exit 0;; run) exit 125;; esac');
  const r = go(b, repo(['crates/paddock-a/src/lib.rs']));
  assert.equal(r.status, 125, r.out);
  assert.match(r.out, /::error::linux-clippy-touched: .*docker login dev\.decryptology\.net or pass --image/);
});

test('docker exit 137 (the container was killed, usually out of memory) says to raise PADDOCK_LINUX_MEMORY, and keeps the code', () => {
  const b = scratch('paddock-bin-');
  shim(b, 'docker', 'case "$1" in info) exit 0;; image) exit 0;; run) exit 137;; esac');
  const r = go(b, repo(['crates/paddock-a/src/lib.rs']));
  assert.equal(r.status, 137, r.out);
  assert.match(r.out, /::error::linux-clippy-touched: .*raise PADDOCK_LINUX_MEMORY/);
});
