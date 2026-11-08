#!/usr/bin/env node
// Drives the real Dyno window on the Windows test VM through the WebView2 DevTools port (win-suite.ps1 starts Dyno with
// WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS=--remote-debugging-port=9222), over an SSH tunnel, with Playwright's connectOverCDP. The DOM is
// real, so paddock-e2e's `screenshotProblems` (clipped text, raw identifiers, chips) runs on it as in the Dyno e2e specs.
//
//   node scripts/vm-smoke/win-cdp.mjs pages            list the windows (pages) and their buttons, links and tabs
//   node scripts/vm-smoke/win-cdp.mjs eval <js>         evaluate in the first Dyno page
// Needs the guest running (run-win.mjs --keep) and the repo's node_modules (Playwright).
import { spawn, spawnSync } from 'node:child_process';
import { dirname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

const HERE = dirname(fileURLToPath(import.meta.url));
const VMWIN = process.env.PADDOCK_VM_WIN ?? '/Volumes/d7sst7/dev-targets/scratch/dyno-vm-t/vm-win.sh';
const VMDIR = process.env.PADDOCK_WIN_VM_DIR ?? '/Volumes/d7sst7/vms/paddock-win11-test';
const LOCAL = 19222;

/** The SSH tunnel to the guest's DevTools port; returns { close }. Bounded by the caller's lifetime. */
export function tunnel() {
  const ip = spawnSync('bash', [VMWIN, 'ip'], { encoding: 'utf8', timeout: 30_000 }).stdout.trim().split('\n')[0];
  const child = spawn('ssh', ['-N', '-L', `${LOCAL}:127.0.0.1:9222`, '-i', join(VMDIR, 'keys', 'paddock-test'), '-o', 'IdentitiesOnly=yes', '-o', 'BatchMode=yes', '-o', `UserKnownHostsFile=${join(VMDIR, 'keys', 'known_hosts')}`, `paddock-test@${ip}`], { stdio: 'ignore' });
  return { close: () => child.kill('SIGTERM') };
}

export async function connect() {
  const { chromium } = await import('@playwright/test');
  for (let i = 0; i < 20; i++) {
    try { return await chromium.connectOverCDP(`http://127.0.0.1:${LOCAL}`); } catch { await new Promise((r) => setTimeout(r, 1500)); }
  }
  throw new Error('no DevTools endpoint on the guest port 9222 (is Dyno running with the remote debugging port?)');
}

async function main(argv) {
  const t = tunnel();
  try {
    const browser = await connect();
    const pages = browser.contexts().flatMap((c) => c.pages());
    if (argv[0] === 'eval') console.log(JSON.stringify(await pages[0].evaluate(argv.slice(1).join(' '))));
    else {
      for (const p of pages) {
        console.log(`== ${p.url()} | ${await p.title()}`);
        const names = await p.evaluate(() => [...document.querySelectorAll('button, a[href], [role=tab], [role=menuitem]')].map((e) => `${e.tagName.toLowerCase()}:${(e.getAttribute('aria-label') || e.textContent || '').trim().replace(/\s+/g, ' ').slice(0, 60)}`).filter((x) => !x.endsWith(':')));
        console.log(names.join('\n'));
      }
    }
    await browser.close();
  } finally { t.close(); }
}

if (process.argv[1] && resolve(process.argv[1]) === fileURLToPath(import.meta.url)) main(process.argv.slice(2)).then(() => process.exit(0), (e) => { console.error(e.message); process.exit(1); });
