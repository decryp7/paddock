// tools/mac-runner-move.sh (ADR 0120's runner move): every run is in a scratch "Mac": PADDOCK_RUNNER_MOVE_SYSROOT stands in
// for /, a scratch home holds the LaunchAgents, and launchctl, id, dscl, ps, chown, sudo, getconf and defaults are shims
// (launchctl keeps its loaded jobs in a file). plutil is the real one, so the rendered plists are checked by Apple's own
// parser (ER-42); these tests therefore run on macOS only.
import assert from 'node:assert/strict';
import { createHash } from 'node:crypto';
import { existsSync, readdirSync, readFileSync, statSync } from 'node:fs';
import { join } from 'node:path';
import { spawnSync } from 'node:child_process';
import { test } from 'node:test';
import { TOOLS, calls, run, scratch, shim, write } from './helpers.mjs';

const skip = process.platform === 'darwin' ? false : 'needs macOS plutil (the plists are checked by Apple’s own parser)';
const PREFIX = 'net.decryptology.gitea-runner.';
const USER = 'ci-tester';
const LANES = ['--user', USER, '--daemon', 'lane-a', '--retire', 'lane-b', '--retire', 'lane-c'];

const agent = (home, name, extraEnv = '') => `<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
	<key>EnvironmentVariables</key>
	<dict>
		<key>HOME</key><string>${home}</string>
		<key>PADDOCK_CI_JOBS_PER_HOST</key><string>3</string>
		<key>PADDOCK_CI_RESERVED_CORES</key><string>2</string>
		<key>PATH</key><string>${home}/.cargo/bin:/usr/bin:/bin</string>${extraEnv}
	</dict>
	<key>KeepAlive</key><true/>
	<key>Label</key><string>${PREFIX}${name}</string>
	<key>ProgramArguments</key>
	<array>
		<string>${home}/runners/${name}/gitea-runner</string>
		<string>daemon</string>
		<string>--config</string>
		<string>${home}/runners/${name}/config.yaml</string>
	</array>
	<key>RunAtLoad</key><true/>
	<key>StandardErrorPath</key><string>${home}/runners/${name}/logs/runner.log</string>
	<key>StandardOutPath</key><string>${home}/runners/${name}/logs/runner.log</string>
	<key>WorkingDirectory</key><string>${home}/runners/${name}</string>
</dict>
</plist>
`;

