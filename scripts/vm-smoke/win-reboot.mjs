// The restart check (high priority, from the user's real PC): after a Windows restart the admin helper must work without Repair or another UAC.
// 1. install and turn the helper on as the owner (UAC over VNC), 2. record the service (sc qc, sc query), the helper.log tail and the Settings card,
// 3. restart the guest (a real OS restart; the auto-logon is switched on again first), wait for the GUI session, 4. record the same again before Dyno
// runs and after Dyno is launched, with no Repair, and read a helper reading, 5. when it fails keep the ACLs and the System event log entries.
import { cardText, helperOn, openSettings, ssh } from './win-helper.mjs';
import { connect, tunnel } from './win-cdp.mjs';
import { Actor } from './win-act.mjs';
import { retryOnClosed } from './reconnect.mjs';
import { APP_LOG_PS, TRAY_ICONS_CMD, dynoTrayIcons, metricsFromLog, notificationAreaButtons, trayLogLines } from './win-tray-icons.mjs';
import { makeResult } from './verdict.mjs';
import { quitStopStep } from './mcp-judges.mjs';
import { judgeAutostart, judgeRestartService, judgeTrayCount, parseProcs, serviceState } from './win-judges.mjs';

const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
const HELPER = 'C:\\ProgramData\\Dyno\\Helper';
const result = makeResult;
const CDP_ENV = 'WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS';


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
  if (now !== on) { await box.first().click(); await s.getByRole('button', { name: /^Save changes$/ }).first().click({ timeout: 5000 }).catch(() => {}); } // the switch edits a draft
  for (let i = 0; i < 30 && (await s.getByText(/Applying…/).count().catch(() => 0)); i++) await sleep(1000);
  await sleep(2500);
}

/**
 * The guest's boot time as ISO "s" text, or null when no way could read it. The Windows 11 Arm guest answers `Get-CimInstance Win32_OperatingSystem` with
 * nothing, so the PowerShell below tries, in order: CIM; the WMI cmdlet; `wmic os get lastbootuptime` (the CIM/WMI date string); the performance
 * counter `System Up Time`; `systeminfo` "System Boot Time"). Event-log times are not used (they move with log clearing). It prints '<time>|<source>'; the first source that answers wins, so a working CIM stays what it was.
 */
export const BOOT_TIME_PS = `$ErrorActionPreference='SilentlyContinue'; $t=$null; $src='';
try { $t=(Get-CimInstance Win32_OperatingSystem).LastBootUpTime; if($t){$src='cim'} } catch {}
if(-not $t){ try { $t=(Get-WmiObject Win32_OperatingSystem).ConvertToDateTime((Get-WmiObject Win32_OperatingSystem).LastBootUpTime); if($t){$src='wmi'} } catch {} }
if(-not $t){ try { $w=(wmic os get lastbootuptime /value 2>$null | Out-String); if($w -match 'LastBootUpTime=(\\d{14})'){ $t=[datetime]::ParseExact($Matches[1],'yyyyMMddHHmmss',$null); $src='wmic' } } catch {} }
if(-not $t){ try { $u=(Get-Counter '\\System\\System Up Time' -ErrorAction Stop).CounterSamples[0].CookedValue; if($u){ $t=(Get-Date).AddSeconds(-[double]$u); $src='counter' } } catch {} }
if(-not $t){ try { $s=(systeminfo 2>$null | Select-String 'Boot Time|Systemstartzeit' | Select-Object -First 1).Line; if($s){ $t=[datetime]::Parse(($s -replace '^[^:]*:\\s*','').Trim()); $src='systeminfo' } } catch {} }
if($t){ $t.ToString('s') + '|' + $src }`;

/** Parses '<ISO s>|<source>' to { time, source } or null. The counter fallback has seconds-level jitter, so it is rounded to the minute only by the judge's "moved" test (a restart moves it by minutes). */
export function parseBootTime(text) {
  const m = /^(\d{4}-\d\d-\d\dT\d\d:\d\d:\d\d)\|(\w+)\s*$/m.exec(String(text ?? '').trim());
  return m ? { time: m[1], source: m[2] } : null;
}

/** The guest's boot time as ISO "s" text, or null when it could not be read (the source is kept in \`lastBootSource\`). */
export function bootTime() {
  // The standard user is refused by CIM, WMI and the counter API on this guest (Access denied): the administrator key reads the boot time.
  const p = parseBootTime(ssh(BOOT_TIME_PS.replace(/\n/g, ' '), { admin: true }));
  bootTime.lastSource = p?.source ?? null;
  return p ? p.time : null;
}

