// The Windows admin helper steps (Dyno 1.1.0): Turn on through the real UAC consent over VNC, the helper's own folders must not be writable
// by a standard user (the 7501 release blocker: C:\ProgramData\Dyno\Helper inherited Users create rights, and the SYSTEM helper installed
// smartctl into a user-owned tree), disk health install, Turn off. Each step returns { title, status, summary, checks, evidence }. They are `extra`
// steps of win-ui.mjs's runUiPhase and need run-win.mjs --vnc (the VM's VNC) and --ui (the DevTools port).
import { APP_LOG_PS, TRAY_ICONS_CMD, dynoTrayIcons, metricsFromLog, trayLogLines } from './win-tray-icons.mjs';
import { spawnSync } from 'node:child_process';
import { randomBytes } from 'node:crypto';
import { Vnc } from './win-vnc.mjs';
import { evidenceFile } from './evidence.mjs';
import { makeResult } from './verdict.mjs';
import { makeMcpGuest } from './mcp-guest.mjs';
import { mcpStep } from './mcp-judges.mjs';
import { NOT_DRIVEN, ROW_TITLES, notDriven } from './release-rows.mjs';
import { judgeJobsRestart, parseConsent, judgeConsentAfterAnswer, installEvents, installOutcomeSince, attemptProbe, judgeControl, judgeExploitPrecondition, judgeInstallOutcome, judgeRefusal, judgeServed, judgeTrayCount, judgeWear } from './win-judges.mjs';

const VMWIN = process.env.PADDOCK_VM_WIN ?? '/Volumes/d7sst7/dev-targets/scratch/dyno-vm-t/vm-win.sh';
const HELPER = 'C:\\ProgramData\\Dyno\\Helper';
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
let runPassword = null; // the per-run random password of paddock-admin (reverted with the VM); never stored or printed
const clean = (s) => s.split('\n').filter((l) => !l.startsWith('**')).join('\n').trim();
export const ssh = (cmd, { admin = false, input, ms = 90_000 } = {}) => clean(spawnSync('bash', [VMWIN, 'ssh', ...(admin ? ['--admin'] : []), cmd], { encoding: 'utf8', timeout: ms, input }).stdout ?? '');
const result = makeResult;
const settingsPage = (browser) => browser.contexts().flatMap((c) => c.pages()).find((p) => p.url().includes('pd.settings'));

export async function openSettings(browser) {
  let s = settingsPage(browser);
  if (!s) {
    const main = browser.contexts().flatMap((c) => c.pages()).find((p) => !p.url().includes('dialog?id='));
    await main.evaluate(async () => {
      const items = () => [...document.querySelectorAll('[role=menuitem]')];
      if (!items().some((e) => e.textContent.trim().startsWith('Settings'))) { [...document.querySelectorAll('button')].find((b) => b.textContent.trim() === 'File').click(); await new Promise((r) => setTimeout(r, 600)); }
      items().find((e) => e.textContent.trim().startsWith('Settings')).click();
    });
    for (let i = 0; i < 30 && !(s = settingsPage(browser)); i++) await sleep(500);
  }
  await s?.getByRole('button', { name: /^Dyno$/ }).click().catch(() => {});
  await sleep(800);
  return s;
}

export const cardText = (s, heading) => s.evaluate((n) => { const h = [...document.querySelectorAll('h1,h2,h3,h4')].find((e) => e.textContent.trim().startsWith(n)); const c = h?.closest('section,article,div[class*=card],pd-card') ?? h?.parentElement?.parentElement; return (c?.innerText ?? '').replace(/\s+/g, ' '); }, heading);

/** The per-run password of paddock-admin: lowercase and digits (VNC typing drops the shift state), set once, never stored or printed. */
export function ensureAdminPassword() {
  if (runPassword) return runPassword;
  const pw = randomBytes(12).toString('hex').replace(/[A-F]/g, '').slice(0, 20) + 'x';
  ssh('$p = [Console]::In.ReadLine(); net user paddock-admin $p | Out-Null', { admin: true, input: `${pw}\n` });
  runPassword = pw;
  return pw;
}

/**
 * Answers the UAC credential prompt (consent.exe on the secure desktop) over the VM's VNC with the administrator's per-run password: waits (bounded)
 * for the prompt, takes a screenshot as evidence, types the credentials, and waits (bounded) for the prompt to go. Returns { ok, detail }.
 */