/** A scratch Mac with three runner lanes, each a loaded LaunchAgent with its runner folder. */
function mac({ root = true, unregistered = [], silent = [], psUser = USER, program = true, parkedByHand = false, signing = 'unsigned' } = {}) {
  const home = scratch('paddock-runner-home-');
  const sys = scratch('paddock-runner-sys-');
  const bin = scratch('paddock-runner-bin-');
  const state = join(bin, 'launchd');
  const files = {};
  for (const name of ['lane-a', 'lane-b', 'lane-c']) {
    files[`Library/LaunchAgents/${PREFIX}${name}.plist`] = agent(home, name);
    if (program) files[`runners/${name}/gitea-runner`] = '#!/bin/bash\nexit 0\n';
    files[`runners/${name}/.runner`] = '{"address":"https://gitea.example.test","token":"invented-secret-token"}';
    files[`runners/${name}/logs/runner.log`] = 'level=error msg="fail to invoke Declare" error="unregistered runner"\n';
  }
  if (parkedByHand)
    for (const n of ['lane-b', 'lane-c']) delete files[`Library/LaunchAgents/${PREFIX}${n}.plist`];
  write(home, files);
  write(sys, { 'Library/LaunchDaemons/.keep': '' });
  const running = parkedByHand ? ['lane-a'] : ['lane-a', 'lane-b', 'lane-c'];
  write(bin, { launchd: running.map((n) => `gui/501/${PREFIX}${n}`).join('\n') + '\n' });
  shim(bin, 'launchctl', `
state="${state}"
case "$1" in
  print) grep -qxF "$2" "$state" || { echo "Could not find service \\"$2\\"" >&2; exit 113; }
         printf '%s = {\\n\\tstate = running\\n\\tpid = 4242\\n}\\n' "$2" ;;
  bootout) grep -vxF "$2" "$state" > "$state.n"; mv "$state.n" "$state" ;;
  bootstrap)
    label=$(plutil -extract Label raw -o - "$3"); echo "$2/$label" >> "$state"
    log=$(plutil -extract StandardErrorPath raw -o - "$3"); name=\${label#${PREFIX}}
    case " ${silent.join(' ')} " in *" $name "*) exit 0 ;; esac
    case " ${unregistered.join(' ')} " in
      *" $name "*) echo 'level=error msg="fail to invoke Declare" error="unknown: rpc error: code = Unauthenticated desc = unregistered runner"' >> "$log" ;;
      *) echo "level=info msg=\\"runner: $name, with version: v5.4.1, with labels: [macos-latest macos-arm64], declare successfully\\"" >> "$log" ;;
    esac ;;
esac`);
  shim(bin, 'id', `
case "$*" in
  -u) echo ${root ? 0 : 501} ;;
  -un) echo ${root ? 'root' : USER} ;;
  "-u ${USER}") echo 501 ;;
  "-gn ${USER}") echo staff ;;
  *) exit 1 ;;
esac`);
  shim(bin, 'dscl', `echo "NFSHomeDirectory: ${home}"`);
  shim(bin, 'ps', `echo " ${psUser}"`);
  shim(bin, 'chown');
  shim(bin, 'sudo', 'shift 2; exec "$@"');
  shim(bin, 'getconf', 'echo /private/var/folders/xx/test/T/');
  shim(bin, 'defaults', 'echo 1');
  // tools/mac-ci-signing.sh: `signing` is the state of the identity and the program ('unsigned', 'signed' or 'none' for no
  // identity); sign marks the program signed in a file, as the real one changes the program.
  const signed = join(bin, 'signed');
  if (signing === 'signed') write(bin, { signed: '' });
  write(bin, {
    'signing.sh': `#!/bin/bash
echo "signing $*" >> "${bin}/calls.log"
${signing === 'none' ? 'echo "error: not measured: no identity" >&2; exit 6' : ''}
case "$1" in
  check) if [ -f "${signed}" ]; then echo "  ok    $4"; exit 0; fi; echo "  fail  $4"; echo '        has: cdhash H"00"'; exit 5 ;;
  sign) : > "${signed}"; echo "  ok    $4: signed" ;;
esac
`,
  });
  return { home, sys, bin, state, signing: join(bin, 'signing.sh') };
}

const move = (m, args) =>
  run('mac-runner-move.sh', args, {
    bin: m.bin,
    cwd: m.home,
    env: { PADDOCK_TOOLS_OS: 'Darwin', PADDOCK_RUNNER_MOVE_SYSROOT: m.sys, PADDOCK_MAC_SIGNING: m.signing },
  });
const loadedJobs = (m) => readFileSync(m.state, 'utf8').trim().split('\n').filter(Boolean).sort();
const plist = (file) => JSON.parse(spawnSync('plutil', ['-convert', 'json', '-o', '-', file], { encoding: 'utf8' }).stdout);
const lint = (file) => spawnSync('plutil', ['-lint', file], { encoding: 'utf8' });
const agentPath = (m, name) => join(m.home, 'Library/LaunchAgents', `${PREFIX}${name}.plist`);
const daemonPath = (m, name) => join(m.sys, 'Library/LaunchDaemons', `${PREFIX}${name}.plist`);

/** Every file under the folders with its content hash: what a dry run must leave as it was. */
function snapshot(...roots) {
  const out = {};
  const walk = (dir) => {
    for (const e of readdirSync(dir)) {
      const p = join(dir, e);
      if (statSync(p).isDirectory()) walk(p);
      else out[p] = createHash('sha256').update(readFileSync(p)).digest('hex');
    }
  };
  roots.forEach(walk);
  return out;
}

test('the plist template passes plutil -lint', { skip }, () => {
  const r = lint(join(TOOLS, 'mac-runners', 'runner-daemon.plist'));
  assert.equal(r.status, 0, `${r.stdout}${r.stderr}`);
});

