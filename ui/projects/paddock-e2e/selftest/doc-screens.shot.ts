import { mkdtempSync, readFileSync, rmSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { expect, test } from '@playwright/test';
import { captureDocScreens, HASHES_FILE, sha256 } from '../src/index.ts';

// The runner on pages that need no server: the output goes to a scratch folder that the last test checks and removes.
const OUT = mkdtempSync(join(tmpdir(), 'paddock-e2e-selftest-'));
const page = (body: string) => `data:text/html,${encodeURIComponent(`<!doctype html><style>:root{color-scheme:light dark}body{margin:0;background:Canvas;color:CanvasText;font:16px sans-serif}</style><h1 data-pd-region>Doc screen</h1>${body}`)}`;
const now = page('<p id="now"></p><script>document.getElementById("now").textContent = new Date().toISOString() + " " + Intl.DateTimeFormat().resolvedOptions().timeZone + " " + navigator.language + " " + (matchMedia("(prefers-color-scheme: dark)").matches ? "dark" : "light")</script>');
const tall = page('<div style="height:2400px">tall</div>');

captureDocScreens({
  outDir: OUT,
  clock: 'paused',
  env: { ...process.env, SCREENSHOTS: '1' },
  screens: [
    { name: 'clock', url: now, ready: async (p, theme) => void (await expect(p.locator('#now')).toHaveText(`2026-10-22T14:30:00.000Z UTC en-GB ${theme}`)) },
    { name: 'tall', url: tall, frame: 'page', themes: ['dark'] },
    { name: 'tray', url: now, size: { width: 440, height: 300 }, clip: async () => ({ x: 0, y: 0, width: 200, height: 100 }) },
  ],
});

test('the images are written by theme with their hashes; the clock, zone, locale and theme were fixed', async () => {
  const files = ['dark/clock.png', 'light/clock.png', 'dark/tall.png', 'dark/tray.png', 'light/tray.png'];
  const hashes = JSON.parse(readFileSync(join(OUT, HASHES_FILE), 'utf8')) as Record<string, string>;
  expect(Object.keys(hashes)).toEqual([...files].sort());
  for (const f of files) expect(hashes[f]).toBe(sha256(readFileSync(join(OUT, f))));
  const size = (f: string) => ({ w: readFileSync(join(OUT, f)).readUInt32BE(16), h: readFileSync(join(OUT, f)).readUInt32BE(20) });
  expect(size('dark/clock.png')).toEqual({ w: 2880, h: 1800 });
  expect(size('dark/tall.png').h).toBeGreaterThan(2400 * 2);
  expect(size('dark/tray.png')).toEqual({ w: 400, h: 200 });
  rmSync(OUT, { recursive: true, force: true });
});
