// The Windows tray right-click menu (ADR 0032 P12): the Keep awake submenu, the Wake device submenu and, while an idle countdown runs, its
// Cancel and Snooze entries. Native input comes from the in-session actor (win-act.ps1: UI Automation finds the taskbar icons and the popup
// menu's items, `rclick`/`click` act), the idle state is read with `dyno idle`. Dyno's icons are promoted to the taskbar (IsPromoted) so they have
// a place to click.
import { spawnSync } from 'node:child_process';
import { makeResult } from './verdict.mjs';
import { judgeTrayClick, rightClicksInLog } from './win-judges.mjs';
import { APP_LOG_PS, TRAY_ICONS_CMD, dynoTrayIcons, metricsFromLog } from './win-tray-icons.mjs';

const VMWIN = process.env.PADDOCK_VM_WIN ?? '/Volumes/d7sst7/dev-targets/scratch/dyno-vm-t/vm-win.sh';
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
const ssh = (cmd, ms = 60_000) => spawnSync('bash', [VMWIN, 'ssh', cmd], { encoding: 'utf8', timeout: ms }).stdout.split('\n').filter((l) => !l.startsWith('**')).join('\n').trim();
const result = makeResult;
const items = (s) => s.split('\n').map((l) => l.split('\t')).filter((c) => c[0] && c[1]).map(([name, at]) => ({ name: name.trim(), at: at.trim().replace(',', ' ') }));

/** A native popup menu blocks the app's UI thread while it is open (a later File > Settings would never open): close every level, then check nothing is left. */
export async function closeMenus(actor) {
  for (let i = 0; i < 3; i++) { await actor.send('key ESC').catch(() => {}); await sleep(500); }
}

async function openMenu(actor) {
  const metrics = metricsFromLog(ssh(APP_LOG_PS));
  const read = async () => dynoTrayIcons(await actor.send(TRAY_ICONS_CMD), metrics).map((i) => ({ name: i.name, at: i.at.replace(',', ' ') }));
  // Counted as the user would find them, BEFORE any icon is promoted; only when none is in reach are they promoted (IsPromoted) so there is a place to click.
  let icons = await read();
  let promoted = false;
  if (!icons.length) {
    ssh(`Get-ChildItem 'HKCU:\\Control Panel\\NotifyIconSettings' | ForEach-Object { $p = Get-ItemProperty $_.PSPath; if ($p.ExecutablePath -like '*dyno-app.exe') { Set-ItemProperty $_.PSPath IsPromoted 1 -Type DWord } }`);
    await sleep(2500);
    icons = await read();
    promoted = true;
  }
  if (!icons.length) return { icons, menu: [], promoted, metrics };
  await actor.send(`rclick ${icons[0].at}`);
  await sleep(1500);
  return { icons, menu: items(await actor.send('menu')), promoted, metrics };
}

