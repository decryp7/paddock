// The restart check (high priority, from the user's real PC): after a Windows restart the admin helper must work without Repair or another UAC.
// 1. install and turn the helper on as the owner (UAC over VNC), 2. record the service (sc qc, sc query), the helper.log tail and the Settings card,
// 3. restart the guest (a real OS restart; the auto-logon is switched on again first), wait for the GUI session, 4. record the same again before Dyno
// runs and after Dyno is launched, with no Repair, and read a helper reading, 5. when it fails keep the ACLs and the System event log entries.
import { cardText, helperOn, openSettings, ssh } from './win-helper.mjs';
import { connect, tunnel } from './win-cdp.mjs';
import { Actor } from './win-act.mjs';
import { retryOnClosed } from './reconnect.mjs';

const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
const HELPER = 'C:\\ProgramData\\Dyno\\Helper';
const result = (title, checks, evidence = [], pass = '') => { const bad = checks.filter((c) => !c.ok); return { title, status: bad.length ? 'fail' : 'pass', summary: bad.length ? bad.map((c) => c.detail).join('; ').slice(0, 240) : pass, checks, evidence }; };


/** Dyno's "Start Dyno at login" setting as the Settings page shows it. */
async function launchAtLogin(browser) {
  const s = await openSettings(browser);
  if (!s) return 'Settings did not open';
  const box = s.getByRole('switch', { name: /Start Dyno at login/i }).or(s.getByRole('checkbox', { name: /Start Dyno at login/i }));
  if (!(await box.count())) return 'no "Start Dyno at login" control found';
  return (await box.first().getAttribute('aria-checked').catch(() => null)) ?? ((await box.first().isChecked().catch(() => false)) ? 'true' : 'false');
}

async function setLaunchAtLogin(browser, on) {
  const s = await openSettings(browser);
  const box = s.getByRole('switch', { name: /Start Dyno at login/i }).or(s.getByRole('checkbox', { name: /Start Dyno at login/i }));
  const now = (await box.first().getAttribute('aria-checked').catch(() => 'false')) === 'true';
  if (now !== on) await box.first().click();
  await sleep(2500);
}

/** What starts Dyno at logon: the Run key, the Startup folder, scheduled tasks and services; and what is running, with its start time against the logon time. */
export function autoStartRecord() {
  const run = ssh(`Get-ItemProperty 'HKCU:\\Software\\Microsoft\\Windows\\CurrentVersion\\Run' -ErrorAction SilentlyContinue | Select-Object * -ExcludeProperty PS* | Format-List | Out-String`);
  const startup = ssh(`Get-ChildItem "$env:APPDATA\\Microsoft\\Windows\\Start Menu\\Programs\\Startup" -ErrorAction SilentlyContinue | Select-Object -ExpandProperty Name`);
  const tasks = ssh(`schtasks /query /fo csv /nh 2>$null | Select-String -Pattern 'Dyno' | ForEach-Object { $_.Line }`, { admin: true });
  const logon = ssh(`(Get-Process -Name explorer -ErrorAction SilentlyContinue | Sort-Object StartTime | Select-Object -First 1).StartTime.ToString('s')`);
  const procs = ssh(`Get-Process dyno-app,dyno-agent -ErrorAction SilentlyContinue | ForEach-Object { "$($_.ProcessName) pid=$($_.Id) started=$($_.StartTime.ToString('s'))" }`);
  return { run, startup, tasks, logon, procs, running: /dyno-app|dyno-agent/.test(procs) };
}

/** Dyno's tray icons: in the visible taskbar area and the overflow (UI Automation), IsPromoted per icon, and the log's "tooltip sent" lines. */
export async function trayRecord(actor) {
  const promoted = ssh(`Get-ItemProperty 'HKCU:\\Control Panel\\NotifyIconSettings\\*' -ErrorAction SilentlyContinue | Where-Object { $_.ExecutablePath -like '*dyno-app.exe' } | ForEach-Object { "promoted=$($_.IsPromoted) tooltip=$($_.InitialTooltip)" }`).split('\n').map((l) => l.trim()).filter(Boolean);
  const visible = (await actor.send('tray Dyno').catch(() => '')).split('\n').map((l) => l.split('\t')[0].trim()).filter(Boolean);
  // the overflow: open the "Show hidden icons" chevron and read again
  const chevron = (await actor.send('tray ^(Show Hidden Icons|Hidden icons|Show hidden)').catch(() => '')).split('\n')[0]?.split('\t')[1];
  let overflow = [];
  if (chevron) { await actor.send(`click ${chevron.replace(',', ' ')}`).catch(() => {}); await sleep(1500); overflow = (await actor.send('tray Dyno').catch(() => '')).split('\n').map((l) => l.split('\t')[0].trim()).filter(Boolean).filter((n) => !visible.includes(n)); await actor.send('key ESC').catch(() => {}); }
  const sent = ssh(`Get-ChildItem "$env:LOCALAPPDATA\\Dyno\\logs" -Filter 'dyno.*.log' -ErrorAction SilentlyContinue | Select-String -Pattern 'tooltip sent' | Select-Object -Last 8 | ForEach-Object { $_.Line }`).split('\n').filter(Boolean);
  return { promoted, visible, overflow, sent };
}