/** What starts Dyno at logon: the Run key, the Startup folder, scheduled tasks and services; and what is running, with its start time against the logon time. */
export function autoStartRecord() {
  const run = ssh(`Get-ItemProperty 'HKCU:\\Software\\Microsoft\\Windows\\CurrentVersion\\Run' -ErrorAction SilentlyContinue | Select-Object * -ExcludeProperty PS* | Format-List | Out-String`);
  const startup = ssh(`Get-ChildItem "$env:APPDATA\\Microsoft\\Windows\\Start Menu\\Programs\\Startup" -ErrorAction SilentlyContinue | Select-Object -ExpandProperty Name`);
  const tasks = ssh(`schtasks /query /fo csv /nh 2>$null | Select-String -Pattern 'Dyno' | ForEach-Object { $_.Line }`, { admin: true });
  const logon = ssh(`(Get-Process -Name explorer -ErrorAction SilentlyContinue | Sort-Object StartTime | Select-Object -First 1).StartTime.ToString('s')`);
  // dyno-app and dyno-agent with their command lines (--hidden shows how the app was started) and the boot time (what "started at logon" is measured from).
  // Read by the administrator: this guest refuses CIM to the standard user and answers with nothing, which read as "nothing started" on the 7d06 and 0ad rounds while the
  // app log showed the autostart (08:18:50, --hidden). A count by Get-Process (no CIM) is kept beside it as a cross-check.
  const procs = ssh(`Get-CimInstance Win32_Process -Filter "Name='dyno-app.exe' OR Name='dyno-agent.exe'" -ErrorAction SilentlyContinue | ForEach-Object { "$($_.Name -replace '\\.exe$','') pid=$($_.ProcessId) started=$($_.CreationDate.ToString('s')) cmd=$($_.CommandLine)" }`, { admin: true });
  const plain = ssh('(Get-Process dyno-app,dyno-agent -ErrorAction SilentlyContinue | ForEach-Object { $_.Name + " " + $_.Id }) -join ", "').trim();
  const boot = bootTime();
  const rows = parseProcs(procs);
  // The control for "nothing started": did Windows start ANY Run-key program at this logon, and is the Dyno value approved (StartupApproved\Run byte 0: 02/06 on, 03/07 off)?
  const others = ssh("Get-Process OneDrive,msedge -ErrorAction SilentlyContinue | Where-Object { $_.StartTime -gt (Get-Process explorer | Sort-Object StartTime | Select-Object -First 1).StartTime.AddSeconds(-5) } | ForEach-Object { $_.Name } | Sort-Object -Unique").split('\n').map((l) => l.trim()).filter(Boolean);
  const approvedRaw = ssh("$v = (Get-ItemProperty 'HKCU:\\Software\\Microsoft\\Windows\\CurrentVersion\\Explorer\\StartupApproved\\Run' -ErrorAction SilentlyContinue).Dyno; if ($v) { '{0:X2}' -f $v[0] } else { 'none' }").trim().split('\n').at(-1);
  const approved = /^0[37]$/.test(approvedRaw) ? 'disabled' : 'enabled';
  // `running` is the APP started after the boot, never the agent alone (the agent is what the helper service keeps up).
  return { run, startup, tasks, logon, procs, plain, boot, rows, control: { othersStarted: others, approved, approvedRaw }, running: rows.some((r) => r.name === 'dyno-app' && boot !== null && r.started > boot) };
}

/**
 * Dyno's tray icons: in the visible taskbar area and the overflow (UI Automation, explorer-owned buttons named after the metrics the app
 * log says it created, never Dyno's own window buttons: win-tray-icons.mjs), the NotifyIconSettings entries, and the app's tray log lines.
 */