export async function trayMenuStep(actor, { browser = null } = {}) {
  const checks = [];
  const evidence = [];
  // The probe must be able to see a native menu (ER-33): close the dialogs that cover the shell, give the shell the foreground, count the app's own "tray click: Right Up" lines
  // before and after, and take a screenshot while the menu is open.
  // Escape, never page.close(): closing the DevTools target leaves the native dialog window open but invisible to the next step (the dry4 run: "focusing the open window d-main-3 of pd.settings" and a Settings page the script could not find).
  if (browser) for (const p of browser.contexts().flatMap((c) => c.pages())) if (p.url().includes('dialog?id=')) { await p.keyboard.press('Escape').catch(() => {}); await sleep(500); }
  await actor.send('fg Dyno').catch(() => {});
  await sleep(1000);
  const logBefore = rightClicksInLog(ssh(APP_LOG_PS));
  const { icons, menu, promoted, metrics } = await openMenu(actor);
  const logAfter = rightClicksInLog(ssh(APP_LOG_PS));
  const menuShot = await actor.shot('tray-menu-open');
  evidence.push(`app log "tray click: Right Up": ${logBefore} before, ${logAfter} after the right click`, `screenshot with the menu open: ${menuShot ?? 'not taken'}`);
  const click = judgeTrayClick({ logBefore, logAfter, menuItems: menu.length });
  evidence.push(`taskbar icons: ${icons.map((i) => i.name).join(' | ') || '(none found)'}${promoted ? ' (found only after promoting them)' : ''}`, `menu: ${menu.map((m) => m.name).join(' | ') || '(none)'}`);
  checks.push(...(metrics.length ? [] : [{ ok: false, kind: 'check', detail: 'the app log names no tray icon it created (metrics = 0), so the icons cannot be recognised' }]));
  checks.push({ ok: icons.length > 0, detail: icons.length ? `${icons.length} Dyno tray icon(s) on the taskbar${promoted ? ' (only after promoting them)' : ''}` : 'no Dyno icon found on the taskbar through UI Automation' });
  checks.push(...click);
  const keep = menu.find((m) => /Keep awake/i.test(m.name));
  checks.push({ ...(menu.length ? {} : { kind: 'check' }), ok: Boolean(keep), detail: keep ? `the menu has "${keep.name}"` : 'the menu has no Keep awake entry' });
  if (keep) {
    await actor.send(`click ${keep.at}`); await sleep(1500);
    const sub = items(await actor.send('menu'));
    evidence.push(`Keep awake submenu: ${sub.map((m) => m.name).join(' | ')}`);
    checks.push({ ok: sub.some((m) => /hour|minutes|Until/i.test(m.name)), detail: sub.some((m) => /hour|minutes|Until/i.test(m.name)) ? 'the Keep awake submenu lists durations' : 'the Keep awake submenu lists no durations' });
    await actor.send('key ESC'); await sleep(600);
  }
  const { menu: menu2 } = await openMenu(actor);
  const wake = menu2.find((m) => /Wake device|Wake/i.test(m.name));
  checks.push({ ...(menu2.length ? {} : { kind: 'check' }), ok: Boolean(wake), detail: wake ? `the menu has "${wake.name}"` : 'the menu has no Wake device entry' });
  if (wake) { await actor.send(`click ${wake.at}`); await sleep(1500); evidence.push(`Wake device submenu: ${items(await actor.send('menu')).map((m) => m.name).join(' | ')}`); }
  await closeMenus(actor);
  await actor.shot('tray-menu');
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
  checks.push({ ...(menu.length ? {} : { kind: 'check' }), ok: menu.some((m) => /Will sleep in|Counting down/i.test(m.name)), detail: 'the menu shows the countdown line ("Will sleep in ...")' });
  const snooze = menu.find((m) => /Snooze/i.test(m.name));
  const cancel = menu.find((m) => /^Cancel/i.test(m.name));
  checks.push({ ...(menu.length ? {} : { kind: 'check' }), ok: Boolean(snooze), detail: snooze ? `the menu has "${snooze.name}"` : 'the menu has no Snooze entry' });
  checks.push({ ...(menu.length ? {} : { kind: 'check' }), ok: Boolean(cancel), detail: cancel ? `the menu has "${cancel.name}"` : 'the menu has no Cancel entry' });
  if (cancel) {
    await actor.send(`click ${cancel.at}`); await sleep(3000);
    const after = ssh('& "$env:ProgramFiles\\Dyno\\dyno.exe" idle 2>&1 | Out-String');
    checks.push({ ok: !/Counting down/.test(after), detail: !/Counting down/.test(after) ? 'Cancel in the tray stopped the countdown' : 'the countdown still runs after Cancel in the tray' });
  } else await closeMenus(actor);
  await closeMenus(actor);
  return result('Windows: tray menu during an idle countdown (Cancel, Snooze)', checks, evidence, 'the countdown entries are in the tray menu and Cancel works');
}
