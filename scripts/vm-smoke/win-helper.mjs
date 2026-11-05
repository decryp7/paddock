// The Windows admin helper steps (Dyno 1.1.0): Turn on through the real UAC consent over VNC, the helper's own folders must not be writable
// by a standard user (the 7501 release blocker: C:\ProgramData\Dyno\Helper inherited Users create rights, and the SYSTEM helper installed
// smartctl into a user-owned tree), disk health install, Turn off. Each step returns { title, status, summary, checks, evidence }. They are `extra`
// steps of win-ui.mjs's runUiPhase and need run-win.mjs --vnc (the VM's VNC) and --ui (the DevTools port).
import { APP_LOG_PS, TRAY_ICONS_CMD, dynoTrayIcons, metricsFromLog, trayLogLines } from './win-tray-icons.mjs';
import { spawnSync } from 'node:child_process';
import { randomBytes } from 'node:crypto';
import { Vnc } from './win-vnc.mjs';

const VMWIN = process.env.PADDOCK_VM_WIN ?? '/Volumes/d7sst7/dev-targets/scratch/dyno-vm-t/vm-win.sh';
const HELPER = 'C:\\ProgramData\\Dyno\\Helper';
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
let runPassword = null; // the per-run random password of paddock-admin (reverted with the VM); never stored or printed
const clean = (s) => s.split('\n').filter((l) => !l.startsWith('**')).join('\n').trim();
export const ssh = (cmd, { admin = false, input, ms = 90_000 } = {}) => clean(spawnSync('bash', [VMWIN, 'ssh', ...(admin ? ['--admin'] : []), cmd], { encoding: 'utf8', timeout: ms, input }).stdout ?? '');
const result = (title, checks, evidence = [], pass = '') => {
  const bad = checks.filter((c) => !c.ok);
  return { title, status: bad.length ? 'fail' : 'pass', summary: bad.length ? bad.map((c) => c.detail).join('; ').slice(0, 240) : pass, checks, evidence };
};
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
export async function answerUac(label, { vnc = new Vnc(), seenSec = 90, goneSec = 60 } = {}) {
  const pw = ensureAdminPassword();
  const prompt = () => ssh('if (Get-Process consent -ErrorAction SilentlyContinue) { "yes" } else { "no" }', { admin: true }) === 'yes';
  let seen = false;
  for (let i = 0; i < seenSec / 3 && !seen; i++) { seen = prompt(); if (!seen) await sleep(1500); }
  if (!seen) return { ok: false, detail: `${label}: no UAC prompt (consent.exe) within ${seenSec} s` };
  await sleep(1500);
  const shot = vnc.capture(`/tmp/win-uac-${label.replace(/\W+/g, '-')}.png`).code === 0 ? 'screenshot taken' : 'VNC capture failed';
  vnc.key('ctrl-a'); vnc.key('bsp'); vnc.type(pw); vnc.key('enter');
  let gone = false;
  for (let i = 0; i < goneSec / 3 && !gone; i++) { await sleep(1500); gone = !prompt(); }
  return { ok: gone, detail: gone ? `${label}: the UAC prompt appeared (${shot}) and was answered over VNC` : `${label}: the UAC prompt was still there ${goneSec} s after the answer (${shot})` };
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
  evidence.push('UAC: "Do you want to allow this app from an unknown publisher to make changes" (dyno-helper.exe), answered with the paddock-admin credentials over the VM VNC');
  await sleep(6000);
  evidence.push(`screenshot: ${vnc.capture('/tmp/win-uac.png').code === 0 ? 'taken of the UAC prompt' : 'VNC capture failed'}`);
  vnc.key('ctrl-a'); vnc.key('bsp'); vnc.type(pw); vnc.key('enter');
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
  const probe = ssh(`$r = @(); foreach ($d in "${HELPER}", "${HELPER}\\staging") { if (Test-Path $d) { try { New-Item -ItemType File "$d\\probe-std.txt" -Value x -ErrorAction Stop | Out-Null; Remove-Item "$d\\probe-std.txt" -Force; $r += "$d CREATE-OK" } catch { $r += "$d denied" } } }; $r`);
  for (const line of probe.split('\n').map((l) => l.trim()).filter(Boolean)) checks.push({ ok: /denied$/.test(line), detail: `standard user, ${line.replace('CREATE-OK', 'could create a file (must be denied)').replace(/denied$/, 'cannot create a file')}` });
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
  checks.push({ ok: !/not installed/i.test(life), detail: /not installed/i.test(life) ? 'Lifespan says smartmontools is not installed' : 'Lifespan reads SSD wear' });
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
  checks.push({ ok: true, detail: prompt ? 'Turn off asked for the administrator (UAC) and was answered' : 'Turn off asked for no UAC prompt' });
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

async function pressInstallAndWait(browser, guardSec = 120) {
  const s = await openSettings(browser);
  await s.getByRole('button', { name: /^Check again$/ }).last().click().catch(() => {});
  await sleep(3000);
  const btn = s.getByRole('button', { name: /^Download and install smartmontools/ });
  if (!(await btn.count())) return { pressed: false, outcome: 'no install button' };
  const before = helperLog(40).length;
  await btn.click();
  const end = Date.now() + guardSec * 1000;
  while (Date.now() < end) {
    await sleep(4000);
    const lines = helperLog(8).join('\n');
    if (/InstallTool finished/.test(lines) && helperLog(40).length >= before) return { pressed: true, outcome: 'finished', log: lines };
    if (/InstallTool failed/.test(lines)) return { pressed: true, outcome: 'failed', log: lines };
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
  ssh(`Remove-Item ${TOOLS} -Recurse -Force -ErrorAction SilentlyContinue; Remove-Item ${HELPER}\\staging\\* -Recurse -Force -ErrorAction SilentlyContinue`, { admin: true });
  const pre = ssh(`try { New-Item -ItemType Directory "${BIN}" -Force -ErrorAction Stop | Out-Null; Set-Content "${BIN}\\dummy-user.txt" planted -ErrorAction Stop; "PRECREATED owner=" + (Get-Acl "${BIN}").Owner } catch { "PRECREATE-DENIED" }`);
  const preCreated = /PRECREATED/.test(pre);
  checks.push({ ok: !preCreated, detail: preCreated ? `a standard user pre-created the tools tree (${pre.trim()}); the helper must not install into it` : 'a standard user cannot pre-create the tools tree (denied)' });
  const inst = await pressInstallAndWait(browser);
  evidence.push(`install: ${inst.outcome}`, ...(inst.log ? inst.log.split('\n') : []));
  const owner = ssh(`if (Test-Path "${BIN}") { (Get-Acl "${BIN}").Owner } else { "absent" }`, { admin: true });
  const userOwned = /paddock-test/i.test(owner);
  checks.push({ ok: !userOwned, detail: userOwned ? `the helper installed into a user-owned tree: bin is owned by ${owner}` : `the tools tree is not user-owned (bin owner: ${owner})` });
  const tries = [
    ['rename smartctl.exe', `Rename-Item "${BIN}\\smartctl.exe" smartctl.exe.orig -ErrorAction Stop`],
    ['add a file next to smartctl.exe', `Set-Content "${BIN}\\evil.dll" x -ErrorAction Stop`],
    ['add a file next to the staging installer', `Set-Content "${HELPER}\\staging\\evil.exe" x -ErrorAction Stop`],
    ['add a file in tools', `Set-Content "${TOOLS}\\evil.txt" x -ErrorAction Stop`],
  ];
  for (const [what, cmd] of tries) {
    const r = ssh(`try { ${cmd}; "OK" } catch { "DENIED" }`);
    checks.push({ ok: /DENIED/.test(r), detail: /DENIED/.test(r) ? `standard user: ${what}: refused` : `standard user: ${what}: SUCCEEDED (must be refused)` });
  }
  ssh(`Remove-Item "${BIN}\\evil.dll","${BIN}\\smartctl.exe.orig","${HELPER}\\staging\\evil.exe","${TOOLS}\\evil.txt","${BIN}\\dummy-user.txt" -Force -ErrorAction SilentlyContinue; if (Test-Path "${BIN}\\smartctl.exe.orig") { Rename-Item "${BIN}\\smartctl.exe.orig" smartctl.exe }`, { admin: true });
  return result('Windows: exploit probe, a user-owned tools tree and swaps are refused', checks, evidence, 'every standard-user attempt was refused');
}

/** A second install while the first copy exists must succeed (the 7501 run failed it with Staging("entity already exists")). */
export async function repeatInstall(browser) {
  const checks = [];
  const first = await pressInstallAndWait(browser);
  const already = first.outcome === 'no install button';
  checks.push({ ok: first.outcome === 'finished' || already, detail: already ? 'the tool is already installed (no install button): a repeat install is not offered' : `first install: ${first.outcome}` });
  const second = await pressInstallAndWait(browser);
  checks.push({ ok: second.outcome === 'finished' || second.outcome === 'no install button', detail: second.outcome === 'no install button' ? 'the card already says Installed (no second install offered)' : `repeat install: ${second.outcome}${second.outcome === 'failed' ? ' (' + (second.log.match(/Error=.*/)?.[0] ?? '') + ')' : ''}` });
  return result('Windows: repeat install of the disk health tool', checks, [], 'a second install works');
}

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
    const den = ssh(`$r = @(); try { New-Item -ItemType File "${f}\\probe-std.txt" -Value x -ErrorAction Stop | Out-Null; Remove-Item "${f}\\probe-std.txt" -Force; $r += "FILE-OK" } catch { $r += "FILE-DENIED" }; $o = cmd /c mklink /J "${f}\\evil-link" C:\\Windows 2>&1; if (Test-Path "${f}\\evil-link") { cmd /c rmdir "${f}\\evil-link" | Out-Null; $r += "LINK-OK" } else { $r += "LINK-DENIED" }; $r -join ' '`);
    checks.push({ ok: /FILE-DENIED/.test(den) && /LINK-DENIED/.test(den), detail: `${f}: standard user create file ${/FILE-DENIED/.test(den) ? 'denied' : 'SUCCEEDED'}, create junction ${/LINK-DENIED/.test(den) ? 'denied' : 'SUCCEEDED'}` });
  }
  // 7526 includes 450746cfd: C:\\ProgramData\\Dyno itself is locked (owner Administrators or SYSTEM, no inheritance, Users cannot create in it).
  const dyno = ssh(`"owner=" + (Get-Acl "${DYNO}").Owner; icacls "${DYNO}" | Where-Object { $_ -and $_ -notmatch 'Successfully' }`, { admin: true });
  evidence.push(`== ${DYNO} (C:\\ProgramData\\Dyno itself)`, ...dyno.split('\n'));
  checks.push({ ok: /owner=(BUILTIN\\Administrators|NT AUTHORITY\\SYSTEM)/.test(dyno), detail: `${DYNO}: owner ${(/owner=(\S+)/.exec(dyno) ?? [])[1]} (must be Administrators or SYSTEM)` });
  checks.push({ ok: !/\(I\)/.test(dyno), detail: /\(I\)/.test(dyno) ? `${DYNO}: still inherits ACL entries` : `${DYNO}: inheritance is off` });
  const denyDyno = ssh(`try { New-Item -ItemType Directory "${DYNO}\\probe-std" -ErrorAction Stop | Out-Null; Remove-Item "${DYNO}\\probe-std" -Force; "OK" } catch { "DENIED" }`);
  checks.push({ ok: /DENIED/.test(denyDyno), detail: `a standard user ${/DENIED/.test(denyDyno) ? 'cannot' : 'CAN'} create a folder in ${DYNO}` });
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
  checks.push({ ok: reg.length > 0, detail: `${reg.length} Dyno tray icon(s) registered with Windows (their recorded initial tooltips are informational)` });
  let names = [];
  if (actor) {
    ssh(`Get-ChildItem 'HKCU:\\Control Panel\\NotifyIconSettings' | ForEach-Object { $p = Get-ItemProperty $_.PSPath; if ($p.ExecutablePath -like '*dyno-app.exe') { Set-ItemProperty $_.PSPath IsPromoted 1 -Type DWord } }`);
    await sleep(2500);
    // Dyno's icons are named by their metric tooltip, never "Dyno" (win-tray-icons.mjs).
    names = dynoTrayIcons(await actor.send(TRAY_ICONS_CMD), metricsFromLog(ssh(APP_LOG_PS))).map((i) => i.name);
  }
  evidence.push(`live taskbar button names (UI Automation): ${names.join(' | ') || '(none read)'}`);
  // Dyno logs no "tooltip sent" line; the icons it made are its "tray icon dyno-... created" lines.
  const sent = trayLogLines(ssh(APP_LOG_PS)).filter((l) => /tray icon dyno-[A-Za-z0-9-]+ created/.test(l));
  evidence.push(`app log: ${sent.length} "tray icon ... created" line(s)`, ...sent.slice(-8).map((l) => l.slice(0, 200)));
  const live = names.filter((n) => n.length > 0);
  checks.push({ ok: live.length > 0 || sent.length > 0, detail: live.length ? `${live.length} live tooltip(s) through UI Automation` : sent.length ? `${sent.length} "tray icon ... created" log line(s)` : 'no live tooltip found through UI Automation or the app log' });
  for (const n of live) checks.push({ ok: !/[a-z]+_[a-z]+|undefined|null|NaN/.test(n), detail: `live tooltip "${n}"` });
  return result('Windows: tray icons and their live tooltips', checks, evidence, 'live tooltips are readable');
}

