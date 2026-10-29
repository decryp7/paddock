import { expect, test } from '@playwright/test';
import { expectNothingCovers } from '../../../../ui/projects/paddock-e2e/src/hit-test.ts';
import { openApp } from './app';

// ER-21: nothing covers an interactive element in the main views (the shared check of @decryp7/paddock-e2e, ADR 0027).

// Designed overlaps, one exact pair each.
const DESIGNED: [string, string][] = [
  // annotation-track.ts: "event markers sit on top of them (so a band may be partly covered)"; the bands and glyphs are pointer
  // shortcuts outside the tab order, and the timeline legend's Evidence menu is the equivalent control (WCAG 2.5.8).
  ['button.marker', 'button.ann'],
  // timeline-lanes.ts `.density`: the lane's volume fill is drawn over the bands and is inert on purpose (`pointer-events: none`).
  ['svg.density', 'button.ann'],
  // The same rule gives `.sparse-ticks` (a quiet lane's event ticks) `pointer-events: none`: drawn over the bands, never a click target.
  ['svg.sparse-ticks', 'button.ann'],
  // `.ticks` (the warning ticks along a lane's foot) is inert too since this fix (it took the clicks on the bottom 6 px of every band); its
  // strokes are still drawn over the foot of the bands, like the density.
  ['svg.ticks', 'button.ann'],
  // run-track.ts `.still`: the "still running" label at the open end of a run is text only (`pointer-events: none`, aria-hidden).
  ['span.still', 'button.run'],
  // annotation-track.ts: "Points sit above bands (a crash inside a downtime band stays clickable) and below event markers".
  ['button.ann.point', 'button.ann'],
  // annotation-track.ts: where bands overlap the more severe one is painted on top (a 6 px critical band is not hidden by an info
  // band), so a less severe band is partly under a more severe one. Exact pairs, not "any band over any band".
  ['button.ann.sev-critical', 'button.ann.sev-warning'],
  ['button.ann.sev-critical', 'button.ann.sev-info'],
  ['button.ann.sev-warning', 'button.ann.sev-info'],
];