test('a dry run changes nothing, needs no sudo and prints every step', { skip }, () => {
  const m = mac({ root: false });
  const before = snapshot(m.home, m.sys);
  const r = move(m, ['install', ...LANES, '--dry-run']);
  assert.equal(r.status, 0, r.out);
  assert.deepEqual(snapshot(m.home, m.sys), before);
  assert.ok(calls(m.bin).filter((c) => c.startsWith('launchctl')).every((c) => c.startsWith('launchctl print ')), 'only launchctl print');
  for (const s of [
    `would run: launchctl bootout gui/501/${PREFIX}lane-a`,
    `would run: launchctl bootstrap system ${daemonPath(m, 'lane-a')}`,
    `would run: launchctl bootout gui/501/${PREFIX}lane-b`,
    `would run: launchctl bootout gui/501/${PREFIX}lane-c`,
    `would run: chown root:wheel ${daemonPath(m, 'lane-a')}`,
    '"UserName" => "ci-tester"',
    'Dry run: nothing was changed.',
  ])
    assert.ok(r.out.includes(s), `missing: ${s}\n${r.out}`);
});

test('install needs sudo unless it is a dry run', { skip }, () => {
  const m = mac({ root: false });
  const r = move(m, ['install', ...LANES]);
  assert.equal(r.status, 1);
  assert.match(r.out, /need sudo/);
  assert.equal(calls(m.bin).filter((c) => /launchctl (bootout|bootstrap)/.test(c)).length, 0);
});

test('install: the daemon lane runs in the system domain as the user, the retired lanes are stopped and parked', { skip }, () => {
  const m = mac();
  const originals = Object.fromEntries(['lane-a', 'lane-b', 'lane-c'].map((n) => [n, readFileSync(agentPath(m, n), 'utf8')]));
  const r = move(m, ['install', ...LANES, '--verify-max', '5']);
  assert.equal(r.status, 0, r.out);

  const daemon = daemonPath(m, 'lane-a');
  assert.equal(lint(daemon).status, 0);
  const d = plist(daemon);
  assert.equal(d.Label, `${PREFIX}lane-a`);
  assert.equal(d.UserName, USER);
  assert.equal(d.GroupName, 'staff');
  assert.equal(d.SessionCreate, true);
  assert.equal(d.WorkingDirectory, join(m.home, 'runners/lane-a'));
  assert.deepEqual(d.ProgramArguments, [join(m.home, 'runners/lane-a/gitea-runner'), 'daemon', '--config', join(m.home, 'runners/lane-a/config.yaml')]);
  assert.equal(d.EnvironmentVariables.PADDOCK_CI_JOBS_PER_HOST, '4');
  assert.equal(d.EnvironmentVariables.PADDOCK_CI_RESERVED_CORES, '2');
  assert.equal(d.EnvironmentVariables.HOME, m.home);
  assert.equal(d.EnvironmentVariables.USER, USER);
  assert.equal(d.StandardErrorPath, join(m.sys, 'Library/Logs/gitea-runner/lane-a.log'));
  assert.ok(!JSON.stringify(d).includes('@set-by-mac-runner-move@'));
  assert.ok(calls(m.bin).includes(`chown root:wheel ${daemon}`));

  for (const n of ['lane-a', 'lane-b', 'lane-c'])
    assert.equal(readFileSync(`${agentPath(m, n)}.before-adr0120`, 'utf8'), originals[n], `${n}: the original is kept`);
  assert.ok(!existsSync(agentPath(m, 'lane-a')), 'the daemon lane no longer has a live agent that a login would load');
  for (const n of ['lane-b', 'lane-c']) assert.ok(!existsSync(agentPath(m, n)), `${n}: the retired lane is parked`);
  assert.deepEqual(loadedJobs(m), [`system/${PREFIX}lane-a`]);
  assert.match(r.out, /ok {4}lane-a \(system\): running, pid 4242, user ci-tester; runner: lane-a, .*declare successfully/);
});

test('install twice is safe: the parked originals stay the source', { skip }, () => {
  const m = mac();
  const original = readFileSync(agentPath(m, 'lane-a'), 'utf8');
  assert.equal(move(m, ['install', ...LANES, '--verify-max', '5']).status, 0);
  const again = move(m, ['install', ...LANES, '--verify-max', '5']);
  assert.equal(again.status, 0, again.out);
  assert.equal(readFileSync(`${agentPath(m, 'lane-a')}.before-adr0120`, 'utf8'), original);
  assert.deepEqual(loadedJobs(m), [`system/${PREFIX}lane-a`]);
});

