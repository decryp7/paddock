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
    if (a === '--app') o.app = v(); else if (a === '--installer') o.installer = v(); else if (a === '--run') o.run = v(); else if (a === '--out') o.out = v(); else if (a === '--keep-vm') o.keep = true;
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
    const inst = g('sudo DEBIAN_FRONTEND=noninteractive apt-get install -y -qq /tmp/Dyno.deb 2>&1 | tail -3; dpkg-query -W -f="${Version}" dyno', 600_000);
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
    for (let i = 0; i < 20 && !prompt; i++) { await sleep(2000); prompt = /Authentication Required/.test(ses('xdotool search --name "Authentication Required" getwindowname %@ 2>/dev/null || true').stdout); }
    checks.push({ ok: prompt, detail: prompt ? 'the polkit "Authentication Required" dialog appeared (dyno-helper install --for uid:1000)' : 'no polkit dialog within 40 s' });
    if (prompt) ses('xdotool type --delay 80 admin; xdotool key Return');
    let active = '';
    for (let i = 0; i < 25 && active !== 'active'; i++) { await sleep(3000); active = g('systemctl is-active dyno-helper.service').stdout.trim(); }
    checks.push({ ok: active === 'active', detail: `dyno-helper.service is ${active || 'absent'}` });
    const stat = g('stat -c "%n %U:%G %a" /var/lib/dyno-helper /var/lib/dyno-helper/* /usr/bin/dyno-helper 2>&1').stdout.trim();
    const bad = stat.split('\n').filter((l) => /\/var\/lib\/dyno-helper/.test(l) && !/ root:root (7[0-5][0-5])$/.test(l));
    checks.push({ ok: bad.length === 0 && /dyno-helper/.test(stat), detail: bad.length ? `helper paths not root-only: ${bad.join('; ')}` : 'the helper data folder and its files are root:root and not writable by others' });
    const w = g('mkdir -p /var/lib/dyno-helper/tools/x 2>&1; touch /var/lib/dyno-helper/probe 2>&1; echo $?').stdout.trim();
    checks.push({ ok: !/^0$/m.test(w.split('\n').at(-1)), detail: 'a standard user cannot create files in the helper data folder' });
    push(result('Admin helper Turn on (polkit) and its folders', checks, stat.split('\n'), 'polkit answered, service active, folders root-only'));

    // 5. One step per helper reading (WP-VMH), read through AT-SPI.
    const labels = { processes: 'Processes', network: 'Network', extras: 'Extras', backup: 'Backup health', firewall: 'Firewall', power: 'Power', inventory: 'Inventory', lifespan: 'Lifespan' };
    const driver = { openPage: async (page, tab) => { const okp = press(`^${labels[page]}$`, 'link'); if (okp && tab) { await sleep(1500); return press(`^${tab}`, 'page tab|radio button|push button'); } return okp; }, text: async () => { await sleep(2500); return atspi('tree dyno 60', 60_000).stdout.split('\n').map((l) => l.replace(/^\s*\S+(\s\S+)?\s\s/, '')).join(' '); } };
    for (const fn of readingSteps('linux', async () => driver)) push(await fn({}));

    // 6. Turn off: the service and data are gone.
    press('^Dyno$');
    checks = [{ ok: press('Turn off administrator readings'), detail: 'pressed "Turn off administrator readings"' }];
    await sleep(6000);
    if (/Authentication Required/.test(ses('xdotool search --name "Authentication Required" getwindowname %@ 2>/dev/null || true').stdout)) { ses('xdotool type --delay 80 admin; xdotool key Return'); checks.push({ ok: true, detail: 'Turn off asked for polkit and was answered' }); }
    let gone = false;
    for (let i = 0; i < 25 && !gone; i++) { await sleep(3000); gone = g('systemctl is-active dyno-helper.service; test -e /var/lib/dyno-helper && echo present || echo gone').stdout.includes('gone'); }
    checks.push({ ok: gone, detail: gone ? 'the helper service and data folder are gone' : 'the helper is still installed 75 s after Turn off' });
    push(result('Admin helper Turn off removes it', checks, [], 'service and data removed'));

    // 7. Quit: File > Quit ends the app and the agent.
    press('^Close$'); press('^File$', 'menu item'); await sleep(1000); press('^Quit$', 'menu item'); await sleep(8000);
    const left = g('pgrep -c -x "dyno-app|dyno-agent" || true').stdout.trim();
    push(result('Quit (File > Quit) leaves nothing running', [{ ok: left === '0' || left === '', detail: `${left || 0} Dyno process(es) after Quit` }], [], 'no process left'));

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
