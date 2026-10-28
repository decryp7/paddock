import { createHash } from 'node:crypto';
import { mkdirSync, readFileSync, renameSync, writeFileSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { expect, test, type Page } from '@playwright/test';
import { THEMES, type Theme } from './axe.ts';
import { DOC_SCREEN_VIEWPORT, isOn } from './config.ts';
import { captureScreen, FRAME_MAX_HEIGHT } from './capture.ts';

/**
 * The documentation screenshot runner (plan X39): a table of screens × both themes, at 1440 x 900 (the project sets
 * `deviceScaleFactor: 2`), with a frozen clock, a fixed time zone and locale, reduced motion, long pages framed whole,
 * written to `docs/screenshots/<theme>/<name>.png` and a `hashes.json` beside them (SHA-256 of every image, merged so a
 * partial run keeps the others), which the review gate (C24, `docs/images/REVIEWED.md`) compares. A spec file calls
 * {@link captureDocScreens} once; it does nothing unless `SCREENSHOTS` is set, and `paddockConfig` adds the project that
 * runs it only then.
 */
export interface DocScreen {
  /** The file name without extension, unique in the table; `folder/name` puts the image in a sub-folder. */
  readonly name: string;
  /** The URL (a path with its query and hash) opened with `page.goto`; relative to the project's `baseURL`. */
  readonly url: string;
  /** Another window size for this screen (a tray popover, a wide table). */
  readonly size?: { readonly width: number; readonly height: number };
  /** Grow the window to the page's height (up to {@link FRAME_MAX_HEIGHT}) so a long page is shown whole. */
  readonly frame?: 'page';
  /** Steps after the page has loaded (open a menu, fill a field, advance the fake clock) until the screen shows what it documents. */
  readonly ready?: (page: Page, theme: Theme) => Promise<void>;
  /** Themes this screen is taken in (default: all). */
  readonly themes?: readonly Theme[];
}

/** What {@link captureDocScreens} needs. */
export interface DocScreensOptions {
  /** The screens, taken in order. */
  readonly screens: readonly DocScreen[];
  /** The output folder (default `../docs/screenshots` beside the app folder Playwright runs from). */
  readonly outDir?: string;
  /** `by-theme` (default): `<theme>/<name>.png`; `flat`: `<name>-<theme>.png`, the layout the apps wrote before X39. */
  readonly layout?: 'by-theme' | 'flat';
  /** The frozen time (default 2026-10-22 14:30 UTC). */
  readonly frozenAt?: number;
  /** Time zone (default UTC) and locale (default en-GB). */
  readonly timezoneId?: string;
  readonly locale?: string;
  /** Run before every screen after the clock is installed and the theme set (mock options, fixed data). */
  readonly prepare?: (page: Page, theme: Theme) => Promise<void>;
  /**
   * `ticking` (default): the fake clock starts at {@link DocScreensOptions.frozenAt} and runs on, as the apps' runners did
   * (animation frames and timers keep working; dates differ by a few milliseconds). `paused`: the clock stops at exactly
   * `frozenAt` and only moves when a step runs it (`page.clock.runFor`); timers and animation frames do not fire on their own.
   */
  readonly clock?: 'ticking' | 'paused';
  /** Fake-clock time to run after loading, for apps whose live values are a function of the clock (default 0). */
  readonly warmupMs?: number;
  /** A selector that must be visible before `ready` runs (the shell element). */
  readonly appSelector?: string;
  /** The page's main scroller (default the shell's `[data-pd-region=main]`, then the document). */
  readonly mainSelector?: string;
  /** The environment (default `process.env`). */
  readonly env?: Readonly<Record<string, string | undefined>>;
}

/** The tallest window a framed page may grow to. */
export { FRAME_MAX_HEIGHT };

/** The default time every screen's clock starts at: 2026-10-22 14:30:00 UTC. */
export const FROZEN_AT = Date.UTC(2026, 9, 22, 14, 30, 0);

/** The name of the manifest next to the images. */
export const HASHES_FILE = 'hashes.json';

/** One image the table produces. */
export interface PlannedShot {
  readonly screen: DocScreen;
  readonly theme: Theme;
  /** Relative to the output folder, `/`-separated. */
  readonly file: string;
}

/** Every image the table produces, in table order (screens first, then themes). Throws on a duplicate or unsafe name. */
export function planShots(screens: readonly DocScreen[], layout: 'by-theme' | 'flat' = 'by-theme'): PlannedShot[] {
  const seen = new Set<string>();
  const shots: PlannedShot[] = [];
  for (const screen of screens) {
    if (!/^[a-z0-9][a-z0-9-]*(?:\/[a-z0-9][a-z0-9-]*)*$/.test(screen.name)) {
      throw new Error(`screen name ${JSON.stringify(screen.name)} must be lower-case words joined by dashes (folders separated by /)`);
    }
    if (seen.has(screen.name)) throw new Error(`screen ${JSON.stringify(screen.name)} is listed twice`);
    seen.add(screen.name);
    for (const theme of screen.themes ?? THEMES) shots.push({ screen, theme, file: layout === 'flat' ? `${screen.name}-${theme}.png` : `${theme}/${screen.name}.png` });
  }
  return shots;
}

/** The SHA-256 of an image, hex. */
export function sha256(bytes: Uint8Array): string {
  return createHash('sha256').update(bytes).digest('hex');
}

/** Merges `entries` into the manifest at `path` (sorted by name; a missing or damaged manifest starts empty) and writes it atomically. */
export function recordHashes(path: string, entries: Readonly<Record<string, string>>): Record<string, string> {
  let current: Record<string, string> = {};
  try {
    const parsed: unknown = JSON.parse(readFileSync(path, 'utf8'));
    if (parsed && typeof parsed === 'object' && !Array.isArray(parsed)) {
      current = Object.fromEntries(Object.entries(parsed).filter((e): e is [string, string] => typeof e[1] === 'string'));
    }
  } catch {
    // Missing or damaged: start again; the images are the source.
  }
  const merged = Object.fromEntries(Object.entries({ ...current, ...entries }).sort(([a], [b]) => a.localeCompare(b)));
  mkdirSync(dirname(path), { recursive: true });
  const temporary = `${path}.${process.pid}.tmp`;
  writeFileSync(temporary, `${JSON.stringify(merged, null, 2)}\n`);
  renameSync(temporary, path);
  return merged;
}

/** Registers the table's tests (`<name> (<theme>)`) in the calling spec file. */
export function captureDocScreens(options: DocScreensOptions): void {
  const env = options.env ?? process.env;
  const layout = options.layout ?? 'by-theme';
  const out = options.outDir ?? join(process.cwd(), '..', 'docs', 'screenshots');
  const shots = planShots(options.screens, layout);

  test.describe('documentation screenshots', () => {
    test.skip(!isOn(env['SCREENSHOTS']), 'set SCREENSHOTS=1 to rewrite the documentation images');
    test.use({ timezoneId: options.timezoneId ?? 'UTC', locale: options.locale ?? 'en-GB', reducedMotion: 'reduce', colorScheme: 'light' });

    for (const { screen, theme, file } of shots) {
      test(`${screen.name} (${theme})`, async ({ page }) => {
        const { width, height } = screen.size ?? DOC_SCREEN_VIEWPORT;
        await page.setViewportSize({ width, height });
        // A fake clock at a fixed time: live values that are a function of the clock come out the same on every run.
        const frozenAt = options.frozenAt ?? FROZEN_AT;
        if (options.clock === 'paused') {
          await page.clock.install({ time: frozenAt - 1000 });
          await page.clock.pauseAt(frozenAt);
        } else await page.clock.install({ time: frozenAt });
        await page.emulateMedia({ colorScheme: theme, reducedMotion: 'reduce' });
        await options.prepare?.(page, theme);
        await page.goto(screen.url);
        if (options.appSelector) await expect(page.locator(options.appSelector).first()).toBeVisible();
        if (options.warmupMs) await page.clock.runFor(options.warmupMs);
        await screen.ready?.(page, theme);
        // Through captureScreen: settled, not cut, not scrolled, no cover. A long page is framed whole (grown up to FRAME_MAX_HEIGHT).
        const path = join(out, file);
        const bytes = await captureScreen(page, path, { fullPage: screen.frame === 'page' ? 'main' : 'viewport', main: options.mainSelector });
        recordHashes(join(out, HASHES_FILE), { [file]: sha256(bytes) });
      });
    }
  });
}