test('rollback puts back the exact LaunchAgents and removes the daemon', { skip }, () => {
  const m = mac();
  const originals = Object.fromEntries(['lane-a', 'lane-b', 'lane-c'].map((n) => [n, readFileSync(agentPath(m, n), 'utf8')]));
  assert.equal(move(m, ['install', ...LANES, '--verify-max', '5']).status, 0);

  const dry = move(m, ['rollback', ...LANES, '--dry-run']);
  assert.equal(dry.status, 0, dry.out);
  assert.match(dry.out, new RegExp(`would run: launchctl bootout system/${PREFIX}lane-a`));
  assert.ok(existsSync(daemonPath(m, 'lane-a')), 'the dry run left the daemon');

  const r = move(m, ['rollback', ...LANES, '--verify-max', '5']);
  assert.equal(r.status, 0, r.out);
  assert.ok(!existsSync(daemonPath(m, 'lane-a')));
  for (const n of ['lane-a', 'lane-b', 'lane-c']) {
    assert.equal(readFileSync(agentPath(m, n), 'utf8'), originals[n], n);
    assert.ok(!existsSync(`${agentPath(m, n)}.before-adr0120`));
  }
  assert.deepEqual(loadedJobs(m), ['lane-a', 'lane-b', 'lane-c'].map((n) => `gui/501/${PREFIX}${n}`));
});

test('a lane Gitea no longer knows is stopped again, with the register command and never the token', { skip }, () => {
  const m = mac({ unregistered: ['lane-a'] });
  const r = move(m, ['install', ...LANES, '--verify-max', '5']);
  assert.equal(r.status, 1, r.out);
  assert.match(r.out, /fail {2}lane-a \(system\): Gitea answers 'unregistered runner'/);
  assert.match(r.out, /register --no-interactive --instance https:\/\/gitea\.example\.test --name lane-a --token <token>/);
  assert.ok(!r.out.includes('invented-secret-token'));
  assert.deepEqual(loadedJobs(m), [], 'the refused daemon is booted out, so it does not restart in a loop');
  assert.match(r.out, /rollback --user ci-tester --daemon lane-a --retire lane-b --retire lane-c/);
});

