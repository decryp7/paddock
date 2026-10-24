import assert from 'node:assert/strict';
import { mkdtempSync, rmSync } from 'node:fs';
import { createServer } from 'node:net';
import { join } from 'node:path';
import { after, test } from 'node:test';
import { calls, run, scratch, shim, write } from './helpers.mjs';

const servers = [];
after(() => Promise.all(servers.map((s) => new Promise((r) => s.close(r)))));
const listen = (path) => new Promise((resolve) => { const s = createServer(); servers.push(s); s.listen(path, resolve); });

const LAUNCHCTL_RUNNING = 'printf "gui/501/x = {\\n\\tstate = running\\n\\tpid = 4242\\n}\\n"';

/** A fake installed Mac: bundle with service binary and plist, shims for launchctl and plutil, data folder with log. */
function mac({ launchctl = LAUNCHCTL_RUNNING, service = true, plist = true } = {}) {
  const home = scratch();
  const bin = scratch('paddock-bin-');
  const bundle = join(home, 'Apps', 'Demo.app');
  write(home, {
    ...(service ? { 'Apps/Demo.app/Contents/MacOS/demo-service': '#!/bin/bash\necho "status: ok"\n' } : { 'Apps/Demo.app/Contents/Info.plist': '' }),
    ...(plist ? { 'Apps/Demo.app/Contents/Library/LaunchAgents/net.test.demo.service.plist': '<plist/>' } : {}),
    'Library/Application Support/Demo/logs/service.log': 'one\ntwo\nthree\n',
  });
  shim(bin, 'launchctl', launchctl);
  shim(bin, 'plutil');
  return { home, bin, bundle };
}
const verify = (m, extra = []) =>
  run('verify-installed.sh', ['--name', 'Demo', '--bundle', m.bundle, '--label', 'net.test.demo.service', '--service', 'demo-service', ...extra], { bin: m.bin, cwd: m.home, env: { PADDOCK_TOOLS_OS: 'Darwin', HOME: m.home } });

test('macOS: a healthy install passes and shows the service status and the log tail', () => {
  const m = mac();
  const r = verify(m);
  assert.equal(r.status, 0, r.out);
  assert.match(r.out, /ok {4}app bundle/);
  assert.match(r.out, /ok {4}launchd runs the agent \(pid 4242\)/);
  assert.match(r.out, /status: ok/);
  assert.match(r.out, /Service log \(last lines of .*service\.log\)\n {4}one\n {4}two\n {4}three/);
  assert.match(r.out, /Everything looks right\./);
  assert.ok(calls(m.bin).some((c) => c.startsWith('launchctl print gui/')));
});

test('macOS: an agent that launchd does not run fails with the way out', () => {
  const stale = verify(mac({ launchctl: 'printf "\\tstate = spawn scheduled\\n"' }));
  assert.equal(stale.status, 1);
  assert.match(stale.out, /fail {2}launch agent\n {8}launchd knows it but its state is 'spawn scheduled'/);
  const unloaded = verify(mac({ launchctl: 'echo "Could not find service" >&2; exit 113' }));
  assert.equal(unloaded.status, 1);
  assert.match(unloaded.out, /not loaded: open Demo > Settings > Service > Turn on/);
  assert.match(unloaded.out, /1 problem\(s\) found\./);
});

test('macOS: a missing bundle, service binary or plist is reported; each is one problem', () => {
  const m = mac({ service: false, plist: false });
  const r = verify(m);
  assert.equal(r.status, 1);
  assert.match(r.out, /service binary\n {8}.*demo-service is missing/);
  assert.match(r.out, /launch agent plist\n {8}Contents\/Library\/LaunchAgents\/net\.test\.demo\.service\.plist is missing/);
  const none = run('verify-installed.sh', ['--name', 'Demo', '--bundle', '/nonexistent/Demo.app'], { bin: m.bin, cwd: m.home, env: { PADDOCK_TOOLS_OS: 'Darwin', HOME: m.home } });
  assert.equal(none.status, 1);
  assert.match(none.out, /app bundle\n {8}not found at \/nonexistent\/Demo\.app/);
});

