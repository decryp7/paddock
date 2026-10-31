// setup-rust's two download steps, run for real with fake programs on PATH: the Rust toolchain step (skip the network sync
// when everything is installed; name a crash's raw exit code) and the sccache install (verify in the job's own folder, fetch
// once more on a mismatch, log both hashes, never keep a copy that does not report the pin).
//
// Run 7267 (job 24328): on the Windows host `rustup toolchain install stable` ended 0.5 s into "syncing channel updates" with
// exit 127 and no message. Git Bash maps any NTSTATUS crash it has no signal for to 127, so the code was lost.
import assert from 'node:assert/strict';
import { spawnSync } from 'node:child_process';
import { createHash } from 'node:crypto';
import { chmodSync, existsSync, mkdirSync, mkdtempSync, readdirSync, readFileSync, statSync, utimesSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { dirname, join } from 'node:path';
import { test } from 'node:test';
import { PADDOCK } from './helpers.mjs';

const ACTION = join(PADDOCK, '.gitea', 'actions', 'setup-rust');
const NATIVE = join(PADDOCK, 'conformance', 'ci', 'run-native.mjs');
const NODE_DIR = dirname(process.execPath);
const read = (p) => readFileSync(p, 'utf8');

/** The body of one run step of setup-rust, unindented, with the action path filled in. */
function stepBody(name, next) {
  const text = read(join(ACTION, 'action.yml'));
  const at = text.indexOf(`- name: ${name}\n`);
  const block = text.slice(at, text.indexOf(`- name: ${next}`, at));
  const body = block.slice(block.indexOf('run: |\n') + 'run: |\n'.length).split('\n').map((l) => l.slice(8)).join('\n');
  return body.replaceAll('${{ github.action_path }}', ACTION);
}

/**
 * Runs the Rust toolchain step with a fake rustup. `install` is the shell the fake runs for `toolchain install` (after it
 * prints rustup's first line to stderr); `stamp` is the age in hours of the last-sync stamp, or null for none.
 */
function runToolchain({ env = {}, toolchains = [], components = [], componentsTail = 'true', targets = [], install = 'exit 0', stamp = null } = {}) {
  const dir = mkdtempSync(join(tmpdir(), 'paddock-rustup-skip-'));
  mkdirSync(join(dir, 'bin'));
  const calls = join(dir, 'calls');
  const list = (v) => v.map((l) => `echo '${l}'`).join('; ') || 'true';
  writeFileSync(
    join(dir, 'bin', 'rustup'),
    [
      '#!/bin/bash',
      `echo "$*" >> '${calls}'`,
      'case "$1 $2" in',
      `  "toolchain list") ${list(toolchains)} ;;`,
      `  "component list") ${list(components)}; ${componentsTail} ;;`,
      `  "target list") ${list(targets)} ;;`,
      `  "toolchain install") echo "info: syncing channel updates for stable-x86_64-pc-windows-msvc" >&2; ${install} ;;`,
      'esac',
      'exit 0',
      '',
    ].join('\n'),
  );
  for (const tool of ['rustc', 'cargo']) writeFileSync(join(dir, 'bin', tool), `#!/bin/sh\necho ${tool} 1.99.1\n`);
  for (const f of ['rustup', 'rustc', 'cargo']) chmodSync(join(dir, 'bin', f), 0o755);
  const rustupHome = join(dir, 'rustup-home');
  mkdirSync(rustupHome);
  const stampFile = join(rustupHome, `paddock-ci-synced-${env.TOOLCHAIN ?? 'stable'}`);
  if (stamp !== null) {
    writeFileSync(stampFile, '');
    const t = Date.now() / 1000 - stamp * 3600;
    utimesSync(stampFile, t, t);
  }
  writeFileSync(join(dir, 'step.sh'), stepBody('Rust toolchain', 'Install sccache'));
  const r = spawnSync('bash', ['-e', '-o', 'pipefail', join(dir, 'step.sh')], {
    env: {
      PATH: `${join(dir, 'bin')}:${NODE_DIR}:/usr/bin:/bin`,
      HOME: dir,
      TMPDIR: dir,
      RUSTUP_HOME: rustupHome,
      RUNNER_OS: 'Windows',
      TOOLCHAIN: 'stable',
      COMPONENTS: 'clippy rustfmt',
      TARGETS: '',
      NATIVE,
      ...env,
    },
    encoding: 'utf8',
  });
  const installs = (existsSync(calls) ? read(calls) : '').split('\n').filter((l) => l.startsWith('toolchain install')).length;
  return { status: r.status, out: r.stdout + r.stderr, installs, stampFile };
}

const HOST = 'x86_64-pc-windows-msvc';
const INSTALLED = { toolchains: [`stable-${HOST} (active, default)`], components: [`cargo-${HOST}`, `clippy-${HOST}`, `rustfmt-${HOST}`, `rust-std-${HOST}`, 'rust-src'] };

test('an installed toolchain with every component and target, synced today, skips the network sync', () => {
  const r = runToolchain({ ...INSTALLED, stamp: 1 });
  assert.equal(r.status, 0, r.out);
  assert.equal(r.installs, 0, `no rustup toolchain install: ${r.out}`);
  assert.match(r.out, /already installed/);
  assert.match(r.out, /rustc 1\.99\.1/);
  // A component without a host suffix (rust-src) and a requested target that is installed also count as present.
  const withTarget = runToolchain({ ...INSTALLED, targets: ['aarch64-pc-windows-msvc'], stamp: 1, env: { COMPONENTS: 'rust-src clippy', TARGETS: 'aarch64-pc-windows-msvc' } });
  assert.equal(withTarget.installs, 0, withTarget.out);
});

test('a missing toolchain, component or target, or a channel not synced for a day, runs the install and stamps the sync', () => {
  assert.equal(runToolchain({ stamp: 1 }).installs, 1, 'no toolchain');
  assert.equal(runToolchain({ ...INSTALLED, stamp: 1, env: { COMPONENTS: 'clippy llvm-tools' } }).installs, 1, 'a missing component');
  assert.equal(runToolchain({ ...INSTALLED, stamp: 1, env: { TARGETS: 'aarch64-apple-darwin' } }).installs, 1, 'a missing target');
  // "clippy" must not match "clippy-preview" or any longer name.
  assert.equal(runToolchain({ toolchains: INSTALLED.toolchains, components: [`clippy-preview-${HOST}`], stamp: 1, env: { COMPONENTS: 'clippy' } }).installs, 1);
  const stale = runToolchain({ ...INSTALLED, stamp: 30 });
  assert.equal(stale.installs, 1, 'a channel synced 30 hours ago syncs again');
  assert.equal(stale.status, 0, stale.out);
  assert.ok(Date.now() - statSync(stale.stampFile).mtimeMs < 60_000, 'a successful sync refreshes the stamp');
  const never = runToolchain({ ...INSTALLED });
  assert.equal(never.installs, 1, 'no stamp yet');
  assert.ok(existsSync(never.stampFile));
  assert.equal(runToolchain({ ...INSTALLED, stamp: 1, env: { PADDOCK_RUSTUP_SYNC_HOURS: '0' } }).installs, 1, '0 hours: always sync');
});

// Seen as a flaky skip under load: `printf "$installed" | grep -q clippy` under pipefail failed whenever grep quit at its match
// before printf had written everything (SIGPIPE), so an installed component read as missing. A list longer than a pipe buffer
// (64 KB) makes that certain, whatever the load.
test('a long component list whose match comes first still counts as installed (no SIGPIPE under pipefail)', () => {
  const filler = `i=0; while [ $i -lt 6000 ]; do echo "rust-docs-filler-$i-${HOST}"; i=$((i+1)); done`;
  const r = runToolchain({ ...INSTALLED, componentsTail: filler, stamp: 1, env: { COMPONENTS: 'cargo clippy' } });
  assert.equal(r.installs, 0, r.out);
});

test('an exact toolchain version, or the Paddock CI image, never needs a sync once installed', () => {
  const exact = runToolchain({ toolchains: [`1.99.1-${HOST}`], components: INSTALLED.components, env: { TOOLCHAIN: '1.99.1' } });
  assert.equal(exact.installs, 0, exact.out);
  const marker = join(mkdtempSync(join(tmpdir(), 'paddock-ci-image-')), 'paddock-ci-release');
  writeFileSync(marker, 'paddock-ci 1\n');
  const image = runToolchain({ ...INSTALLED, env: { PADDOCK_CI_RELEASE_FILE: marker } });
  assert.equal(image.installs, 0, `the CI image needs no stamp: ${image.out}`);
  assert.equal(runToolchain({ ...INSTALLED, env: { PADDOCK_CI_RELEASE_FILE: marker, COMPONENTS: 'miri' } }).installs, 1, 'but a component it lacks');
});

test('a crashed install names the raw exit code and the last stderr lines, and is retried once; a reported error is not', () => {
  // A program killed by a signal stands in for a Windows NTSTATUS crash (both are crashes, not errors the program reported).
  const crashed = runToolchain({ install: 'kill -ABRT $$' });
  assert.equal(crashed.status, 1, crashed.out);
  assert.equal(crashed.installs, 2, 'one bounded retry after a crash');
  assert.match(crashed.out, /try 1 of 2: signal SIGABRT/);
  assert.match(crashed.out, /try 2 of 2: signal SIGABRT/);
  assert.match(crashed.out, /stderr:\n\s+info: syncing channel updates/);
  assert.match(crashed.out, /::error::rustup could not install the stable toolchain \(ended with signal SIGABRT\)/);
  const once = runToolchain({ install: '[ -f "$HOME/crashed" ] && exit 0; touch "$HOME/crashed"; kill -ABRT $$' });
  assert.equal(once.status, 0, once.out);
  assert.equal(once.installs, 2);
  assert.match(once.out, /::warning::rustup crashed \(signal SIGABRT\)/);
  const failed = runToolchain({ env: { RUNNER_OS: 'Linux' }, install: 'exit 1' });
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
  const t = lineTail(3);
  t.push('a\nb\r\nc\nd\ne');
  assert.deepEqual(t.lines(), ['c', 'd', 'e']);
  for (let i = 0; i < 10_000; i++) t.push(`line ${i}\n`);
  assert.deepEqual(t.lines(), ['line 9997', 'line 9998', 'line 9999']);
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
 * Runs the Install sccache step as on a macOS arm64 runner with a fake curl that serves `downloads` in order (Buffers; the
 * pinned SHA-256 is replaced by the good archive's). Returns the result, the tool cache and the job's temp folder.
 */
function runInstall({ downloads, preinstalled = null }) {
  const dir = mkdtempSync(join(tmpdir(), 'paddock-sccache-install-'));
  const good = fakeArchive(dir);
  const sum = createHash('sha256').update(good).digest('hex');
  const queue = join(dir, 'queue');
  mkdirSync(queue);
  (downloads ?? [good]).forEach((b, i) => writeFileSync(join(queue, String(i + 1)), b === 'good' ? good : b));
  mkdirSync(join(dir, 'bin'));
  writeFileSync(
    join(dir, 'bin', 'curl'),
    [
      '#!/bin/bash',
      'out=""; url=""',
      'while [ $# -gt 0 ]; do case "$1" in -o) out="$2"; shift 2 ;; -*) shift ;; *) url="$1"; shift ;; esac; done',
      `n=$(( $(cat '${dir}/count' 2>/dev/null || echo 0) + 1 )); echo $n > '${dir}/count'`,
      `[ -f '${queue}'/$n ] || exit 22`,
      `cp '${queue}'/$n "$out"`,
      '',
    ].join('\n'),
  );
  chmodSync(join(dir, 'bin', 'curl'), 0o755);
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
    env: { PATH: `${join(dir, 'bin')}:/usr/bin:/bin`, HOME: dir, TMPDIR: jobTmp, RUNNER_OS: 'macOS', RUNNER_TOOL_CACHE: tools, GITHUB_PATH: ghPath },
    encoding: 'utf8',
  });
  const count = existsSync(join(dir, 'count')) ? Number(read(join(dir, 'count'))) : 0;
  return { status: r.status, out: r.stdout + r.stderr, dest, sum, jobTmp, downloads: count, ghPath };
}

const ERROR_PAGE = Buffer.from('<html><body>502 Bad Gateway</body></html>\n');

test('a sccache download that does not match the pin is deleted and fetched once more, with both hashes in the log', () => {
  const r = runInstall({ downloads: [ERROR_PAGE, 'good'] });
  assert.equal(r.status, 0, r.out);
  assert.equal(r.downloads, 2);
  const bad = createHash('sha256').update(ERROR_PAGE).digest('hex');
  assert.match(r.out, new RegExp(`download 1 of 2 .*expected SHA-256 ${r.sum}, got ${bad}`));
  assert.match(r.out, /502 Bad Gateway/, 'the first bytes show what was served instead');
  assert.equal(spawnSync(join(r.dest, 'sccache'), ['--version'], { encoding: 'utf8' }).stdout.trim(), 'sccache 0.19.0');
  assert.deepEqual(readdirSync(r.dest), ['sccache'], 'no part file left in the shared tool cache');
  assert.deepEqual(readdirSync(r.jobTmp), [], "the job's download folder is removed");
});

test('two mismatching sccache downloads fail the step with both hashes and install nothing', () => {
  const r = runInstall({ downloads: [ERROR_PAGE, Buffer.from('truncated')] });
  assert.equal(r.status, 1, r.out);
  assert.equal(r.downloads, 2);
  assert.match(r.out, new RegExp(`::error::sccache 0\\.19\\.0 checksum mismatch for aarch64-apple-darwin: expected ${r.sum}, got [0-9a-f]{64} and [0-9a-f]{64} on both downloads`));
  assert.ok(!existsSync(join(r.dest, 'sccache')), 'nothing unverified reaches the tool cache');
  assert.deepEqual(readdirSync(r.jobTmp), []);
});

test('a cached sccache that does not report the pin (a half-written or foreign copy) is replaced, a good one is kept', () => {
  const broken = runInstall({ preinstalled: '#!/bin/sh\nexit 1\n' });
  assert.equal(broken.status, 0, broken.out);
  assert.equal(broken.downloads, 1, 'replaced by a verified download');
  assert.equal(spawnSync(join(broken.dest, 'sccache'), ['--version'], { encoding: 'utf8' }).stdout.trim(), 'sccache 0.19.0');
  const kept = runInstall({ preinstalled: '#!/bin/sh\necho sccache 0.19.0\n', downloads: [] });
  assert.equal(kept.status, 0, kept.out);
  assert.equal(kept.downloads, 0, 'a working copy of the pin is not downloaded again');
  assert.match(read(kept.ghPath), /sccache\/0\.19\.0/);
});
