import { expect, test } from './test';
import { existsSync, readFileSync } from 'node:fs';
import { join, resolve } from 'node:path';
import { DEFAULT_TOLERANCE, LOOSER } from './screenshot-tolerance';

/**
 * Whether the tracked documentation screenshots are current (ER-40: generated images are regenerated from their spec, never edited, and a
 * check says when they are stale). `SCREENSHOTS_CHECK=1 SCREENSHOT_DIR=<scratch> npx playwright test --project=screenshots-check` first regenerates
 * `screenshots.spec` and `screenshots-docs-dialogs.spec` into SCREENSHOT_DIR (the project depends on it), then compares each tracked picture
 * in the tracked pictures folder (apps/dyno/docs) with its regenerated one by a pixel tolerance (e2e/screenshot-tolerance.ts), not byte for byte: two runs of one tree differ
 * in the pictures of polled pages, and another OS renders text differently. A stale picture fails with the command that regenerates it.
 */
const TRACKED = process.env['SCREENSHOTS_TRACKED'] ?? resolve(__dirname, '..', '..', 'docs', 'screenshots');
const OUT = process.env['SCREENSHOT_DIR'] ?? '';
const REGENERATE = 'cd apps/dyno/app && npm run screenshots -- -g "documentation screenshots"   (then commit the changed pictures in the tracked pictures folder of apps/dyno/docs, after an independent look)';

// The pictures the runner makes: the names in hashes.json (captureDocScreens records every picture it writes). Pictures captured outside it (a real OS
// dialog, a Windows machine) are not regenerated here; conformance C24 keeps their review.
const MANIFEST = join(TRACKED, 'hashes.json');
const names = existsSync(MANIFEST) ? Object.keys(JSON.parse(readFileSync(MANIFEST, 'utf8')) as Record<string, string>).sort() : [];

test.describe('tracked documentation screenshots are current', () => {
  test('there is a list of the pictures the runner makes', () => {
    expect(existsSync(MANIFEST), 'the tracked pictures have no hashes.json: regenerate them from their specs').toBe(true);
  });

  test.skip(!OUT, 'set SCREENSHOT_DIR (the folder screenshots.spec and screenshots-docs-dialogs.spec wrote)');
  for (const name of names) {
    test(name, async () => {
      const made = join(OUT, name);
      expect(existsSync(made), `${name} is tracked but no spec made it: remove it from the tracked pictures folder, or restore the spec that makes it`).toBe(true);
      const tolerance = LOOSER[name] ?? DEFAULT_TOLERANCE;
      try {
        expect(readFileSync(made)).toMatchSnapshot(name.split('/'), { threshold: tolerance.threshold, maxDiffPixelRatio: tolerance.maxDiffPixelRatio });
      } catch (e) {
        throw new Error(`${name} is stale (it differs from a fresh run beyond threshold ${tolerance.threshold}, ratio ${tolerance.maxDiffPixelRatio}). Regenerate it from its spec, never by hand (ER-40):\n  ${REGENERATE}\n${(e as Error).message}`, { cause: e });
      }
    });
  }
});