test('macOS: the service socket is checked in the data folder', async () => {
  const m = mac();
  // A unix socket path is limited to about 100 characters, so the data folder is a short one.
  const data = mkdtempSync('/tmp/pt-');
  after(() => rmSync(data, { recursive: true, force: true }));
  assert.match(verify(m, ['--socket', 'demo.sock', '--data', data]).out, /fail {2}service socket/);
  await listen(join(data, 'demo.sock'));
  const r = verify(m, ['--socket', 'demo.sock', '--data', data]);
  assert.equal(r.status, 0, r.out);
  assert.match(r.out, /ok {4}service socket/);
});

test('macOS: a service that does not answer is a problem that names the log', () => {
  const m = mac();
  write(m.bundle, { 'Contents/MacOS/demo-service': '#!/bin/bash\nexit 3\n' });
  const r = verify(m);
  assert.equal(r.status, 1);
  assert.match(r.out, /service status\n {8}the service did not answer; see .*logs\/service\.log/);
});

function linux({ active = 'active', env = true, linger = 'yes' } = {}) {
  const home = scratch();
  const bin = scratch('paddock-bin-');
  write(home, { '.local/share/Demo/bin/demo-service': '#!/bin/bash\necho "status: ok"\n' });
  shim(bin, 'systemctl', `case "$2" in show-environment) ${env ? 'exit 0' : 'exit 1'};; cat) exit 0;; is-active) echo ${active}; [ ${active} = active ];; is-enabled) exit 1;; esac`);
  shim(bin, 'loginctl', `echo ${linger}`);
  return { home, bin };
}
const verifyLinux = (m, extra = []) =>
  run('verify-installed.sh', ['--name', 'Demo', '--unit', 'demo.service', '--service', 'demo-service', ...extra], { bin: m.bin, cwd: m.home, env: { PADDOCK_TOOLS_OS: 'Linux', HOME: m.home, USER: 'tester', XDG_DATA_HOME: join(m.home, '.local/share') } });

test('Linux: unit, state, enablement and lingering are read; not enabled and no linger are notes, not failures', () => {
  const r = verifyLinux(linux({ linger: 'no' }));
  assert.equal(r.status, 0, r.out);
  assert.match(r.out, /ok {4}service program .*demo-service/);
  assert.match(r.out, /ok {4}user unit demo\.service is installed/);
  assert.match(r.out, /ok {4}systemd runs the service \(active\)/);
  assert.match(r.out, /\.\. {4}not enabled/);
  assert.match(r.out, /\.\. {4}lingering is off: .* loginctl enable-linger tester/);
});

test('Linux: an inactive unit, no user systemd and a missing program fail with the way out', () => {
  const inactive = verifyLinux(linux({ active: 'failed' }));
  assert.equal(inactive.status, 1);
  assert.match(inactive.out, /service state\n {8}'failed': systemctl --user status demo\.service/);
  const noEnv = verifyLinux(linux({ env: false }));
  assert.match(noEnv.out, /systemd user instance\n {8}not reachable from this shell/);
  const m = linux();
  const missing = run('verify-installed.sh', ['--name', 'Demo', '--service', 'other-service'], { bin: m.bin, cwd: m.home, env: { PADDOCK_TOOLS_OS: 'Linux', HOME: m.home, XDG_DATA_HOME: join(m.home, '.local/share') } });
  assert.equal(missing.status, 1);
  assert.match(missing.out, /service program\n {8}other-service not found/);
});

test('usage errors and unsupported systems', () => {
  assert.equal(run('verify-installed.sh', [], {}).status, 2);
  assert.equal(run('verify-installed.sh', ['--name', 'D', '--nope'], {}).status, 2);
  const w = run('verify-installed.sh', ['--name', 'D'], { env: { PADDOCK_TOOLS_OS: 'MINGW64' } });
  assert.equal(w.status, 2);
});
