// The Windows admin helper steps (Dyno 1.1.0): Turn on through the real UAC consent over VNC, the helper's own folders must not be writable
// by a standard user (the 7501 release blocker: C:\ProgramData\Dyno\Helper inherited Users create rights, and the SYSTEM helper installed
// smartctl into a user-owned tree), disk health install, Turn off. Each step returns { title, status, summary, checks, evidence }. They are `extra`
// steps of win-ui.mjs's runUiPhase and need run-win.mjs --vnc (the VM's VNC) and --ui (the DevTools port).
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

async function openSettings(browser) {
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

const cardText = (s, heading) => s.evaluate((n) => { const h = [...document.querySelectorAll('h1,h2,h3,h4')].find((e) => e.textContent.trim().startsWith(n)); const c = h?.closest('section,article,div[class*=card],pd-card') ?? h?.parentElement?.parentElement; return (c?.innerText ?? '').replace(/\s+/g, ' '); }, heading);

/** Turn on: the UAC credential prompt (the test account is a standard user) is answered over VNC with a per-run password for paddock-admin. */
export async function helperOn(browser, { vnc = new Vnc() } = {}) {
  const checks = [];
  const evidence = [];
  const s = await openSettings(browser);
  checks.push({ ok: Boolean(s), detail: s ? 'opened Settings > Dyno' : 'the Settings dialog did not open' });
  if (!s) return result('Windows: admin helper Turn on (UAC consent over VNC)', checks);
  const pw = randomBytes(12).toString('hex').replace(/[A-F]/g, '').slice(0, 20) + 'x'; // lowercase and digits: VNC typing drops the shift state
  ssh('$p = [Console]::In.ReadLine(); net user paddock-admin $p | Out-Null', { admin: true, input: `${pw}\n` });
  runPassword = pw;
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
  checks.push({ ok: first.outcome === 'finished', detail: `first install: ${first.outcome}` });
  const second = await pressInstallAndWait(browser);
  checks.push({ ok: second.outcome === 'finished' || second.outcome === 'no install button', detail: second.outcome === 'no install button' ? 'the card already says Installed (no second install offered)' : `repeat install: ${second.outcome}${second.outcome === 'failed' ? ' (' + (second.log.match(/Error=.*/)?.[0] ?? '') + ')' : ''}` });
  return result('Windows: repeat install of the disk health tool', checks, [], 'a second install works');
}

import { readingSteps, windowsDriver } from './helper-readings.mjs';
const mainPage = (browser) => browser.contexts().flatMap((c) => c.pages()).find((p) => !p.url().includes('dialog?id='));

export const helperSteps = [
  ({ browser }) => helperOn(browser),
  () => helperFolderAcl(),
  ({ browser }) => helperExploit(browser),
  ({ browser }) => diskHealth(browser),
  ({ browser }) => repeatInstall(browser),
  // WP-VMH: one step per helper reading while the helper is on
  ...readingSteps('windows', async ({ browser }) => windowsDriver(mainPage(browser))),
  ({ browser }) => helperOff(browser),
];
