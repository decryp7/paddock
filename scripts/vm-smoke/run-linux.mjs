#!/usr/bin/env node
// The Linux guest of the VM release checks (local only, never CI): a fresh Tart clone of the GNOME desktop base `linux-gnome-base` (Ubuntu 24.04
// arm64 with GNOME on X11, auto-login, screen lock off, polkit, AT-SPI, and Rosetta mounted at /mnt/rosetta so the CI's amd64 .deb runs), the
// installer is the CI's `dyno-Linux` artifact. Steps are judged by the OS (ps, systemd, stat) and by the app's own text through AT-SPI
// (guest/atspi.py); the helper's polkit prompt is answered like a person would. The clone is deleted afterwards.
//
//   node scripts/vm-smoke/run-linux.mjs --app dyno (--installer <x.deb> | --run <ci run id>) [--out <dir>] [--keep-vm]
import { spawn, spawnSync } from 'node:child_process';
import { existsSync, mkdirSync, mkdtempSync, readFileSync, readdirSync, rmSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { basename, dirname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
import { renderResult } from './judge.mjs';
import { readingSteps } from './helper-readings.mjs';

const HERE = dirname(fileURLToPath(import.meta.url));
const REPO = resolve(HERE, '..', '..');
const BASE = process.env.PADDOCK_LINUX_BASE ?? 'linux-gnome-base';
const TART_HOME = process.env.TART_HOME ?? '/Volumes/d7sst7/dev-targets/tart-home';
const SESSION = 'export XDG_RUNTIME_DIR=/run/user/1000 DISPLAY=:0 XAUTHORITY=/run/user/1000/gdm/Xauthority DBUS_SESSION_BUS_ADDRESS=unix:path=/run/user/1000/bus';
const log = (m) => console.error(`vm-smoke(linux): ${m}`);
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));

export function parseArgs(argv) {
  const o = { out: null, keep: false };
  for (let i = 0; i < argv.length; i++) {
    const a = argv[i];
    const v = () => { if (i + 1 >= argv.length) throw new Error(`${a} needs a value`); return argv[++i]; };
    if (a === '--app') o.app = v(); else if (a === '--installer') o.installer = v(); else if (a === '--run') o.run = v(); else if (a === '--out') o.out = v(); else if (a === '--keep-vm') o.keep = true; else if (a === '--reboot-check') o.reboot = true;
    else throw new Error(`unknown option ${a}`);
  }
  if (o.app !== 'dyno') throw new Error('--app dyno is the only app with a Linux suite');
  if (!o.installer === !o.run) throw new Error('give exactly one of --installer <x.deb> or --run <CI run id>');
  return o;
}

const sh = (cmd, args, ms, input) => { const r = spawnSync(cmd, args, { encoding: 'utf8', timeout: ms, input, env: { ...process.env, TART_HOME } }); return { code: r.status ?? 124, stdout: r.stdout ?? '', stderr: r.stderr ?? '' }; };
const result = (title, checks, evidence = [], pass = '') => { const bad = checks.filter((c) => !c.ok); return { title, status: bad.length ? 'fail' : 'pass', summary: bad.length ? bad.map((c) => c.detail).join('; ').slice(0, 240) : pass, checks, evidence }; };