/** The new crash and removal logging: Application log entries of source "Dyno helper", and the System service events for it. */
export function eventsDump() {
  const app = ssh(`Get-WinEvent -FilterHashtable @{ LogName = 'Application'; ProviderName = 'Dyno helper' } -MaxEvents 40 -ErrorAction SilentlyContinue | ForEach-Object { "$($_.TimeCreated.ToString('s')) id=$($_.Id) level=$($_.LevelDisplayName): $(($_.Message -replace '\\s+', ' ').Substring(0, [Math]::Min(300, $_.Message.Length)))" }`, { admin: true });
  const sys = ssh(`Get-WinEvent -FilterHashtable @{ LogName = 'System'; Id = 7034,7036,7040,7045 } -MaxEvents 200 -ErrorAction SilentlyContinue | Where-Object { $_.Message -match 'Dyno' } | Select-Object -First 25 | ForEach-Object { "$($_.TimeCreated.ToString('s')) id=$($_.Id): $(($_.Message -split '\\n')[0])" }`, { admin: true });
  return ['--- Application log, source "Dyno helper"', ...(app ? app.split('\n') : ['(none)']), '--- System 7034/7036/7040/7045 for Dyno', ...(sys ? sys.split('\n') : ['(none)'])];
}

/** The service as the OS and the helper's own log report it. */
export function snapshot() {
  const qc = ssh('sc.exe qc DynoHelper 2>&1 | Out-String', { admin: true });
  const q = ssh('sc.exe query DynoHelper 2>&1 | Out-String', { admin: true });
  const log = ssh(`Get-Content ${HELPER}\\helper.log -Tail 15 -ErrorAction SilentlyContinue`, { admin: true });
  const start = /START_TYPE\s*:\s*\d+\s+(\S+(?: \(\S+\))?)/.exec(qc)?.[1] ?? 'unknown';
  const state = /STATE\s*:\s*\d+\s+(\S+)/.exec(q)?.[1] ?? 'absent';
  return { qc, q, log, start, state };
}

async function cardState(browser) {
  const s = await openSettings(browser);
  if (!s) return 'Settings did not open';
  await s.getByRole('button', { name: /^Check again$/ }).first().click().catch(() => {});
  await sleep(3000);
  return (await cardText(s, 'Administrator readings')).slice(0, 220);
}

async function restartGuest(ctx) {
  ctx.log('restarting the guest (a real OS restart)');
  ssh('Set-ItemProperty "HKLM:\\SOFTWARE\\Microsoft\\Windows NT\\CurrentVersion\\Winlogon" AutoAdminLogon 1; shutdown /r /t 3', { admin: true, ms: 60_000 });
  await sleep(50_000);
  const ok = await ctx.waitConsole();
  if (ok) await sleep(45_000); // services and the logon's own start-ups settle
  return ok;
}

const fmtTray = (r) => [`visible taskbar area (UI Automation): ${r.visible.length} icon(s): ${r.visible.join(' | ') || '-'}`, `overflow: ${r.overflow.length} icon(s): ${r.overflow.join(' | ') || '-'}`, `NotifyIconSettings: ${r.promoted.join(' ; ') || '-'}`, `app log "tooltip sent" lines: ${r.sent.length}`, ...r.sent.map((l) => `  ${l.slice(0, 180)}`)];

/** After a restart, before Dyno is started by hand: what (if anything) started it, by which mechanism, how long after logon; and the tray. */
async function afterRestartRecord(actor, label) {
  const auto = autoStartRecord();
  actor.start(); await sleep(8000);
  const tray = await trayRecord(actor);
  await actor.send('quit', 5000).catch(() => {});
  const evidence = [`logon (explorer started): ${auto.logon}`, `running without a hand start: ${auto.procs || 'nothing'}`, '--- Run key', auto.run, `--- Startup folder: ${auto.startup || '(empty)'}`, `--- scheduled tasks naming Dyno: ${auto.tasks || '(none)'}`, '--- tray', ...fmtTray(tray)];
  return { auto, tray, evidence };
}