const CONSENT_PS = `Get-CimInstance Win32_Process -Filter "Name='consent.exe'" | ForEach-Object { 'pid=' + $_.ProcessId + ' start=' + $_.CreationDate.ToString('s') + ' parent=' + $_.ParentProcessId + ' cmd=' + $_.CommandLine }`;
/** Every elevation prompt is accounted for: the consent.exe rows (pid, start, parent) and the installer processes at the answer and after it go into the detail, so a prompt after the answer is judged on evidence. */
export async function answerUac(label, { vnc = new Vnc(), seenSec = 90, goneSec = 60 } = {}) {
  const pw = ensureAdminPassword();
  const prompt = () => ssh('if (Get-Process consent -ErrorAction SilentlyContinue) { "yes" } else { "no" }', { admin: true }) === 'yes';
  let seen = false;
  for (let i = 0; i < seenSec / 3 && !seen; i++) { seen = prompt(); if (!seen) await sleep(1500); }
  if (!seen) return { ok: false, detail: `${label}: no UAC prompt (consent.exe) within ${seenSec} s` };
  await sleep(1500);
  const answered = parseConsent(ssh(CONSENT_PS, { admin: true }));
  const shotFile = evidenceFile(`win-uac-${label}`);
  const shot = vnc.capture(shotFile).code === 0 ? `screenshot ${shotFile}` : 'VNC capture failed';
  vnc.key('ctrl-a'); vnc.key('bsp'); vnc.type(pw); vnc.key('enter');
  let gone = false;
  for (let i = 0; i < goneSec / 3 && !gone; i++) { await sleep(1500); gone = !prompt(); }
  if (!gone) {
    // Still a consent.exe after the answer. The process table decides (judgeConsentAfterAnswer): the same consent.exe = the first prompt lingering or not taken, another = a second elevation.
    const later = parseConsent(ssh(CONSENT_PS, { admin: true }));
    const tree = ssh(`Get-CimInstance Win32_Process | Where-Object { $_.Name -match '^(setup|paddock-setup|consent|dyno-helper|dyno-app)\\.exe$' } | ForEach-Object { $_.Name + ' pid=' + $_.ProcessId + ' parent=' + $_.ParentProcessId + ' ' + $_.CommandLine }`, { admin: true }).split('\n').join(' | ');
    const j = judgeConsentAfterAnswer(answered, later);
    const second = evidenceFile(`win-uac-${label}-second`);
    const shot2 = vnc.capture(second).code === 0 ? `screenshot ${second}` : 'VNC capture failed';
    vnc.key('ctrl-a'); vnc.key('bsp'); vnc.type(pw); vnc.key('enter');
    for (let i = 0; i < goneSec / 3 && !gone; i++) { await sleep(1500); gone = !prompt(); }
    const ev = `${j.detail}; process tree: ${tree.slice(0, 700)}`;
    return { ok: gone, second: j.second, detail: gone ? `${label}: the UAC prompt was answered (${shot}); a prompt was still listed ${goneSec} s later (${shot2}) and was answered again; ${ev}` : `${label}: a UAC prompt was still there after two answers (${shot}; ${shot2}); ${ev}` };
  }
  return { ok: gone, detail: gone ? `${label}: the UAC prompt appeared (${shot}) and was answered over VNC (consent.exe ${answered.map((c) => `pid ${c.pid}`).join(', ') || 'unlisted'}, gone within ${goneSec} s)` : `${label}: the UAC prompt was still there ${goneSec} s after the answer (${shot})` };
}

/** Turn on: the UAC credential prompt (the test account is a standard user) is answered over VNC with a per-run password for paddock-admin. */
export async function helperOn(browser, { vnc = new Vnc() } = {}) {
  const checks = [];
  const evidence = [];
  const s = await openSettings(browser);
  checks.push({ ok: Boolean(s), detail: s ? 'opened Settings > Dyno' : 'the Settings dialog did not open' });
  if (!s) return result('Windows: admin helper Turn on (UAC consent over VNC)', checks);
  const pw = ensureAdminPassword();
  await s.getByRole('button', { name: /Turn on administrator readings/ }).click();
  // The same answer as Repair and the install: wait for consent.exe (the UAC prompt) to exist, keep its screenshot in the evidence folder, type the credentials, wait for it to go.
  const uac = await answerUac('turn-on', { vnc });
  checks.push({ ok: uac.ok, detail: uac.detail });
  evidence.push(uac.detail);
  void pw;
  let status = '';
  for (let i = 0; i < 30 && status !== 'Running'; i++) { await sleep(3000); status = ssh('(Get-Service DynoHelper -ErrorAction SilentlyContinue).Status', { admin: true }); }
  checks.push({ ok: status === 'Running', detail: status === 'Running' ? 'the DynoHelper service is Running' : `the DynoHelper service is "${status || 'absent'}" 90 s after the UAC answer` });
  let card = '';
  for (let i = 0; i < 10; i++) { await s.getByRole('button', { name: /^Check again$/ }).first().click().catch(() => {}); await sleep(3000); card = await cardText(s, 'Administrator readings'); if (/^Administrator readings On/.test(card)) break; }
  checks.push({ ok: /^Administrator readings On/.test(card), detail: /^Administrator readings On/.test(card) ? 'the card reads "On"' : `the card says: ${card.slice(0, 80)}` });
  return result('Windows: admin helper Turn on (UAC consent over VNC)', checks, evidence, 'UAC answered over VNC, service Running, card "On"');
}

