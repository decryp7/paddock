import assert from 'node:assert/strict';
import { existsSync, realpathSync } from 'node:fs';
import { join } from 'node:path';
import { test } from 'node:test';
import { calls, run, scratch, shim, write } from './helpers.mjs';

/** A fake docker: records every call; `image inspect` fails (no image yet) unless HAVE_IMAGE is set; `build` reads its Dockerfile from stdin like the real one (an unread pipe would end the script's printf with SIGPIPE). */
function docker({ ps = '', images = [] } = {}) {
  const bin = scratch('paddock-bin-');
  shim(bin, 'docker', `case "$1 $2" in "image inspect") [ -n "$HAVE_IMAGE" ];; "ps -aq") echo "${ps}";; "image ls") printf '%s\\n' ${images.join(' ')};; "build "*) cat >/dev/null;; esac`);
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

test('--clean removes only containers with this app\'s label and only its volumes (registry, amd64 and arm64 targets)', () => {
  const bin = docker({ ps: 'abc123 def456' });
  const r = go(bin, ['--id', 'dyno', '--clean']);
  assert.equal(r.status, 0, r.out);
  const log = calls(bin);
  assert.deepEqual(log, [
    'docker ps -aq --filter label=paddock.tools=dyno',
    'docker rm -f abc123 def456',
    'docker volume rm -f dyno-cargo-registry dyno-linux-target dyno-linux-target-arm64',
    'docker image ls --filter label=paddock.tools=dyno --format {{.Repository}}:{{.Tag}}',
  ]);
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

/** A repository root with the files --ci-image reads. */
function ciRepo() {
  return write(scratch(), {
    'ci-image/VERSION': '3\n',
    'ci-image/Dockerfile': 'FROM ${GARAGE_IMAGE}\n',
    'ci-image/garage-arm64.Dockerfile': 'FROM base\n',
    'ci-image/fetch-verified.sh': '#!/bin/sh\n',
    'ci-image/verify.sh': '#!/bin/sh\n',
  });
}
const EMULATION = /warning: linux\/amd64 runs under emulation/;

test('--ci-image on an arm64 host builds the native image once (garage stand-in, then ci-image/Dockerfile) and runs it as linux/arm64', () => {
  const bin = docker();
  const repo = ciRepo();
  const r = go(bin, ['--id', 'paddock', '--root', repo, '--ci-image', '--', 'cargo', 'nextest', 'run'], { PADDOCK_LINUX_HOST_ARCH: 'arm64' });
  assert.equal(r.status, 0, r.out);
  assert.doesNotMatch(r.out, EMULATION);
  const log = calls(bin);
  const builds = log.filter((l) => l.startsWith('docker build'));
  assert.equal(builds.length, 2, log.join('\n'));
  assert.match(builds[0], /^docker build --platform linux\/arm64 --label paddock\.tools=paddock -f \S+\/ci-image\/garage-arm64\.Dockerfile -t paddock-garage-local:arm64-([0-9a-f]{12}) /);
  const sum = builds[0].match(/arm64-([0-9a-f]{12})/)[1];
  assert.ok(builds[1].includes(`--build-arg GARAGE_IMAGE=paddock-garage-local:arm64-${sum} -f ${repo}/ci-image/Dockerfile -t paddock-ci-local:v3-arm64-${sum}`), builds[1]);
  const runLine = log.at(-1);
  assert.match(runLine, /--platform linux\/arm64 /);
  assert.match(runLine, /type=volume,source=paddock-linux-target-arm64,target=\/target/);
  assert.ok(log.includes('docker volume create --label paddock.tools=paddock paddock-linux-target-arm64'));
  assert.ok(runLine.endsWith(` paddock-ci-local:v3-arm64-${sum} cargo nextest run`), runLine);

  const again = docker();
  const r2 = go(again, ['--id', 'paddock', '--root', repo, '--ci-image', '--', 'true'], { PADDOCK_LINUX_HOST_ARCH: 'aarch64', HAVE_IMAGE: '1' });
  assert.equal(r2.status, 0, r2.out);
  assert.ok(!calls(again).some((l) => l.startsWith('docker build')), 'a built image is reused');
  assert.ok(calls(again).at(-1).includes(`paddock-ci-local:v3-arm64-${sum} true`), 'same files, same tag');
});

test('--ci-image on an amd64 host, or with --platform linux/amd64, runs the registry image; amd64 on an arm64 host warns about emulation', () => {
  const repo = ciRepo();
  const native = docker();
  const r = go(native, ['--id', 'paddock', '--root', repo, '--ci-image', '--', 'true'], { PADDOCK_LINUX_HOST_ARCH: 'x86_64' });
  assert.equal(r.status, 0, r.out);
  assert.doesNotMatch(r.out, EMULATION);
  assert.ok(!calls(native).some((l) => l.startsWith('docker build') || l.includes('image inspect')));
  assert.match(calls(native).at(-1), /--platform linux\/amd64 .* dev\.decryptology\.net\/decryp7\/paddock-ci:v3 true$/);

  const emulated = docker();
  const e = go(emulated, ['--id', 'paddock', '--root', repo, '--ci-image', '--platform', 'linux/amd64', '--', 'true'], { PADDOCK_LINUX_HOST_ARCH: 'arm64' });
  assert.equal(e.status, 0, e.out);
  assert.match(e.out, EMULATION);
  assert.match(e.out, /a failed exec is not reported/i);
  assert.match(calls(emulated).at(-1), /--platform linux\/amd64 .* dev\.decryptology\.net\/decryp7\/paddock-ci:v3 true$/);

  const ready = docker();
  const w = go(ready, ['--id', 'paddock', '--image', 'reg.example/ci:v2', '--platform', 'linux/amd64', '--', 'true'], { PADDOCK_LINUX_HOST_ARCH: 'arm64' });
  assert.equal(w.status, 0, w.out);
  assert.match(w.out, EMULATION, 'a ready image under emulation warns too');
});

test('--ci-image refuses --image beside it and fails with one error line when ci-image/VERSION is missing', () => {
  const bin = docker();
  assert.equal(go(bin, ['--id', 'paddock', '--ci-image', '--image', 'x', '--', 'true']).status, 2);
  const r = go(bin, ['--id', 'paddock', '--root', scratch(), '--ci-image', '--', 'true'], { PADDOCK_LINUX_HOST_ARCH: 'x86_64' });
  assert.equal(r.status, 1, r.out);
  assert.match(r.out, /::error::linux-container: cannot read .*ci-image\/VERSION/);
  assert.ok(!calls(bin).some((l) => l.startsWith('docker run')), 'nothing was run');
});

test('a linked worktree gets its common git folder read-only at its own path, a tmpfs pre-push state folder and safe.directory', () => {
  const bin = docker();
  const common = realpathSync(write(scratch(), { 'HEAD': 'ref: refs/heads/main\n', 'worktrees/wt/commondir': '../..\n', 'worktrees/wt/HEAD': 'x\n' }));
  const wt = write(scratch(), { '.git': `gitdir: ${common}/worktrees/wt\n` });
  const r = go(bin, ['--id', 'paddock', '--root', wt, '--image', 'img', '--', 'git', 'status'], { PADDOCK_LINUX_HOST_ARCH: 'x86_64' });
  assert.equal(r.status, 0, r.out);
  const runLine = calls(bin).at(-1);
  assert.ok(runLine.includes(`type=bind,source=${wt},target=/work,readonly`), runLine);
  assert.ok(runLine.includes(`type=bind,source=${common},target=${common},readonly`), runLine);
  assert.ok(!runLine.includes(`target=${common}/worktrees/wt,`), 'a git folder inside the common one is not mounted twice');
  assert.ok(runLine.includes(`type=tmpfs,target=${common}/paddock-pre-push`), runLine);
  // safe.directory comes from the CI image's /etc/gitconfig, as in CI: no GIT_* variable, no wrapper, so the command's own HOME
  // and its .gitconfig are read as usual (paddock-dev's kept_fixture turns git maintenance off through a temp HOME's .gitconfig).
  assert.doesNotMatch(runLine, / -e GIT_/);
  assert.ok(runLine.endsWith(' img git status'), runLine);
  assert.ok(existsSync(join(common, 'paddock-pre-push')), 'the mount point exists');

  const elsewhere = realpathSync(write(scratch(), { 'commondir': `${common}\n`, 'HEAD': 'x\n' }));
  const wt2 = write(scratch(), { '.git': `gitdir: ${elsewhere}\n` });
  const bin2 = docker();
  go(bin2, ['--id', 'paddock', '--root', wt2, '--image', 'img', '--', 'true'], { PADDOCK_LINUX_HOST_ARCH: 'x86_64' });
  const line2 = calls(bin2).at(-1);
  assert.ok(line2.includes(`source=${common},target=${common},readonly`) && line2.includes(`source=${elsewhere},target=${elsewhere},readonly`), line2);
});

test('a checkout with a .git folder needs no extra git mount; its pre-push state folder is a tmpfs under /work', () => {
  const bin = docker();
  const repo = write(scratch(), { '.git/HEAD': 'ref: refs/heads/main\n' });
  const r = go(bin, ['--id', 'paddock', '--root', repo, '--image', 'img', '--', 'true'], { PADDOCK_LINUX_HOST_ARCH: 'x86_64' });
  assert.equal(r.status, 0, r.out);
  const runLine = calls(bin).at(-1);
  assert.ok(!runLine.includes('type=bind,source=' + repo + '/.git'), runLine);
  assert.ok(runLine.includes('type=tmpfs,target=/work/.git/paddock-pre-push'), runLine);
});

test('a new arm64 build removes this id\'s older local CI images and keeps the new pair; --clean removes them all, never another image', () => {
  const images = ['paddock-ci-local:v3-arm64-aaaaaaaaaaaa', 'paddock-garage-local:arm64-aaaaaaaaaaaa', 'paddock-linux-dev:1', 'someone-else:latest'];
  const bin = docker({ images });
  const repo = ciRepo();
  const r = go(bin, ['--id', 'paddock', '--root', repo, '--ci-image', '--', 'true'], { PADDOCK_LINUX_HOST_ARCH: 'arm64' });
  assert.equal(r.status, 0, r.out);
  const log = calls(bin);
  assert.ok(log.includes('docker image ls --filter label=paddock.tools=paddock --format {{.Repository}}:{{.Tag}}'), log.join('\n'));
  const removed = log.filter((l) => l.startsWith('docker image rm'));
  assert.deepEqual(removed, ['docker image rm paddock-ci-local:v3-arm64-aaaaaaaaaaaa', 'docker image rm paddock-garage-local:arm64-aaaaaaaaaaaa']);
  assert.match(r.out, /removed the old local image paddock-ci-local:v3-arm64-aaaaaaaaaaaa/);
  const fresh = log.find((l) => l.startsWith('docker build') && l.includes('ci-image/Dockerfile')).match(/-t (\S+)/)[1];
  const kept = docker({ images: [fresh] });
  go(kept, ['--id', 'paddock', '--root', repo, '--ci-image', '--', 'true'], { PADDOCK_LINUX_HOST_ARCH: 'arm64' });
  assert.ok(!calls(kept).some((l) => l.startsWith('docker image rm')), 'the image just built stays');

  const all = docker({ images });
  assert.equal(go(all, ['--id', 'paddock', '--clean']).status, 0);
  assert.deepEqual(calls(all).filter((l) => l.startsWith('docker image rm')), removed);
  const nobuild = docker({ images });
  go(nobuild, ['--id', 'paddock', '--root', repo, '--ci-image', '--', 'true'], { PADDOCK_LINUX_HOST_ARCH: 'arm64', HAVE_IMAGE: '1' });
  assert.ok(!calls(nobuild).some((l) => l.startsWith('docker image')  && !l.startsWith('docker image inspect')), 'no build, no clean-up');
});

/** Every `-e NAME=` the script passes to docker run. */
const envNames = (line) => [...line.matchAll(/ -e ([A-Z_][A-Z0-9_]*)=/g)].map((m) => m[1]);

test('env parity with CI: the container gets only variables the Linux jobs of CI set too, and never a GIT_* one', () => {
  // CI's jobs set these (.gitea/workflows/ci.yml `CARGO_TERM_COLOR: always`; CARGO_TARGET_DIR and CARGO_BUILD_JOBS through the
  // build-folder action and the job's share of the cores); HOME, CARGO_HOME and the /tmp target only in PADDOCK_LINUX_USER mode,
  // where a user without a home needs them, as the runner's own user has them.
  const ci = new Set(['CARGO_TARGET_DIR', 'CARGO_BUILD_JOBS', 'CARGO_TERM_COLOR']);
  const userMode = new Set(['HOME', 'CARGO_HOME']);
  const common = realpathSync(write(scratch(), { 'HEAD': 'x\n', 'worktrees/wt/commondir': '../..\n', 'worktrees/wt/HEAD': 'x\n' }));
  const wt = write(scratch(), { '.git': `gitdir: ${common}/worktrees/wt\n` });
  const cases = [
    [['--id', 'paddock', '--root', wt, '--image', 'img', '--', 'true'], {}, ci],
    [['--id', 'paddock', '--root', ciRepo(), '--ci-image', '--', 'true'], { PADDOCK_LINUX_HOST_ARCH: 'arm64' }, ci],
    [['--id', 'paddock', '--root', wt, '--image', 'img', '--', 'true'], { PADDOCK_LINUX_USER: '1000:1000' }, new Set([...ci, ...userMode])],
  ];
  for (const [args, env, allowed] of cases) {
    const bin = docker();
    const r = go(bin, args, { PADDOCK_LINUX_HOST_ARCH: 'x86_64', ...env });
    assert.equal(r.status, 0, r.out);
    const names = envNames(calls(bin).at(-1));
    assert.ok(names.length > 0, 'the run line was read');
    for (const n of names) assert.ok(allowed.has(n) && !n.startsWith('GIT_'), `${n} is set in the container but not by CI's jobs`);
  }
});