export async function trayRecord(actor) {
  const promoted = ssh(`Get-ItemProperty 'HKCU:\\Control Panel\\NotifyIconSettings\\*' -ErrorAction SilentlyContinue | Where-Object { $_.ExecutablePath -like '*dyno-app.exe' } | ForEach-Object { "$($_.PSChildName) promoted=$($_.IsPromoted) tooltip=$($_.InitialTooltip) uid=$($_.UID) guid=$($_.IconGuid)" }`).split('\n').map((l) => l.trim()).filter(Boolean);
  const log = ssh(APP_LOG_PS);
  const metrics = metricsFromLog(log);
  const errors = [];
  // An actor that does not answer is a measurement failure, never "0 icons": its error is recorded.
  const read = async () => (await actor.send(TRAY_ICONS_CMD).catch((e) => { errors.push(String(e.message ?? e)); return ''; }));
  const rows1 = await read();
  const first = dynoTrayIcons(rows1, metrics);
  const visible = first.filter((i) => i.where === 'taskbar').map((i) => i.name);
  // the overflow: open the "Show hidden icons" chevron (an explorer button) and read again
  const chevron = (await actor.send('tray ^(Show Hidden Icons|Hidden icons|Show hidden)').catch(() => '')).split('\n')[0]?.split('\t')[1];
  let overflow = [];
  let rows2 = '';
  if (chevron) {
    await actor.send(`click ${chevron.replace(',', ' ')}`).catch(() => {});
    await sleep(1500);
    rows2 = await read();
    overflow = dynoTrayIcons(rows2, metrics).filter((i) => i.where === 'overflow' && !visible.includes(i.name)).map((i) => i.name);
    await actor.send('key ESC').catch(() => {});
  }
  const lines = trayLogLines(log);
  const buttons = [...new Set([...notificationAreaButtons(rows1), ...notificationAreaButtons(rows2)])];
  return { promoted, visible, overflow, metrics, logFile: [...log.matchAll(/^@@ (.*)$/gm)].map((m) => m[1]).join(', ') || '(none)', lines, buttons, errors, measured: buttons.length > 0, registered: promoted.length };
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

/**
 * A real OS restart. The service state is sampled from the first moment the guest answers (a boot-time race shows as a STOP after a RUN), the
 * boot time is compared before and after, and the CDP environment of the user is set BEFORE the restart so the autostarted instance has a
 * DevTools port and is the one that is read afterwards.
 * @returns {Promise<{up:boolean, bootBefore:string|null, bootAfter:string|null, samples:{t:number,state:string}[]}>}
 */
async function restartGuest(ctx) {
  ctx.log('restarting the guest (a real OS restart)');
  const bootBefore = bootTime();
  // No user-wide DevTools variable: every WebView2 program would get it and the first (the Widgets board) takes the port (0ad round 2). Dyno is restarted with the port in
  // its own environment after the autostart has been recorded.
  ssh(`[Environment]::SetEnvironmentVariable('${CDP_ENV}', $null, 'User')`);
  ssh('Set-ItemProperty "HKLM:\\SOFTWARE\\Microsoft\\Windows NT\\CurrentVersion\\Winlogon" AutoAdminLogon 1; shutdown /r /t 3', { admin: true, ms: 60_000 });
  await sleep(20_000);
  const samples = [];
  const t0 = Date.now();
  let up = false;
  for (let i = 0; i < 100 && !up; i++) {
    const state = serviceState(ssh('sc.exe query DynoHelper 2>&1 | Out-String', { admin: true, ms: 20_000 }));
    samples.push({ t: Math.round((Date.now() - t0) / 1000), state });
    up = state !== 'unreachable' && (await ctx.waitConsole(1));
    if (!up) await sleep(3000);
  }
  if (up) {
    for (let i = 0; i < 9; i++) { await sleep(5000); samples.push({ t: Math.round((Date.now() - t0) / 1000), state: serviceState(ssh('sc.exe query DynoHelper 2>&1 | Out-String', { admin: true, ms: 20_000 })) }); }
  }
  return { up, bootBefore, bootAfter: bootTime(), samples };
}

const fmtTray = (r) => [`visible taskbar area (UI Automation, explorer buttons named after a Dyno metric): ${r.visible.length} icon(s): ${r.visible.join(' | ') || '-'}`, `overflow: ${r.overflow.length} icon(s): ${r.overflow.join(' | ') || '-'}`, `metrics the app log says it created icons for: ${r.metrics.join(', ') || '(none logged)'}`, `NotifyIconSettings (dyno-app.exe): ${r.promoted.join(' ; ') || '-'}`, `app log ${r.logFile}: ${r.lines.length} tray line(s)`, ...r.lines.slice(-60).map((l) => `  ${l.slice(0, 220)}`), `measurement: ${r.measured ? 'valid (the notification area was read)' : `NOT VALID: no notification-area button was read at all${r.errors.length ? ` (actor: ${r.errors[0]})` : ''}`}`, `notification-area buttons seen (name, class, top-level class): ${r.buttons.length}`, ...r.buttons.slice(0, 60).map((b) => `  ${b.slice(0, 200)}`)];

/** After a restart, before Dyno is started by hand: what (if anything) started it, by which mechanism, how long after logon; and the tray. */
async function afterRestartRecord(actor, label) {
  // Windows starts Run-key programs a little after the logon (its startup delay, longer under x64 emulation on this Arm guest): a condition wait of up to 120 s
  // for Dyno to run before "nothing started it" is concluded.
  let auto = autoStartRecord();
  for (let i = 0; i < 12 && !auto.running; i++) { await sleep(10_000); auto = autoStartRecord(); }
  // After a restart the in-session actor may not start at once (the console session is still being set up): start it again until it
  // answers, bounded, and record how long that took.
  let actorUp = null;
  for (let i = 0; i < 8 && actorUp === null; i++) {
    actor.start(); await sleep(8000);
    if ((await actor.send('windows', 20_000).then(() => true).catch(() => false))) actorUp = i;
  }
  let tray = await trayRecord(actor);
  // An autostarted app builds its tray icons a little after it starts: poll up to 60 s when it is running and nothing shows yet.
  for (let i = 0; i < 6 && auto.running && (!tray.measured || tray.visible.length + tray.overflow.length === 0); i++) { await sleep(10_000); tray = await trayRecord(actor); }
  await actor.send('quit', 5000).catch(() => {});
  const evidence = [`control: Run-key programs started at this logon: ${auto.control.othersStarted.join(', ') || 'none'}; StartupApproved\\Run "Dyno" first byte: ${auto.control.approvedRaw}`, `logon (explorer started): ${auto.logon}`, `running without a hand start: ${auto.procs || 'nothing'} (Get-Process: ${auto.plain || 'none'})`, '--- Run key', auto.run, `--- Startup folder: ${auto.startup || '(empty)'}`, `--- scheduled tasks naming Dyno: ${auto.tasks || '(none)'}`, `--- actor: ${actorUp === null ? 'did NOT answer after 8 starts' : `answered after ${actorUp + 1} start(s)`}`, '--- tray', ...fmtTray(tray)];
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
    { ok: true, info: true, detail: `Launch at login after install: ${launchDefault === 'true' ? 'ON' : launchDefault === 'false' ? 'OFF' : launchDefault}` },
    // Counted before any icon is promoted: with Dyno's window open (Settings) every icon the log says was created must be found, and no button of Dyno's own window counts.
    ...judgeTrayCount({ metrics: trayBefore.metrics, visible: trayBefore.visible.length, overflow: trayBefore.overflow.length, measured: trayBefore.measured, registered: trayBefore.registered }),
  ], [before.qc, before.q, '--- helper.log tail', before.log, `card: ${cardBefore}`, ...eventsDump(), '--- tray before', ...fmtTray(trayBefore)].join('\n').split('\n'), `service RUNNING (${before.start}), card On`));
  // The 7521 death at app quit: read Processes through the helper, quit Dyno (File > Quit Dyno), and see whether the helper is still RUNNING 30 s later.
  try {
    const main = browser.contexts().flatMap((c) => c.pages()).find((p) => !p.url().includes('dialog?id='));
    await main.evaluate(() => { location.hash = '#/processes'; }); await sleep(4000);
    const chip = /Read by the administrator helper/i.test(await main.evaluate(() => document.body.innerText));
    const pre = snapshot();
    await main.evaluate(async () => {
      const quit = (e) => /^Quit Dyno( \(keeps measuring\))?$/.test(e.textContent.trim()); // 1.1.0: never "Quit and stop measuring"
      const items = () => [...document.querySelectorAll('[role=menuitem]')];
      if (!items().some(quit)) { [...document.querySelectorAll('button')].find((b) => b.textContent.trim() === 'File').click(); await new Promise((r) => setTimeout(r, 600)); }
      items().find(quit).click();
    }).catch(() => {});
    await sleep(30_000);
    const post = snapshot();
    const apps = ssh('(Get-Process dyno-app,dyno-agent -ErrorAction SilentlyContinue | Measure-Object).Count');
    steps.push(result('Windows restart: the helper after Dyno is quit (the 7521 death at app quit)', [
      { ok: chip, detail: chip ? 'Processes was read through the helper before the quit' : 'Processes did not show the helper chip before the quit' },
      { ok: pre.state === 'RUNNING', detail: `helper before the quit: ${pre.state}` },
      { ok: post.state === 'RUNNING', detail: `helper 30 s after the quit: ${post.state} (Dyno processes left: ${apps})` },
    ], [post.qc, post.q, '--- helper.log tail', post.log, ...eventsDump().map((l) => `[after quit] ${l}`)], 'the helper keeps running after Dyno quits'));
  } catch (e) { steps.push(result('Windows restart: the helper after Dyno is quit', [{ ok: false, detail: String(e.message).split('\n')[0] }])); }
  await browser.close().catch(() => {});
  t.close();

  const rs1 = await restartGuest(ctx);
  if (!rs1.up) { steps.push(result('Windows restart: the guest did not come back to a GUI session', [{ ok: false, detail: 'no console session after the restart (6 min)' }])); return; }
  const rec1 = await afterRestartRecord(actor, 'default');
  const expectOn = launchDefault === 'true';
  const delay = rec1.auto.running ? 'started by something at logon' : 'nothing started Dyno at logon';
  steps.push(result(`Windows restart: auto start with Launch at login ${expectOn ? 'ON' : 'OFF'} (the default after install)`, [
    { ok: true, info: true, detail: `${delay}: ${rec1.auto.procs.replace(/\n/g, '; ') || 'no Dyno process'}` },
    // The setting must have been read (true or false), else the expectation below is a guess.
    ...(launchDefault === 'true' || launchDefault === 'false' ? [] : [{ ok: false, kind: 'check', detail: `Launch at login could not be read after the install (${launchDefault})` }]),
    ...judgeAutostart({ boot: rec1.auto.boot, procs: rec1.auto.rows, expectOn, control: rec1.auto.control }),
  ], rec1.evidence, expectOn ? 'dyno-app started by itself after the boot with --hidden' : 'Dyno is off at logon as configured'));
  steps.push(result('Windows restart: tray icons after the restart (compared with before)', [
    { ok: true, info: true, detail: `before: ${trayBefore.visible.length} visible, ${trayBefore.overflow.length} overflow; after (no hand start): ${rec1.tray.visible.length} visible, ${rec1.tray.overflow.length} overflow` },
    ...judgeTrayCount({ metrics: rec1.tray.metrics, visible: rec1.tray.visible.length, overflow: rec1.tray.overflow.length, measured: rec1.tray.measured, registered: rec1.tray.registered, running: rec1.auto.running }),
  ], fmtTray(rec1.tray), 'every icon the app created is in the notification area'));

  // The helper after the restart, with no Repair. The CDP environment was set for the user BEFORE the restart, so an autostarted Dyno has its
  // DevTools port and is the instance that is read; only when nothing started Dyno is it started by hand, as a user would reopen it.
  // The autostarted instance has no DevTools port (and must not: see restartGuest). After it was recorded, Dyno is reopened with the port in its own environment, as a person
  // reopens it; the helper rows below read that instance. Waited on the window, not on a fixed delay.
  ssh('Get-Process dyno-app,dyno-agent -ErrorAction SilentlyContinue | Stop-Process -Force; Start-Sleep 3');
  ctx.launchDyno();
  for (let i = 0; i < 20; i++) {
    await sleep(2000);
    if (ssh('(Get-Process dyno-app -ErrorAction SilentlyContinue | Where-Object { $_.MainWindowHandle -ne 0 } | Measure-Object).Count').trim() !== '0') break;
  }
  await sleep(5000);
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
  const cardRead = !/^(no window|could not read|Settings did not open)/.test(card);
  const checks = [
    ...judgeRestartService({ bootBefore: rs1.bootBefore, bootAfter: rs1.bootAfter, samples: rs1.samples }),
    { ok: after.state === 'RUNNING', detail: `after Dyno ${rec1.auto.running ? 'autostarted' : 'was started'} (no Repair): service ${after.state}, start type ${after.start}` },
    { ok: cardRead && /On/.test(card) && !/Repair|Not running|Needs repair/.test(card), ...(cardRead ? {} : { kind: 'check' }), detail: `card after the restart: ${card.slice(0, 100)}` },
    { ok: /Read by the administrator helper/i.test(readingText), ...(cardRead ? {} : { kind: 'check' }), detail: /Read by the administrator helper/i.test(readingText) ? 'a helper reading (Processes) works after the restart' : 'the Processes page does not show the helper chip after the restart' },
  ];
  const acl = ssh(`"== C:\\ProgramData\\Dyno"; (Get-Acl "C:\\ProgramData\\Dyno").Owner; icacls "C:\\ProgramData\\Dyno"; "== ${HELPER}"; (Get-Acl "${HELPER}").Owner; icacls "${HELPER}"`, { admin: true });
  const ev = ssh(`Get-WinEvent -FilterHashtable @{ LogName = 'System'; Id = 7034,7036,7040,7045,1074,6005,6006 } -MaxEvents 300 -ErrorAction SilentlyContinue | Where-Object { $_.Id -in 1074,6005,6006 -or $_.Message -match 'Dyno' } | Select-Object -First 30 | ForEach-Object { "$($_.TimeCreated.ToString('s')) id=$($_.Id) $($_.ProviderName): $(($_.Message -split '\n')[0])" }`, { admin: true });
  const fail = ssh('sc.exe qfailure DynoHelper 2>&1 | Out-String; sc.exe qfailureflag DynoHelper 2>&1 | Out-String', { admin: true });
  const proc = ssh(`Get-WinEvent -FilterHashtable @{ LogName = 'Security'; Id = 4688 } -MaxEvents 400 -ErrorAction SilentlyContinue | Where-Object { $_.Message -match 'dyno-helper' } | Select-Object -First 10 | ForEach-Object { "$($_.TimeCreated.ToString('s')) $(($_.Message -split '\n' | Select-String 'New Process Name|Creator Process Name' | ForEach-Object { $_.Line.Trim() }) -join ' ; ')" }`, { admin: true });
  const evidence = [`--- DynoHelper service samples from the first moment the guest answered (seconds: state): ${rs1.samples.map((x) => `${x.t}: ${x.state}`).join(', ')}`, `boot time before ${rs1.bootBefore}, after ${rs1.bootAfter}`, '--- after Dyno started', after.qc, after.q, '--- helper.log tail', after.log, `card: ${card}`, `Processes text: ${readingText.slice(0, 600)}`, '--- ACLs (always recorded)', ...acl.split('\n'), ...eventsDump().map((l) => `[after restart] ${l}`), '--- System event log (service events 7034/7036/7040/7045, restart 1074/6005/6006)', ...ev.split('\n'), '--- sc qfailure DynoHelper', ...fail.split('\n'), '--- Security 4688 dyno-helper process creations (if auditing is on)', ...(proc ? proc.split('\n') : ['(none or auditing off)'])];
  steps.push(result('Windows restart: the helper works after a real restart without Repair or UAC', checks, evidence, 'service running, card On, readings work, no Repair'));

  // With Launch at login on (when it was off): does Dyno start by itself after a restart?
  if (launchDefault !== 'true') {
    t = tunnel(); browser = await connect();
    await setLaunchAtLogin(browser, true);
    const nowOn = await launchAtLogin(browser);
    await browser.close().catch(() => {}); t.close();
    const rs2 = await restartGuest(ctx);
    if (rs2.up) {
      const rec2 = await afterRestartRecord(actor, 'on');
      steps.push(result('Windows restart: auto start with Launch at login switched ON', [
        { ok: nowOn === 'true', ...(nowOn === 'true' || nowOn === 'false' ? {} : { kind: 'check' }), detail: `Launch at login after switching it on: ${nowOn}` },
        ...judgeAutostart({ boot: rec2.auto.boot, procs: rec2.auto.rows, expectOn: true, control: rec2.auto.control }),
        ...judgeRestartService({ bootBefore: rs2.bootBefore, bootAfter: rs2.bootAfter, samples: rs2.samples }),
      ], rec2.evidence, 'dyno-app started by itself after the boot with --hidden and the helper service ran'));
      steps.push(result('Windows restart: tray icons after the restart (Launch at login ON)', judgeTrayCount({ metrics: rec2.tray.metrics, visible: rec2.tray.visible.length, overflow: rec2.tray.overflow.length, measured: rec2.tray.measured, registered: rec2.tray.registered, running: rec2.auto.running }), fmtTray(rec2.tray), 'every icon the app created is in the notification area'));
    } else steps.push(result('Windows restart (Launch at login on): the guest did not come back', [{ ok: false, detail: 'no console session after the second restart' }]));
  }
  // Rule 71: Quit and stop measuring with its reboot half (the release row): File > Quit and stop measuring over the DevTools port, 60 s with no Dyno process, reopen,
  // then a real restart: the Run-key entry must bring the agent back (Quit and stop does not remove the start at login). Polled up to 150 s after the console session.
  try { steps.push(await quitStopWithRestart(ctx)); } catch (e) { steps.push(result('Rule 71: Quit and stop measuring stops the agent and it comes back', [{ ok: false, kind: 'check', detail: `the step broke: ${String(e.message).split('\n')[0]}` }])); }
  ssh(`[Environment]::SetEnvironmentVariable('${CDP_ENV}', $null, 'User')`);
  // The first uninstall after this boot (fix/uninstall-cold-leftover): the guest has just restarted.
  { const { coldUninstallStep } = await import('./win-machine.mjs'); steps.push(await coldUninstallStep()); }
}

