// tools/mac-runner-move.sh (ADR 0120's runner move): every run is in a scratch "Mac": PADDOCK_RUNNER_MOVE_SYSROOT stands in
// for /, a scratch home holds the LaunchAgents, and launchctl, id, dscl, ps, chown, sudo, getconf and defaults are shims
// (launchctl keeps its loaded jobs in a file). plutil is the real one, so the rendered plists are checked by Apple's own
// parser (ER-42); these tests therefore run on macOS only.
import assert from 'node:assert/strict';
import { createHash } from 'node:crypto';
import { existsSync, readdirSync, readFileSync, statSync, writeFileSync } from 'node:fs';
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
function mac({ root = true, unregistered = [], silent = [], psUser = USER, program = true, parkedByHand = false, signing = 'unsigned', jobPolls = 0, jobScript = null, bootstrapFails = [], notRunning = [] } = {}) {
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
         st=running; case " ${notRunning.join(' ')} " in *" \${2##*.} "*) st='spawn scheduled' ;; esac
         printf '%s = {\\n\\tstate = %s\\n\\tpid = 4242\\n}\\n' "$2" "$st" ;;
  kill) : > "$state.draining" ;;
  bootout) grep -vxF "$2" "$state" > "$state.n"; mv "$state.n" "$state" ;;
  bootstrap)
    label=$(plutil -extract Label raw -o - "$3"); name=\${label#${PREFIX}}
    case " ${bootstrapFails.join(' ')} " in *" $name "*) echo "Bootstrap failed: 5: Input/output error" >&2; exit 5 ;; esac
    echo "$2/$label" >> "$state"
    log=$(plutil -extract StandardErrorPath raw -o - "$3")
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
  // `ps -axo pid=,ppid=,command=` is the process table: the lane's runner (pid 4242) and, while the job's polls last, a job's
  // processes under it (each read of the table is one poll, so a job "ends" after N reads, never after a time). Any other call asks
  // for a process user.
  // `jobScript` ('1,1,0,1,0'): one answer per read of the table instead, 1 a job under the runner and 0 none, then none for ever.
  write(bin, { jobpolls: `${jobPolls}\n`, jobscript: jobScript ? jobScript.split(',').join('\n') + '\n' : '' });
  shim(bin, 'ps', `case "$*" in
  *-axo*)
    echo "    1     0 /sbin/launchd"
    echo " 4242     1 ${home}/runners/lane-a/gitea-runner daemon --config ${home}/runners/lane-a/config.yaml"
    if [ -s "${bin}/jobscript" ]; then
      n=$(head -1 "${bin}/jobscript"); tail -n +2 "${bin}/jobscript" > "${bin}/jobscript.n"; mv "${bin}/jobscript.n" "${bin}/jobscript"
    else
      n=$(cat "${bin}/jobpolls"); [ "$n" -le 0 ] || echo "$((n - 1))" > "${bin}/jobpolls"
    fi
    if [ "$n" -gt 0 ]; then echo " 5001  4242 /bin/bash -e /tmp/act/step.sh"; echo " 5002  5001 ./dev ci run crates"; fi ;;
  *) echo " ${psUser}" ;;
esac`);
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
    env: { PADDOCK_TOOLS_OS: 'Darwin', PADDOCK_RUNNER_MOVE_SYSROOT: m.sys, PADDOCK_MAC_SIGNING: m.signing, PADDOCK_RUNNER_MOVE_POLL: '0.05', PADDOCK_RUNNER_MOVE_REPORT: '0' },
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

// ---- install waits, visibly and bounded, for the job a runner is running (ADR 0120; the 2026-11-26 install that stopped a running job) ----

/** The runner's log with the task it logged when it took the job: the line the wait prints as "which job". */
const taskLine = 'level=info msg="task 9001 repo is decryp7/paddock https://gitea.example.test"\n';

test('install while a job runs: the runner takes no new job, install waits and says which job, then stops, swaps and verifies', { skip }, () => {
  const m = mac({ jobPolls: 4 });
  writeFileSync(join(m.home, 'runners/lane-a/logs/runner.log'), taskLine);
  const r = move(m, ['install', ...LANES, '--wait-max', '60']);
  assert.equal(r.status, 0, r.out);
  assert.match(r.out, /lane-a: a job is running/);
  assert.match(r.out, /task 9001 repo is decryp7\/paddock/, 'it names the job from the runner log');
  assert.match(r.out, /\.\/dev ci run crates/, 'and the processes of the job');
  assert.match(r.out, /waiting for the job on lane-a/);
  assert.match(r.out, /all jobs have finished/);
  const log = calls(m.bin).filter((l) => l.startsWith('launchctl '));
  const at = (re) => log.findIndex((l) => re.test(l));
  const kill = at(/^launchctl kill SIGTERM gui\/501\/net\.decryptology\.gitea-runner\.lane-a$/);
  assert.ok(kill >= 0, log.join('\n'));
  assert.ok(kill < at(/^launchctl bootout gui\/501\/.*lane-a$/), 'new jobs stop before anything is booted out');
  assert.ok(at(/^launchctl bootout gui\/501\/.*lane-a$/) < at(/^launchctl bootstrap system /), 'then the daemon starts');
  assert.deepEqual(loadedJobs(m), [`system/${PREFIX}lane-a`], 'the daemon runs; the agents are gone');
  assert.ok(existsSync(daemonPath(m, 'lane-a')));
  assert.match(r.out, /ok\s+lane-a \(system\): running, pid 4242/);
});

