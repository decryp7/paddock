// The Windows tray right-click menu (ADR 0032 P12): the Keep awake submenu, the Wake device submenu and, while an idle countdown runs, its
// Cancel and Snooze entries. Native input comes from the in-session actor (win-act.ps1: UI Automation finds the taskbar icons and the popup
// menu's items, `rclick`/`click` act), the idle state is read with `dyno idle`. Dyno's icons are promoted to the taskbar (IsPromoted) so they have
// a place to click.
import { spawnSync } from 'node:child_process';
import { APP_LOG_PS, TRAY_ICONS_CMD, dynoTrayIcons, metricsFromLog } from './win-tray-icons.mjs';

const VMWIN = process.env.PADDOCK_VM_WIN ?? '/Volumes/d7sst7/dev-targets/scratch/dyno-vm-t/vm-win.sh';
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
const ssh = (cmd, ms = 60_000) => spawnSync('bash', [VMWIN, 'ssh', cmd], { encoding: 'utf8', timeout: ms }).stdout.split('\n').filter((l) => !l.startsWith('**')).join('\n').trim();
const result = (title, checks, evidence = [], pass = '') => { const bad = checks.filter((c) => !c.ok); return { title, status: bad.length ? 'fail' : 'pass', summary: bad.length ? bad.map((c) => c.detail).join('; ').slice(0, 240) : pass, checks, evidence }; };
const items = (s) => s.split('\n').map((l) => l.split('\t')).filter((c) => c[0] && c[1]).map(([name, at]) => ({ name: name.trim(), at: at.trim().replace(',', ' ') }));

async function openMenu(actor) {
  ssh(`Get-ChildItem 'HKCU:\\Control Panel\\NotifyIconSettings' | ForEach-Object { $p = Get-ItemProperty $_.PSPath; if ($p.ExecutablePath -like '*dyno-app.exe') { Set-ItemProperty $_.PSPath IsPromoted 1 -Type DWord } }`);
  await sleep(2500);
  // Dyno's icons are named by their metric tooltip, never "Dyno": a search for "Dyno" finds Dyno's own window buttons (win-tray-icons.mjs).
  const icons = dynoTrayIcons(await actor.send(TRAY_ICONS_CMD), metricsFromLog(ssh(APP_LOG_PS))).map((i) => ({ name: i.name, at: i.at.replace(',', ' ') }));
  if (!icons.length) return { icons, menu: [] };
  await actor.send(`rclick ${icons[0].at}`);
  await sleep(1500);
  return { icons, menu: items(await actor.send('menu')) };
}

export async function trayMenuStep(actor) {
  const checks = [];
  const evidence = [];
  const { icons, menu } = await openMenu(actor);
  evidence.push(`taskbar icons: ${icons.map((i) => i.name).join(' | ') || '(none found)'}`, `menu: ${menu.map((m) => m.name).join(' | ') || '(none)'}`);
  checks.push({ ok: icons.length > 0, detail: icons.length ? `${icons.length} Dyno tray icon(s) on the taskbar` : 'no Dyno icon found on the taskbar through UI Automation' });
  checks.push({ ok: menu.length > 0, detail: menu.length ? 'right-click opened a menu' : 'right-click opened no menu' });
  const keep = menu.find((m) => /Keep awake/i.test(m.name));
  checks.push({ ok: Boolean(keep), detail: keep ? `the menu has "${keep.name}"` : 'the menu has no Keep awake entry' });
  if (keep) {
    await actor.send(`click ${keep.at}`); await sleep(1500);
    const sub = items(await actor.send('menu'));
    evidence.push(`Keep awake submenu: ${sub.map((m) => m.name).join(' | ')}`);
    checks.push({ ok: sub.some((m) => /hour|minutes|Until/i.test(m.name)), detail: sub.some((m) => /hour|minutes|Until/i.test(m.name)) ? 'the Keep awake submenu lists durations' : 'the Keep awake submenu lists no durations' });
    await actor.send('key ESC'); await sleep(600);
  }
  const { menu: menu2 } = await openMenu(actor);
  const wake = menu2.find((m) => /Wake device|Wake/i.test(m.name));
  checks.push({ ok: Boolean(wake), detail: wake ? `the menu has "${wake.name}"` : 'the menu has no Wake device entry' });
  if (wake) { await actor.send(`click ${wake.at}`); await sleep(1500); evidence.push(`Wake device submenu: ${items(await actor.send('menu')).map((m) => m.name).join(' | ')}`); }
  await actor.send('key ESC'); await sleep(600);
  await actor.send('shot tray-menu');
  return result('Windows: tray right-click menu with the Keep awake and Wake device submenus', checks, evidence, 'both submenus are in the tray menu');
}

/** With an idle countdown running the menu shows its Cancel and Snooze entries; Cancel stops it. */
export async function trayCountdownStep(actor, { idleRuleSeeded = true } = {}) {
  const checks = [];
  const evidence = [];
  let running = false;
  for (let i = 0; i < 50 && !running; i++) { running = /Counting down/.test(ssh('& "$env:ProgramFiles\\Dyno\\dyno.exe" idle 2>&1 | Out-String')); if (!running) await sleep(4000); }
  checks.push({ ok: running, detail: running ? 'an idle countdown is running' : `no idle countdown within 200 s${idleRuleSeeded ? '' : ' (no idle rule seeded)'}` });
  if (!running) return result('Windows: tray menu during an idle countdown (Cancel, Snooze)', checks, evidence);
  const { menu } = await openMenu(actor);
  evidence.push(`menu: ${menu.map((m) => m.name).join(' | ')}`);
  checks.push({ ok: menu.some((m) => /Will sleep in|Counting down/i.test(m.name)), detail: 'the menu shows the countdown line ("Will sleep in ...")' });
  const snooze = menu.find((m) => /Snooze/i.test(m.name));
  const cancel = menu.find((m) => /^Cancel/i.test(m.name));
  checks.push({ ok: Boolean(snooze), detail: snooze ? `the menu has "${snooze.name}"` : 'the menu has no Snooze entry' });
  checks.push({ ok: Boolean(cancel), detail: cancel ? `the menu has "${cancel.name}"` : 'the menu has no Cancel entry' });
  if (cancel) {
    await actor.send(`click ${cancel.at}`); await sleep(3000);
    const after = ssh('& "$env:ProgramFiles\\Dyno\\dyno.exe" idle 2>&1 | Out-String');
    checks.push({ ok: !/Counting down/.test(after), detail: !/Counting down/.test(after) ? 'Cancel in the tray stopped the countdown' : 'the countdown still runs after Cancel in the tray' });
  } else await actor.send('key ESC');
  return result('Windows: tray menu during an idle countdown (Cancel, Snooze)', checks, evidence, 'the countdown entries are in the tray menu and Cancel works');
}
