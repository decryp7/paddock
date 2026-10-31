// setup-rust's two download steps, run for real with fake programs on PATH: the Rust toolchain step (skip the network sync
// when everything is installed; one sync per host under a lock; name a crash's raw exit code) and the sccache install (verify
// in the job's own folder, fetch once more on a mismatch, log both hashes, never keep a copy that does not report the pin).
//
// Run 7267 (job 24328): on the Windows host `rustup toolchain install stable` ended 0.5 s into "syncing channel updates" with
// exit 127 and no message. Git Bash maps any NTSTATUS crash it has no signal for to 127, so the code was lost.
import assert from 'node:assert/strict';
import { spawnSync } from 'node:child_process';
import { createHash } from 'node:crypto';
import { chmodSync, existsSync, mkdirSync, mkdtempSync, readdirSync, readFileSync, rmSync, statSync, utimesSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { dirname, join } from 'node:path';
import { test } from 'node:test';
import { PADDOCK } from './helpers.mjs';

const ACTION = join(PADDOCK, '.gitea', 'actions', 'setup-rust');
const NATIVE = join(PADDOCK, 'conformance', 'ci', 'run-native.mjs');
const NODE_DIR = dirname(process.execPath);
const read = (p) => readFileSync(p, 'utf8');

/** A temp folder removed when the test ends. */
function tempDir(t, prefix) {
  const dir = mkdtempSync(join(tmpdir(), prefix));
  t.after(() => rmSync(dir, { recursive: true, force: true }));
  return dir;
}

/** The body of one run step of setup-rust, unindented, with the action path filled in. */
function stepBody(name, next) {
  const text = read(join(ACTION, 'action.yml'));
  const at = text.indexOf(`- name: ${name}\n`);
  const block = text.slice(at, text.indexOf(`- name: ${next}`, at));
  const body = block.slice(block.indexOf('run: |\n') + 'run: |\n'.length).split('\n').map((l) => l.slice(8)).join('\n');
  return body.replaceAll('${{ github.action_path }}', ACTION);
}

const HOST = 'x86_64-pc-windows-msvc';

/**
 * Runs the Rust toolchain step with a fake rustup. `toolchains` are `rustup toolchain list` lines; `rustup run <name>` works
 * for a name that is one of them minus `-<host>` and annotations, as rustup resolves names. `install` is the shell the fake
 * runs for `toolchain install` (after rustup's first stderr line); `stamp` is the age in hours of the last-sync stamp, or
 * null; `lock` is the owner file of a sync lock another job holds ('' for a lock with no owner file), or null, and `lockAge`
 * its folder's age in seconds; `onSleep` is shell the fake `sleep` runs (the step sleeps only while it waits on a held lock);
 * `mv` is shell for a fake mv (the step's only mv is the move of a stale lock). FAKE_NO_DEFAULT=1 in `env` makes the host
 * have no default toolchain.
 */
function runToolchain(t, { env = {}, toolchains = [], components = [], componentsTail = 'true', targets = [], install = 'exit 0', stamp = null, lock = null, lockAge = null, onSleep = '', mv = null } = {}) {
  const dir = tempDir(t, 'paddock-rustup-skip-');
  mkdirSync(join(dir, 'bin'));
  const calls = join(dir, 'calls');
  const list = (v) => v.map((l) => `echo '${l}'`).join('; ') || 'true';
  const runnable = toolchains.map((l) => l.replace(/ .*$/, '').replace(`-${HOST}`, '')).join(' ');
  writeFileSync(
    join(dir, 'bin', 'rustup'),
    [
      '#!/bin/bash',
      `echo "$*" >> '${calls}'`,
      'case "$1 $2" in',
      `  "toolchain list") ${list(toolchains)} ;;`,
      `  "component list") ${list(components)}; ${componentsTail} ;;`,
      `  "target list") ${list(targets)} ;;`,
      `  "toolchain install") echo "info: syncing channel updates for stable-${HOST}" >&2; ${install} ;;`,
      '  "default ") [ -n "$FAKE_NO_DEFAULT" ] && exit 1; exit 0 ;;',
      `  "run "*) [ "$RUSTUP_AUTO_INSTALL" = 0 ] || exit 9; for n in ${runnable}; do [ "$n" = "$2" ] && echo "rustc 1.99.1" && exit 0; done; exit 1 ;;`,
      'esac',
      'exit 0',
      '',
    ].join('\n'),
  );
  writeFileSync(join(dir, 'bin', 'sleep'), `#!/bin/bash\necho sleep >> '${calls}'\n${onSleep}\n`);
  for (const tool of ['rustc', 'cargo']) writeFileSync(join(dir, 'bin', tool), `#!/bin/sh\necho "${tool} 1.99.1 ($RUSTUP_TOOLCHAIN)"\n`);
  if (mv !== null) writeFileSync(join(dir, 'bin', 'mv'), `#!/bin/bash\n${mv}\n`);
  for (const f of ['rustup', 'rustc', 'cargo', 'sleep', ...(mv === null ? [] : ['mv'])]) chmodSync(join(dir, 'bin', f), 0o755);
  const rustupHome = join(dir, 'rustup-home');
  mkdirSync(rustupHome);
  const stampFile = join(rustupHome, `paddock-ci-synced-${env.TOOLCHAIN ?? 'stable'}`);
  if (stamp !== null) {
    writeFileSync(stampFile, '');
    const s = Date.now() / 1000 - stamp * 3600;
    utimesSync(stampFile, s, s);
  }
  const lockDir = join(rustupHome, 'paddock-ci-sync.lock');
  if (lock !== null) {
    mkdirSync(lockDir);
    if (lock !== '') writeFileSync(join(lockDir, 'owner'), lock);
    if (lockAge !== null) {
      const a = Date.now() / 1000 - lockAge;
      utimesSync(lockDir, a, a);
    }
  }
  const runnerTemp = join(dir, 'runner-temp');
  mkdirSync(runnerTemp);
  const ghEnv = join(dir, 'github-env');
  writeFileSync(ghEnv, '');
  writeFileSync(join(dir, 'step.sh'), stepBody('Rust toolchain', 'Install sccache'));
  const r = spawnSync('bash', ['-e', '-o', 'pipefail', join(dir, 'step.sh')], {
    env: {
      PATH: `${join(dir, 'bin')}:${NODE_DIR}:/usr/bin:/bin`,
      HOME: dir,
      TMPDIR: runnerTemp,
      RUNNER_TEMP: runnerTemp,
      RUSTUP_HOME: rustupHome,
      RUNNER_OS: 'Windows',
      TOOLCHAIN: 'stable',
      COMPONENTS: 'clippy rustfmt',
      TARGETS: '',
      NATIVE,
      GITHUB_ENV: ghEnv,
      ...env,
    },
    encoding: 'utf8',
    timeout: 60_000, // a lock regression fails here instead of hanging the suite
  });
  const log = existsSync(calls) ? read(calls).split('\n') : [];
  return {
    status: r.status,
    out: r.stdout + r.stderr,
    installs: log.filter((l) => l.startsWith('toolchain install')).length,
    calls: log,
    stampFile,
    lockDir,
    rustupHome,
    runnerTemp,
    githubEnv: read(ghEnv),
  };
}

const INSTALLED = { toolchains: [`stable-${HOST} (active, default)`], components: [`cargo-${HOST}`, `clippy-${HOST}`, `rustfmt-${HOST}`, `rust-std-${HOST}`, 'rust-src'] };

test('an installed toolchain with every component and target, synced today, skips the network sync', (t) => {
  const r = runToolchain(t, { ...INSTALLED, stamp: 1 });
  assert.equal(r.status, 0, r.out);
  assert.equal(r.installs, 0, `no rustup toolchain install: ${r.out}`);
  assert.match(r.out, /already installed/);
  assert.match(r.out, /rustc 1\.99\.1 \(stable\)/, 'rustc runs with the job toolchain');
  assert.match(r.githubEnv, /^RUSTUP_TOOLCHAIN=stable$/m, 'later steps use it through RUSTUP_TOOLCHAIN');
  assert.ok(!r.calls.some((c) => c.startsWith('default')), "the host's default toolchain is left alone");
  assert.ok(!existsSync(r.lockDir), 'no lock is taken when nothing syncs');
  // A component without a host suffix (rust-src) and a requested target that is installed also count as present.
  const withTarget = runToolchain(t, { ...INSTALLED, targets: ['aarch64-pc-windows-msvc'], stamp: 1, env: { COMPONENTS: 'rust-src clippy', TARGETS: 'aarch64-pc-windows-msvc' } });
  assert.equal(withTarget.installs, 0, withTarget.out);
  // ci.yml passes "clippy,rustfmt".
  const commas = runToolchain(t, { ...INSTALLED, stamp: 1, env: { COMPONENTS: 'clippy,rustfmt' } });
  assert.equal(commas.installs, 0, `comma-separated components: ${commas.out}`);
  assert.equal(runToolchain(t, { ...INSTALLED, stamp: 1, env: { COMPONENTS: 'clippy,miri' } }).installs, 1, 'a missing one among commas');
});

test('a missing toolchain, component or target, or a channel not synced for a day, runs the install and stamps the sync', (t) => {
  const none = runToolchain(t, { stamp: 1, env: { COMPONENTS: '' } });
  assert.equal(none.installs, 1, `no toolchain listed and no components: ${none.out}`);
  assert.match(none.out, /Syncing Rust stable: missing the stable toolchain/);
  assert.equal(runToolchain(t, { ...INSTALLED, stamp: 1, env: { COMPONENTS: 'clippy llvm-tools' } }).installs, 1, 'a missing component');
  assert.equal(runToolchain(t, { ...INSTALLED, stamp: 1, env: { TARGETS: 'aarch64-apple-darwin' } }).installs, 1, 'a missing target');
  // "clippy" must not match "clippy-preview" or any longer name.
  assert.equal(runToolchain(t, { toolchains: INSTALLED.toolchains, components: [`cargo-${HOST}`, `clippy-preview-${HOST}`], stamp: 1, env: { COMPONENTS: 'clippy' } }).installs, 1);
  const stale = runToolchain(t, { ...INSTALLED, stamp: 30 });
  assert.equal(stale.installs, 1, 'a channel synced 30 hours ago syncs again');
  assert.equal(stale.status, 0, stale.out);
  assert.match(stale.out, /Syncing Rust stable: last sync older than 24 h/);
  assert.ok(Date.now() - statSync(stale.stampFile).mtimeMs < 60_000, 'a successful sync refreshes the stamp');
  assert.match(stale.githubEnv, /^RUSTUP_TOOLCHAIN=stable$/m);
  assert.ok(!existsSync(stale.lockDir), 'the job removes its own lock');
  const never = runToolchain(t, { ...INSTALLED });
  assert.equal(never.installs, 1, 'no stamp yet');
  assert.match(never.out, /no sync recorded on this host yet/);
  assert.ok(existsSync(never.stampFile));
  assert.equal(runToolchain(t, { ...INSTALLED, stamp: 1, env: { PADDOCK_RUSTUP_SYNC_HOURS: '0' } }).installs, 1, '0 hours: always sync');
});

test('a toolchain name matches only itself: "nightly" is not a dated nightly or another host\'s toolchain', (t) => {
  const dated = runToolchain(t, { toolchains: [`nightly-2026-10-01-${HOST}`], components: INSTALLED.components, stamp: 1, env: { TOOLCHAIN: 'nightly', COMPONENTS: '' } });
  assert.equal(dated.installs, 1, dated.out);
  const exactDated = runToolchain(t, { toolchains: [`nightly-2026-10-01-${HOST}`], components: INSTALLED.components, stamp: 1, env: { TOOLCHAIN: 'nightly-2026-10-01', COMPONENTS: '' } });
  assert.equal(exactDated.installs, 0, exactDated.out);
  const otherHost = runToolchain(t, { toolchains: ['stable-x86_64-pc-windows-gnu'], components: INSTALLED.components, stamp: 1, env: { COMPONENTS: '' } });
  assert.equal(otherHost.installs, 1, otherHost.out);
});

test('an exact toolchain version, or the Paddock CI image, never needs a sync once installed', (t) => {
  const exact = runToolchain(t, { toolchains: [`1.99.1-${HOST}`], components: INSTALLED.components, env: { TOOLCHAIN: '1.99.1' } });
  assert.equal(exact.installs, 0, exact.out);
  const marker = join(tempDir(t, 'paddock-ci-image-'), 'paddock-ci-release');
  writeFileSync(marker, 'paddock-ci 1\n');
  const image = runToolchain(t, { ...INSTALLED, env: { PADDOCK_CI_RELEASE_FILE: marker } });
  assert.equal(image.installs, 0, `the CI image needs no stamp: ${image.out}`);
  assert.equal(runToolchain(t, { ...INSTALLED, env: { PADDOCK_CI_RELEASE_FILE: marker, COMPONENTS: 'miri' } }).installs, 1, 'but a component it lacks');
});

// Seen as a flaky skip under load: `printf "$installed" | grep -q clippy` under pipefail failed whenever grep quit at its match
// before printf had written everything (SIGPIPE), so an installed component read as missing. A list longer than a pipe buffer
// (64 KB) makes that certain, whatever the load.
test('a long component list whose match comes first still counts as installed (no SIGPIPE under pipefail)', (t) => {
  const filler = `i=0; while [ $i -lt 6000 ]; do echo "rust-docs-filler-$i-${HOST}"; i=$((i+1)); done`;
  const r = runToolchain(t, { ...INSTALLED, componentsTail: filler, stamp: 1, env: { COMPONENTS: 'cargo clippy' } });
  assert.equal(r.installs, 0, r.out);
});

const nowSecs = () => Math.floor(Date.now() / 1000);
const owner = (temp, since = nowSecs(), host = 'h') => `temp=${temp}\npid=1\nhost=${host}\nsince=${since}`;
const graves = (r) => readdirSync(r.rustupHome).filter((n) => n.startsWith('paddock-ci-sync.lock.stale.'));

test('one sync per host: a live lock is waited for, a finished job\'s lock is removed, another job\'s lock is never removed', (t) => {
  const liveTemp = tempDir(t, 'paddock-live-owner-');
  const live = owner(liveTemp);
  const waited = runToolchain(t, { ...INSTALLED, stamp: 30, lock: live, env: { PADDOCK_RUSTUP_LOCK_WAIT: '0' } });
  assert.equal(waited.status, 0, waited.out);
  assert.equal(waited.installs, 1);
  assert.match(waited.out, /::warning::the Rust sync lock .* is still held after 0s \(temp=.* pid=1 host=h since=\d+\); syncing without it/);
  assert.equal(read(join(waited.lockDir, 'owner')), live, "another job's live lock is left as it was");

  const stale = runToolchain(t, { ...INSTALLED, stamp: 30, lock: owner('/nonexistent/job-folder-gone'), env: { PADDOCK_RUSTUP_LOCK_WAIT: '5' } });
  assert.equal(stale.status, 0, stale.out);
  assert.equal(stale.installs, 1);
  assert.match(stale.out, /Removed a stale Rust sync lock: its job's folder \/nonexistent\/job-folder-gone is gone/);
  assert.ok(!existsSync(stale.lockDir), 'and the job removes its own lock after the sync');
  assert.ok(!stale.calls.includes('sleep'), 'a finished job is not waited for');
  assert.deepEqual(graves(stale), []);

  // While this job waits, the holder finishes its sync (stamp and lock): the job re-checks under the lock and skips.
  const meanwhile = runToolchain(t, {
    ...INSTALLED,
    stamp: 30,
    lock: live,
    onSleep: `touch "$RUSTUP_HOME/paddock-ci-synced-stable"; rm -rf "$RUSTUP_HOME/paddock-ci-sync.lock"`,
  });
  assert.equal(meanwhile.status, 0, meanwhile.out);
  assert.equal(meanwhile.installs, 0, meanwhile.out);
  assert.match(meanwhile.out, /synced by another job meanwhile/);
  assert.ok(!existsSync(meanwhile.lockDir), 'its own lock is removed');
  assert.match(meanwhile.githubEnv, /^RUSTUP_TOOLCHAIN=stable$/m);

  // A failed install still removes the job's own lock.
  const failed = runToolchain(t, { ...INSTALLED, stamp: 30, install: 'exit 1', env: { RUNNER_OS: 'Linux' } });
  assert.equal(failed.status, 1, failed.out);
  assert.ok(!existsSync(failed.lockDir));
});

// A runner that crashed or rebooted leaves its RUNNER_TEMP, and a job that died between mkdir and writing the owner leaves a
// lock with no owner: without an age rule every later sync on the host waited the full PADDOCK_RUSTUP_LOCK_WAIT.
test('an old lock is reclaimed by age: taken more than 2 x WAIT ago, or ownerless for more than 60 s; a young one is not', (t) => {
  const liveTemp = tempDir(t, 'paddock-survivor-');
  const aged = runToolchain(t, { ...INSTALLED, stamp: 30, lock: owner(liveTemp, nowSecs() - 1000), env: { PADDOCK_RUSTUP_LOCK_WAIT: '5' } });
  assert.equal(aged.status, 0, aged.out);
  assert.equal(aged.installs, 1);
  assert.match(aged.out, /Removed a stale Rust sync lock: it was taken \d+ s ago, more than 60 s/);
  assert.ok(!aged.calls.includes('sleep'), aged.out);
  assert.ok(!existsSync(aged.lockDir));

  const ownerless = runToolchain(t, { ...INSTALLED, stamp: 30, lock: '', lockAge: 300, env: { PADDOCK_RUSTUP_LOCK_WAIT: '5' } });
  assert.equal(ownerless.status, 0, ownerless.out);
  assert.match(ownerless.out, /Removed a stale Rust sync lock: it has no owner and is more than 60 s old/);
  assert.ok(!existsSync(ownerless.lockDir));

  const young = runToolchain(t, { ...INSTALLED, stamp: 30, lock: '', env: { PADDOCK_RUSTUP_LOCK_WAIT: '0' } });
  assert.equal(young.status, 0, young.out);
  assert.match(young.out, /still held after 0s/);
  assert.ok(existsSync(young.lockDir), 'a lock just made (its owner not written yet) is left alone');
});

// Between reading a stale lock and moving it, its owner may change (another job reclaimed it and took it again).
test('a lock that changed hands while it was reclaimed is left under its new name, never deleted or moved back', (t) => {
  const other = owner('/elsewhere/live-job', nowSecs(), 'other');
  const r = runToolchain(t, {
    ...INSTALLED,
    stamp: 30,
    lock: owner('/nonexistent/job-folder-gone'),
    env: { PADDOCK_RUSTUP_LOCK_WAIT: '5' },
    mv: `printf '%s\\n' '${other.replaceAll('\n', "' '")}' > "$1/owner"; exec /bin/mv "$@"`,
  });
  assert.equal(r.status, 0, r.out);
  assert.match(r.out, /::warning::the Rust sync lock changed hands while it was reclaimed; left as .*paddock-ci-sync\.lock\.stale\.\d+\.1 \(temp=\/elsewhere\/live-job pid=1 host=other/);
  const left = graves(r);
  assert.equal(left.length, 1, r.out);
  assert.equal(read(join(r.rustupHome, left[0], 'owner')).trim(), other, 'the other job\'s lock is kept');
  assert.equal(r.installs, 1, 'the job then takes a lock of its own and syncs');
  assert.ok(!existsSync(r.lockDir), 'and removes only its own');
});

test('a sync sets the host default only on a host without one, and only to stable or an exact version', (t) => {
  const fresh = runToolchain(t, { stamp: 30, env: { COMPONENTS: '', FAKE_NO_DEFAULT: '1' } });
  assert.ok(fresh.calls.includes('default stable'), fresh.out);
  const exact = runToolchain(t, { env: { TOOLCHAIN: '1.99.1', COMPONENTS: '', FAKE_NO_DEFAULT: '1' } });
  assert.ok(exact.calls.includes('default 1.99.1'), exact.out);
  const dated = runToolchain(t, { env: { TOOLCHAIN: 'nightly-2026-10-01', COMPONENTS: '', FAKE_NO_DEFAULT: '1' } });
  assert.equal(dated.installs, 1, dated.out);
  assert.ok(!dated.calls.some((c) => /^default \S/.test(c)), `a dated nightly never becomes the host default: ${dated.calls}`);
  const has = runToolchain(t, { stamp: 30, env: { COMPONENTS: '' } });
  assert.ok(!has.calls.some((c) => /^default \S/.test(c)), 'a host with a default keeps it');
});

test('a crashed install names the raw exit code and the last stderr lines, and is retried once; a reported error is not', (t) => {
  // A program killed by a signal stands in for a Windows NTSTATUS crash (both are crashes, not errors the program reported).
  const crashed = runToolchain(t, { install: 'kill -ABRT $$' });
  assert.equal(crashed.status, 1, crashed.out);
  assert.equal(crashed.installs, 2, 'one bounded retry after a crash');
  assert.match(crashed.out, /try 1 of 2: signal SIGABRT/);
  assert.match(crashed.out, /try 2 of 2: signal SIGABRT/);
  assert.match(crashed.out, /stderr:\n\s+info: syncing channel updates/);
  assert.match(crashed.out, /::error::rustup could not install the stable toolchain \(ended with signal SIGABRT\)/);
  assert.deepEqual(readdirSync(crashed.runnerTemp), [], 'the exit-code folder is made under TMPDIR and removed');
  const once = runToolchain(t, { install: '[ -f "$HOME/crashed" ] && exit 0; touch "$HOME/crashed"; kill -ABRT $$' });
  assert.equal(once.status, 0, once.out);
  assert.equal(once.installs, 2);
  assert.match(once.out, /::warning::rustup crashed \(signal SIGABRT\)/);
  const failed = runToolchain(t, { env: { RUNNER_OS: 'Linux' }, install: 'exit 1' });
  assert.equal(failed.status, 1, failed.out);
  assert.equal(failed.installs, 1, 'an error rustup reported is not retried');
  assert.match(failed.out, /::error::rustup could not install the stable toolchain \(ended with 1 0x00000001 exit code\)/);
});

test('run-native describes Windows crash codes in hex with their NTSTATUS name and keeps a bounded stderr tail', async () => {
  const { describeExit, isCrash, lineTail } = await import('../ci/run-native.mjs');
  assert.equal(describeExit(0xc0000409), '3221226505 0xC0000409 STATUS_STACK_BUFFER_OVERRUN (fast-fail: abort() or a Rust panic=abort)');
  assert.equal(describeExit(-1073740791), describeExit(0xc0000409), 'a signed 32-bit code is the same NTSTATUS');
  assert.equal(describeExit(0xc0000005), '3221225477 0xC0000005 STATUS_ACCESS_VIOLATION');
  assert.equal(describeExit(0xc0001234), '3221230132 0xC0001234 NTSTATUS error');
  assert.equal(describeExit(127), '127 0x0000007F exit code');
  assert.equal(describeExit(null, 'SIGSEGV'), 'signal SIGSEGV');
  assert.ok(isCrash(0xc0000409) && isCrash(null, 'SIGABRT') && !isCrash(1) && !isCrash(127));
  const tail = lineTail(3);
  tail.push('a\nb\r\nc\nd\ne');
  assert.deepEqual(tail.lines(), ['c', 'd', 'e']);
  for (let i = 0; i < 10_000; i++) tail.push(`line ${i}\n`);
  assert.deepEqual(tail.lines(), ['line 9997', 'line 9998', 'line 9999']);
});

// --- sccache ---------------------------------------------------------------------------------------------------------------

/** A fake sccache release archive for macOS arm64 whose binary reports `version`; returns its bytes. */
function fakeArchive(dir, version = '0.19.0') {
  const src = join(dir, 'archive-src');
  const inner = join(src, 'sccache-v0.19.0-aarch64-apple-darwin');
  mkdirSync(inner, { recursive: true });
  writeFileSync(join(inner, 'sccache'), `#!/bin/sh\necho sccache ${version}\n`);
  chmodSync(join(inner, 'sccache'), 0o755);
  const out = join(dir, 'archive.tar.gz');
  const r = spawnSync('tar', ['-czf', out, '-C', src, 'sccache-v0.19.0-aarch64-apple-darwin']);
  assert.equal(r.status, 0, String(r.stderr));
  return readFileSync(out);
}

/**
 * Runs the Install sccache step as on a macOS arm64 runner with a fake curl that serves `downloads` in order (Buffers, or
 * 'good'; the pinned SHA-256 is replaced by the good archive's) and records where it wrote. `mv` is shell for a fake mv that
 * replaces the real one (the rename into the tool cache), or null.
 */
function runInstall(t, { downloads, preinstalled = null, mv = null }) {
  const dir = tempDir(t, 'paddock-sccache-install-');
  const good = fakeArchive(dir);
  const sum = createHash('sha256').update(good).digest('hex');
  const queue = join(dir, 'queue');
  mkdirSync(queue);
  (downloads ?? ['good']).forEach((b, i) => writeFileSync(join(queue, String(i + 1)), b === 'good' ? good : b));
  mkdirSync(join(dir, 'bin'));
  writeFileSync(
    join(dir, 'bin', 'curl'),
    [
      '#!/bin/bash',
      'out=""; url=""',
      'while [ $# -gt 0 ]; do case "$1" in -o) out="$2"; shift 2 ;; -*) shift ;; *) url="$1"; shift ;; esac; done',
      `n=$(( $(cat '${dir}/count' 2>/dev/null || echo 0) + 1 )); echo $n > '${dir}/count'`,
      `echo "$out" >> '${dir}/written'`,
      `[ -f '${queue}'/$n ] || exit 22`,
      `cp '${queue}'/$n "$out"`,
      '',
    ].join('\n'),
  );
  chmodSync(join(dir, 'bin', 'curl'), 0o755);
  if (mv !== null) {
    writeFileSync(join(dir, 'bin', 'mv'), `#!/bin/bash\n${mv}\n`);
    chmodSync(join(dir, 'bin', 'mv'), 0o755);
  }
  const tools = join(dir, 'tools');
  const dest = join(tools, 'sccache', '0.19.0');
  if (preinstalled) {
    mkdirSync(dest, { recursive: true });
    writeFileSync(join(dest, 'sccache'), preinstalled);
    chmodSync(join(dest, 'sccache'), 0o755);
  }
  const body = stepBody('Install sccache', 'sccache\n').replace(/(T=aarch64-apple-darwin;\s+EXT=tar\.gz;\s+SUM=)[0-9a-f]{64}/, `$1${sum}`);
  assert.ok(body.includes(sum), 'the macOS arm64 pin was found and replaced');
  writeFileSync(join(dir, 'step.sh'), body);
  const ghPath = join(dir, 'github-path');
  writeFileSync(ghPath, '');
  const jobTmp = join(dir, 'job-tmp');
  mkdirSync(jobTmp);
  const r = spawnSync('bash', ['-e', '-o', 'pipefail', join(dir, 'step.sh')], {
    env: { PATH: `${join(dir, 'bin')}:/usr/bin:/bin`, HOME: dir, TMPDIR: jobTmp, RUNNER_OS: 'macOS', RUNNER_TOOL_CACHE: tools, GITHUB_PATH: ghPath, GOOD_BIN: join(dir, 'archive-src', 'sccache-v0.19.0-aarch64-apple-darwin', 'sccache') },
    encoding: 'utf8',
  });
  const count = existsSync(join(dir, 'count')) ? Number(read(join(dir, 'count'))) : 0;
  const written = existsSync(join(dir, 'written')) ? read(join(dir, 'written')).split('\n').filter(Boolean) : [];
  return { status: r.status, out: r.stdout + r.stderr, dest, sum, jobTmp, downloads: count, written, ghPath };
}

const ERROR_PAGE = Buffer.from('<html><body>502 Bad Gateway</body></html>\n');
const version = (bin) => spawnSync(bin, ['--version'], { encoding: 'utf8' }).stdout.trim();

test('a sccache download that does not match the pin is deleted and fetched once more, with both hashes in the log', (t) => {
  const r = runInstall(t, { downloads: [ERROR_PAGE, 'good'] });
  assert.equal(r.status, 0, r.out);
  assert.equal(r.downloads, 2);
  for (const w of r.written) assert.ok(w.startsWith(`${r.jobTmp}/sccache.`), `downloaded into the job's TMPDIR: ${w}`);
  const bad = createHash('sha256').update(ERROR_PAGE).digest('hex');
  assert.match(r.out, new RegExp(`download 1 of 2 .*expected SHA-256 ${r.sum}, got ${bad}`));
  assert.match(r.out, /502 Bad Gateway/, 'the first bytes show what was served instead');
  assert.equal(version(join(r.dest, 'sccache')), 'sccache 0.19.0');
  assert.deepEqual(readdirSync(r.dest), ['sccache'], 'no part file left in the shared tool cache');
  assert.deepEqual(readdirSync(r.jobTmp), [], "the job's download folder is removed");
});

test('two mismatching sccache downloads fail the step with both hashes and install nothing', (t) => {
  const r = runInstall(t, { downloads: [ERROR_PAGE, Buffer.from('truncated')] });
  assert.equal(r.status, 1, r.out);
  assert.equal(r.downloads, 2);
  for (const w of r.written) assert.ok(w.startsWith(`${r.jobTmp}/sccache.`), w);
  assert.match(r.out, new RegExp(`::error::sccache 0\\.19\\.0 checksum mismatch for aarch64-apple-darwin: expected ${r.sum}, got [0-9a-f]{64} and [0-9a-f]{64} on both downloads`));
  assert.ok(!existsSync(join(r.dest, 'sccache')), 'nothing unverified reaches the tool cache');
  assert.deepEqual(readdirSync(r.jobTmp), [], "the job's download folder is removed after a failure too");
});

test('a cached sccache that does not report the pin (a half-written or foreign copy) is replaced, a good one is kept', (t) => {
  const broken = runInstall(t, { preinstalled: '#!/bin/sh\nexit 1\n' });
  assert.equal(broken.status, 0, broken.out);
  assert.equal(broken.downloads, 1, 'replaced by a verified download');
  assert.equal(version(join(broken.dest, 'sccache')), 'sccache 0.19.0');
  const kept = runInstall(t, { preinstalled: '#!/bin/sh\necho sccache 0.19.0\n', downloads: [] });
  assert.equal(kept.status, 0, kept.out);
  assert.equal(kept.downloads, 0, 'a working copy of the pin is not downloaded again');
  assert.match(read(kept.ghPath), /sccache\/0\.19\.0/);
});

// Windows refuses to replace an .exe that is running (another job's sccache server).
test('a rename into the tool cache that fails uses the copy another job installed, or builds without the cache', (t) => {
  const other = runInstall(t, { mv: 'dst="${@: -1}"; cp "$GOOD_BIN" "$dst"; exit 1' });
  assert.equal(other.status, 0, other.out);
  assert.match(other.out, /installed in .* by another job meanwhile: using that copy/);
  assert.deepEqual(readdirSync(other.dest), ['sccache'], 'the part file is removed');
  assert.deepEqual(readdirSync(other.jobTmp), []);
  assert.match(read(other.ghPath), /sccache\/0\.19\.0/);
  const none = runInstall(t, { mv: 'exit 1' });
  assert.equal(none.status, 0, `a cache that cannot be installed never fails the job: ${none.out}`);
  assert.match(none.out, /::warning::could not put sccache 0\.19\.0 into .*; building without the compile cache/);
  assert.deepEqual(readdirSync(none.dest), [], 'no part file is left');
  assert.deepEqual(readdirSync(none.jobTmp), []);
  assert.equal(read(none.ghPath), '', 'nothing is added to PATH');
});
