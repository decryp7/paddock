// Finds the headless browser the image scripts render with. Never the installed Google Chrome (Paddock rule 7): only an explicit
// $CHROME or $CHROMIUM_TEST_PATH (set by each app's install-test-browser script), or Playwright's pinned Chrome for Testing or
// Chromium ($PLAYWRIGHT_BROWSERS_PATH or the default cache; install with `npx playwright install chromium`). Plain Node, no dependencies.
import { existsSync, mkdtempSync, readdirSync, rmSync, statSync, writeFileSync } from 'node:fs';
import { homedir, platform, tmpdir } from 'node:os';
import { execFileSync } from 'node:child_process';
import { join } from 'node:path';

const cacheDirs = () => {
  const env = process.env.PLAYWRIGHT_BROWSERS_PATH;
  if (env && env !== '0') return [env];
  const os = platform();
  if (os === 'darwin') return [join(homedir(), 'Library/Caches/ms-playwright')];
  if (os === 'win32') return [join(process.env.LOCALAPPDATA || join(homedir(), 'AppData/Local'), 'ms-playwright')];
  return [join(process.env.XDG_CACHE_HOME || join(homedir(), '.cache'), 'ms-playwright')];
};
// Executables inside one chromium-<rev>/<chrome-dir>/ folder, by platform.
const candidates = (dir) => [
  'Google Chrome for Testing.app/Contents/MacOS/Google Chrome for Testing', 'Chromium.app/Contents/MacOS/Chromium',
  'chrome', 'chrome.exe',
].map((p) => join(dir, p));

/** Path of the browser to run headless; throws with a clear next step when none is found. */
export function findChrome() {
  for (const name of ['CHROME', 'CHROMIUM_TEST_PATH']) {
    const p = process.env[name];
    if (!p) continue;
    if (!existsSync(p)) throw new Error(`${name}=${p} does not exist`);
    return p;
  }
  for (const cache of cacheDirs()) {
    if (!existsSync(cache)) continue;
    const revs = readdirSync(cache).filter((d) => /^chromium-\d+$/.test(d)).sort((a, b) => Number(b.split('-')[1]) - Number(a.split('-')[1]));
    for (const rev of revs) {
      for (const sub of readdirSync(join(cache, rev)).filter((d) => d.startsWith('chrome-'))) {
        const hit = candidates(join(cache, rev, sub)).find(existsSync);
        if (hit) return hit;
      }
    }
  }
  throw new Error('No pinned Chrome for Testing found. Run `npx playwright install chromium` or set CHROME to a Chrome for Testing/Chromium '
    + 'binary. The installed Google Chrome is never used.');
}

/** Renders `svgPath` at w x h (CSS px) in the given theme to the PNG `outPath`, at `scale` device pixels per CSS px. Chrome for
 *  Testing can exit non-zero after writing the screenshot (its teardown watchdog), so success is judged by a fresh output file. */
export function screenshotSvg(svgPath, outPath, w, h, theme, scale = 2) {
  const dir = mkdtempSync(join(tmpdir(), 'paddock-shot-'));
  const html = join(dir, `${theme}.html`), bg = theme === 'dark' ? '#0d1117' : '#ffffff';
  writeFileSync(html, `<!doctype html><meta name="color-scheme" content="${theme}"><style>html,body{margin:0;background:${bg};color-scheme:${theme}}img{display:block}</style><img src="file://${svgPath}" width="${w}" height="${h}">`);
  rmSync(outPath, { force: true });
  try {
    execFileSync(findChrome(), ['--headless=new', '--disable-gpu', '--hide-scrollbars', `--force-device-scale-factor=${scale}`, `--force-dark-mode=${theme === 'dark'}`,
      `--user-data-dir=${join(dir, 'profile')}`, `--screenshot=${outPath}`, `--window-size=${w},${h}`, `file://${html}`], { stdio: 'ignore', timeout: 120_000 });
  } catch (e) {
    if (!existsSync(outPath) || statSync(outPath).size === 0) throw e;
  } finally {
    rmSync(dir, { recursive: true, force: true });
  }
}