/** `ctx`: { vm(args, ms), waitConsole(), launchDyno(), log }. Returns the steps. */
export async function runRebootPhase(ctx) {
  const steps = [];
  try { await runRebootSteps(ctx, steps); } catch (e) { steps.push(result('Windows restart check: aborted', [{ ok: false, detail: String(e.message).split('\n')[0] }], [String(e.stack ?? e.message).slice(0, 1500)])); }
  return steps;
}

async function runRebootSteps(ctx, steps) {
  let t = tunnel();
  let browser = await connect();
  // The window can be replaced while the helper turns on (a closed page): reconnect once and retry.
  const safe = async (fn) => { const r = await retryOnClosed(fn, browser, async () => { t.close(); t = tunnel(); await sleep(3000); browser = await connect(); return browser; }); browser = r.handle; return r.value; };
  // The turn-on is not repeated if the window is replaced during it (the second press would be a second UAC): reconnect and judge the state instead.
  try { steps.push(await helperOn(browser)); } catch (e) {
    if (!/closed/i.test(String(e.message))) throw e;
    t.close(); t = tunnel(); await sleep(5000); browser = await connect();
    const s = snapshot();
    steps.push(result('Windows: admin helper Turn on (the app window was replaced during the turn-on; reconnected)', [{ ok: s.state === 'RUNNING', detail: `service ${s.state}, start type ${s.start} after reconnecting` }], [`first error: ${String(e.message).split('\n')[0]}`], 'service running after the window was replaced'));
  }
  const launchDefault = await safe(launchAtLogin);
  const before = snapshot();
  const cardBefore = await safe(cardState);
  const actor = new Actor();
  actor.start(); await sleep(6000);
  const trayBefore = await trayRecord(actor);
  await actor.send('quit', 5000).catch(() => {});
  steps.push(result('Windows restart: helper, Launch at login and tray before the restart', [
    { ok: before.state === 'RUNNING', detail: `service ${before.state}, start type ${before.start}` },
    { ok: /On/.test(cardBefore), detail: `card: ${cardBefore.slice(0, 80)}` },
    { ok: true, detail: `Launch at login after install: ${launchDefault === 'true' ? 'ON' : launchDefault === 'false' ? 'OFF' : launchDefault}` },
    { ok: trayBefore.visible.length + trayBefore.overflow.length > 0, detail: `${trayBefore.visible.length} tray icon(s) visible, ${trayBefore.overflow.length} in the overflow` },
  ], [before.qc, before.q, '--- helper.log tail', before.log, `card: ${cardBefore}`, ...eventsDump(), '--- tray before', ...fmtTray(trayBefore)].join('\n').split('\n'), `service RUNNING (${before.start}), card On`));
  await browser.close().catch(() => {});
  t.close();

  if (!(await restartGuest(ctx))) { steps.push(result('Windows restart: the guest did not come back to a GUI session', [{ ok: false, detail: 'no console session after the restart (6 min)' }])); return; }
  const rec1 = await afterRestartRecord(actor, 'default');
  const idle = snapshot();
  const delay = rec1.auto.running ? 'started by something at logon' : 'nothing started Dyno at logon';
  steps.push(result(`Windows restart: auto start with Launch at login ${launchDefault === 'true' ? 'ON' : 'OFF'} (the default after install)`, [
    { ok: true, detail: `${delay}: ${rec1.auto.procs.replace(/\n/g, '; ') || 'no Dyno process'}` },
    { ok: launchDefault === 'true' ? rec1.auto.running : true, detail: launchDefault === 'true' ? (rec1.auto.running ? 'Dyno started by itself at logon' : 'Launch at login is ON but Dyno did not start at logon') : 'Launch at login is off by default: Dyno is not expected to start by itself (tested with it on below)' },
  ], rec1.evidence, delay));
  steps.push(result('Windows restart: tray icons after the restart (compared with before)', [
    { ok: true, detail: `before: ${trayBefore.visible.length} visible, ${trayBefore.overflow.length} overflow; after (no hand start): ${rec1.tray.visible.length} visible, ${rec1.tray.overflow.length} overflow` },
    { ok: rec1.auto.running ? rec1.tray.visible.length + rec1.tray.overflow.length > 0 : true, detail: rec1.auto.running ? 'Dyno is running and its icons exist' : 'Dyno is not running at logon, so no icons are expected yet' },
  ], fmtTray(rec1.tray), 'recorded'));

  // The helper after the restart, with Dyno started by hand and no Repair.
  ctx.launchDyno();
  await sleep(25_000);
  t = tunnel();
  let card = 'no window';
  let readingText = '';
  try {
    browser = await connect();
    card = await cardState(browser);
    const main = browser.contexts().flatMap((c) => c.pages()).find((p) => !p.url().includes('dialog?id='));
    await main.evaluate(() => { location.hash = '#/processes'; }); await sleep(4000);
    readingText = (await main.evaluate(() => document.body.innerText)).replace(/\s+/g, ' ');
    await browser.close().catch(() => {});
  } catch (e) { card = `could not read the window: ${e.message}`; }
  t.close();
  const after = snapshot();
  const checks = [
    { ok: idle.state === 'RUNNING' || /DEMAND|AUTO/i.test(idle.start), detail: `after the restart, before Dyno runs: service ${idle.state}, start type ${idle.start}` },
    { ok: after.state === 'RUNNING', detail: `after Dyno started (no Repair): service ${after.state}, start type ${after.start}` },
    { ok: /On/.test(card) && !/Repair|Not running|Needs repair/.test(card), detail: `card after the restart: ${card.slice(0, 100)}` },
    { ok: /Read by the administrator helper/i.test(readingText), detail: /Read by the administrator helper/i.test(readingText) ? 'a helper reading (Processes) works after the restart' : 'the Processes page does not show the helper chip after the restart' },
  ];
  const acl = ssh(`"== C:\\ProgramData\\Dyno"; (Get-Acl "C:\\ProgramData\\Dyno").Owner; icacls "C:\\ProgramData\\Dyno"; "== ${HELPER}"; (Get-Acl "${HELPER}").Owner; icacls "${HELPER}"`, { admin: true });
  const ev = ssh(`Get-WinEvent -FilterHashtable @{ LogName = 'System'; Id = 7034,7036,7040,7045,1074,6005,6006 } -MaxEvents 300 -ErrorAction SilentlyContinue | Where-Object { $_.Id -in 1074,6005,6006 -or $_.Message -match 'Dyno' } | Select-Object -First 30 | ForEach-Object { "$($_.TimeCreated.ToString('s')) id=$($_.Id) $($_.ProviderName): $(($_.Message -split '\n')[0])" }`, { admin: true });
  const fail = ssh('sc.exe qfailure DynoHelper 2>&1 | Out-String; sc.exe qfailureflag DynoHelper 2>&1 | Out-String', { admin: true });
  const proc = ssh(`Get-WinEvent -FilterHashtable @{ LogName = 'Security'; Id = 4688 } -MaxEvents 400 -ErrorAction SilentlyContinue | Where-Object { $_.Message -match 'dyno-helper' } | Select-Object -First 10 | ForEach-Object { "$($_.TimeCreated.ToString('s')) $(($_.Message -split '\n' | Select-String 'New Process Name|Creator Process Name' | ForEach-Object { $_.Line.Trim() }) -join ' ; ')" }`, { admin: true });
  const evidence = ['--- after the restart, before Dyno runs', idle.qc, idle.q, '--- after Dyno started', after.qc, after.q, '--- helper.log tail', after.log, `card: ${card}`, `Processes text: ${readingText.slice(0, 600)}`, '--- ACLs (always recorded)', ...acl.split('\n'), ...eventsDump().map((l) => `[after restart] ${l}`), '--- System event log (service events 7034/7036/7040/7045, restart 1074/6005/6006)', ...ev.split('\n'), '--- sc qfailure DynoHelper', ...fail.split('\n'), '--- Security 4688 dyno-helper process creations (if auditing is on)', ...(proc ? proc.split('\n') : ['(none or auditing off)'])];
  steps.push(result('Windows restart: the helper works after a real restart without Repair or UAC', checks, evidence, 'service running, card On, readings work, no Repair'));

  // With Launch at login on (when it was off): does Dyno start by itself after a restart?
  if (launchDefault !== 'true') {
    t = tunnel(); browser = await connect();
    await setLaunchAtLogin(browser, true);
    const nowOn = await launchAtLogin(browser);
    await browser.close().catch(() => {}); t.close();
    if (await restartGuest(ctx)) {
      const rec2 = await afterRestartRecord(actor, 'on');
      steps.push(result('Windows restart: auto start with Launch at login switched ON', [
        { ok: nowOn === 'true', detail: `Launch at login after switching it on: ${nowOn}` },
        { ok: rec2.auto.running, detail: rec2.auto.running ? `Dyno started by itself at logon: ${rec2.auto.procs.replace(/\n/g, '; ')}` : 'Dyno did not start by itself at logon with Launch at login on' },
        { ok: rec2.tray.visible.length + rec2.tray.overflow.length > 0, detail: `tray after logon: ${rec2.tray.visible.length} visible, ${rec2.tray.overflow.length} overflow` },
      ], rec2.evidence, 'Dyno starts by itself and its tray icons exist'));
    } else steps.push(result('Windows restart (Launch at login on): the guest did not come back', [{ ok: false, detail: 'no console session after the second restart' }]));
  }
}
