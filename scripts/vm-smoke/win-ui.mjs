// The UI phase of the Windows run: with Dyno up (win-suite.ps1 -KeepRunning), the real WebView2 DOM through the DevTools port and native input
// through the in-session actor. Steps return the same { title, status, summary, checks, evidence } shape as the other suites.
import { spawnSync } from 'node:child_process';
import { dirname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
import { Actor } from './win-act.mjs';
import { connect, tunnel } from './win-cdp.mjs';
import { importScreenshotRules } from './screenshot-rules.mjs';
import { snapCheck } from './win-snap.mjs';
import { domTour } from './win-dom-tour.mjs';
import { trayCountdownStep, trayMenuStep } from './win-tray.mjs';

const HERE = dirname(fileURLToPath(import.meta.url));
const VMWIN = process.env.PADDOCK_VM_WIN ?? '/Volumes/d7sst7/dev-targets/scratch/dyno-vm-t/vm-win.sh';
/** Windows's own firewall rule names (`@{MicrosoftWindows.Client.OOBE_1000...}`) are printed unchanged in the Firewall page's name column: not a Dyno raw identifier (friendly names come in 1.1.1). */
const SCREENSHOT_OPTIONS = { allow: [/@\{[^}]+\}/] };
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
const ssh = (cmd, ms = 60_000) => spawnSync('bash', [VMWIN, 'ssh', cmd], { encoding: 'utf8', timeout: ms }).stdout.split('\n').filter((l) => !l.startsWith('**')).join('\n').trim();
const result = (title, checks, evidence = [], pass = '') => {
  const bad = checks.filter((c) => !c.ok);
  return { title, status: bad.length ? 'fail' : 'pass', summary: bad.length ? bad.map((c) => c.detail).join('; ').slice(0, 240) : pass, checks, evidence };
};

/** The shared screenshot check (`screenshotProblems`: clipped text, raw identifiers, mock/debug strings, colour-only chips) on the real DOM of every Dyno window. */
export async function domScreenCheck(browser, { open = [] } = {}) {
  let mod;
  try { mod = await importScreenshotRules(); } catch (e) { return result('Windows: shared screenshot check on the real WebView2 DOM', [{ ok: false, kind: 'check', detail: String(e.message) }]); }
  const checks = [];
  for (const p of browser.contexts().flatMap((c) => c.pages())) {
    const name = (await p.title()) || p.url();
    const problems = await mod.screenshotProblems(p, SCREENSHOT_OPTIONS).catch((e) => [`check failed: ${e.message}`]);
    checks.push({ ok: problems.length === 0, detail: problems.length ? `${name}: ${problems.slice(0, 3).join('; ')}` : `${name}: clean (no clipped text, raw identifiers, mock text or colour-only chips)` });
  }
  void open;
  return result('Windows: shared screenshot check on the real WebView2 DOM', checks, [], `${checks.length} window(s) clean`);
}

/**
 * Dyno 1.1.0 plain Quit: File > "Quit Dyno (keeps measuring)" (or "Quit Dyno" when no service keeps the agent; never "Quit and stop measuring", which has its
 * own row, winQuitStop). The app must be gone; the agent may be kept (1.1.0 CHANGELOG, quit.rs), so it is reported, not judged. The processes are counted
 * with a condition wait (up to 15 s), never one immediate count.
 */
export async function quitViaMenu(browser) {
  const main = browser.contexts().flatMap((c) => c.pages()).find((p) => /#\/dashboard|#\//.test(p.url()) && !p.url().includes('dialog?id='));
  const checks = [];
  let label = null;
  const clicked = await main?.evaluate(async () => {
    // Item text may carry an icon ligature or a shortcut hint: match the label inside, never "Quit and stop measuring" (its own row).
    const pick = () => [...document.querySelectorAll('[role=menuitem]')].find((e) => /^(Quit Dyno|Exit)/.test(e.textContent.trim()) && !/stop/i.test(e.textContent));
    if (!pick()) { [...document.querySelectorAll('button')].find((b) => b.textContent.trim() === 'File')?.click(); await new Promise((r) => setTimeout(r, 600)); }
    const it = pick();
    if (!it) return null;
    const text = it.textContent.trim();
    it.click();
    return text;
  }).catch(() => 'Quit Dyno'); // the page goes away with the app
  label = clicked;
  const labels = clicked ? [] : await main?.evaluate(() => [...document.querySelectorAll('[role=menuitem]')].map((e) => e.textContent.trim())).catch(() => []);
  checks.push({ ok: Boolean(clicked), detail: clicked ? `File > "${clicked}" pressed` : `no "Quit Dyno" / "Quit Dyno (keeps measuring)" item under File; the menu items read: ${JSON.stringify(labels)}` });
  const count = (names) => Number(ssh(`(Get-Process ${names} -ErrorAction SilentlyContinue | Measure-Object).Count`));
  let app = NaN;
  for (let i = 0; i < 8; i++) { // condition wait, up to 15 s
    app = count('dyno-app');
    if (app === 0) break;
    await sleep(2000);
  }
  checks.push({ ok: app === 0, detail: app === 0 ? 'no dyno-app process within 15 s of Quit' : `${app} dyno-app process(es) still running 15 s after Quit` });
  const agent = count('dyno-agent');
  checks.push({ ok: true, info: true, detail: `${agent} dyno-agent process(es) after Quit (1.1.0: the agent may be kept; label "${label}")` });
  return result('Windows: Quit Dyno (File menu) ends the app', checks, [], 'the app ended; the agent is reported');
}

export async function runUiPhase({ log = console.error, extra = [], after = [] } = {}) {
  const steps = [];
  const actor = new Actor();
  actor.start();
  await sleep(5000);
  const t = tunnel();
  try {
    const browser = await connect();
    const snaps = await snapCheck(actor, { log });
    steps.push(result('Windows: Aero snap announces Manual (form and viewer dialog, Win+Left)', snaps.map((s) => ({ ok: s.ok, detail: `${s.label}: ${s.detail}` })), [], 'both dialogs announced Manual after Win+Left'));
    const tour = await domTour(browser, { shotDir: process.env.PADDOCK_WIN_SHOTS ?? null, log });
    steps.push(result('Windows: shared screenshot check on every page and dialog of the real WebView2 DOM', tour.checks, tour.evidence, `${tour.checks.length} pages and dialogs clean`));
    steps.push(await trayMenuStep(actor, { browser }).catch((e) => result('Windows: tray right-click menu', [{ ok: false, detail: e.message }])));
    steps.push(await trayCountdownStep(actor).catch((e) => result('Windows: tray menu during an idle countdown', [{ ok: false, detail: e.message }])));
    // A native tray menu blocks the app's UI thread while it is open: make sure the page is in front and answering before the helper steps open Settings.
    { const main = browser.contexts().flatMap((c) => c.pages()).find((p) => !p.url().includes('dialog?id=')); await main?.bringToFront().catch(() => {}); await main?.evaluate(() => 1).catch(() => {}); }
    // Extension point: one function per wired helper reading (each `async ({ browser, actor }) => step`), run before Quit.
    for (const fn of extra) steps.push(await fn({ browser, actor }));
    steps.push(await quitViaMenu(browser));
    await browser.close().catch(() => {});
    // After Quit: steps that need the app gone (the same-version update, the uninstall).
    for (const fn of after) steps.push(...[].concat(await fn()));
  } catch (e) {
    steps.push(result('Windows: UI phase', [{ ok: false, detail: e.message }]));
  } finally { t.close(); await actor.send('quit', 5000).catch(() => {}); }
  void join; void log;
  return steps;
}
