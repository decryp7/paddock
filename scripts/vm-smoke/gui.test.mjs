import assert from 'node:assert/strict';
import { existsSync, readdirSync, readFileSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { test } from 'node:test';
import { fileURLToPath } from 'node:url';
import { GuiCheck, runGui } from './gui.mjs';
import { parseVncUrl, vncDoubleClickCommands, vncDragCommands } from './run.mjs';

const config = JSON.parse(readFileSync(join(dirname(fileURLToPath(import.meta.url)), '..', '..', 'apps', 'dyno', 'vm-smoke.json'), 'utf8'));
const B = '/Applications/Dyno.app';
const LABEL = config.gui.launchAtLogin.label;

/** Splits the single-quoted words the checks send (`'a' 'b'\''c'`). */
const words = (s) => [...s.matchAll(/'((?:[^']|'\\'')*)'/g)].map((m) => m[1].replace(/'\\''/g, "'"));

/**
 * A fake GUI guest: Safari, the dmg window, Gatekeeper's dialogs, Dyno's Settings, Login Items, BTM, launchd and a reboot, as a
 * small state machine answering the commands gui.mjs sends. `bug` seeds one fault.
 */
function fakeGui({ bug = null } = {}) {
  const st = { gk: 'assessments enabled', downloaded: false, dmgOpen: false, installed: false, quarantine: null, dialog: null, settings: null, auth: false,
    app: null, window: false, settingsWin: false, appSwitch: 0, liSwitch: 0, btm: null, job: false, said: '', loginItemsOpen: false, log: '', agents: [], drags: 0, lostPress: bug === 'lost-press' };
  let clock = 0, pid = 400;
  const procs = () => [
    ...(st.app ? [`${st.app.pid} 1 00:10 Fri Oct 30 14:00:00 2026 ${bug === 'translocated' ? '/private/var/folders/x/T/AppTranslocation/1/d/Dyno.app/Contents/MacOS/dyno-app' : `${B}/Contents/MacOS/dyno-app`}`] : []),
    ...st.agents.map((a) => `${a.pid} ${a.ppid} 00:09 Fri Oct 30 14:00:01 2026 ${a.ppid === 1 ? 'dyno-agent run' : `${B}/Contents/MacOS/dyno-agent run`}`),
  ].join('\n');
  const startApp = ({ login = false, hidden = false } = {}) => {
    st.app = { pid: ++pid };
    st.window = hidden ? false : !login || bug === 'login-window';
    st.log += login && bug !== 'login-window' ? 'started hidden (flag or login item)\n' : 'started with a window\n';
    st.agents = [{ pid: ++pid, ppid: st.job && login ? 1 : st.app.pid }];
    if (bug === 'second-agent' && login) st.agents.push({ pid: ++pid, ppid: st.app.pid });
  };
  const ok = (stdout = '') => ({ code: 0, stdout, stderr: '' });
  const no = { code: 1, stdout: '', stderr: '' };
  const disp = () => (st.btm === 'allowed' ? '[enabled, allowed, notified] (0xb)' : '[enabled, disallowed, notified] (0x9)');
  const register = () => {
    if (st.liSwitch === 0 && st.btm === 'refused') { st.said = 'macOS needs your approval: allow Dyno in System Settings'; st.loginItemsOpen = true; return; }
    st.btm = 'allowed'; st.liSwitch = 1; st.job = true;
  };
  const loginTree = () => ['      AXHeading  Open at Login', '          AXOutline  ', ...(st.job ? ['            AXRow  ', '              AXCell  ', '                AXStaticText  Dyno'] : []),
    '      AXHeading  App Background Activity', '        AXStaticText  Cirrus Labs, Inc.', '        AXStaticText  2 items | background-detail-for-Cirrus Labs, Inc.-app-login-items',
    '        AXCheckBox  background-switch-Cirrus Labs, Inc. | value=1', '        AXStaticText  Dyno', '        AXStaticText  1 item | background-detail-for-Dyno-app-login-items',
    `        AXCheckBox  background-switch-Dyno | value=${st.liSwitch}`, ...(bug === 'legacy-row' ? ['        AXStaticText  dyno-app', '        AXStaticText  Item from unidentified developer. | background-detail-for-dyno-app-app-login-items', '        AXCheckBox  background-switch-dyno-app | value=1'] : []),
    '      AXHeading  Extensions'].join('\n');
  const ax = (w, input) => {
    const [verb, app, re, role = 'AXButton'] = w;
    const m = (s) => new RegExp(re).test(s);
    if (verb === 'trusted') return ok();
    if (verb === 'windows') return ok(st.app && st.window ? `Dyno\t${st.app.pid}\t0\tDyno\t0,30,1024x680` : 'Dock\t1\t20\tDock\t0,0,1024x768');
    if (verb === 'tree' && app === 'CoreServicesUIAgent') return ok(st.dialog === 'blocked' ? '    AXStaticText  “Dyno” Not Opened | _NS:74' : '    AXStaticText  Open “Dyno”? | _NS:74');
    if (verb === 'tree' && app === 'System Settings') return ok(loginTree());
    if (verb === 'tree' && app === 'Dyno') return ok(`    AXStaticText  Your settings are saved, but ${st.said}`);
    if (verb === 'setfield') { st.pw = input.trim(); return ok('set'); }
    const el = (() => {
      if (app === 'Finder' && role === 'AXImage') return st.dmgOpen && (m('Dyno | Dyno | Dyno') ? '300 160' : m('Applications | Applications') ? '150 160' : m('Dyno') ? '200 200' : null);
      if (app === 'Safari') return null;
      if (app === 'CoreServicesUIAgent') return (st.dialog === 'blocked' && (m('“Dyno” Not Opened') || m('Done'))) || (st.dialog === 'confirm' && m('Open Anyway'));
      if (app === 'coreautha') return st.auth && m('OK');
      if (app === 'System Settings' && m('Open Anyway')) return st.settings === 'open-anyway';
      if (app === 'System Settings' && role === 'AXCheckBox') return st.loginItemsOpen && m('background-switch-Dyno |') ? String(st.liSwitch) : null;
      if (app === 'Dyno' && role === 'AXStaticText') return st.window && m('Dashboard');
      if (app === 'Dyno' && role === 'AXMenuItem') return st.app && m('Settings…');
      if (app === 'Dyno' && role === 'AXButton' && m('Dyno')) return st.settingsWin;
      if (app === 'Dyno' && role === 'AXButton' && m('Save changes')) return st.settingsWin && st.dirty;
      if (app === 'Dyno' && role === 'AXCheckBox') return st.settingsWin && st.sectionOpen ? String(st.appSwitch) : null;
      return null;
    })();
    if (!el) return verb === 'center' || verb === 'value' ? ok('not found') && no : no;
    if (verb === 'has') return ok();
    if (verb === 'center') return ok(el);
    if (verb === 'value') return ok(el);
    // press
    if (app === 'CoreServicesUIAgent' && m('Done')) { st.dialog = null; st.settings = 'open-anyway'; }
    else if (app === 'System Settings' && m('Open Anyway')) { st.settings = null; st.dialog = 'confirm'; }
    else if (app === 'CoreServicesUIAgent' && m('Open Anyway')) { st.dialog = null; st.auth = true; }
    else if (app === 'coreautha') { st.auth = false; if (st.pw === 'admin') { st.quarantine = '01c3;6ae4;Safari;U'; startApp(); } }
    else if (app === 'Dyno' && role === 'AXMenuItem') st.settingsWin = true;
    else if (app === 'Dyno' && role === 'AXButton' && m('Dyno')) st.sectionOpen = true;
    else if (app === 'Dyno' && role === 'AXCheckBox') { st.appSwitch = 1 - st.appSwitch; st.dirty = true; }
    else if (app === 'Dyno' && m('Save changes')) { st.dirty = false; if (st.appSwitch) register(); else { st.job = false; st.agents = st.agents.map((a) => (a.ppid === 1 && st.app ? { pid: ++pid, ppid: st.app.pid } : a)); } }
    else if (app === 'System Settings' && role === 'AXCheckBox') {
      if (st.lostPress) { st.lostPress = false; return ok('pressed'); }
      st.liSwitch = 1 - st.liSwitch;
      if (st.liSwitch === 0) st.btm = 'refused'; else { st.btm = 'allowed'; st.job = true; }
    }
    return ok('pressed');
  };
  const exec = async (cmd, opts = {}) => {
    clock += 500;
    if (cmd.startsWith('axdrive ')) return ax(words(cmd), opts.input);
    if (cmd.startsWith('cat /usr/local/share/paddock-vmgui')) return ok('base x\nmacos 26.6.2');
    if (cmd.startsWith('spctl --status')) return ok(st.gk);
    if (cmd.startsWith('launchctl managername')) return ok('Aqua');
    if (cmd.startsWith('printf %s "$HOME"')) return ok('/Users/admin');
    if (cmd.includes('open -a Safari')) { st.downloaded = true; return ok(); }
    if (cmd.startsWith(`open -a '${B}'`)) { if (!st.app) startApp({ hidden: cmd.includes('--hidden') }); return ok(); }
    if (cmd.startsWith('[ -f ')) return st.downloaded ? ok('0083;6ae4;Safari;U') : no;
    if (cmd.startsWith('xattr -p')) { const q = cmd.includes('Downloads') ? '0083;6ae4;Safari;U' : st.installed ? st.quarantine : null; return q ? ok(q) : no; }
    if (cmd.startsWith('shasum')) return ok('feed');
    if (cmd.startsWith('sudo rm -rf') && cmd.includes('open ')) { st.installed = false; st.dmgOpen = true; return ok(); }
    if (cmd.startsWith('[ -x ')) return st.installed ? ok() : no;
    if (cmd.startsWith('codesign') || cmd.startsWith('mkdir -p') || cmd.startsWith('osascript') && !cmd.includes('open (POSIX')) {
      if (cmd.includes('"Dyno" to quit')) { st.app = null; st.agents = st.agents.filter((a) => a.ppid === 1); st.window = false; }
      if (cmd.includes('"System Settings" to quit')) st.loginItemsOpen = false;
      return ok();
    }
    if (cmd.startsWith('defaults read')) return ok('1.0.0');
    if (cmd.includes('open (POSIX file')) { if (st.quarantine?.startsWith('01c3')) startApp(); else st.dialog = 'blocked'; return ok(); }
    if (cmd.includes('PrivacySecurity')) return ok();
    if (cmd.includes('LoginItems-Settings')) { st.loginItemsOpen = true; return ok(); }
    if (cmd.startsWith('pgrep -x "System Settings"')) return st.loginItemsOpen ? ok('1') : no;
    if (cmd.startsWith('sudo sfltool')) return ok(st.btm ? ` #1:\n                 Name: dyno-agent\n          Disposition: ${disp()}\n           Identifier: 8.${LABEL}\n` : '');
    if (cmd.startsWith('launchctl print')) return st.job ? ok(`gui/501/${LABEL} = {\n\tstate = ${st.agents.some((a) => a.ppid === 1) ? 'running' : 'spawn scheduled'}\n}`) : ok('Could not find service');
    if (cmd.startsWith('ps -axww')) return ok(procs());
    if (cmd.startsWith('stat -f %z')) return ok(String(st.log.length));
    if (cmd.startsWith('tail -c +')) { const n = Number(/tail -c \+(\d+)/.exec(cmd)[1]); return ok(st.log.slice(n - 1)); }
    if (cmd.startsWith('tail -n')) return ok(st.log);
    if (cmd.startsWith('open /Applications')) return ok();
    if (cmd.includes('Finder" to close')) return ok();
    if (cmd.startsWith('cat ') && cmd.includes('agent.json')) return st.agents.length ? ok(JSON.stringify({ pid: st.agents[0].pid })) : no;
    if (cmd.startsWith('rm -f')) return ok();
    if (cmd.startsWith('[ "$(ps -o lstart=')) { st.agents = []; st.app = null; return ok(); }
    return ok();
  };
  const host = {
    installerUrl: 'http://192.168.64.1:8091/Dyno_1.0.0_aarch64.dmg', installerSha256: 'feed',
    drag: async () => { st.drags++; if (bug === 'first-drag-lost' && st.drags === 1) return; st.installed = true; st.quarantine = '0183;6ae4;Safari;U'; },
    doubleClick: async () => { if (st.app) st.window = true; },
    reboot: async () => { st.app = null; st.agents = []; st.window = false; st.settingsWin = false; st.sectionOpen = false; if (st.job) startApp({ login: true }); return true; },
  };
  const guest = { exec, now: () => clock, sleep: async (ms) => { clock += ms; } };
  return { guest, host, st };
}

async function run(bug) {
  const f = fakeGui({ bug });
  const check = new GuiCheck(f.guest, { ...config, settleSec: 4 }, f.host, { intervalMs: 1000 });
  const r = await runGui(check);
  return { ...r, by: Object.fromEntries(r.steps.map((s) => [s.title, s])), st: f.st };
}

test('vnc helpers: the URL Tart prints, a drag that clicks the window first, a double-click', () => {
  assert.deepEqual(parseVncUrl('VNC server is running at vnc://:pw-1@127.0.0.1:56263\n'), { password: 'pw-1', host: '127.0.0.1', port: 56263 });
  assert.equal(parseVncUrl('Opening vnc://example.com:5900'), null, 'only the VM\'s own loopback server');
  const d = vncDragCommands({ x: 317, y: 160 }, { x: 147, y: 160 });
  assert.deepEqual(d.slice(0, 7), ['move', '232', '310', 'pause', '0.3', 'click', '1']);
  assert.ok(d.indexOf('mousedown') < d.indexOf('mouseup') && d.at(-3) === 'mouseup' || d.includes('mouseup'));
  assert.deepEqual(vncDoubleClickCommands({ x: 5, y: 6 }).filter((x) => x === 'click').length, 2);
});

test('GUI sequence passes on a well-behaved app: download, drag, Gatekeeper, rows, approval, hidden login, double-click', async () => {
  const r = await run(null);
  const failed = r.steps.filter((s) => s.status === 'fail').map((s) => `${s.title}: ${s.summary}`);
  assert.deepEqual(failed, []);
  for (const t of ['Download with Safari (quarantine)', 'Open the dmg and drag to Applications', 'Gatekeeper first launch (Open Anyway)', 'Launch at login on: Login Items rows', 'Login Items approval (requiresApproval path)', 'Reboot: login start hidden, one agent', 'Finder double-click opens a window', 'Launch at login off', 'Quit at the end']) {
    assert.equal(r.by[t]?.status, 'pass', `${t}: ${r.by[t]?.summary}`);
  }
});

test('a drag that does not take is retried, judged by the copy', async () => {
  const r = await run('first-drag-lost');
  assert.equal(r.by['Open the dmg and drag to Applications'].status, 'pass');
  assert.match(r.by['Open the dmg and drag to Applications'].checks[0].detail, /attempt 2/);
});

test('a translocated first launch fails the Gatekeeper step', async () => {
  const r = await run('translocated');
  assert.equal(r.by['Gatekeeper first launch (Open Anyway)'].status, 'fail');
  assert.match(r.by['Gatekeeper first launch (Open Anyway)'].summary, /translocated/);
});

test('a Login Items press lost while the pane rebuilds is pressed again', async () => {
  const r = await run('lost-press');
  const s = r.by['Login Items approval (requiresApproval path)'];
  assert.equal(s.status, 'pass', s.summary);
});

test('an unidentified legacy row in Login Items fails the rows check', async () => {
  const r = await run('legacy-row');
  assert.equal(r.by['Launch at login on: Login Items rows'].status, 'fail');
  assert.match(r.by['Launch at login on: Login Items rows'].summary, /dyno-app/);
});

test('a login start that shows a window fails; the double-click then starts from the --hidden start', async () => {
  const r = await run('login-window');
  assert.equal(r.by['Reboot: login start hidden, one agent'].status, 'fail');
  assert.match(r.by['Reboot: login start hidden, one agent'].summary, /started with a window/);
  assert.equal(r.by['Finder double-click opens a window'].status, 'pass', 'it quits and uses the --hidden start instead');
  assert.match(r.by['Finder double-click opens a window'].checks[0].detail, /--hidden/);
});

test('a second agent after the login fails the reboot step', async () => {
  const r = await run('second-agent');
  assert.equal(r.by['Reboot: login start hidden, one agent'].status, 'fail');
});

test('Gatekeeper off in the guest stops the GUI run before anything is installed', async () => {
  const f = fakeGui();
  f.st.gk = 'assessments disabled';
  const r = await runGui(new GuiCheck(f.guest, config, f.host));
  assert.equal(r.steps[0].status, 'fail');
  assert.equal(f.st.downloaded, false);
});

test('every app config has a valid gui block (window owner, content regex, tray start, Launch at login null or complete)', () => {
  const apps = join(dirname(fileURLToPath(import.meta.url)), '..', '..', 'apps');
  for (const app of readdirSync(apps)) {
    const f = join(apps, app, 'vm-smoke.json');
    if (!existsSync(f)) continue;
    const g = JSON.parse(readFileSync(f, 'utf8')).gui;
    assert.ok(g, `${app}: no gui block`);
    assert.equal(typeof g.windowOwner, 'string', app);
    assert.doesNotThrow(() => new RegExp(g.contentRegex), app);
    assert.equal(typeof g.hiddenStart, 'boolean', app);
    if (g.launchAtLogin === null) continue;
    const l = g.launchAtLogin;
    assert.match(l.label, /^[a-z0-9.-]+$/, app);
    for (const a of [...l.open, ['press', ...l.switch]]) assert.ok(a[0] === 'press' && typeof a[1] === 'string' && new RegExp(a[2]), `${app}: ${JSON.stringify(a)}`);
    assert.ok(Array.isArray(l.rows.openAtLogin) && Array.isArray(l.rows.background) && l.rows.background.length >= 1, app);
    assert.doesNotThrow(() => new RegExp(l.needsApprovalText), app);
    assert.ok(['launchd', 'app'].includes(l.helperParentAfterLogin), app);
    assert.ok(g.loginStart && new RegExp(g.loginStart.hidden) && new RegExp(g.loginStart.shown) && g.loginStart.log, `${app}: loginStart`);
  }
});
