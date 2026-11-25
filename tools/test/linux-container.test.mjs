import assert from 'node:assert/strict';
import { existsSync, realpathSync } from 'node:fs';
import { join } from 'node:path';
import { test } from 'node:test';
import { TOOLS, calls, run, scratch, shim, write } from './helpers.mjs';
import { readFileSync, writeFileSync } from 'node:fs';
import { spawnSync } from 'node:child_process';
import { job, loadWorkflow, loadYaml } from '../../conformance/test/lib/workflow.mjs';

/** A fake docker: records every call; `image inspect` fails (no image yet) unless HAVE_IMAGE is set or a build of this fake made that
 * tag; `image ls` prints `images` (`ref`, or `ref<TAB>created`); `build` reads its Dockerfile from stdin like the real one (an unread
 * pipe would end the script's printf with SIGPIPE), sleeps BUILD_SLEEP seconds when set, and marks its `-t` tag as built. */
function docker({ ps = '', images = [], dir } = {}) {
  const bin = dir ?? scratch('paddock-bin-');
  writeFileSync(join(bin, 'images.txt'), images.map((i) => `${i}\n`).join(''));
  shim(
    bin,
    'docker',
    `state="${bin}"
case "$1 $2" in
  "image inspect") [ -n "$HAVE_IMAGE" ] || [ -e "$state/built-$3" ];;
  "ps -aq") echo "${ps}";;
  "image ls") cat "$state/images.txt";;
  "build "*) cat >/dev/null; while [ $# -gt 0 ]; do [ "$1" = -t ] && tag="$2"; shift; done
    echo "start $tag" >> "$state/builds.log"; [ -z "$BUILD_SLEEP" ] || sleep "$BUILD_SLEEP"; echo "end $tag" >> "$state/builds.log"
    : > "$state/built-$tag";;
esac`,
  );
  return bin;
}
const go = (bin, args, env = {}) => run('linux-container.sh', args, { bin, cwd: scratch(), env: { DOCKER: join(bin, 'docker'), ...env } });