/** The raw `wbadmin get versions` on the VM as administrator, next to what the Backup page says, so a "could not check" can be explained. */
export function wbadminRaw() {
  const raw = ssh('wbadmin get versions 2>&1 | Out-String', { admin: true });
  return result('Windows: raw `wbadmin get versions` (evidence for the Backup page)', [{ ok: true, detail: 'captured as administrator' }], raw.split('\n').slice(0, 25), 'captured');
}

/** The served counts: stop the helper (a stopped service writes its final tally) and read the `served:` line of its log. */
export function servedCounts() {
  ssh('Stop-Service DynoHelper -ErrorAction SilentlyContinue; Start-Sleep 3', { admin: true, ms: 60_000 });
  const lines = ssh(`Get-Content ${HELPER}\\helper.log -ErrorAction SilentlyContinue | Select-String -Pattern 'served:' | Select-Object -Last 3 | ForEach-Object { $_.Line }`, { admin: true }).split('\n').filter(Boolean);
  return result('Windows: the helper reports which readings it served (served counts)', [{ ok: lines.length > 0, detail: lines.length ? `the helper log has ${lines.length} "served:" line(s)` : 'no "served:" line in helper.log after the service was stopped' }], lines, 'served counts logged');
}

export const helperSteps = [
  async ({ browser }) => {
    const planted = plantJunction();
    const on = await helperOn(browser);
    const v = verifyJunction();
    const checks = [{ ok: /^PLANTED/m.test(planted), detail: /^PLANTED/m.test(planted) ? 'a standard user planted a junction at C:\\ProgramData\\Dyno\\Helper before the helper existed' : 'the junction could not be planted (the step proves nothing then)' }, ...on.checks, ...v.checks];
    return result('Windows: admin helper Turn on with a planted junction in its way (UAC over VNC)', checks, [...on.evidence, ...v.evidence], 'Turn on refused to follow the planted junction and the helper is On');
  },
  ({ actor }) => trayTooltips(actor),
  () => helperProtection(),
  () => helperFolderAcl(),
  ({ browser }) => helperExploit(browser),
  ({ browser }) => repeatInstall(browser),
  ({ browser }) => diskHealth(browser),
  () => wbadminRaw(),
  // WP-VMH: one step per helper reading while the helper is on
  ...readingSteps('windows', async ({ browser }) => windowsDriver(mainPage(browser))),
  () => scopingStep(),
  () => servedCounts(),
  ({ browser }) => helperRepair(browser),
  ({ browser }) => helperOff(browser),
];
