// The restart check (high priority, from the user's real PC): after a Windows restart the admin helper must work without Repair or another UAC.
// 1. install and turn the helper on as the owner (UAC over VNC), 2. record the service (sc qc, sc query), the helper.log tail and the Settings card,
// 3. restart the guest (a real OS restart; the auto-logon is switched on again first), wait for the GUI session, 4. record the same again before Dyno
// runs and after Dyno is launched, with no Repair, and read a helper reading, 5. when it fails keep the ACLs and the System event log entries.
import { cardText, helperOn, openSettings, ssh } from './win-helper.mjs';
import { connect, tunnel } from './win-cdp.mjs';

const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
const HELPER = 'C:\\ProgramData\\Dyno\\Helper';
const result = (title, checks, evidence = [], pass = '') => { const bad = checks.filter((c) => !c.ok); return { title, status: bad.length ? 'fail' : 'pass', summary: bad.length ? bad.map((c) => c.detail).join('; ').slice(0, 240) : pass, checks, evidence }; };

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

/** `ctx`: { vm(args, ms), waitConsole(), launchDyno(), log }. Returns the steps. */
export async function runRebootPhase(ctx) {
  const steps = [];
  let t = tunnel();
  let browser = await connect();
  const on = await helperOn(browser);
  steps.push(on);
  const before = snapshot();
  const cardBefore = await cardState(browser);
  steps.push(result('Windows restart: helper before the restart (installed and on as the owner)', [
    { ok: before.state === 'RUNNING', detail: `service ${before.state}, start type ${before.start}` },
    { ok: /On/.test(cardBefore), detail: `card: ${cardBefore.slice(0, 80)}` },
  ], [before.qc, before.q, '--- helper.log tail', before.log, `card: ${cardBefore}`].join('\n').split('\n'), `service RUNNING (${before.start}), card On`));
  await browser.close().catch(() => {});
  t.close();

  // The restart. Windows resets AutoAdminLogon after a restart that ends a manual session, so it is set again first.
  ctx.log('restarting the guest (a real OS restart)');
  ssh('Set-ItemProperty "HKLM:\\SOFTWARE\\Microsoft\\Windows NT\\CurrentVersion\\Winlogon" AutoAdminLogon 1; shutdown /r /t 3', { admin: true, ms: 60_000 });
  await sleep(50_000);
  const session = await ctx.waitConsole();
  if (!session) { steps.push(result('Windows restart: the guest did not come back to a GUI session', [{ ok: false, detail: 'no console session after the restart (6 min)' }])); return steps; }
  await sleep(40_000); // services and the logon's own start-ups settle

  const idle = snapshot();
  const checks = [
    { ok: idle.state === 'RUNNING' || /DEMAND|AUTO/i.test(idle.start), detail: `after the restart, before Dyno runs: service ${idle.state}, start type ${idle.start}` },
  ];
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
  checks.push({ ok: after.state === 'RUNNING', detail: `after Dyno started (no Repair): service ${after.state}, start type ${after.start}` });
  checks.push({ ok: /On/.test(card) && !/Repair|Not running|Needs repair/.test(card), detail: `card after the restart: ${card.slice(0, 100)}` });
  checks.push({ ok: /Read by the administrator helper/i.test(readingText), detail: /Read by the administrator helper/i.test(readingText) ? 'a helper reading (Processes) works after the restart' : 'the Processes page does not show the helper chip after the restart' });
  const evidence = ['--- after the restart, before Dyno runs', idle.qc, idle.q, '--- after Dyno started', after.qc, after.q, '--- helper.log tail', after.log, `card: ${card}`, `Processes text: ${readingText.slice(0, 600)}`].join('\n').split('\n');
  {
    const acl = ssh(`"== C:\\ProgramData\\Dyno"; (Get-Acl "C:\\ProgramData\\Dyno").Owner; icacls "C:\\ProgramData\\Dyno"; "== ${HELPER}"; (Get-Acl "${HELPER}").Owner; icacls "${HELPER}"`, { admin: true });
    const ev = ssh(`Get-WinEvent -LogName System -MaxEvents 400 -ErrorAction SilentlyContinue | Where-Object { $_.Message -match 'DynoHelper|Dyno Helper' } | Select-Object -First 15 | ForEach-Object { "$($_.TimeCreated.ToString('s')) id=$($_.Id) $($_.ProviderName): $(($_.Message -split '\\n')[0])" }`, { admin: true });
    evidence.push('--- ACLs (always recorded)', ...acl.split('\n'), '--- System event log, DynoHelper entries', ...ev.split('\n'));
  }
  steps.push(result('Windows restart: the helper works after a real restart without Repair or UAC', checks, evidence, 'service running, card On, readings work, no Repair'));
  return steps;
}