test('first use builds the labelled image, creates the labelled volumes and runs --rm with a read-only source', () => {
  const bin = docker();
  const repo = scratch();
  const r = go(bin, ['--id', 'dyno', '--root', repo, '--packages', 'dyno-platform']);
  assert.equal(r.status, 0, r.out);
  const log = calls(bin);
  assert.match(log[0], /docker image inspect dyno-linux-dev:2/);
  assert.match(log[1], /docker build --label paddock\.tools=dyno -t dyno-linux-dev:2 -/);
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
  assert.match(runLine, /spotter-linux-dev:2 sh -c ls \/proc \| head$/);
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
    'docker image ls --filter label=paddock.tools=dyno --format {{.Repository}}:{{.Tag}}\t{{.CreatedAt}}',
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
function ciRepo(variant = '') {
  return write(scratch(), {
    'ci-image/VERSION': '3\n',
    'ci-image/Dockerfile': `FROM \${GARAGE_IMAGE}\n${variant}`,
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

test('a new arm64 build keeps the newest two local CI images (each with its garage image) and removes older ones; --clean removes them all, never another image', () => {
  const T = (ref, created) => `${ref}\t${created}`;
  // An older pair from another worktree's hash: kept, as one of the newest two (it was removed before 2026-11-26, and that worktree
  // rebuilt for about 20 min).
  const images = [T('paddock-ci-local:v3-arm64-aaaaaaaaaaaa', '2026-11-25 10:00:00 +0800 +08'), T('paddock-garage-local:arm64-aaaaaaaaaaaa', '2026-11-25 09:50:00 +0800 +08'), 'paddock-linux-dev:1', 'someone-else:latest'];
  const bin = docker({ images });
  const repo = ciRepo();
  const r = go(bin, ['--id', 'paddock', '--root', repo, '--ci-image', '--', 'true'], { PADDOCK_LINUX_HOST_ARCH: 'arm64' });
  assert.equal(r.status, 0, r.out);
  const log = calls(bin);
  assert.ok(log.some((l) => l.startsWith('docker image ls --filter label=paddock.tools=paddock --format {{.Repository}}:{{.Tag}}')), log.join('\n'));
  assert.deepEqual(log.filter((l) => l.startsWith('docker image rm')), [], 'two pairs: nothing is removed');
  const fresh = log.find((l) => l.startsWith('docker build') && l.includes('ci-image/Dockerfile')).match(/-t (\S+)/)[1];
  const freshSum = fresh.split('-').at(-1);

  // A third hash: the oldest pair goes, the two newest stay (listed oldest last here, so the order is the creation time's, not the list's).
  const three = [
    T('paddock-ci-local:v3-arm64-000000000000', '2026-11-24 08:00:00 +0800 +08'),
    T('paddock-garage-local:arm64-000000000000', '2026-11-24 07:50:00 +0800 +08'),
    ...images,
  ];
  const evict = docker({ images: [T(fresh, '2026-11-26 01:00:00 +0800 +08'), T(`paddock-garage-local:arm64-${freshSum}`, '2026-11-26 00:50:00 +0800 +08'), ...three] });
  const r3 = go(evict, ['--id', 'paddock', '--root', ciRepo('changed'), '--ci-image', '--', 'true'], { PADDOCK_LINUX_HOST_ARCH: 'arm64' });
  assert.equal(r3.status, 0, r3.out);
  const removed = calls(evict).filter((l) => l.startsWith('docker image rm'));
  // The fake lists three pairs (fresh, aaaa, 0000); the two newest by creation time stay, whatever order docker lists them in.
  assert.deepEqual(removed.sort(), [
    'docker image rm paddock-ci-local:v3-arm64-000000000000',
    'docker image rm paddock-garage-local:arm64-000000000000',
  ]);
  assert.match(r3.out, /removed the old local image paddock-ci-local:v3-arm64-000000000000/);

  // A second run with the old inputs builds nothing: its image is still there.
  const again = docker({ dir: bin });
  const r2 = go(again, ['--id', 'paddock', '--root', repo, '--ci-image', '--', 'true'], { PADDOCK_LINUX_HOST_ARCH: 'arm64' });
  assert.equal(r2.status, 0, r2.out);
  assert.equal(calls(again).filter((l) => l.startsWith('docker build')).length, 2, 'only the first run built (garage and image)');

  const all = docker({ images: three });
  assert.equal(go(all, ['--id', 'paddock', '--clean']).status, 0);
  assert.deepEqual(calls(all).filter((l) => l.startsWith('docker image rm')).sort(), [
    'docker image rm paddock-ci-local:v3-arm64-000000000000',
    'docker image rm paddock-ci-local:v3-arm64-aaaaaaaaaaaa',
    'docker image rm paddock-garage-local:arm64-000000000000',
    'docker image rm paddock-garage-local:arm64-aaaaaaaaaaaa',
  ]);
  const nobuild = docker({ images: three });
  go(nobuild, ['--id', 'paddock', '--root', repo, '--ci-image', '--', 'true'], { PADDOCK_LINUX_HOST_ARCH: 'arm64', HAVE_IMAGE: '1' });
  assert.ok(!calls(nobuild).some((l) => l.startsWith('docker image') && !l.startsWith('docker image inspect')), 'no build, no clean-up');
});

test('two worktree roots with the same ci-image content get the same tag, and one build: the second waits for the build lock', async () => {
  const bin = docker();
  const common = write(scratch(), { 'HEAD': 'ref: refs/heads/main\n' });
  const a = ciRepo();
  const b = ciRepo();
  const tag = (root) => {
    const r = go(docker(), ['--id', 'paddock', '--root', root, '--ci-image', '--image-ready'], { PADDOCK_LINUX_HOST_ARCH: 'arm64' });
    return r.out.match(/(paddock-ci-local:\S+)/)[1];
  };
  assert.equal(tag(a), tag(b), 'the tag is the content hash, not the path');
  assert.notEqual(tag(a), tag(ciRepo('changed')), 'control: other content, another tag');
  // Both start at once on the same hash; the lock (in a shared folder: TMPDIR stands in for the common git folder) lets one build.
  const { spawn } = await import('node:child_process');
  const start = (root) =>
    new Promise((resolve) => {
      const c = spawn('bash', [join(TOOLS, 'linux-container.sh'), '--id', 'paddock', '--root', root, '--ci-image', '--', 'true'], {
        env: { PATH: `${bin}:/usr/bin:/bin`, HOME: common, TMPDIR: common, DOCKER: join(bin, 'docker'), PADDOCK_LINUX_HOST_ARCH: 'arm64', BUILD_SLEEP: '1' },
        stdio: ['ignore', 'ignore', 'pipe'],
        timeout: 60_000,
      });
      let out = '';
      c.stderr.on('data', (d) => (out += d));
      c.on('close', (code) => resolve({ code, out }));
    });
  const [ra, rb] = await Promise.all([start(a), start(b)]);
  assert.equal(ra.code, 0, ra.out);
  assert.equal(rb.code, 0, rb.out);
  const builds = readFileSync(join(bin, 'builds.log'), 'utf8').trim().split('\n');
  assert.equal(builds.filter((l) => l.startsWith('start ')).length, 2, `one build of the pair (garage, image), not two: ${builds.join(', ')}`);
  assert.match(ra.out + rb.out, /was built while waiting for its build lock/);
});

/** Every `-e NAME=` the script passes to docker run. */
const envOf = (line) => Object.fromEntries([...line.matchAll(/ -e ([A-Z_][A-Z0-9_]*)=(\S*)/g)].map((m) => [m[1], m[2]]));

/** The CARGO_TERM_COLOR values ci.yml's jobs set, read through the workflow accessor (rule 79). */
function ciTermColors() {
  const wf = loadWorkflow('ci.yml');
  const values = new Set();
  if (wf.env?.CARGO_TERM_COLOR) values.add(wf.env.CARGO_TERM_COLOR);
  for (const id of Object.keys(wf.jobs)) {
    const v = job(wf, id).env?.CARGO_TERM_COLOR;
    if (v) values.add(v);
  }
  return values;
}

/**
 * Each variable the container gets, beside where CI sets it (or why the local value differs). `ci` is read from its owner in the
 * test below, so a CI change that drops or changes it turns the test red.
 */
const PARITY = {
  CARGO_BUILD_JOBS: { owner: 'conformance/ci/build-jobs.sh', why: 'the one place CI sets it, written to $GITHUB_ENV (the build-jobs action and setup-rust run it)' },
  CARGO_TERM_COLOR: { local: 'never', ci: 'always', why: "CI's log viewer renders colour; local output goes to files and greps, where colour codes split the words" },
  CARGO_TARGET_DIR: { local: ['/target', '/tmp/target'], ci: null, why: '/tmp/target in PADDOCK_LINUX_USER mode. CI names the build folder in .cargo/ci.toml (the build-folder action), which the read-only /work cannot take; never set in CI, because sccache hashes it' },
  HOME: { userMode: true, why: 'PADDOCK_LINUX_USER runs as a user without a home; the runner user has one' },
  CARGO_HOME: { userMode: true, why: 'as HOME: that user cannot write the image cargo home' },
};

test('env parity with CI: every variable the container gets is declared with its CI source or the reason it differs; never GIT_*', () => {
  // The declared CI facts still hold, read from their owners.
  assert.deepEqual([...ciTermColors()], [PARITY.CARGO_TERM_COLOR.ci], 'ci.yml no longer sets CARGO_TERM_COLOR as declared: update PARITY');
  // The owner is run, not read: on a Linux runner with a set share it writes CARGO_BUILD_JOBS to the job's environment file.
  const ghenv = join(scratch(), 'github-env');
  writeFileSync(ghenv, '');
  const bj = spawnSync('bash', [join(TOOLS, '..', PARITY.CARGO_BUILD_JOBS.owner)], {
    encoding: 'utf8',
    env: { PATH: '/usr/bin:/bin', HOME: scratch(), GITHUB_ENV: ghenv, RUNNER_OS: 'Linux', PADDOCK_CI_BUILD_JOBS: '3' },
  });
  assert.equal(bj.status, 0, bj.stderr);
  assert.match(readFileSync(ghenv, 'utf8'), /^CARGO_BUILD_JOBS=3$/m, `${PARITY.CARGO_BUILD_JOBS.owner} no longer sets CARGO_BUILD_JOBS`);
  const buildFolder = loadYaml('.gitea', 'actions', 'build-folder', 'action.yml');
  assert.match(buildFolder.description, /never CARGO_TARGET_DIR/, 'the build-folder action now sets CARGO_TARGET_DIR: drop the local deviation');
  const ci = new Set(Object.keys(PARITY).filter((k) => !PARITY[k].userMode));
  const userMode = new Set(Object.keys(PARITY).filter((k) => PARITY[k].userMode));
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
    const got = envOf(calls(bin).at(-1));
    assert.ok(Object.keys(got).length > 0, 'the run line was read');
    for (const [n, v] of Object.entries(got)) {
      assert.ok(allowed.has(n) && !n.startsWith('GIT_'), `${n} is set in the container but not declared in PARITY`);
      if (PARITY[n].local) assert.ok([].concat(PARITY[n].local).includes(v), `${n}=${v}: the declared local value is ${PARITY[n].local}`);
    }
  }
});

test('the image built from PADDOCK_LINUX_BASE_IMAGE marks every directory safe in its system gitconfig, with no GIT_* variable', () => {
  const bin = scratch('paddock-bin-');
  shim(bin, 'docker', `case "$1 $2" in "image inspect") false;; "build "*) cat > "${bin}/dockerfile";; esac`);
  const r = go(bin, ['--id', 'dyno', '--packages', 'dyno-platform']);
  assert.equal(r.status, 0, r.out);
  assert.match(readFileSync(join(bin, 'dockerfile'), 'utf8'), /^RUN rustup component add clippy rustfmt && git config --system --add safe\.directory "\*"$/m);
  assert.doesNotMatch(calls(bin).at(-1), / -e GIT_/);
});

test('--image-ready only probes: exit 3 when the native image is not built (no build, no volume, no run), 0 when it is, 0 for the registry image', () => {
  const repo = ciRepo();
  const missing = docker();
  const r = go(missing, ['--id', 'paddock', '--root', repo, '--ci-image', '--image-ready'], { PADDOCK_LINUX_HOST_ARCH: 'arm64' });
  assert.equal(r.status, 3, r.out);
  assert.match(r.out, /paddock-ci-local:v3-arm64-[0-9a-f]{12} is not built/);
  assert.deepEqual(calls(missing).filter((l) => /build|volume create|docker run/.test(l)), []);
  const built = docker();
  assert.equal(go(built, ['--id', 'paddock', '--root', repo, '--ci-image', '--image-ready'], { PADDOCK_LINUX_HOST_ARCH: 'arm64', HAVE_IMAGE: '1' }).status, 0);
  assert.deepEqual(calls(built).filter((l) => /build|volume create|docker run/.test(l)), []);
  const registry = docker();
  assert.equal(go(registry, ['--id', 'paddock', '--root', repo, '--ci-image', '--image-ready'], { PADDOCK_LINUX_HOST_ARCH: 'x86_64' }).status, 0);
  assert.equal(go(docker(), ['--id', 'paddock', '--root', repo, '--image-ready']).status, 2, '--image-ready needs --ci-image');
});