test('retire accepts lanes already stopped and parked by hand elsewhere; rollback leaves them there', { skip }, () => {
  const m = mac({ parkedByHand: true });
  const r = move(m, ['install', ...LANES, '--verify-max', '5']);
  assert.equal(r.status, 0, r.out);
  assert.match(r.out, /ok {4}lane-b: already stopped and parked outside/);
  assert.match(r.out, /- lane-c: retired already \(not loaded, no agent in /);
  assert.equal(calls(m.bin).filter((c) => / bootout gui\/501\/.*lane-[bc]/.test(c)).length, 0);
  assert.deepEqual(loadedJobs(m), [`system/${PREFIX}lane-a`]);
  const back = move(m, ['rollback', ...LANES, '--verify-max', '5']);
  assert.equal(back.status, 0, back.out);
  assert.deepEqual(loadedJobs(m), [`gui/501/${PREFIX}lane-a`]);
  for (const n of ['lane-b', 'lane-c']) assert.ok(!existsSync(agentPath(m, n)), `${n} stays parked`);
});

test('verify sees its signal: a lane that never declares itself fails at --verify-max (positive control, ER-33)', { skip }, () => {
  const m = mac({ silent: ['lane-a'] });
  const r = move(m, ['install', '--user', USER, '--daemon', 'lane-a', '--verify-max', '1']);
  assert.equal(r.status, 1, r.out);
  assert.match(r.out, /fail {2}lane-a \(system\): no 'declare successfully' in .*lane-a\.log within 1s/);
});

test('verify checks the process user: a daemon running as someone else fails', { skip }, () => {
  const m = mac({ psUser: 'root' });
  const r = move(m, ['install', '--user', USER, '--daemon', 'lane-a', '--verify-max', '5']);
  assert.equal(r.status, 1, r.out);
  assert.match(r.out, /fail {2}lane-a \(system\): runs as 'root', not 'ci-tester'/);
});

test('a preflight problem changes nothing', { skip }, () => {
  const m = mac({ program: false });
  const before = snapshot(m.home, m.sys);
  const r = move(m, ['install', ...LANES]);
  assert.equal(r.status, 1, r.out);
  assert.match(r.out, /fail {2}lane-a: the runner program .* is missing or not executable/);
  assert.match(r.out, /nothing was changed/);
  assert.deepEqual(snapshot(m.home, m.sys), before);
  assert.equal(calls(m.bin).filter((c) => /launchctl (bootout|bootstrap)/.test(c)).length, 0);
});

test('status is read-only and names where each lane runs', { skip }, () => {
  const m = mac();
  const before = snapshot(m.home, m.sys);
  const r = move(m, ['status', ...LANES]);
  assert.equal(r.status, 0, r.out);
  assert.match(r.out, /- lane-a\n {4}agent plist .*\n {4}loaded in {5}gui\/501: running, pid 4242, user ci-tester/);
  assert.deepEqual(snapshot(m.home, m.sys), before);
});

test('the daemon names no throttle lock folder: throttle decides its own, and ./dev is not asked for one', { skip }, () => {
  const m = mac();
  const r = move(m, ['install', ...LANES]);
  assert.equal(r.status, 0, r.out);
  const env = plist(daemonPath(m, 'lane-a')).EnvironmentVariables;
  assert.equal(env.THROTTLE_DIR, undefined);
  assert.equal(env.HEAVY_DIR, undefined);
  assert.ok(!calls(m.bin).some((c) => c.startsWith('dev ')), calls(m.bin).join('\n'));
});

test('install signs the runner with the Mac CI identity after the old one stopped and before the daemon starts', { skip }, () => {
  const m = mac();
  const r = move(m, ['install', ...LANES, '--verify-max', '5']);
  assert.equal(r.status, 0, r.out);
  const program = join(m.home, 'runners/lane-a/gitea-runner');
  const log = calls(m.bin);
  const signAt = log.indexOf(`signing sign --identifier net.decryptology.gitea-runner ${program}`);
  const bootoutAt = log.indexOf(`launchctl bootout gui/501/${PREFIX}lane-a`);
  const bootstrapAt = log.findIndex((c) => c.startsWith('launchctl bootstrap system '));
  assert.ok(signAt > bootoutAt && signAt < bootstrapAt, `sign between the stop and the start:\n${log.join('\n')}`);
  assert.ok(!log.some((c) => c.startsWith('signing sign') && c.includes('lane-b')), 'retired lanes are not signed');
  assert.match(r.out, /ok {4}lane-a: .* will be signed with the Mac CI identity/);
});

test('a runner that carries the identity is checked, not signed again', { skip }, () => {
  const m = mac({ signing: 'signed' });
  const r = move(m, ['install', ...LANES, '--verify-max', '5']);
  assert.equal(r.status, 0, r.out);
  assert.match(r.out, /ok {4}lane-a: .* carries the Mac CI identity/);
});

test('no identity fails the preflight with the create command, and changes nothing (ER-33: not measured is no pass)', { skip }, () => {
  const m = mac({ signing: 'none' });
  const before = snapshot(m.home, m.sys);
  const r = move(m, ['install', ...LANES]);
  assert.equal(r.status, 1, r.out);
  assert.match(r.out, /fail {2}lane-a: no Mac CI signing identity for ci-tester/);
  assert.match(r.out, /mac-ci-signing\.sh create|signing\.sh create/);
  assert.deepEqual(snapshot(m.home, m.sys), before);
  assert.equal(calls(m.bin).filter((c) => /launchctl (bootout|bootstrap)/.test(c)).length, 0);
});

test('--no-sign installs without signing, and says what that costs', { skip }, () => {
  const m = mac({ signing: 'none' });
  const r = move(m, ['install', ...LANES, '--verify-max', '5', '--no-sign']);
  assert.equal(r.status, 0, r.out);
  assert.match(r.out, /warn {2}lane-a: --no-sign/);
  assert.ok(!calls(m.bin).some((c) => c.startsWith('signing sign')));
});

test('status says whether the runner carries the identity', { skip }, () => {
  const m = mac();
  assert.match(move(m, ['status', ...LANES]).out, /- lane-a\n(.*\n)* {4}signature {5}NOT the Mac CI identity: has: cdhash/);
  const s = mac({ signing: 'signed' });
  assert.match(move(s, ['status', ...LANES]).out, /signature {5}the Mac CI identity \(net\.decryptology\.gitea-runner\)/);
});
