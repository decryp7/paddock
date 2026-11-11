import assert from 'node:assert/strict';
import { spawnSync } from 'node:child_process';
import { join } from 'node:path';
import { test } from 'node:test';
import { calls, run, scratch, shim, write } from './helpers.mjs';

const git = (cwd, ...a) => spawnSync('git', ['-c', 'user.name=t', '-c', 'user.email=t@t', ...a], { cwd, encoding: 'utf8' });

/** A repo with crates a and b committed on `main`, then `changed` edited on a branch. */
function repo(changed) {
  const root = scratch('paddock-repo-');
  git(root, 'init', '-q', '-b', 'main');
  write(root, {
    'crates/paddock-a/Cargo.toml': '[package]\nname = "paddock-a"\n',
    'crates/paddock-a/src/lib.rs': '//',
    'crates/paddock-b/Cargo.toml': '[package]\nname = "paddock-b"\n',
    'crates/paddock-b/src/lib.rs': '//',
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

test('only the touched crates are linted, in the capped container, with -D warnings', () => {
  const b = bin();
  const r = go(b, repo(['crates/paddock-a/src/lib.rs', 'apps/x/file.rs']));
  assert.equal(r.status, 0, r.out);
  const last = calls(b).at(-1);
  assert.match(last, /^docker run --rm .*--cpus 4 --memory 12g /);
  assert.match(last, /cargo clippy --locked -p paddock-a --all-targets -- -D warnings$/);
  assert.doesNotMatch(last, /paddock-b/);
});

test('two touched crates are both named', () => {
  const b = bin();
  go(b, repo(['crates/paddock-a/src/lib.rs', 'crates/paddock-b/src/lib.rs']));
  assert.match(calls(b).at(-1), /-p paddock-a -p paddock-b --all-targets/);
});

test('no shared crate touched: nothing runs, not even docker', () => {
  const b = bin();
  const r = go(b, repo(['apps/x/file.rs']));
  assert.equal(r.status, 0, r.out);
  assert.match(r.out, /nothing to check/);
  assert.deepEqual(calls(b), []);
});

test('no Docker daemon: a ::warning:: with the next step, exit 0, no container started', () => {
  const b = bin();
  const r = go(b, repo(['crates/paddock-a/src/lib.rs']), [], { NO_DOCKER_DAEMON: '1' });
  assert.equal(r.status, 0, r.out);
  assert.match(r.out, /::warning::.*SKIPPED.*Start Docker/);
  assert.ok(!calls(b).some((l) => l.startsWith('docker run')));
});

test('no docker binary at all: the same warning', () => {
  const root = repo(['crates/paddock-a/src/lib.rs']);
  const r = run('linux-clippy-touched.sh', ['--base', 'main', '--root', root], { cwd: root, env: { DOCKER: '/nonexistent/docker' } });
  assert.equal(r.status, 0, r.out);
  assert.match(r.out, /::warning::.*SKIPPED/);
});

test('a bad base is an error with the next step; an unknown option is a usage error', () => {
  const b = bin();
  const root = repo(['crates/paddock-a/src/lib.rs']);
  const bad = run('linux-clippy-touched.sh', ['--base', 'nope', '--root', root], { bin: b, cwd: root, env: { DOCKER: join(b, 'docker') } });
  assert.equal(bad.status, 1);
  assert.match(bad.out, /::error::.*git fetch/);
  assert.equal(go(b, root, ['--wat']).status, 2);
});
