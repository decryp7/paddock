// The UI phase of the Windows run: with Dyno up (win-suite.ps1 -KeepRunning), the real WebView2 DOM through the DevTools port and native input
// through the in-session actor. Steps return the same { title, status, summary, checks, evidence } shape as the other suites.
import { spawnSync } from 'node:child_process';
import { dirname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
import { Actor } from './win-act.mjs';
import { connect, tunnel } from './win-cdp.mjs';
import { snapCheck } from './win-snap.mjs';
import { domTour } from './win-dom-tour.mjs';

const HERE = dirname(fileURLToPath(import.meta.url));
const RULES = resolve(HERE, '..', '..', 'ui', 'projects', 'paddock-e2e', 'src', 'screenshot-check.ts');
const VMWIN = process.env.PADDOCK_VM_WIN ?? '/Volumes/d7sst7/dev-targets/scratch/dyno-vm-t/vm-win.sh';
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
const ssh = (cmd, ms = 60_000) => spawnSync('bash', [VMWIN, 'ssh', cmd], { encoding: 'utf8', timeout: ms }).stdout.split('\n').filter((l) => !l.startsWith('**')).join('\n').trim();
const result = (title, checks, evidence = [], pass = '') => {
  const bad = checks.filter((c) => !c.ok);
  return { title, status: bad.length ? 'fail' : 'pass', summary: bad.length ? bad.map((c) => c.detail).join('; ').slice(0, 240) : pass, checks, evidence };
};

/** The shared screenshot check (`screenshotProblems`: clipped text, raw identifiers, mock/debug strings, colour-only chips) on the real DOM of every Dyno window. */
export async function domScreenCheck(browser, { open = [] } = {}) {
  let mod;
  try { mod = await import(RULES); } catch (e) { return result('Windows: shared screenshot check on the real WebView2 DOM', [{ ok: false, detail: `the shared rules could not be loaded: ${String(e.message).split('\n')[0]}` }]); }
  const checks = [];
  for (const p of browser.contexts().flatMap((c) => c.pages())) {
    const name = (await p.title()) || p.url();
    const problems = await mod.screenshotProblems(p).catch((e) => [`check failed: ${e.message}`]);
    checks.push({ ok: problems.length === 0, detail: problems.length ? `${name}: ${problems.slice(0, 3).join('; ')}` : `${name}: clean (no clipped text, raw identifiers, mock text or colour-only chips)` });
  }
  void open;
  return result('Windows: shared screenshot check on the real WebView2 DOM', checks, [], `${checks.length} window(s) clean`);
}

/** File > Exit (Ctrl+Q): every Dyno process must be gone. */
export async function quitViaMenu(browser) {
  const main = browser.contexts().flatMap((c) => c.pages()).find((p) => /#\/dashboard|#\//.test(p.url()) && !p.url().includes('dialog?id='));
  const checks = [];
  const clicked = await main?.evaluate(async () => {
    const items = () => [...document.querySelectorAll('[role=menuitem]')];
    if (!items().some((e) => e.textContent.trim().startsWith('Exit'))) { [...document.querySelectorAll('button')].find((b) => b.textContent.trim() === 'File').click(); await new Promise((r) => setTimeout(r, 600)); }
    items().find((e) => e.textContent.trim().startsWith('Exit')).click();
    return true;
  }).catch(() => true); // the page goes away with the app
  checks.push({ ok: Boolean(clicked), detail: clicked ? 'File > Exit pressed' : 'no File > Exit item' });
  let left = 'unknown';
  for (let i = 0; i < 20; i++) {
    left = ssh('(Get-Process dyno-app,dyno-agent -ErrorAction SilentlyContinue | Measure-Object).Count');
    if (left === '0') break;
    await sleep(2000);
  }
  checks.push({ ok: left === '0', detail: left === '0' ? 'no dyno-app or dyno-agent process 40 s after Exit' : `${left} Dyno process(es) still running after Exit` });
  return result('Windows: Quit (File > Exit) leaves nothing running', checks, [], 'File > Exit ended the app and the agent');
}

export async function runUiPhase({ log = console.error, extra = [] } = {}) {
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
    // Extension point: one function per wired helper reading (each `async ({ browser, actor }) => step`), run before Quit.
    for (const fn of extra) steps.push(await fn({ browser, actor }));
    steps.push(await quitViaMenu(browser));
    await browser.close().catch(() => {});
  } catch (e) {
    steps.push(result('Windows: UI phase', [{ ok: false, detail: e.message }]));
  } finally { t.close(); await actor.send('quit', 5000).catch(() => {}); }
  void join; void log;
  return steps;
}