const DYNO_COUNT = '(Get-Process dyno-app,dyno-agent -ErrorAction SilentlyContinue | Measure-Object).Count';
const AGENT_COUNT = '(Get-Process dyno-agent -ErrorAction SilentlyContinue | Measure-Object).Count';

/** Quit and stop measuring, then reopen, then a real restart (`restartGuest`); the agent count after the restart, polled while Windows runs its Run-key entries. */
async function quitStopWithRestart(ctx) {
  const count = async () => Number(ssh(DYNO_COUNT).trim().split('\n').at(-1)) || 0;
  const r = await quitStopStep({
    now: () => Date.now(), sleep,
    trigger: async () => {
      // A fresh Dyno with its DevTools port (the instance before may be one without it).
      ssh('Get-Process dyno-app,dyno-agent -ErrorAction SilentlyContinue | Stop-Process -Force; Start-Sleep 3');
      ctx.launchDyno(); await sleep(20_000);
      const t = tunnel();
      try {
        const browser = await connect();
        // The main window is the page with the app's File menu (a hidden start or a dialog page has none): polled up to 20 s.
        let main = null;
        for (let i = 0; i < 10 && !main; i++) {
          for (const p of browser.contexts().flatMap((c) => c.pages()).filter((x) => !x.url().includes('dialog?id='))) if (await p.evaluate(() => [...document.querySelectorAll('button')].some((b) => b.textContent.trim() === 'File')).catch(() => false)) { main = p; break; }
          if (!main) await sleep(2000);
        }
        if (!main) throw new Error(`no Dyno window with a File menu over the DevTools port (pages: ${browser.contexts().flatMap((c) => c.pages()).map((p) => p.url()).join(', ').slice(0, 200)})`);
        const clicked = await main.evaluate(async () => {
          const items = () => [...document.querySelectorAll('[role=menuitem]')];
          if (!items().some((e) => e.textContent.trim().startsWith('Quit and stop'))) { [...document.querySelectorAll('button')].find((b) => b.textContent.trim() === 'File')?.click(); await new Promise((r) => setTimeout(r, 600)); }
          const it = items().find((e) => e.textContent.trim().startsWith('Quit and stop'));
          if (!it) return false;
          it.click(); return true;
        }).catch(() => true); // the page goes away with the app
        if (!clicked) throw new Error('no "Quit and stop measuring" item in the File menu');
        await browser.close().catch(() => {});
      } finally { t.close(); }
    },
    count,
    reopen: async () => { ctx.launchDyno(); for (let i = 0; i < 10; i++) { await sleep(5000); if (Number(ssh(AGENT_COUNT).trim().split('\n').at(-1)) > 0) break; } return Number(ssh(AGENT_COUNT).trim().split('\n').at(-1)) || 0; },
    reboot: async () => {
      const rs = await restartGuest(ctx);
      if (!rs.up) return null;
      let n = 0;
      for (let i = 0; i < 30 && n === 0; i++) { n = Number(ssh(AGENT_COUNT).trim().split('\n').at(-1)) || 0; if (!n) await sleep(5000); }
      ctx.log(`quit-stop restart: boot ${rs.bootBefore} -> ${rs.bootAfter}; dyno-agent processes after the restart: ${n}`);
      return rs.bootBefore && rs.bootAfter && rs.bootBefore === rs.bootAfter ? null : n;
    },
  });
  r.evidence = [...(r.evidence ?? []), '--- after the restart', autoStartRecord().procs || '(no Dyno process)'];
  return r;
}