/** The release blocker: nothing under the helper's data folder may let a standard user create files or folders, and the helper must refuse a user-owned tools tree. */
export async function helperFolderAcl() {
  const checks = [];
  const evidence = [];
  const acl = ssh(`foreach ($p in "${HELPER}", "${HELPER}\\staging", "${HELPER}\\tools") { if (Test-Path $p) { "== $p owner=" + (Get-Acl $p).Owner; icacls $p | Where-Object { $_ -and $_ -notmatch 'Successfully' } } }`, { admin: true });
  evidence.push(...acl.split('\n'));
  const probe = ssh(`$r = @(); foreach ($d in "${HELPER}", "${HELPER}\\staging") { if (Test-Path $d) { try { New-Item -ItemType File "$d\\probe-std.txt" -Value x -ErrorAction Stop | Out-Null; Remove-Item "$d\\probe-std.txt" -Force; $r += "$d CREATE-OK" } catch [System.UnauthorizedAccessException] { $r += "$d denied" } catch { $r += "$d other:" + $_.Exception.GetType().Name } } }; $r`);
  for (const line of probe.split('\n').map((l) => l.trim()).filter(Boolean)) checks.push({ ok: /denied$/.test(line), ...(/other:/.test(line) ? { kind: 'check' } : {}), detail: `standard user, ${line.replace('CREATE-OK', 'could create a file (must be denied)').replace(/denied$/, 'cannot create a file').replace(/ other:(\S+)$/, ' the attempt failed for another reason ($1), not a refusal')}` });
  checks.push({ ok: !/BUILTIN\\Users:\(I\)\(CI\)\(WD,AD/.test(acl), detail: /BUILTIN\\Users:\(I\)\(CI\)\(WD,AD/.test(acl) ? 'the data folder inherits BUILTIN\\Users create rights from C:\\ProgramData' : 'no inherited Users create right on the helper data folders' });
  const attack = ssh(`New-Item -ItemType Directory "${HELPER}\\tools\\smartmontools-7.5\\bin" -Force -ErrorAction SilentlyContinue | Out-Null; if (Test-Path "${HELPER}\\tools\\smartmontools-7.5\\bin") { "PRECREATE-OK owner=" + (Get-Acl "${HELPER}\\tools\\smartmontools-7.5\\bin").Owner; Remove-Item "${HELPER}\\tools\\smartmontools-7.5" -Recurse -Force -ErrorAction SilentlyContinue } else { "PRECREATE-DENIED" }`);
  checks.push({ ok: /PRECREATE-DENIED/.test(attack), detail: /PRECREATE-DENIED/.test(attack) ? 'a standard user cannot pre-create tools\\smartmontools-7.5\\bin' : `a standard user pre-created tools\\smartmontools-7.5\\bin (${attack.trim()}): the helper would install into a user-owned tree` });
  return result('Windows: helper data folder is not writable by a standard user (ACL)', checks, evidence, 'no standard-user create right on the helper folders');
}

/** Disk health: Download and install, then the card says Installed (which copy) and Lifespan reads through the helper. */
export async function diskHealth(browser) {
  const checks = [];
  const evidence = [];
  const s = await openSettings(browser);
  const check = async () => { await s.getByRole('button', { name: /^Check again$/ }).last().click().catch(() => {}); await sleep(3500); return cardText(s, 'Disk health tool'); };
  let card = await check();
  const btn = s.getByRole('button', { name: /^Download and install smartmontools/ });
  checks.push({ ok: (await btn.count()) > 0 || /Installed/.test(card), detail: `the card offers the install (${card.slice(0, 60)})` });
  if (await btn.count()) await btn.click();
  for (let i = 0; i < 20; i++) { await sleep(4000); card = await check(); if (/Installed/.test(card)) break; }
  evidence.push(`card: ${card.slice(0, 300)}`);
  checks.push({ ok: /Installed/.test(card), detail: /Installed/.test(card) ? 'the card says Installed' : `the card still says: ${card.slice(0, 60)}` });
  checks.push({ ok: /copy|helper|Dyno's own|installed by/i.test(card.replace(/Not installed/, '')) && /Installed/.test(card), detail: 'the card names which copy is used' });
  const main = browser.contexts().flatMap((c) => c.pages()).find((p) => !p.url().includes('dialog?id='));
  await main.evaluate(() => { location.hash = '#/lifespan'; });
  await sleep(6000);
  const life = (await main.evaluate(() => document.body.innerText)).replace(/\s+/g, ' ');
  evidence.push(`Lifespan: ${life.slice(0, 300)}`);
  checks.push(...judgeWear(life)); // a real wear or forecast value, none of the failure wording (the heading alone is not a reading)
  return result('Windows: disk health install, card and Lifespan through the helper', checks, evidence, 'installed, the card names the copy, Lifespan reads');
}

/**
 * Repair through the card: the service is stopped (as the servedCounts step leaves it), so the card says "Not running" and offers "Repair the helper";
 * pressing it asks for the administrator (UAC, answered over VNC) and the service runs again with the card "On".
 */
export async function helperRepair(browser) {
  const checks = [];
  const evidence = [];
  ssh('Stop-Service DynoHelper -ErrorAction SilentlyContinue', { admin: true, ms: 60_000 });
  const s = await openSettings(browser);
  let card = '';
  for (let i = 0; i < 10; i++) { await s.getByRole('button', { name: /^Check again$/ }).first().click().catch(() => {}); await sleep(3000); card = await cardText(s, 'Administrator readings'); if (/Not running|Needs repair|Repair/.test(card)) break; }
  evidence.push(`card before: ${card.slice(0, 200)}`);
  const btn = s.getByRole('button', { name: /Repair the helper/ });
  checks.push({ ok: (await btn.count()) > 0, detail: (await btn.count()) > 0 ? `the card offers "Repair the helper" (${card.slice(0, 60)})` : `no "Repair the helper" button; the card says: ${card.slice(0, 80)}` });
  if (!(await btn.count())) return result('Windows: Repair through the card (real UAC)', checks, evidence);
  await btn.first().click();
  const uac = await answerUac('repair');
  checks.push({ ok: uac.ok, detail: uac.detail });
  let status = '';
  for (let i = 0; i < 30 && status !== 'Running'; i++) { await sleep(3000); status = ssh('(Get-Service DynoHelper -ErrorAction SilentlyContinue).Status', { admin: true }); }
  checks.push({ ok: status === 'Running', detail: status === 'Running' ? 'the DynoHelper service is Running again' : `the DynoHelper service is "${status || 'absent'}" 90 s after Repair` });
  for (let i = 0; i < 10; i++) { await s.getByRole('button', { name: /^Check again$/ }).first().click().catch(() => {}); await sleep(3000); card = await cardText(s, 'Administrator readings'); if (/^Administrator readings On/.test(card)) break; }
  evidence.push(`card after: ${card.slice(0, 200)}`);
  checks.push({ ok: /^Administrator readings On/.test(card), detail: /^Administrator readings On/.test(card) ? 'the card reads "On" after Repair' : `the card says: ${card.slice(0, 80)}` });
  return result('Windows: Repair through the card (real UAC)', checks, evidence, 'Repair asked for the administrator, the service runs, the card reads On');
}

/** Turn off: the service and the helper's data folder are gone. */
export async function helperOff(browser) {
  const checks = [];
  const s = await openSettings(browser);
  const pressed = await s.getByRole('button', { name: /Turn off administrator readings/ }).click().then(() => true, () => false);
  checks.push({ ok: pressed, detail: pressed ? 'pressed "Turn off administrator readings"' : 'no "Turn off administrator readings" button' });
  // Removing the service is a change too: the standard user is asked for the administrator again (consent.exe is the UAC prompt).
  let prompt = false;
  for (let i = 0; i < 10 && !prompt; i++) { await sleep(2000); prompt = ssh('if (Get-Process consent -ErrorAction SilentlyContinue) { "yes" } else { "no" }', { admin: true }) === 'yes'; }
  checks.push({ ok: true, info: true, detail: prompt ? 'Turn off asked for the administrator (UAC) and was answered' : 'Turn off asked for no UAC prompt' });
  if (prompt && runPassword) { const v = new Vnc(); v.key('ctrl-a'); v.key('bsp'); v.type(runPassword); v.key('enter'); }
  let gone = false;
  for (let i = 0; i < 30 && !gone; i++) { await sleep(3000); gone = ssh('if (Get-Service DynoHelper -ErrorAction SilentlyContinue) { "present" } else { "gone" }', { admin: true }) === 'gone'; }
  checks.push({ ok: gone, detail: gone ? 'the DynoHelper service is removed' : 'the DynoHelper service is still there 90 s after Turn off' });
  const folder = ssh(`Test-Path "${HELPER}"`, { admin: true });
  checks.push({ ok: folder === 'False', detail: folder === 'False' ? 'the helper data folder is removed' : 'the helper data folder remains after Turn off' });
  return result('Windows: admin helper Turn off removes the service', checks, [], 'service and folder removed');
}

const TOOLS = `${HELPER}\\tools`;
const BIN = `${TOOLS}\\smartmontools-7.5\\bin`;
const helperLog = (n = 6) => ssh(`Get-Content ${HELPER}\\helper.log -Tail ${n}`, { admin: true }).split('\n');

async function pressInstallAndWait(browser, guardSec = 240) {
  const s = await openSettings(browser);
  await s.getByRole('button', { name: /^Check again$/ }).last().click().catch(() => {});
  await sleep(3000);
  const btn = s.getByRole('button', { name: /^Download and install smartmontools/ });
  if (!(await btn.count())) return { pressed: false, outcome: 'no install button' };
  const fullLog = () => ssh(`Get-Content ${HELPER}\\helper.log`, { admin: true, ms: 60_000 });
  const before = installEvents(fullLog());
  await btn.click();
  const end = Date.now() + guardSec * 1000;
  while (Date.now() < end) {
    await sleep(4000);
    const outcome = installOutcomeSince(before, fullLog());
    if (outcome) return { pressed: true, outcome, log: helperLog(40).filter((l) => /InstallTool/.test(l)).join('\n') };
  }
  return { pressed: true, outcome: 'timeout', log: helperLog(8).join('\n') };
}

/**
 * The 7501 exploit as a regression step. A standard user pre-creates a user-owned tools tree, the install runs, then the user tries to swap
 * smartctl.exe, add a file next to it, next to the staging installer and in tools. Every attempt must be refused, or the tree must have been
 * recreated (owned by Administrators); nothing the user planted may end up beside a SYSTEM-run binary.
 */
export async function helperExploit(browser) {
  const checks = [];
  const evidence = [];
  const title = 'Windows: exploit probe, a user-owned tools tree and swaps are refused';
  ssh(`Remove-Item ${TOOLS} -Recurse -Force -ErrorAction SilentlyContinue; Remove-Item ${HELPER}\\staging\\* -Recurse -Force -ErrorAction SilentlyContinue`, { admin: true });
  checks.push(judgeRefusal('pre-create the tools tree', ssh(attemptProbe(`New-Item -ItemType Directory "${BIN}" -Force -ErrorAction Stop | Out-Null; Set-Content "${BIN}\\dummy-user.txt" planted -ErrorAction Stop`))));
  const inst = await pressInstallAndWait(browser);
  evidence.push(`install: ${inst.outcome}`, ...(inst.log ? inst.log.split('\n') : []));
  const smartctl = ssh(`Test-Path "${BIN}\\smartctl.exe"`, { admin: true }) === 'True';
  const pre = judgeExploitPrecondition({ outcome: inst.outcome, smartctlExists: smartctl });
  checks.push(...pre);
  if (pre.some((c) => !c.ok)) return result(title, checks, evidence); // nothing installed: the swaps below would be "refused" because the file is not there
  const owner = ssh(`if (Test-Path "${BIN}") { (Get-Acl "${BIN}").Owner } else { "absent" }`, { admin: true });
  const userOwned = /paddock-test/i.test(owner);
  checks.push({ ok: !userOwned, detail: userOwned ? `the helper installed into a user-owned tree: bin is owned by ${owner}` : `the tools tree is not user-owned (bin owner: ${owner})` });
  const tries = [
    ['rename smartctl.exe', (d) => `Rename-Item "${d}\\smartctl.exe" smartctl.exe.orig -ErrorAction Stop`, BIN],
    ['add a file next to smartctl.exe', (d) => `Set-Content "${d}\\evil.dll" x -ErrorAction Stop`, BIN],
    ['add a file next to the staging installer', (d) => `Set-Content "${d}\\evil.exe" x -ErrorAction Stop`, `${HELPER}\\staging`],
    ['add a file in tools', (d) => `Set-Content "${d}\\evil.txt" x -ErrorAction Stop`, TOOLS],
  ];
  // The control: the same probes in a folder the standard user owns must succeed, else "refused" would only mean the probe cannot work.
  const ctl = '$env:TEMP\\vmcheck-ctl';
  ssh(`Remove-Item "${ctl}" -Recurse -Force -ErrorAction SilentlyContinue; New-Item -ItemType Directory "${ctl}" -Force | Out-Null; Set-Content "${ctl}\\smartctl.exe" x`);
  for (const [what, cmd, dir] of tries) {
    checks.push(judgeRefusal(what, ssh(attemptProbe(cmd(dir)))));
    const c = judgeControl(ssh(attemptProbe(cmd(ctl))));
    checks.push({ ...c, detail: `${what}: ${c.detail}` });
  }
  ssh(`Remove-Item "${ctl}" -Recurse -Force -ErrorAction SilentlyContinue`);
  ssh(`Remove-Item "${BIN}\\evil.dll","${BIN}\\smartctl.exe.orig","${HELPER}\\staging\\evil.exe","${TOOLS}\\evil.txt","${BIN}\\dummy-user.txt" -Force -ErrorAction SilentlyContinue; if (Test-Path "${BIN}\\smartctl.exe.orig") { Rename-Item "${BIN}\\smartctl.exe.orig" smartctl.exe }`, { admin: true });
  return result(title, checks, evidence, 'every standard-user attempt was refused (access denied) and the same probes succeed in a folder the user owns');
}

/** A second install while the first copy exists must succeed (the 7501 run failed it with Staging("entity already exists")). */
export async function repeatInstall(browser) {
  const checks = [];
  const smartctl = () => ssh(`Test-Path "${BIN}\\smartctl.exe"`, { admin: true }) === 'True';
  const first = await pressInstallAndWait(browser);
  checks.push(judgeInstallOutcome('first install', first.outcome, smartctl()));
  const second = await pressInstallAndWait(browser);
  checks.push(judgeInstallOutcome('repeat install', second.outcome, smartctl()));
  if (second.outcome === 'failed') checks.push({ ok: false, detail: `repeat install: ${second.log?.match(/Error=.*/)?.[0] ?? 'failed'}` });
  return result('Windows: repeat install of the disk health tool', checks, [`first: ${first.outcome}`, `second: ${second.outcome}`], 'a second install works and smartctl.exe is there');
}

import { loginJobsRow, windowsLoginJobsDriver } from './login-jobs-drivers.mjs';
import { diskSpaceStep, windowsAdapter } from './disk-space-row.mjs';
import { discoveryStep, windowsDiscoveryPrims } from './discovery-row.mjs';
import { loginSwitchStep } from './login-start-rows.mjs';
import { readingSteps, windowsDriver } from './helper-readings.mjs';
import { scopingStep } from './win-scoping.mjs';
const mainPage = (browser) => browser.contexts().flatMap((c) => c.pages()).find((p) => !p.url().includes('dialog?id='));


const DYNO = 'C:\\ProgramData\\Dyno';
const JUNCTION_TARGET = 'C:\\Users\\paddock-test\\evil-target';

/** WP-SEC (fc140fc6f): before the helper exists, a standard user plants a junction where the helper's data folder goes; the install must not follow it. */
export function plantJunction() {
  return ssh(`Remove-Item "${JUNCTION_TARGET}" -Recurse -Force -ErrorAction SilentlyContinue; New-Item -ItemType Directory "${JUNCTION_TARGET}" -Force | Out-Null; Set-Content "${JUNCTION_TARGET}\\canary.txt" canary; New-Item -ItemType Directory "${DYNO}" -Force -ErrorAction SilentlyContinue | Out-Null; cmd /c mklink /J "${DYNO}\\Helper" "${JUNCTION_TARGET}" 2>&1; if ((Get-Item "${DYNO}\\Helper" -Force -ErrorAction SilentlyContinue).Attributes -match 'ReparsePoint') { "PLANTED" } else { "NOT-PLANTED" }`);
}

export function verifyJunction() {
  const checks = [];
  const evidence = [];
  const target = ssh(`Get-ChildItem "${JUNCTION_TARGET}" -Force | Select-Object -ExpandProperty Name`, { admin: true }).split('\n').map((l) => l.trim()).filter(Boolean);
  evidence.push(`junction target now holds: ${target.join(', ') || '(empty)'}`);
  checks.push({ ok: target.length === 1 && target[0] === 'canary.txt', detail: target.length === 1 && target[0] === 'canary.txt' ? 'the helper did not write through the planted junction (target holds only the canary)' : `the helper wrote through the planted junction: target holds ${target.join(', ')}` });
  const real = ssh(`$i = Get-Item "${HELPER}" -Force -ErrorAction SilentlyContinue; if (-not $i) { "ABSENT" } elseif ($i.Attributes -match 'ReparsePoint') { "STILL-A-LINK" } else { "REAL owner=" + (Get-Acl "${HELPER}").Owner }`, { admin: true });
  evidence.push(`Helper folder: ${real}`);
  checks.push({ ok: /^REAL owner=(BUILTIN\\Administrators|NT AUTHORITY\\SYSTEM)/.test(real), detail: /^REAL/.test(real) ? `the helper folder is a real folder, ${real.replace('REAL ', '')}` : `the helper folder is ${real}` });
  const aside = ssh(`Get-ChildItem "${DYNO}" -Force | Select-Object -ExpandProperty Name`, { admin: true }).split('\n').map((l) => l.trim()).filter(Boolean);
  evidence.push(`${DYNO} holds: ${aside.join(', ')}`);
  return { checks, evidence };
}

/** The owner and inheritance of the helper's folders, and the standard user's denials (create, junction) on each. */
export async function helperProtection() {
  const checks = [];
  const evidence = [];
  const folders = [HELPER, `${HELPER}\\staging`, `${HELPER}\\tools`];
  for (const f of folders) {
    const info = ssh(`if (Test-Path "${f}") { "owner=" + (Get-Acl "${f}").Owner; icacls "${f}" | Where-Object { $_ -and $_ -notmatch 'Successfully' } } else { "ABSENT" }`, { admin: true });
    evidence.push(`== ${f}`, ...info.split('\n'));
    if (/ABSENT/.test(info)) { checks.push({ ok: f.endsWith('tools') || f.endsWith('staging'), detail: `${f} is absent (allowed only for tools and staging before a tool install)` }); continue; }
    checks.push({ ok: /owner=(BUILTIN\\Administrators|NT AUTHORITY\\SYSTEM)/.test(info), detail: `${f}: owner ${(/owner=(\S+)/.exec(info) ?? [])[1]} (must be Administrators or SYSTEM)` });
    checks.push({ ok: !/\(I\)/.test(info), detail: /\(I\)/.test(info) ? `${f}: still inherits ACL entries (protected folders must not)` : `${f}: inheritance is off` });
    const den = ssh(`$r = @(); try { New-Item -ItemType File "${f}\\probe-std.txt" -Value x -ErrorAction Stop | Out-Null; Remove-Item "${f}\\probe-std.txt" -Force; $r += "FILE-OK" } catch [System.UnauthorizedAccessException] { $r += "FILE-DENIED" } catch { $r += "FILE-OTHER" }; $o = cmd /c mklink /J "${f}\\evil-link" C:\\Windows 2>&1; if (Test-Path "${f}\\evil-link") { cmd /c rmdir "${f}\\evil-link" | Out-Null; $r += "LINK-OK" } else { $r += "LINK-DENIED" }; $r -join ' '`);
    checks.push({ ok: /FILE-DENIED/.test(den) && /LINK-DENIED/.test(den), ...(/FILE-OTHER/.test(den) ? { kind: 'check' } : {}), detail: `${f}: standard user create file ${/FILE-DENIED/.test(den) ? 'denied' : /FILE-OTHER/.test(den) ? 'failed for another reason (not a refusal)' : 'SUCCEEDED'}, create junction ${/LINK-DENIED/.test(den) ? 'denied' : 'SUCCEEDED'}` });
  }
  // 7526 includes 450746cfd: C:\\ProgramData\\Dyno itself is locked (owner Administrators or SYSTEM, no inheritance, Users cannot create in it).
  const dyno = ssh(`"owner=" + (Get-Acl "${DYNO}").Owner; icacls "${DYNO}" | Where-Object { $_ -and $_ -notmatch 'Successfully' }`, { admin: true });
  evidence.push(`== ${DYNO} (C:\\ProgramData\\Dyno itself)`, ...dyno.split('\n'));
  checks.push({ ok: /owner=(BUILTIN\\Administrators|NT AUTHORITY\\SYSTEM)/.test(dyno), detail: `${DYNO}: owner ${(/owner=(\S+)/.exec(dyno) ?? [])[1]} (must be Administrators or SYSTEM)` });
  checks.push({ ok: !/\(I\)/.test(dyno), detail: /\(I\)/.test(dyno) ? `${DYNO}: still inherits ACL entries` : `${DYNO}: inheritance is off` });
  checks.push(judgeRefusal(`create a folder in ${DYNO}`, ssh(attemptProbe(`New-Item -ItemType Directory "${DYNO}\\probe-std" -ErrorAction Stop | Out-Null; Remove-Item "${DYNO}\\probe-std" -Force`))));
  return result('Windows: WP-SEC folder protection (owner, no inheritance, standard-user denials incl. junctions)', checks, evidence, 'owner, inheritance and denials as designed');
}

/**
 * Tray tooltips. The registry's InitialTooltip is only what Windows recorded at registration, so it is informational. The live tooltip is read
 * through UI Automation (the taskbar button's name for each Dyno icon, from the in-session actor) and from the app's own "tray icon ... created" log lines.
 */
export async function trayTooltips(actor) {
  const checks = [];
  const evidence = [];
  const reg = ssh(`Get-ItemProperty 'HKCU:\\Control Panel\\NotifyIconSettings\\*' -ErrorAction SilentlyContinue | Where-Object { $_.ExecutablePath -like '*dyno-app.exe' } | ForEach-Object { "$($_.InitialTooltip)|promoted=$($_.IsPromoted)" }`).split('\n').map((l) => l.trim()).filter(Boolean);
  evidence.push(`registry InitialTooltip (informational only): ${reg.join(' ; ')}`);
  checks.push({ ok: reg.length > 0, detail: reg.length ? `${reg.length} Dyno tray icon(s) registered with Windows` : 'no Dyno tray icon is registered with Windows' });
  let names = [];
  if (actor) {
    // Count first, before any icon is promoted: promoting moves icons into the taskbar and would change what is counted.
    const { trayRecord } = await import('./win-reboot.mjs');
    const rec = await trayRecord(actor);
    checks.push(...judgeTrayCount({ metrics: rec.metrics, visible: rec.visible.length, overflow: rec.overflow.length, measured: rec.measured, registered: rec.registered }));
    evidence.push(`counted before promoting: ${rec.visible.length} visible, ${rec.overflow.length} overflow, ${rec.metrics.length} created (${rec.metrics.join(', ') || 'none logged'})`);
    ssh(`Get-ChildItem 'HKCU:\\Control Panel\\NotifyIconSettings' | ForEach-Object { $p = Get-ItemProperty $_.PSPath; if ($p.ExecutablePath -like '*dyno-app.exe') { Set-ItemProperty $_.PSPath IsPromoted 1 -Type DWord } }`);
    await sleep(2500);
    // Dyno's icons are named by their metric tooltip, never "Dyno" (win-tray-icons.mjs).
    names = dynoTrayIcons(await actor.send(TRAY_ICONS_CMD), rec.metrics).map((i) => i.name);
  } else checks.push({ ok: false, kind: 'check', detail: 'no in-session actor, so the tray was not read' });
  evidence.push(`live taskbar button names (UI Automation, after promoting): ${names.join(' | ') || '(none read)'}`);
  // Dyno logs no "tooltip sent" line; the icons it made are its "tray icon dyno-... created" lines.
  const sent = trayLogLines(ssh(APP_LOG_PS)).filter((l) => /tray icon dyno-[A-Za-z0-9-]+ created/.test(l));
  evidence.push(`app log: ${sent.length} "tray icon ... created" line(s)`, ...sent.slice(-8).map((l) => l.slice(0, 200)));
  const live = names.filter((n) => n.length > 0);
  checks.push({ ok: live.length > 0, detail: live.length ? `${live.length} live tooltip(s) through UI Automation` : 'no live tooltip was read through UI Automation (the log lines alone do not show what a person sees)' });
  for (const n of live) checks.push({ ok: !/[a-z]+_[a-z]+|undefined|null|NaN/.test(n), detail: `live tooltip "${n}"` });
  return result('Windows: tray icons and their live tooltips', checks, evidence, 'every created icon is in the notification area with a readable tooltip');
}


/** The served counts: stop the helper (a stopped service writes its final tally) and read the `served:` line of its log. */
export function servedCounts() {
  ssh('Stop-Service DynoHelper -ErrorAction SilentlyContinue; Start-Sleep 3', { admin: true, ms: 60_000 });
  const lines = ssh(`Get-Content ${HELPER}\\helper.log -ErrorAction SilentlyContinue | Select-String -Pattern 'served:' | Select-Object -Last 3 | ForEach-Object { $_.Line }`, { admin: true }).split('\n').filter(Boolean);
  // Per kind: the readings this suite took through the helper must each have a count above 0 (a bare line proves nothing). The kind names are the helper's.
  return result('Windows: the helper reports which readings it served (served counts)', judgeServed(lines, SERVED_KINDS.windows), lines, 'every reading kind taken in this run was served');
}

/** RequestKind names of the helper's `served:` line (dyno-helper server.rs `ServedCounts::summary`, `Kind=count` for every kind) that this suite's readings go through: Processes, Traffic (per program), Smart (Lifespan), Extras (wake requests and backup versions). */
export const SERVED_KINDS = { windows: ['Processes', 'Traffic', 'Smart', 'Extras'] };

/** Rule 71 MCP row on the Windows guest (guest only, over the test user's SSH; the app is started in the console session through a scheduled task). */
export async function winMcpStep() {
  const sh = async (c) => { const out = ssh(`${c}; "RC=$(if ($?) { 0 } else { 1 })"`); const m = /RC=(\d)\s*$/.exec(out); return { code: m ? Number(m[1]) : null, stdout: out.replace(/RC=\d\s*$/, '') }; };
  const startApp = async () => {
    ssh('Set-Content C:\\paddock-test\\vmcheck\\launch-mcp.ps1 \'Start-Process "$env:ProgramFiles\\Dyno\\dyno-app.exe" -ArgumentList "--hidden"\'; schtasks /create /tn DynoMcpLaunch /sc once /st 00:00 /ru paddock-test /it /f /tr "cmd /c powershell -NoProfile -ExecutionPolicy Bypass -File C:\\paddock-test\\vmcheck\\launch-mcp.ps1" | Out-Null; schtasks /run /tn DynoMcpLaunch | Out-Null');
    for (let i = 0; i < 30; i++) { await sleep(4000); if (ssh('& "$env:ProgramFiles\\Dyno\\dyno.exe" status 2>&1 | Out-String; "EXIT=$LASTEXITCODE"').includes('EXIT=0')) break; }
    await sleep(8000);
  };
  const mg = makeMcpGuest({
    os: 'windows', dyno: 'C:\\Program Files\\Dyno\\dyno.exe', sh,
    configDir: async () => ssh('foreach ($d in "$env:LOCALAPPDATA\\Dyno", "$env:APPDATA\\Dyno") { if (Test-Path "$d\\settings.json") { $d; break } }').trim() || null,
    stopApp: async () => { ssh('Get-Process dyno-app -ErrorAction SilentlyContinue | Stop-Process -Force'); await sleep(3000); }, startApp,
    killAgent: async () => { ssh('Get-Process dyno-agent -ErrorAction SilentlyContinue | Stop-Process -Force'); },
    agentAlive: async () => ssh('[bool](Get-Process dyno-agent -ErrorAction SilentlyContinue)') === 'True',
    quitApp: async () => { ssh('Get-Process dyno-app -ErrorAction SilentlyContinue | Stop-Process -Force'); await sleep(3000); },
    pidAlive: async (pid) => ssh(`[bool](Get-Process -Id ${pid} -ErrorAction SilentlyContinue)`) === 'True',
  });
  const r = await mcpStep(mg);
  await startApp(); // the later steps need the app (and its DevTools port is gone: they reconnect or report)
  return r;
}

/** The release rows the Windows suite cannot drive yet, as not-measured steps with their reasons. */
export const winUndrivenRows = () => [['winReinstall'], ['winFirstUninstall'], ['hungPage'], ['secondUser'], ['killInstaller'], ['installAgain']].map(([k]) => notDriven(ROW_TITLES[k], NOT_DRIVEN[k]));

/**
 * The Jobs panel across an agent restart: the agent is ended, the app starts a new one, and "The jobs could not be read" must be gone from the open panel once it answers.
 * The page's own MutationObserver notes whether the message ever showed (the control) and whether it is there now.
 */
export async function jobsAfterAgentRestart(browser) {
  const title = 'Windows: the Jobs panel after an agent restart no longer says "The jobs could not be read"';
  const p = mainPage(browser);
  const d = windowsLoginJobsDriver(browser, { openSettings, mainPage });
  const panelOpen = await d.openJobs();
  if (!p || !panelOpen) return result(title, judgeJobsRestart({ panelOpen: false }), []);
  const probe = () => p.evaluate(() => /jobs could not be read/i.test(document.body.innerText)).catch(() => null);
  await p.evaluate(() => { window.__jobsSeen = false; const f = () => { if (/jobs could not be read/i.test(document.body.innerText)) window.__jobsSeen = true; }; window.__jobsObs?.disconnect(); window.__jobsObs = new MutationObserver(f); window.__jobsObs.observe(document.body, { subtree: true, childList: true, characterData: true }); f(); });
  const shownBefore = (await probe()) === true;
  const oldPid = ssh('(Get-Process dyno-agent -ErrorAction SilentlyContinue | Select-Object -First 1).Id', { admin: true }).trim().split('\n').at(-1);
  const killed = ssh('Get-Process dyno-agent -ErrorAction SilentlyContinue | Stop-Process -Force; "killed"', { admin: true });
  let agentBack = false;
  for (let i = 0; i < 45 && !agentBack; i++) { await sleep(2000); const pid = ssh('(Get-Process dyno-agent -ErrorAction SilentlyContinue | Select-Object -First 1).Id', { admin: true }).trim().split('\n').at(-1); agentBack = /^\d+$/.test(pid) && pid !== oldPid && /^\s*\S/.test(ssh('& "$env:ProgramFiles\\Dyno\\dyno.exe" status 2>&1 | Select-Object -First 1')) && /Cpu|CPU|Memory/i.test(ssh('& "$env:ProgramFiles\\Dyno\\dyno.exe" status 2>&1 | Out-String')); }
  const backAt = Date.now();
  const everShown = await p.evaluate(() => window.__jobsSeen === true).catch(() => false);
  await d.openJobs();
  let shownAfterBack = null; let secondsToClear = null;
  for (let i = 0; i < 30; i++) { shownAfterBack = await probe(); if (shownAfterBack === false) { secondsToClear = Math.round((Date.now() - backAt) / 1000); break; } await sleep(2000); }
  const r = result(title, judgeJobsRestart({ panelOpen, shownBefore, agentBack, everShown, shownAfterBack, secondsToClear }), [`old agent pid ${oldPid || '?'}; ${killed.trim() || 'kill gave no answer'}`, `message ever shown during the gap: ${everShown}; shown now: ${shownAfterBack}`], 'the message showed while the agent was gone and cleared once it was back');
  await p.evaluate(() => window.__jobsObs?.disconnect()).catch(() => {});
  return r;
}

export const helperStepTable = [
  ['helper-on', async ({ browser }) => {
    const planted = plantJunction();
    const on = await helperOn(browser);
    const v = verifyJunction();
    const checks = [{ ok: /^PLANTED/m.test(planted), detail: /^PLANTED/m.test(planted) ? 'a standard user planted a junction at C:\\ProgramData\\Dyno\\Helper before the helper existed' : 'the junction could not be planted (the step proves nothing then)' }, ...on.checks, ...v.checks];
    return result('Windows: admin helper Turn on with a planted junction in its way (UAC over VNC)', checks, [...on.evidence, ...v.evidence], 'Turn on refused to follow the planted junction and the helper is On');
  }],
  ['tray-tooltips', ({ actor }) => trayTooltips(actor)],
  ['wp-sec', () => helperProtection()],
  ['helper-acl', () => helperFolderAcl()],
  ['exploit', ({ browser }) => helperExploit(browser)],
  ['repeat-install', ({ browser }) => repeatInstall(browser)],
  ['disk-health', ({ browser }) => diskHealth(browser)],
  // Release row: Launch at login (first-run default, the switch, the entry HKCU Run "Dyno"), BEFORE row 5a presses the switch.
  ['login-switch', async ({ browser }) => {
    const d = windowsLoginJobsDriver(browser, { openSettings, mainPage });
    const RUN = "(Get-ItemProperty 'HKCU:\\Software\\Microsoft\\Windows\\CurrentVersion\\Run' -ErrorAction SilentlyContinue).Dyno";
    return loginSwitchStep({ sleep, readSwitch: () => d.readSwitch(), pressSwitch: () => d.pressSwitch(), apply: () => d.apply(),
      witness: async () => ssh(APP_LOG_PS).split('\n').filter((l) => /Run value|start at login|launch at login/i.test(l)).slice(-12), readEntry: async () => { const c = ssh(RUN).trim().split('\n').at(-1); return { present: Boolean(c), command: c, count: c ? 1 : 0 }; } });
  }],
  // Row 5a: the Start at login change in the Jobs list (login-jobs-row.mjs). The raw `wbadmin` evidence step is gone with Backup health (removed in 1.1.0).
  ['jobs-restart', ({ browser }) => jobsAfterAgentRestart(browser)],
  ['login-jobs', async ({ browser }) => loginJobsRow(windowsLoginJobsDriver(browser, { openSettings, mainPage }), { now: () => Date.now(), sleep })],
  // Release row: disk space (Dyno's figure against Explorer's, decimal units, refresh within 60 s of a 1 GB write)
  ['disk-space', async () => { const adapter = windowsAdapter({ ssh }); return diskSpaceStep({ exec: async (c) => ({ code: 0, stdout: ssh(c, { ms: 280_000 }) }), sleep, now: () => Date.now(), adapter }); }],
  ['discovery', async () => discoveryStep(windowsDiscoveryPrims({ ssh, sleep, now: () => Date.now() }))],
  // WP-VMH: one step per helper reading while the helper is on
  ...readingSteps('windows', async ({ browser }) => windowsDriver(mainPage(browser), { ssh })).map((fn, i) => [`reading-${i}`, fn]),
  ['scoping', () => scopingStep()],
  ['served', () => servedCounts()],
  ['repair', ({ browser }) => helperRepair(browser)],
  ['helper-off', ({ browser }) => helperOff(browser)],
];
export const helperSteps = helperStepTable.map(([, fn]) => fn);

/** The steps of `--only` UI rows (win-rows.mjs): the rows' own steps in table order, the helper turned on before and off after when one of them reads through it. */
export const rowSteps = (ids, { helper }) => [...(helper ? ['helper-on'] : []), ...ids, ...(helper ? ['helper-off'] : [])].map((id) => helperStepTable.find(([k]) => k === id)?.[1]).filter(Boolean);