async function main(argv) {
  const o = parseArgs(argv);
  const out = resolve(o.out ?? process.env.PADDOCK_VM_SMOKE_OUT ?? join(REPO, 'vm-smoke-results'));
  mkdirSync(out, { recursive: true });
  const scratch = mkdtempSync(join(tmpdir(), 'vm-smoke-linux-'));
  const vm = `paddock-vmsmoke-linux-${new Date().toISOString().replace(/\D/g, '').slice(0, 14)}-${process.pid}`;
  let cloned = false, run = null;
  const cleanup = () => {
    if (run) run.kill('SIGTERM');
    if (cloned) { if (o.keep) log(`--keep-vm: ${vm} stays; delete it with: tart delete ${vm}`); else { sh('tart', ['stop', vm], 120_000); sh('tart', ['delete', vm], 120_000); } }
    rmSync(scratch, { recursive: true, force: true });
  };
  for (const sig of ['SIGINT', 'SIGTERM']) process.on(sig, () => { cleanup(); process.exit(130); });
  const g = (cmd, ms = 120_000) => sh('tart', ['exec', vm, 'bash', '-c', cmd], ms);
  const ses = (cmd, ms = 120_000) => g(`${SESSION}; ${cmd}`, ms);
  const atspi = (args, ms = 60_000) => ses(`python3 /var/cache/vmsmoke/atspi.py ${args}`, ms);
  try {
    let deb = o.installer ? resolve(o.installer) : null;
    if (!deb) {
      const token = process.env.GITEA_TOKEN;
      if (!token) throw new Error('GITEA_TOKEN is not set (source ~/.zshenv)');
      const zip = join(scratch, 'dyno-Linux.zip');
      const url = `${process.env.PADDOCK_GITEA ?? 'https://dev.decryptology.net'}/decryp7/paddock/actions/runs/${o.run}/artifacts/dyno-Linux`;
      if (sh('curl', ['-sS', '-L', '--fail', '-o', zip, '--config', '-'], 900_000, `header = "Authorization: token ${token}"\nurl = "${url}"\n`).code !== 0) throw new Error('downloading dyno-Linux failed');
      const dir = join(scratch, 'x'); mkdirSync(dir); sh('unzip', ['-q', '-o', zip, '-d', dir], 120_000);
      deb = join(dir, readdirSync(dir).find((f) => f.endsWith('.deb')));
    }
    if (!existsSync(deb)) throw new Error(`${deb} is not a file`);
    log(`cloning ${BASE} as ${vm}`);
    cloned = true;
    if (sh('tart', ['clone', BASE, vm], 600_000).code !== 0) throw new Error('tart clone failed');
    run = spawn('tart', ['run', '--no-graphics', '--vnc-experimental', '--rosetta=rosetta', vm], { env: { ...process.env, TART_HOME }, stdio: 'ignore' });
    for (let i = 0; i < 75 && g('true', 20_000).code !== 0; i++) await sleep(4000);
    // the desktop session: gnome-shell running as admin, Rosetta registered
    for (let i = 0; i < 60 && !/gnome-shell/.test(g('pgrep -x gnome-shell || true').stdout); i++) await sleep(3000);
    const steps = [];
    const push = (s) => { steps.push(s); log(`${s.status.toUpperCase()}  ${s.title}: ${s.summary}`); return s; };
    const rosetta = g('cat /proc/sys/fs/binfmt_misc/rosetta | head -1').stdout.trim();
    log(`desktop up; rosetta ${rosetta || 'NOT registered'}`);

    // 1. Install the CI's amd64 .deb (Rosetta runs it; the amd64 libraries were installed in the base).
    let checks = [];
    sh('bash', ['-c', `cat ${JSON.stringify(deb)} | TART_HOME=${TART_HOME} tart exec -i ${vm} bash -c 'cat > /tmp/Dyno.deb'`], 600_000);
    const inst = g('sudo DEBIAN_FRONTEND=noninteractive apt-get install -y -qq /tmp/Dyno.deb 2>&1 | tail -3; dpkg-query -W -f="\\${Version}\\n" dyno', 600_000);
    const ver = inst.stdout.trim().split('\n').at(-1);
    checks.push({ ok: /^\d+\.\d+\.\d+/.test(ver), detail: `dpkg reports dyno ${ver}` });
    for (const f of ['/usr/bin/dyno', '/usr/bin/dyno-agent', '/usr/bin/dyno-app', '/usr/bin/dyno-helper']) checks.push({ ok: g(`test -x ${f}`).code === 0, detail: `${f} installed` });
    push(result('Install (.deb, amd64 under Rosetta)', checks, [inst.stdout.trim()], `dyno ${ver} installed`));

    // 2. Cold start in the desktop session as GNOME does it (a systemd user unit: polkit needs the session's agent for the process).
    ses('systemd-run --user --collect --unit=dyno-gui -E DISPLAY=:0 -E XAUTHORITY=$XAUTHORITY /usr/bin/dyno-app >/dev/null 2>&1');
    let ready = false;
    for (let i = 0; i < 60 && !ready; i++) { await sleep(3000); ready = ses('dyno status >/dev/null 2>&1; echo $?').stdout.trim() === '0'; }
    await sleep(15_000);
    const ps = g('ps -eo pid=,ppid=,args= | grep -E "dyno-(app|agent)( |$)" | grep -v grep').stdout.trim().split('\n').filter(Boolean);
    const apps = ps.filter((l) => /dyno-app$/.test(l.trim())), agents = ps.filter((l) => /dyno-agent run$/.test(l.trim()));
    const appPid = apps[0]?.trim().split(/\s+/)[0];
    checks = [{ ok: ready, detail: ready ? 'dyno status answers' : 'no answer from dyno status within 3 min' }, { ok: apps.length === 1, detail: `dyno-app: ${apps.length} running (allowed 1)` }, { ok: agents.length === 1, detail: `dyno-agent run: ${agents.length} running (allowed 1)` }, { ok: agents.length === 1 && agents[0].trim().split(/\s+/)[1] === appPid, detail: 'the agent is the app\'s child' }];
    push(result('Cold start, one instance', checks, ps, 'one app, one agent under it'));

    // 3. Keep awake: systemd-inhibit lists a Dyno block inhibitor, then not.
    const inhib = () => g('systemd-inhibit --list --no-pager').stdout;
    checks = [{ ok: !/dyno/i.test(inhib()), detail: 'no Dyno inhibitor before "on"' }];
    const on = ses('dyno awake on --for 1h 2>&1');
    checks.push({ ok: on.code === 0, detail: `dyno awake on --for 1h exit ${on.code}` });
    await sleep(3000);
    const held = inhib();
    checks.push({ ok: /dyno/i.test(held) && /block/.test(held), detail: 'systemd-inhibit lists a Dyno inhibitor that blocks sleep' });
    ses('dyno awake off >/dev/null 2>&1'); await sleep(3000);
    checks.push({ ok: !/dyno/i.test(inhib()), detail: 'the inhibitor is gone after "off"' });
    push(result('Keep awake: held by the OS, then released', checks, held.split('\n').slice(0, 6), 'held 1 h, released after off'));

    // 4. Helper on through the real polkit prompt, then the data folders' permissions.
    const press = (re, role = 'push button') => atspi(`press dyno ${JSON.stringify(re)} ${JSON.stringify(role)}`).code === 0;
    checks = [];
    press('^File$', 'menu item'); await sleep(1200);
    checks.push({ ok: press('^Settings…$', 'menu item'), detail: 'opened Settings (File > Settings…)' }); await sleep(4000);
    press('^Dyno$'); await sleep(2000);
    checks.push({ ok: press('Turn on administrator readings'), detail: 'pressed "Turn on administrator readings"' });
    let prompt = false;
    for (let i = 0; i < 20 && !prompt; i++) { await sleep(2000); prompt = g('pgrep -x pkexec >/dev/null && echo yes || echo no').stdout.trim() === 'yes'; }
    checks.push({ ok: prompt, detail: prompt ? 'pkexec is waiting: the polkit "Authentication Required" dialog is up (dyno-helper install --for uid:1000)' : 'no pkexec (polkit prompt) within 40 s' });
    if (prompt) ses('xdotool type --delay 80 admin; xdotool key Return');
    let active = '';
    for (let i = 0; i < 25 && active !== 'active'; i++) { await sleep(3000); active = g('systemctl is-active dyno-helper.service').stdout.trim(); }
    checks.push({ ok: active === 'active', detail: `dyno-helper.service is ${active || 'absent'}` });
    const stat = g('sudo sh -c "stat -c \'%n %U:%G %a\' /var/lib/dyno-helper /var/lib/dyno-helper/* /usr/bin/dyno-helper" 2>&1').stdout.trim();
    const bad = stat.split('\n').filter((l) => /\/var\/lib\/dyno-helper/.test(l) && !/ root:root [0-7][0-5][0-5]$/.test(l));
    checks.push({ ok: bad.length === 0 && /dyno-helper/.test(stat), detail: bad.length ? `helper paths not root-only: ${bad.join('; ')}` : 'the helper data folder and its files are root:root and not writable by others' });
    const w = g('mkdir -p /var/lib/dyno-helper/tools/x 2>&1; touch /var/lib/dyno-helper/probe 2>&1; echo $?').stdout.trim();
    checks.push({ ok: !/^0$/m.test(w.split('\n').at(-1)), detail: 'a standard user cannot create files in the helper data folder' });
    push(result('Admin helper Turn on (polkit) and its folders', checks, stat.split('\n'), 'polkit answered, service active, folders root-only'));

    if (o.reboot) {
      // The restart check (user's real PC): helper on + Launch at login on, record start-at-boot, reboot the guest, log in, and without Repair
      // record the helper, the card, whether Dyno started itself (XDG autostart) and how long after login, the tray, and one reading.
      const boot = () => g('echo "is-enabled: $(systemctl is-enabled dyno-helper.service 2>&1)"; echo "is-active: $(systemctl is-active dyno-helper.service)"; systemctl cat dyno-helper.service 2>&1 | grep -E "^(WantedBy|Restart|RestartSec|ExecStart|\\[)" ; ls -l ~/.config/autostart/ /etc/xdg/autostart/ 2>&1 | grep -i dyno').stdout.trim().split('\n');
      press('^Dyno$');
      const toggled = atspi('press dyno "Start Dyno at login" "check box|toggle button|switch"').code === 0;
      await sleep(3000);
      const preBoot = boot();
      steps.push(result('Restart check: before the reboot (helper on, Launch at login on)', [{ ok: toggled, detail: toggled ? 'switched "Start Dyno at login" on' : 'no "Start Dyno at login" control found' }, { ok: /is-enabled: enabled/.test(preBoot.join('\n')), detail: `start-at-boot: ${preBoot[0]}; ${preBoot[1]}` }], preBoot, 'enabled at boot'));
      log('rebooting the guest');
      g('sudo systemctl reboot', 30_000);
      await sleep(40_000);
      for (let i = 0; i < 75 && g('true', 20_000).code !== 0; i++) await sleep(4000);
      let up = false; for (let i = 0; i < 60 && !up; i++) { up = /gnome-shell/.test(g('pgrep -x gnome-shell || true').stdout); if (!up) await sleep(3000); }
      const t0 = Date.now();
      await sleep(30_000);
      const postBoot = boot();
      const procs = g('ps -eo pid=,etimes=,args= | grep -E "dyno-(app|agent)" | grep -v grep').stdout.trim();
      const sess = g('loginctl show-session $(loginctl list-sessions --no-legend | awk \'$3=="admin"{print $1; exit}\') -p Timestamp -p State 2>&1').stdout.trim();
      const up2 = g('uptime -s; date "+%F %T"').stdout.trim();
      const autostarted = /dyno-app/.test(procs);
      steps.push(result('Restart check: Dyno after the reboot, no Repair (helper, autostart, tray)', [
        { ok: /is-active: active/.test(postBoot.join('\n')), detail: `helper after reboot: ${postBoot[0]}; ${postBoot[1]}` },
        { ok: autostarted, detail: autostarted ? 'Dyno started by itself after login (XDG autostart)' : 'Dyno did not start by itself after login' },
      ], [`session: ${sess.replace(/\n/g, ' ')}`, `boot/now: ${up2.replace(/\n/g, ' / ')}`, '--- Dyno processes (pid etimes args)', ...procs.split('\n'), '--- start-at-boot', ...postBoot], 'helper active, Dyno autostarted'));
      const ext = g('gnome-extensions list --enabled 2>&1 | grep -i -E "appindicator|tray" || echo "no appindicator extension enabled"').stdout.trim();
      const bar = atspi('tree gnome-shell 14 2>&1 | grep -i dyno | head -6').stdout.trim();
      steps.push(result('Restart check: tray icons on GNOME after the reboot', [{ ok: true, detail: `indicator support: ${ext}` }, { ok: /dyno/i.test(bar), detail: /dyno/i.test(bar) ? 'a Dyno indicator exists in the GNOME top bar' : 'no Dyno indicator in the GNOME top bar (GNOME shows tray icons only with an AppIndicator extension)' }], [ext, bar], 'recorded'));
      if (!autostarted) {
        ses('systemd-run --user --collect --unit=dyno-gui2 -E DISPLAY=:0 -E XAUTHORITY=$XAUTHORITY /usr/bin/dyno-app >/dev/null 2>&1');
        for (let i = 0; i < 30 && ses('dyno status >/dev/null 2>&1; echo $?').stdout.trim() !== '0'; i++) await sleep(4000);
        await sleep(15_000);
      }
      const cardTxt = atspi('tree dyno 60 2>&1 | grep -i -E "Administrator readings|Turn on|Turn off" | head -6').stdout.trim();
      press('^Processes$', 'link'); await sleep(4000);
      const procPage = atspi('tree dyno 80').stdout;
      steps.push(result('Restart check: card and a helper reading after the reboot', [{ ok: /Turn off administrator readings/.test(cardTxt) || !/Turn on administrator readings/.test(cardTxt), detail: `card: ${cardTxt.split('\n').slice(0, 3).join(' | ') || '(window not up)'}` }, { ok: /Read by the administrator helper/i.test(procPage), detail: /Read by the administrator helper/i.test(procPage) ? 'Processes shows "Read by the administrator helper"' : 'Processes does not show the helper chip after the reboot' }], [cardTxt], 'card On and a reading works'));
      void t0;
    }
    // 5. One step per helper reading (WP-VMH), read through AT-SPI.
    const labels = { processes: 'Processes', network: 'Network', extras: 'Extras', backup: 'Backup health', firewall: 'Firewall', power: 'Power', inventory: 'Inventory', lifespan: 'Lifespan' };
    const driver = { openPage: async (page, tab) => { const okp = press(`^${labels[page]}$`, 'link'); if (okp && tab) { await sleep(1500); return press(`^${tab}`, 'page tab|radio button|push button'); } return okp; }, text: async () => { await sleep(2500); return atspi('tree dyno 60', 60_000).stdout.split('\n').map((l) => l.replace(/^\s*\S+(\s\S+)?\s\s/, '')).join(' '); } };
    for (const fn of readingSteps('linux', async () => driver)) push(await fn({}));

    // 6. Turn off: the service and data are gone.
    press('^Dyno$');
    checks = [{ ok: press('Turn off administrator readings'), detail: 'pressed "Turn off administrator readings"' }];
    let offPrompt = false;
    for (let i = 0; i < 15 && !offPrompt; i++) { await sleep(2000); offPrompt = g('pgrep -x pkexec >/dev/null && echo yes || echo no').stdout.trim() === 'yes'; }
    if (offPrompt) { ses('xdotool type --delay 80 admin; xdotool key Return'); checks.push({ ok: true, detail: 'Turn off asked for polkit and was answered' }); }
    // "Installed" = the unit file is there or the service is active. A leftover data folder is its own line, with what is in it.
    const installed = () => g('if [ -e /usr/lib/systemd/system/dyno-helper.service ] || systemctl is-active --quiet dyno-helper.service; then echo installed; else echo removed; fi').stdout.trim();
    let state = installed();
    for (let i = 0; i < 25 && state !== 'removed'; i++) { await sleep(3000); state = installed(); }
    checks.push({ ok: state === 'removed', detail: state === 'removed' ? 'the unit file is gone and the service is not active' : 'the unit file or an active service is still there 75 s after Turn off' });
    const folder = g('sudo ls -la /var/lib/dyno-helper 2>&1').stdout.trim();
    const folderGone = /No such file or directory/.test(folder);
    // 7526 includes the log-race fix (450746cfd): the data folder must be gone.
    checks.push({ ok: folderGone, detail: folderGone ? 'the helper data folder is gone' : 'the helper data folder /var/lib/dyno-helper is left in place after Turn off (contents below)' });
    const why = g('echo "pkexec: $(pgrep -x pkexec || echo none)"; echo "service: $(systemctl is-active dyno-helper.service) enabled: $(systemctl is-enabled dyno-helper.service 2>&1)"; ls -l /usr/lib/systemd/system/dyno-helper.service /etc/systemd/system/dyno-helper.service /etc/systemd/system/multi-user.target.wants/dyno-helper.service 2>&1; ls -ld /var/lib/dyno-helper 2>&1; sudo journalctl -u dyno-helper.service --no-pager 2>&1 | tail -5').stdout.trim();
    const card = atspi('tree dyno 60').stdout.split('\n').map((l) => l.trim()).filter((l) => /administrator|Turn|helper|polkit|authenticate|not allowed|cancel/i.test(l)).slice(0, 8);
    push(result('Admin helper Turn off removes it', checks, [...why.split('\n'), '$ sudo ls -la /var/lib/dyno-helper', ...folder.split('\n'), ...card], 'service and data removed'));

    // 7. Quit: File > Quit ends the app and the agent.
    press('^Close$'); press('^File$', 'menu item'); await sleep(1000); press('^Quit$', 'menu item'); await sleep(8000);
    // Quit: poll up to 40 s (as on macOS and Windows) for no dyno process; print what is left with its parent.
    let leftList = '';
    for (let i = 0; i < 20; i++) { leftList = g('ps -eo pid=,ppid=,etimes=,args= | grep -E "dyno-(app|agent|helper)" | grep -v grep').stdout.trim(); if (!leftList) break; await sleep(2000); }
    const leftN = leftList ? leftList.split('\n').length : 0;
    push(result('Quit (File > Quit) leaves nothing running', [{ ok: leftN === 0, detail: leftN === 0 ? 'no Dyno process 40 s after Quit' : `${leftN} Dyno process(es) still running 40 s after Quit (pid ppid seconds args below)` }], leftList ? ['pid ppid age-s args', ...leftList.split('\n')] : [], 'no process left'));

    steps.push({ title: 'Real sleep, audio wait, scheduled wake on Linux hardware (manual)', status: 'manual', summary: 'The VM has no sleep state or sound device; the rows stay on the hardware checklist. Rosetta needs `MemoryDenyWriteExecute=no` in a drop-in for dyno-helper.service in this guest (the JIT needs W+X pages); that is an emulation workaround, not a product setting.' });
    const started = new Date().toISOString();
    const md = renderResult({ app: 'Dyno', version: ver, installer: o.installer ?? `CI run ${o.run}, artifact dyno-Linux, ${basename(deb)}`, guest: `Tart Ubuntu 24.04 arm64 GNOME (X11) clone of ${BASE}, amd64 .deb under Rosetta`, started, commit: sh('git', ['-C', REPO, 'rev-parse', '--short', 'HEAD'], 20_000).stdout.trim(), steps });
    const file = join(out, `${started.replace(/[:.]/g, '-')}-dyno-linux.md`);
    writeFileSync(file, md);
    log(`result: ${file}`); console.log(file);
    return steps.some((s) => s.status === 'fail') ? 1 : 0;
  } catch (e) { log(`error: ${e.message}`); return 2; } finally { cleanup(); }
}

if (process.argv[1] && resolve(process.argv[1]) === fileURLToPath(import.meta.url)) main(process.argv.slice(2)).then((c) => process.exit(c));
