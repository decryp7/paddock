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
const LANES = ['--user', USER, '--daemon', 'lane-a', '--gui', 'lane-b', '--retire', 'lane-c'];

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
function mac({ root = true, unregistered = [], silent = [], psUser = USER, program = true } = {}) {
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
  write(home, files);
  write(sys, { 'Library/LaunchDaemons/.keep': '' });
  write(bin, { launchd: ['lane-a', 'lane-b', 'lane-c'].map((n) => `gui/501/${PREFIX}${n}`).join('\n') + '\n' });
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
  return { home, sys, bin, state };
}

const move = (m, args) =>
  run('mac-runner-move.sh', args, { bin: m.bin, cwd: m.home, env: { PADDOCK_TOOLS_OS: 'Darwin', PADDOCK_RUNNER_MOVE_SYSROOT: m.sys } });
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

test('the plist templates pass plutil -lint', { skip }, () => {
  for (const t of ['runner-daemon.plist', 'runner-gui-agent.plist']) {
    const r = lint(join(TOOLS, 'mac-runners', t));
    assert.equal(r.status, 0, `${t}: ${r.stdout}${r.stderr}`);
  }
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
    `would run: launchctl bootout gui/501/${PREFIX}lane-c`,
    `would run: launchctl bootstrap gui/501 ${agentPath(m, 'lane-b')}`,
    `would run: chown root:wheel ${daemonPath(m, 'lane-a')}`,
    '"UserName" => "ci-tester"',
    '"LimitLoadToSessionType" => "Aqua"',
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

test('install: the daemon lane runs in the system domain as the user, the GUI lane stays an Aqua agent, the third is parked', { skip }, () => {
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

  const g = plist(agentPath(m, 'lane-b'));
  assert.equal(lint(agentPath(m, 'lane-b')).status, 0);
  assert.equal(g.LimitLoadToSessionType, 'Aqua');
  assert.equal(g.UserName, undefined);
  assert.equal(g.EnvironmentVariables.PADDOCK_CI_JOBS_PER_HOST, '4');
  assert.equal(g.StandardErrorPath, join(m.home, 'runners/lane-b/logs/runner.log'));

  for (const n of ['lane-a', 'lane-b', 'lane-c'])
    assert.equal(readFileSync(`${agentPath(m, n)}.before-adr0120`, 'utf8'), originals[n], `${n}: the original is kept`);
  assert.ok(!existsSync(agentPath(m, 'lane-a')), 'the daemon lane no longer has a live agent that a login would load');
  assert.ok(!existsSync(agentPath(m, 'lane-c')), 'the retired lane is parked');
  assert.deepEqual(loadedJobs(m), [`gui/501/${PREFIX}lane-b`, `system/${PREFIX}lane-a`]);
  assert.match(r.out, /ok {4}lane-a \(system\): running, pid 4242, user ci-tester; runner: lane-a, .*declare successfully/);
  assert.match(r.out, /ok {4}lane-b \(gui\/501\)/);
});

test('install twice is safe: the parked originals stay the source', { skip }, () => {
  const m = mac();
  const original = readFileSync(agentPath(m, 'lane-b'), 'utf8');
  assert.equal(move(m, ['install', ...LANES, '--verify-max', '5']).status, 0);
  const again = move(m, ['install', ...LANES, '--verify-max', '5']);
  assert.equal(again.status, 0, again.out);
  assert.equal(readFileSync(`${agentPath(m, 'lane-b')}.before-adr0120`, 'utf8'), original);
  assert.deepEqual(loadedJobs(m), [`gui/501/${PREFIX}lane-b`, `system/${PREFIX}lane-a`]);
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
  const m = mac({ unregistered: ['lane-b'] });
  const r = move(m, ['install', ...LANES, '--verify-max', '5']);
  assert.equal(r.status, 1, r.out);
  assert.match(r.out, /fail {2}lane-b \(gui\/501\): Gitea answers 'unregistered runner'/);
  assert.match(r.out, /register --no-interactive --instance https:\/\/gitea\.example\.test --name lane-b --token <token>/);
  assert.ok(!r.out.includes('invented-secret-token'));
  assert.deepEqual(loadedJobs(m), [`system/${PREFIX}lane-a`], 'the daemon lane runs; the unregistered GUI lane is booted out');
  assert.match(r.out, /rollback --user ci-tester --daemon lane-a --gui lane-b --retire lane-c/);
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