test('install does not wait when no job runs (positive control: the same lanes without a job)', { skip }, () => {
  const m = mac();
  const r = move(m, ['install', ...LANES]);
  assert.equal(r.status, 0, r.out);
  assert.match(r.out, /No job is running/);
  assert.doesNotMatch(r.out, /waiting for the job/);
  assert.equal(calls(m.bin).filter((l) => l.startsWith('launchctl kill SIGTERM')).length, 3, 'every loaded lane is told to take no new job, idle or not');
});

test('install that waits past --wait-max changes nothing, ends nothing and says so; the old daemon keeps running', { skip }, () => {
  const m = mac({ jobPolls: 1_000_000 });
  const before = snapshot(m.home, m.sys);
  const r = move(m, ['install', ...LANES, '--wait-max', '1']);
  assert.notEqual(r.status, 0, r.out);
  assert.match(r.out, /error: lane-a still runs a job after 1 s/);
  assert.match(r.out, /nothing was changed/);
  assert.match(r.out, /next: .*--wait-max/);
  assert.deepEqual(snapshot(m.home, m.sys), before, 'no file changed');
  assert.deepEqual(
    loadedJobs(m),
    ['lane-a', 'lane-b', 'lane-c'].map((n) => `gui/501/${PREFIX}${n}`),
    'every agent is still loaded, none was booted out'
  );
  const log = calls(m.bin).filter((l) => l.startsWith('launchctl '));
  assert.ok(!log.some((l) => /bootout|bootstrap/.test(l)), log.join('\n'));
  assert.ok(!calls(m.bin).some((l) => /^(kill|pkill|killall) /.test(l)), 'no process was killed, by pid or by name (ER-51)');
});

test('a dry run with a job running says it would wait and signals nothing', { skip }, () => {
  const m = mac({ jobPolls: 1_000_000 });
  const r = move(m, ['install', ...LANES, '--dry-run']);
  assert.equal(r.status, 0, r.out);
  assert.match(r.out, /would wait for the job on lane-a/);
  assert.ok(!calls(m.bin).some((l) => l.startsWith('launchctl kill')), 'dry run signals nothing');
});

test('a daemon that does not start is a failure: launchctl bootstrap refusing, and a state that is not running', { skip }, () => {
  const refused = move(mac({ bootstrapFails: ['lane-a'] }), ['install', ...LANES]);
  assert.notEqual(refused.status, 0, refused.out);
  assert.match(refused.out, /fail\s+lane-a \(system\): launchctl bootstrap failed/);
  assert.match(refused.out, /next: .*launchctl print system\//);
  const waiting = move(mac({ notRunning: ['lane-a'] }), ['install', ...LANES]);
  assert.notEqual(waiting.status, 0, waiting.out);
  assert.match(waiting.out, /fail\s+lane-a \(system\): .*state is 'spawn scheduled', not running/);
  // Positive control (ER-33): the same lanes with a daemon that starts pass.
  assert.equal(move(mac(), ['install', ...LANES]).status, 0);
});

test('a job that starts in the window between the end of the wait and the boot-out is seen and waited for, never booted out under', { skip }, () => {
  // Only lane-a is loaded. The table's answers: a job, the job still there, none (the wait is over), a NEW job (read by the check
  // just before the boot-out), still there, none, none (the check passes and the lane is stopped).
  const m = mac({ parkedByHand: true, jobScript: '1,1,0,1,1,0,0' });
  const r = move(m, ['install', '--user', USER, '--daemon', 'lane-a', '--wait-max', '60']);
  assert.equal(r.status, 0, r.out);
  assert.match(r.out, /a job started on lane-a before it could be stopped; waiting for it too/);
  assert.equal(readFileSync(join(m.bin, 'jobscript'), 'utf8'), '', 'every answer was read: the check ran again after the new job');
  const log = calls(m.bin).filter((l) => l.startsWith('launchctl '));
  assert.equal(log.filter((l) => l.startsWith('launchctl kill SIGTERM')).length, 2, 'the runner is told again, in case launchd had restarted it');
  assert.equal(log.filter((l) => /^launchctl bootout gui\/501\/.*lane-a$/.test(l)).length, 1, 'one boot-out, after the last job ended');
  assert.ok(r.out.indexOf('a job started on lane-a') < r.out.indexOf('+ launchctl bootout'), 'the boot-out came after the new job was waited for');
  assert.deepEqual(loadedJobs(m), [`system/${PREFIX}lane-a`]);
  // Positive control (ER-33): the same lane with no new job in the window is stopped without the second wait.
  const calm = mac({ parkedByHand: true, jobScript: '1,1,0,0' });
  const c = move(calm, ['install', '--user', USER, '--daemon', 'lane-a', '--wait-max', '60']);
  assert.equal(c.status, 0, c.out);
  assert.doesNotMatch(c.out, /a job started on/);
});

test('a job that keeps starting in the window is bounded by --wait-max too', { skip }, () => {
  const m = mac({ parkedByHand: true, jobPolls: 0, jobScript: Array(5000).fill('1').join(',') });
  const before = snapshot(m.home, m.sys);
  const r = move(m, ['install', '--user', USER, '--daemon', 'lane-a', '--wait-max', '1']);
  assert.notEqual(r.status, 0, r.out);
  assert.match(r.out, /still runs a job after 1 s; nothing was changed/);
  assert.deepEqual(snapshot(m.home, m.sys), before);
  assert.ok(!calls(m.bin).some((l) => l.startsWith('launchctl bootout')), 'never booted out under a job');
});
