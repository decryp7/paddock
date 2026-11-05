// The wide DOM check of the Windows run: every page of the sidebar and every dialog (Settings with each of its sections, About, What's new,
// Report a problem) is opened in the real app over the WebView2 DevTools port, and paddock-e2e's `screenshotProblems` (clipped text, raw
// identifiers, mock and debug strings, colour-only status chips) runs on the live DOM, with a screenshot kept as evidence. The shared
// layout checks that are not on main yet (checkboxLayoutProblems, button-label wrap, widget scroll) are added to `EXTRA_CHECKS` when they land.
import { dirname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
import { mkdirSync, writeFileSync } from 'node:fs';

const HERE = dirname(fileURLToPath(import.meta.url));
const RULES = resolve(HERE, '..', '..', 'ui', 'projects', 'paddock-e2e', 'src', 'screenshot-check.ts');
/** Windows's own firewall rule names (`@{MicrosoftWindows.Client.OOBE_1000...}`) are printed unchanged in the Firewall page's name column: not a Dyno raw identifier (friendly names come in 1.1.1). */
const SCREENSHOT_OPTIONS = { allow: [/@\{[^}]+\}/] };
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
/** Page-level checks beyond screenshotProblems: each is `async (page) => string[]` (problems). Add the shared layout checks here when they land. */
export const EXTRA_CHECKS = [
  // A dialog must fit its window: no scrolling of the page itself and the window inside the screen (the check that matters at 150 % scale).
  async (page) => {
    if (!page.url().includes('dialog?id=')) return [];
    const m = await page.evaluate(() => ({ sw: document.documentElement.scrollWidth, cw: document.documentElement.clientWidth, sh: document.documentElement.scrollHeight, ch: document.documentElement.clientHeight, dpr: window.devicePixelRatio, ow: window.outerWidth, oh: window.outerHeight, aw: screen.availWidth, ah: screen.availHeight }));
    const out = [];
    if (m.sw > m.cw + 1) out.push(`cut off sideways: content ${m.sw}px wide in a ${m.cw}px window (scale ${m.dpr})`);
    if (m.sh > m.ch + 1) out.push(`cut off at the bottom: content ${m.sh}px high in a ${m.ch}px window (scale ${m.dpr})`);
    if (m.ow > m.aw + 1 || m.oh > m.ah + 1) out.push(`the window (${m.ow}x${m.oh}) is larger than the screen (${m.aw}x${m.ah}) at scale ${m.dpr}`);
    return out;
  },
];

const slug = (s) => s.replace(/[^\w]+/g, '-').replace(/^-|-$/g, '').toLowerCase().slice(0, 40);

async function problemsOf(mod, page) {
  const out = (await mod.screenshotProblems(page, SCREENSHOT_OPTIONS).catch((e) => [`check failed: ${e.message}`])).slice();
  for (const extra of EXTRA_CHECKS) out.push(...(await extra(page).catch((e) => [`extra check failed: ${e.message}`])));
  return out;
}

async function openDialog(browser, menu, item) {
  const before = new Set(browser.contexts().flatMap((c) => c.pages()));
  const main = [...before].find((p) => !p.url().includes('dialog?id='));
  const want = { 'Settings': 'pd.settings', 'About': 'pd.about', 'What': 'pd.whats-new', 'Report': 'pd.report-problem' }[item.split(/[ …]/)[0]];
  const existing = [...before].find((p) => want && p.url().includes(want));
  if (existing) return existing;
  await main.evaluate(async ([m, i]) => {
    const items = () => [...document.querySelectorAll('[role=menuitem]')];
    if (!items().some((e) => e.textContent.trim().startsWith(i))) { [...document.querySelectorAll('button')].find((b) => b.textContent.trim() === m).click(); await new Promise((r) => setTimeout(r, 600)); }
    items().find((e) => e.textContent.trim().startsWith(i)).click();
  }, [menu, item]);
  for (let k = 0; k < 40; k++) { await sleep(500); const now = browser.contexts().flatMap((c) => c.pages()).filter((p) => !before.has(p)); if (now.length) return now[0]; }
  return null;
}

/** Returns { checks, evidence }: one check per page or dialog. */
export async function domTour(browser, { shotDir = null, log = () => {} } = {}) {
  let mod;
  try { mod = await import(RULES); } catch (e) { return { checks: [{ ok: false, detail: `the shared screenshot rules could not be loaded: ${String(e.message).split('\n')[0]}` }], evidence: [] }; }
  const checks = [];
  const evidence = [];
  if (shotDir) mkdirSync(shotDir, { recursive: true });
  const shot = async (page, name) => { if (!shotDir) return; try { writeFileSync(join(shotDir, `dom-${slug(name)}.png`), await page.screenshot()); evidence.push(`screenshot: dom-${slug(name)}.png`); } catch { /* the window may not be visible */ } };
  const record = async (page, name) => {
    await sleep(2000);
    const problems = await problemsOf(mod, page);
    checks.push({ ok: problems.length === 0, detail: problems.length ? `${name}: ${problems.slice(0, 3).join('; ')}` : `${name}: clean` });
    await shot(page, name);
    log(`dom: ${checks.at(-1).detail}`);
  };
  const main = browser.contexts().flatMap((c) => c.pages()).find((p) => !p.url().includes('dialog?id='));
  const routes = await main.evaluate(() => [...new Set([...document.querySelectorAll('a[href^="#/"], a[routerlink]')].map((a) => a.getAttribute('href') || '').filter((h) => /^#\//.test(h)))]);
  for (const r of routes) { await main.evaluate((h) => { location.hash = h; }, r); await record(main, `page ${r}`); }
  // Dialogs: Settings with each of its sections, then the Help dialogs.
  const settings = await openDialog(browser, 'File', 'Settings');
  if (settings) {
    const sections = await settings.evaluate(() => [...document.querySelectorAll('[aria-label="Settings pages"] button, [aria-label="Settings pages"] a')].map((b) => b.textContent.trim()).filter(Boolean));
    for (const name of [...new Set(sections)].slice(0, 12)) { await settings.getByRole('button', { name: new RegExp(`^${name.replace(/[.*+?^${}()|[\]\\]/g, '\\$&')}$`) }).first().click().catch(() => {}); await record(settings, `Settings > ${name}`); }
    if (!sections.length) await record(settings, 'Settings');
  } else checks.push({ ok: false, detail: 'Settings did not open' });
  // The file hash dialog opens from a process row; look for its entry point on the Processes page.
  await main.evaluate(() => { location.hash = '#/processes'; }); await sleep(3000);
  const hashEntry = await main.evaluate(() => { const c = [...document.querySelectorAll('button, [role=menuitem], a')].find((e) => /hash|VirusTotal/i.test((e.getAttribute('aria-label') || '') + ' ' + e.textContent)); if (c) { c.click(); return true; } return false; });
  if (hashEntry) { await sleep(2500); const hp = browser.contexts().flatMap((c) => c.pages()).find((p) => p.url().includes('dy.file-hash')); if (hp) await record(hp, 'dialog file hash'); else checks.push({ ok: false, detail: 'the file hash entry was clicked but no dialog opened' }); }
  else checks.push({ ok: true, info: true, detail: 'file hash dialog: no entry point found on the Processes page without selecting a row (not checked)' });
  for (const item of ['About', 'What', 'Report']) {
    const label = { About: 'About', What: 'What’s new', Report: 'Report a problem' }[item];
    const p = await openDialog(browser, 'Help', label);
    if (p) await record(p, `dialog ${label}`); else checks.push({ ok: false, detail: `${label} did not open` });
  }
  return { checks, evidence };
}