test.describe('nothing covers an interactive element', () => {
  test('the timeline panel', async ({ page }) => {
    await openApp(page);
    await expectNothingCovers(page.locator('debrief-timeline-panel'), { allow: DESIGNED });
  });

  test('the events grid', async ({ page }) => {
    await openApp(page);
    await expectNothingCovers(page.locator('debrief-events-grid'));
  });

  // hit-test-2 (fixed): a marker at a lane's first moment overhung the label column by 7 px (`.glyph { margin-left: -7px }`) and its
  // button took the clicks on the label's right edge. Clipping the plot cut the glyph and its focus ring (WCAG 2.4.7), so the
  // markers are kept MARKER_EDGE_INSET_PX inside the plot instead.
  test('the first and last glyph of every lane, with their focus ring, stay inside the time scale\'s inset', async ({ page }) => {
    await openApp(page);
    const outside = await page.evaluate(() => {
      const RING = 4; // outline 2 px at an offset of 2 px
      const INSET = 12; // the time scale's inset (--plot-inset): the glyph and its ring may use it, not the label column
      const bad: string[] = [];
      let seen = 0;
      for (const plot of document.querySelectorAll('debrief-timeline-panel .plot.track')) {
        const p = plot.getBoundingClientRect();
        const glyphs = [...plot.querySelectorAll('.marker .glyph debrief-marker-glyph')].map((g) => g.getBoundingClientRect());
        if (!glyphs.length || p.width === 0) continue;
        const first = glyphs.reduce((a, b) => (b.left < a.left ? b : a));
        const last = glyphs.reduce((a, b) => (b.right > a.right ? b : a));
        seen++;
        if (first.left - RING < p.left - INSET - 0.5) bad.push(`first glyph ring ${Math.round(p.left - INSET - (first.left - RING))} px into the label column`);
        if (last.right + RING > p.right + INSET + 0.5) bad.push(`last glyph ring ${Math.round(last.right + RING - p.right - INSET)} px past the inset`);
      }
      return seen > 0 ? bad : ['no lane with markers'];
    });
    expect(outside).toEqual([]);
  });

  test('a more severe annotation band is painted over a less severe one where they overlap', async ({ page }) => {
    await openApp(page);
    const rank: Record<string, number> = { info: 0, warning: 1, critical: 2 };
    const wrong = await page.evaluate(async (rankOf) => {
      const sev = (e: Element) => [...e.classList].find((c) => c.startsWith('sev-'))?.slice(4) ?? '';
      const bands = [...document.querySelectorAll('debrief-timeline-panel button.ann:not(.point)')].filter((b) => b.getBoundingClientRect().width > 0);
      const out: string[] = [];
      let overlaps = 0;
      let sc: Element | null = document.querySelector('debrief-timeline-panel button.lane-label');
      while (sc && !(sc.scrollHeight > sc.clientHeight + 1 && /(auto|scroll)/.test(getComputedStyle(sc).overflowY))) sc = sc.parentElement;
      const scan = () => {
      for (let i = 0; i < bands.length; i++) {
        for (let j = i + 1; j < bands.length; j++) {
          const a = bands[i]!.getBoundingClientRect();
          const b = bands[j]!.getBoundingClientRect();
          const [l, r, t, bt] = [Math.max(a.left, b.left), Math.min(a.right, b.right), Math.max(a.top, b.top), Math.min(a.bottom, b.bottom)];
          if (r - l < 1 || bt - t < 1) continue;
          const stack = document.elementsFromPoint((l + r) / 2, (t + bt) / 2).map((e) => e.closest('button.ann')).filter((e) => e === bands[i] || e === bands[j]);
          if (!stack.length) continue; // not both reachable here (a marker or the scroller edge)
          overlaps++;
          const want = (rankOf[sev(bands[i]!)] ?? 0) >= (rankOf[sev(bands[j]!)] ?? 0) ? bands[i]! : bands[j]!;
          if (rankOf[sev(bands[i]!)] !== rankOf[sev(bands[j]!)] && stack[0] !== want) out.push(`${sev(stack[0]!)} band over ${sev(want)}`);
        }
      }
      };
      // The overlap of a 6 px critical band and an info band shows once the lanes scroll.
      const frame = () => new Promise<void>((done) => requestAnimationFrame(() => requestAnimationFrame(() => done())));
      for (const share of [0, 1 / 3, 2 / 3, 1]) {
        sc?.scrollTo({ top: (sc.scrollHeight - sc.clientHeight) * share, behavior: 'instant' });
        await frame();
        scan();
      }
      sc?.scrollTo({ top: 0, behavior: 'instant' });
      return overlaps > 0 ? out : ['no overlapping bands to check'];
    }, rank);
    expect(wrong).toEqual([]);
  });

  test('the warning ticks along a lane foot take no clicks: the annotation bands under them do', async ({ page }) => {
    await openApp(page);
    const taken = await page.evaluate(() => {
      const ticks = [...document.querySelectorAll('debrief-timeline-panel svg.ticks')];
      const bad = ticks.flatMap((t) => {
        const r = t.getBoundingClientRect();
        // Every 12th px along the strip: no point of it may hit the svg or its strokes.
        return Array.from({ length: Math.floor(r.width / 12) }, (_, i) => document.elementFromPoint(r.left + 6 + i * 12, r.top + r.height / 2)).filter((h) => h && t.contains(h)).map(() => 'tick strip took a click');
      });
      return ticks.length ? bad : ['no tick strip to check'];
    });
    expect(taken).toEqual([]);
  });

  test('the right edge of a lane label belongs to the label, not to a marker', async ({ page }) => {
    await openApp(page);
    const wrong = await page.evaluate(() =>
      [...document.querySelectorAll('debrief-timeline-panel button.lane-label')].flatMap((label) => {
        const r = label.getBoundingClientRect();
        const own = document.elementFromPoint(r.left + r.width / 2, r.top + r.height / 2);
        if (r.width === 0 || !own || !label.contains(own)) return []; // scrolled out of the panel
        // Every pixel row of the label's last 3 px (a run point's hit area is a few px tall and sits off the centre).
        const rows = Array.from({ length: Math.floor(r.height) - 2 }, (_, i) => r.top + 1 + i).filter((y) => label.contains(document.elementFromPoint(r.left + r.width / 2, y))); // rows the panel shows
        return rows.flatMap((y) => [r.right - 1, r.right - 3].map((x) => [x, y] as const)).flatMap(([x, y]) => {
          const hit = document.elementFromPoint(x, y);
          return hit && label.contains(hit) ? [] : [`${label.getAttribute('aria-label')?.slice(0, 24)} y+${Math.round(y - r.top)} -> ${hit?.getAttribute('class')?.slice(0, 20)}`];
        }).slice(0, 2);
      }),
    );
    expect(wrong).toEqual([]);
  });
});
