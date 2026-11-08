// The Windows tray right-click menu (ADR 0032 P12): the Keep awake submenu, the Wake device submenu and, while an idle countdown runs, its
// Cancel and Snooze entries. Native input comes from the in-session actor (win-act.ps1: UI Automation finds the taskbar icons and the popup
// menu's items, `rclick`/`click` act), the idle state is read with `dyno idle`. Dyno's icons are promoted to the taskbar (IsPromoted) so they have
// a place to click.
import { spawnSync } from 'node:child_process';
import { makeResult } from './verdict.mjs';
import { judgeNativeMenu, parseMenuWatch, rightClicksInLog } from './win-judges.mjs';
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

/**
 * The tray menu has a "Wake device" submenu only when a computer is saved (wol_menu.rs: none when nothing is saved), and "Keep awake" only when the setting is on (seeded by
 * win-suite.ps1 start-tray). The host saves one made-up computer through the Power page, by hand (the app's own form), and says what it did.
 */
export async function addWakeDevice(browser) {
  let stage = 'start';
  let main = null;
  try {
    main = browser.contexts().flatMap((c) => c.pages()).find((p) => !p.url().includes('dialog?id='));
    stage = 'open #/power'; await main.evaluate(() => { location.hash = '#/power'; }); await sleep(2500);
    stage = 'tab Wake devices'; await main.getByRole('tab', { name: /^Wake devices$/ }).click({ timeout: 10_000 }); await sleep(1500);
    stage = 'button Add by hand'; await main.getByRole('button', { name: /Add by hand/ }).first().click({ timeout: 10_000 });
    stage = 'fill name'; await main.getByLabel('Name of the computer').fill('vm-check-device', { timeout: 10_000 });
    stage = 'fill address'; await main.getByLabel('Hardware address').fill('02:00:00:aa:bb:cc', { timeout: 10_000 });
    stage = 'button Add computer'; await main.getByRole('button', { name: 'Add computer' }).click({ timeout: 10_000 });
    await sleep(2500);
    const listed = /vm-check-device/.test(await main.evaluate(() => document.body.innerText));
    return listed ? 'saved the computer "vm-check-device" (02:00:00:aa:bb:cc) on the Power page, Wake devices' : 'pressed Add computer but the page does not list "vm-check-device"';
  } catch (e) {
    const text = main ? await main.evaluate(() => document.body.innerText.replace(/\s+/g, ' ').slice(0, 500)).catch(() => '') : '';
    return `could not save a wake device through the Power page at "${stage}": ${String(e.message).split('\n')[0]}; the page says: ${text}`;
  }
}

