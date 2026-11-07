// Release rows for "Launch at login" (Dyno 1.1.0, start at login in paddock-service): the FIRST-RUN DEFAULT of a fresh install, the switch in Settings (off removes the entry, on puts it
// back exactly once) and the HIDDEN start of the entry's own command (no window). Pure judges plus per-OS readers. The registration is the OS's own state, not a screenshot:
//   Windows: HKCU\Software\Microsoft\Windows\CurrentVersion\Run value "Dyno" = "<install>\dyno-app.exe" --hidden
//   Linux:   ~/.config/autostart/Dyno.desktop with Exec="/usr/bin/dyno-app" "--hidden"
//   macOS:   SMAppService.mainApp (Background Items, read with sfltool dumpbtm in the guest) and, after a 1.0 upgrade, no ~/Library/LaunchAgents/Dyno.plist
import { makeResult } from './verdict.mjs';

export const TITLE_LOGIN = 'Launch at login: on by default on a fresh install; the switch removes and restores the entry once';
export const TITLE_HIDDEN = 'Launch at login: the entry starts Dyno hidden (no window)';
export const TITLE_MAC_UPGRADE = 'Launch at login: upgrade from 1.0 replaces ~/Library/LaunchAgents/Dyno.plist by the login item once';
const chk = (ok, detail, kind) => ({ ok, detail, ...(kind ? { kind } : {}) });
const sleepOf = (p) => p.sleep ?? ((ms) => new Promise((r) => setTimeout(r, ms)));

/** Whether an entry's command starts Dyno hidden: names dyno-app and carries --hidden. */
export const startsHidden = (cmd) => /dyno-app(?:\.exe)?["']?\s+["']?--hidden["']?\s*$/i.test(String(cmd ?? '').trim());

/** Pure judge. f: { defaultEntry:{present,command,count}, afterOff:{present,count}|null, afterOn:{present,command,count}|null, switchBefore } */
export function judgeLoginSwitch(f) {
  const out = [];
  out.push(chk(f.defaultEntry?.present === true, f.defaultEntry?.present ? `first run: the entry is there without Settings being touched (${f.defaultEntry.command || 'registered'})` : 'first run: no start-at-login entry after a fresh install and first start (the default is ON)'));
  if (f.defaultEntry?.present) {
    if (f.defaultEntry.command !== undefined) out.push(chk(startsHidden(f.defaultEntry.command), startsHidden(f.defaultEntry.command) ? 'the entry starts dyno-app with --hidden' : `the entry's command is not "dyno-app --hidden": ${f.defaultEntry.command}`));
    out.push(chk(f.defaultEntry.count === 1, f.defaultEntry.count === 1 ? 'exactly one entry' : `${f.defaultEntry.count} entries (expected exactly one)`));
  }
  if (f.switchBefore !== undefined) out.push(chk(f.switchBefore === 'on', f.switchBefore === 'on' ? 'Settings shows "Start Dyno at login" ON' : `Settings shows the switch ${f.switchBefore}, expected on`, f.switchBefore === 'unknown' ? 'check' : undefined));
  if (!f.afterOff) out.push(chk(false, 'the switch could not be turned off, so removal was not tested', 'check'));
  else out.push(chk(f.afterOff.present === false, f.afterOff.present === false ? 'switch off: the entry is gone' : 'switch off: the entry is still registered'));
  if (!f.afterOn) out.push(chk(false, 'the switch could not be turned on again, so restoration was not tested', 'check'));
  else {
    out.push(chk(f.afterOn.present === true && f.afterOn.count === 1, f.afterOn.present && f.afterOn.count === 1 ? 'switch on again: the entry is back exactly once' : `switch on again: ${f.afterOn.present ? `${f.afterOn.count} entries` : 'no entry'}`));
    if (f.afterOn.command !== undefined && f.afterOn.present) out.push(chk(startsHidden(f.afterOn.command), startsHidden(f.afterOn.command) ? 'the restored entry starts dyno-app with --hidden' : `the restored entry's command is not "dyno-app --hidden": ${f.afterOn.command}`));
  }
  return out;
}

/**
 * The step. p: { readSwitch() -> 'on'|'off'|'unknown', pressSwitch() -> bool, readEntry() -> {present, command?, count}, sleep, now }. A change is polled for up to 20 s (the switch is a job).
 * The default is read FIRST, before anything touches the switch.
 */
export async function loginSwitchStep(p) {
  try {
    const sleep = sleepOf(p);
    const defaultEntry = await p.readEntry();
    const switchBefore = await p.readSwitch();
    const poll = async (want) => { for (let i = 0; i < 10; i++) { const e = await p.readEntry(); if (e.present === want) return e; await sleep(2000); } return p.readEntry(); };
    let afterOff = null, afterOn = null;
    if (switchBefore === 'on' && (await p.pressSwitch())) { afterOff = await poll(false); if (await p.pressSwitch()) afterOn = await poll(true); }
    else if (switchBefore === 'on') afterOff = null;
    return makeResult(TITLE_LOGIN, judgeLoginSwitch({ defaultEntry, afterOff, afterOn, switchBefore }), [JSON.stringify({ defaultEntry, afterOff, afterOn })], 'on by default; off removes the entry; on restores it once');
  } catch (e) {
    return { title: TITLE_LOGIN, status: 'not-measured', cls: 'check', summary: `not measured: ${String(e.message ?? e).split('\n')[0]}`, checks: [{ ok: false, kind: 'check', detail: String(e.message ?? e) }], evidence: [] };
  }
}

/** The hidden start: Dyno is stopped, started with the entry's own command, and after `settleMs` the agent answers and NO Dyno window exists. p: { stop(), startEntry(), agentUp(), windows() -> string[] }. */
export async function hiddenStartStep(p) {
  try {
    const sleep = sleepOf(p);
    await p.stop();
    await p.startEntry();
    let up = false;
    for (let i = 0; i < 30 && !up; i++) { await sleep(4000); up = await p.agentUp(); }
    await sleep(15_000);
    const w = await p.windows();
    const checks = [chk(up, up ? 'the app started and its agent answers' : 'the agent did not answer within 2 minutes of the hidden start', up ? undefined : 'check'), chk(w.length === 0, w.length === 0 ? 'no Dyno window is open (hidden start)' : `a Dyno window is open after a hidden start: ${w.join(' | ')}`)];
    return makeResult(TITLE_HIDDEN, checks, [], 'the entry starts Dyno in the tray without a window');
  } catch (e) {
    return { title: TITLE_HIDDEN, status: 'not-measured', cls: 'check', summary: `not measured: ${String(e.message ?? e).split('\n')[0]}`, checks: [{ ok: false, kind: 'check', detail: String(e.message ?? e) }], evidence: [] };
  }
}

/** Parses `sfltool dumpbtm` text: the Dyno login items (blocks "#N:" naming Dyno with a login-item or app type): { present (an enabled one), count, raw }. */
export function parseBtm(text) {
  const blocks = String(text ?? '').split(/\n\s*#\d+:/).slice(1);
  const mine = blocks.filter((b) => /Dyno/i.test(b) && /Type:\s*(?:app|login item)/i.test(b) && !/dyno\.(?:agent|helper)|Type:\s*(?:legacy )?(?:daemon|agent)/i.test(b));
  const enabled = mine.filter((b) => /Disposition:\s*\[[^\]]*\benabled\b/i.test(b));
  return { present: enabled.length > 0, count: enabled.length, total: mine.length, raw: mine.map((b) => b.replace(/\s+/g, ' ').slice(0, 260)) };
}
