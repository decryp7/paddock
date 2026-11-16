/**
 * How far a regenerated documentation screenshot may differ from the tracked one before the check calls the tracked one stale
 * (`screenshots-check.spec.ts`, Playwright's `toMatchSnapshot`: `threshold` is the per-pixel colour distance (YIQ, 0 to 1) above which a pixel
 * counts as different, `maxDiffPixelRatio` the share of all pixels that may differ). Report-only until its first green run on the macOS runner
 * (ER-41), recorded by run id where the CI job is declared.
 *
 * One limit for every picture, measured 2026-11-16 on the Mac that makes the tracked pictures (ER-12: a limit comes with its measurement). Since the
 * runner holds the clock (every picture is taken at one instant, 14:40:30, with the live updates stopped) and the mock rounds what its polled replies
 * read of it to a minute, regenerations of the whole set (92 pictures: the pages, both themes, the dialogs and the helper cards) were compared with the
 * tracked set at threshold 0.2: five of six comparisons had 0 different pixels (network connections and the recorder included), and one had 6 pixels
 * (0.00012%) in processes-dark, a live value that follows the phase of a one-second tick against the instant the clock lands on (the chance of a
 * different sample is about the landing's few tens of milliseconds in a second; it cannot be removed without flattening the live sparklines).
 * Before that (the old spec, a ticking clock) the same pictures differed by up to 0.36% between two runs (recorder-dark), and the stale tracked pictures
 * differed from a fresh run by 0.030% at the least (throttling) or in size. The limit is 0.001% (about 50 pixels of 5.2 million): eight times the largest
 * noise measured, thirty times below the smallest stale picture, and below what a changed number of two digits adds. If a picture ever fails with nothing
 * changed (another OS update, a font), measure again before raising it, and say so here.
 */
export const DEFAULT_TOLERANCE = { threshold: 0.2, maxDiffPixelRatio: 0.00001 } as const;

/** Pictures that need a looser limit, each with the largest difference measured between two runs of one tree. None today. */
export const LOOSER: Readonly<Record<string, { threshold: number; maxDiffPixelRatio: number }>> = {};