/** The control icon's own menu (win-menu-control.ps1): started in the console session, promoted to the taskbar, right-clicked; always removed afterwards. */
async function controlMenu(actor, { hereDir = 'C:\\paddock-test\\vmcheck' } = {}) {
  ssh(`schtasks /end /tn VmMenuControl 2>$null | Out-Null; schtasks /create /tn VmMenuControl /sc once /st 00:00 /ru paddock-test /it /f /tr "cmd /c powershell -NoProfile -WindowStyle Hidden -ExecutionPolicy Bypass -File ${hereDir}\\win-menu-control.ps1 -MaxSec 300" | Out-Null; schtasks /run /tn VmMenuControl | Out-Null`);
  let at = null;
  for (let i = 0; i < 8 && !at; i++) {
    await sleep(2500);
    ssh(`Get-ChildItem 'HKCU:\\Control Panel\\NotifyIconSettings' -ErrorAction SilentlyContinue | ForEach-Object { $p = Get-ItemProperty $_.PSPath; if ($p.ExecutablePath -like '*powershell.exe') { Set-ItemProperty $_.PSPath IsPromoted 1 -Type DWord } }`);
    at = items(await actor.send('tray VMCHECK-MENU-CONTROL'))[0]?.at ?? null;
  }
  let watch = null;
  if (at) { watch = parseMenuWatch(await actor.send(`rclickwatch ${at}`, 60_000)); await closeMenus(actor); }
  const shot = at ? await actor.fetch(watch?.seen ? 'rcw-open' : 'rcw-none', 'control-menu') : null;
  ssh('schtasks /end /tn VmMenuControl 2>$null | Out-Null; Get-CimInstance Win32_Process -Filter "Name=\'powershell.exe\'" | Where-Object { $_.CommandLine -match \'win-menu-control\' } | ForEach-Object { Stop-Process -Id $_.ProcessId -Force }; schtasks /delete /tn VmMenuControl /f 2>$null | Out-Null');
  return { icon: Boolean(at), watch, shot };
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
  if (!icons.length) return { icons, menu: [], promoted, metrics, watch: parseMenuWatch(''), logged: false };
  // The click and the look for a native menu (window class #32768) happen in the guest, every 100 ms for 6 s (win-act.ps1 `rclickwatch`).
  const before = rightClicksInLog(ssh(APP_LOG_PS));
  const watch = parseMenuWatch(await actor.send(`rclickwatch ${icons[0].at}`, 60_000));
  const logged = rightClicksInLog(ssh(APP_LOG_PS)) > before;
  return { icons, menu: watch.items, promoted, metrics, watch, logged };
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
  const seeded = browser ? await addWakeDevice(browser) : 'no browser: no wake device saved';
  evidence.push(`wake device: ${seeded}`);
  // The probe is shown to see a native tray menu first (the harness's own icon), then Dyno's icon is clicked, twice at most.
  const control = await controlMenu(actor);
  await actor.send('fg Dyno').catch(() => {});
  await sleep(1000);
  await closeMenus(actor);
  const attempts = [];
  let first = await openMenu(actor);
  attempts.push({ logged: first.logged, watch: first.watch });
  let { icons, menu, promoted, metrics } = first;
  const menuShot = await actor.fetch(first.watch.seen ? 'rcw-open' : 'rcw-none', 'tray-menu-open');
  if (icons.length && !first.watch.seen) { await closeMenus(actor); await actor.send('fg Dyno').catch(() => {}); await sleep(1500); const again = await openMenu(actor); attempts.push({ logged: again.logged, watch: again.watch }); if (again.watch.seen) { menu = again.menu; } }
  evidence.push(`screenshot after the first right click: ${menuShot ?? 'not taken'} (taken in the guest at the first sight of a menu, or after 6 s with none)`, `control menu screenshot: ${control.shot ?? 'not taken'}`,
    ...attempts.map((a, i) => `attempt ${i + 1}: app log "tray click: Right Up" ${a.logged ? 'logged' : 'NOT logged'}; native menu windows: ${a.watch.seen ? `${a.watch.windows.map((w) => `${w.proc} ${w.rect}`).join(', ')} after ${a.watch.ms} ms` : 'none in 6 s'}; items: ${a.watch.items.map((m) => m.name).join(' | ') || '(none)'}`));
  evidence.push(`taskbar icons: ${icons.map((i) => i.name).join(' | ') || '(none found)'}${promoted ? ' (found only after promoting them)' : ''}`, `menu: ${menu.map((m) => m.name).join(' | ') || '(none)'}`);
  checks.push(...judgeNativeMenu({ control: control.watch, controlIcon: control.icon, attempts }));
  checks.push(...(metrics.length ? [] : [{ ok: false, kind: 'check', detail: 'the app log names no tray icon it created (metrics = 0), so the icons cannot be recognised' }]));
  checks.push({ ok: icons.length > 0, detail: icons.length ? `${icons.length} Dyno tray icon(s) on the taskbar${promoted ? ' (only after promoting them)' : ''}` : 'no Dyno icon found on the taskbar through UI Automation' });
  // The submenus are read from the menu's own structure (win-act.ps1 Read-Menu: "Parent > Item"), without opening them: the click on an item did not open a readable submenu in 0ad round 5.
  const top = menu.filter((m) => !m.name.includes(' > '));
  const keep = top.find((m) => /^Keep awake$/i.test(m.name));
  checks.push({ ...(menu.length ? {} : { kind: 'check' }), ok: Boolean(keep), detail: keep ? `the menu has "${keep.name}"` : 'the menu has no Keep awake entry' });
  const keepSub = menu.filter((m) => /^Keep awake > /.test(m.name)).map((m) => m.name.replace(/^Keep awake > /, ''));
  evidence.push(`Keep awake submenu: ${keepSub.join(' | ')}`);
  if (keep) checks.push({ ok: keepSub.some((n) => /hour|minutes|Until/i.test(n)), detail: keepSub.some((n) => /hour|minutes|Until/i.test(n)) ? 'the Keep awake submenu lists durations' : 'the Keep awake submenu lists no durations' });
  const wake = top.find((m) => /^Wake device$/i.test(m.name));
  const wakeSub = menu.filter((m) => /^Wake device > /.test(m.name)).map((m) => m.name.replace(/^Wake device > /, ''));
  evidence.push(`Wake device submenu: ${wakeSub.join(' | ')}`);
  checks.push({ ...(menu.length ? {} : { kind: 'check' }), ok: Boolean(wake), detail: wake ? `the menu has "${wake.name}"` : 'the menu has no Wake device entry' });
  if (wake) checks.push({ ok: wakeSub.includes('vm-check-device'), detail: wakeSub.includes('vm-check-device') ? 'the Wake device submenu lists the saved computer' : `the Wake device submenu does not list the saved computer (${wakeSub.join(', ') || 'empty'})` });
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
  // Up to three tries: the countdown re-arms on input and the menu from an earlier click may still be closing (0ad round 5: an empty read with the countdown running).
  let menu = [];
  for (let k = 1; k <= 3 && !menu.length; k++) {
    await closeMenus(actor);
    const o = await openMenu(actor);
    menu = o.menu;
    evidence.push(`try ${k}: native menu ${o.watch.seen ? `seen after ${o.watch.ms} ms` : 'NOT seen in 6 s'}; app log right click ${o.logged ? 'logged' : 'NOT logged'}; ${o.menu.length} item(s)`);
    if (!menu.length) await sleep(2000);
  }
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
