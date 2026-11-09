#!/usr/bin/env node
// The Aero snap check on the Windows test VM (Dyno 1.1.0, ADR 0043 D6, fix aea9cd7f4): a form dialog and a viewer dialog are opened, Win+Left
// is pressed on each, and each must announce size mode `manual` (the shell sets `data-size-mode` on <html> from the size-mode event).
// The app is driven through its DevTools port (win-cdp.mjs), the key through the in-session actor (win-act.ps1).
//   node scripts/vm-smoke/win-snap.mjs        needs the guest running with Dyno up (run-win.mjs --keep) and the actor script staged
import { Actor } from './win-act.mjs';
import { connect, tunnel } from './win-cdp.mjs';

const DIALOG_ID = { 'Report a problem': 'pd.report-problem', About: 'pd.about' };
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
const modeOf = (page) => page.evaluate(() => ({ mode: document.documentElement.dataset.sizeMode ?? null, kind: document.documentElement.dataset.dialogKind ?? null, title: document.title }));

/** Opens a dialog from the main page's menu and returns its page (the one that was not there before). */
async function open(browser, menu, item) {
  const before = new Set(browser.contexts().flatMap((c) => c.pages()));
  const existing = [...before].find((p) => p.url().includes('dialog?id=') && p.url().includes(DIALOG_ID[item]));
  if (existing) return existing;
  const main = [...before].find((p) => /tauri\.localhost\/#/.test(p.url()) && !/dialog/i.test(p.url())) ?? [...before][0];
  await main.evaluate(async ([m, i]) => {
    const items = () => [...document.querySelectorAll('[role=menuitem]')];
    if (!items().some((e) => e.textContent.trim().startsWith(i))) {
      [...document.querySelectorAll('button')].find((b) => b.textContent.trim() === m).click();
      await new Promise((r) => setTimeout(r, 600));
    }
    items().find((e) => e.textContent.trim().startsWith(i)).click();
  }, [menu, item]);
  for (let k = 0; k < 40; k++) {
    await sleep(500);
    const now = browser.contexts().flatMap((c) => c.pages()).filter((p) => !before.has(p));
    if (now.length) return now[0];
  }
  return null;
}

export async function snapCheck(actor, { log = console.error } = {}) {
  const t = tunnel();
  const results = [];
  try {
    const browser = await connect();
    for (const [label, menu, item] of [['form dialog', 'Help', 'Report a problem'], ['viewer dialog', 'Help', 'About']]) {
      let page;
      try { page = await open(browser, menu, item); } catch (e) { results.push({ label, ok: false, detail: `${item}: ${e.message}` }); continue; }
      if (!page) { results.push({ label, ok: false, detail: `${item} did not open a window` }); continue; }
      await page.waitForLoadState('domcontentloaded');
      await sleep(1500);
      try {
      const before = await modeOf(page);
      const title = before.title || item;
      const fg = await actor.send(`fg ${title.replace(/[^\w ]/g, '.')}`);
      const rectBefore = await actor.send(`rect ${title.replace(/[^\w ]/g, '.')}`);
      await actor.send('key LWIN+LEFT');
      let after = before;
      for (let k = 0; k < 20; k++) { await sleep(500); after = await modeOf(page); if (after.mode === 'manual') break; }
      const rectAfter = await actor.send(`rect ${title.replace(/[^\w ]/g, '.')}`);
      await actor.send(`shot snap-${label.split(' ')[0]}`);
      results.push({ label, ok: after.mode === 'manual', detail: `${item}: size mode "${before.mode}" before, "${after.mode}" after Win+Left (window ${rectBefore} to ${rectAfter}; ${fg})` });
      log(`snap: ${results.at(-1).detail}`);
      } catch (e) { results.push({ label, ok: false, detail: `${item}: ${e.message}` }); log(`snap: ${e.message}`); }
      await page.keyboard.press('Escape').catch(() => {});
      await sleep(800);
    }
    await browser.close();
  } finally { t.close(); }
  return results;
}

if (process.argv[1] && process.argv[1].endsWith('win-snap.mjs')) {
  const a = new Actor();
  a.start();
  await sleep(4000);
  const r = await snapCheck(a);
  console.log(JSON.stringify(r, null, 1));
  await a.send('quit').catch(() => {});
  process.exit(r.every((x) => x.ok) ? 0 : 1);
}
