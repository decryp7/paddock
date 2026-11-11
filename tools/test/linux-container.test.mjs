import assert from 'node:assert/strict';
import { join } from 'node:path';
import { test } from 'node:test';
import { calls, run, scratch, shim } from './helpers.mjs';

/** A fake docker: records every call; `image inspect` fails (no image yet) unless HAVE_IMAGE is set; `build` reads its Dockerfile from stdin like the real one (an unread pipe would end the script's printf with SIGPIPE). */
function docker({ ps = '' } = {}) {
  const bin = scratch('paddock-bin-');
  shim(bin, 'docker', `case "$1 $2" in "image inspect") [ -n "$HAVE_IMAGE" ];; "ps -aq") echo "${ps}";; "build "*) cat >/dev/null;; esac`);
  return bin;
}
const go = (bin, args, env = {}) => run('linux-container.sh', args, { bin, cwd: scratch(), env: { DOCKER: join(bin, 'docker'), ...env } });

test('first use builds the labelled image, creates the labelled volumes and runs --rm with a read-only source', () => {
  const bin = docker();
  const repo = scratch();
  const r = go(bin, ['--id', 'dyno', '--root', repo, '--packages', 'dyno-platform']);
  assert.equal(r.status, 0, r.out);
  const log = calls(bin);
  assert.match(log[0], /docker image inspect dyno-linux-dev:1/);
  assert.match(log[1], /docker build --label paddock\.tools=dyno -t dyno-linux-dev:1 -/);
  assert.match(log[2], /docker volume create --label paddock\.tools=dyno dyno-cargo-registry/);
  assert.match(log[3], /docker volume create --label paddock\.tools=dyno dyno-linux-target/);
  const runLine = log[4];
  assert.match(runLine, /^docker run --rm --label paddock\.tools=dyno --cpus 4 --memory 12g /);
  assert.ok(runLine.includes(`type=bind,source=${repo},target=/work,readonly`));
  assert.match(runLine, /type=volume,source=dyno-cargo-registry,target=\/usr\/local\/cargo\/registry/);
  assert.match(runLine, /CARGO_BUILD_JOBS=2/);
  assert.match(runLine, /sh -c cargo test -p dyno-platform && cargo clippy -p dyno-platform --all-targets -- -D warnings$/);
  assert.doesNotMatch(runLine, /--privileged/);
});

test('an existing image is reused; a command after -- replaces the default; user and sizes come from the environment', () => {
  const bin = docker();
  const r = go(bin, ['--id', 'spotter', '--', 'sh', '-c', 'ls /proc | head'], { HAVE_IMAGE: '1', PADDOCK_LINUX_USER: '1000:1000', PADDOCK_LINUX_CPUS: '2', PADDOCK_LINUX_JOBS: '1' });
  assert.equal(r.status, 0, r.out);
  const log = calls(bin);
  assert.ok(!log.some((l) => l.includes('docker build')));
  const runLine = log.at(-1);
  assert.match(runLine, /--cpus 2 /);
  assert.match(runLine, /--user 1000:1000 -e HOME=\/tmp\/home -e CARGO_HOME=\/tmp\/cargo-home -e CARGO_TARGET_DIR=\/tmp\/target/);
  assert.match(runLine, /spotter-linux-dev:1 sh -c ls \/proc \| head$/);
});

test('--clean removes only containers with this app\'s label and only its two volumes', () => {
  const bin = docker({ ps: 'abc123 def456' });
  const r = go(bin, ['--id', 'dyno', '--clean']);
  assert.equal(r.status, 0, r.out);
  const log = calls(bin);
  assert.deepEqual(log, ['docker ps -aq --filter label=paddock.tools=dyno', 'docker rm -f abc123 def456', 'docker volume rm -f dyno-cargo-registry dyno-linux-target']);
  const none = docker();
  go(none, ['--id', 'dyno', '--clean']);
  assert.ok(!calls(none).some((l) => l.startsWith('docker rm')), 'no container, no rm');
});

test('usage errors: no id, bad id, nothing to run, unknown option, no docker', () => {
  const bin = docker();
  assert.equal(go(bin, []).status, 2);
  assert.equal(go(bin, ['--id', 'Bad_Id']).status, 2);
  assert.equal(go(bin, ['--id', 'dyno']).status, 2);
  assert.equal(go(bin, ['--id', 'dyno', '--wat']).status, 2);
  const none = run('linux-container.sh', ['--id', 'dyno', '--packages', 'x'], { cwd: scratch(), env: { DOCKER: '/nonexistent/docker' } });
  assert.equal(none.status, 1);
  assert.match(none.out, /is needed/);
  assert.deepEqual(calls(bin), [], 'nothing was started');
});

test('--image runs a ready image as it is: no inspect, no build, the platform passed, the caps kept', () => {
  const bin = docker();
  const r = go(bin, ['--id', 'paddock', '--image', 'reg.example/ci:v2', '--platform', 'linux/amd64', '--', 'rustc', '--version']);
  assert.equal(r.status, 0, r.out);
  const log = calls(bin);
  assert.ok(!log.some((l) => l.includes('image inspect') || l.includes('docker build')));
  const runLine = log.at(-1);
  assert.match(runLine, /^docker run --rm --label paddock\.tools=paddock --cpus 4 --memory 12g --platform linux\/amd64 /);
  assert.match(runLine, / reg\.example\/ci:v2 rustc --version$/);
});
