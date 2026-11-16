/**
 * How far a regenerated documentation screenshot may differ from the tracked one before the check calls the tracked one stale
 * (`screenshots-check.spec.ts`, Playwright's `toMatchSnapshot`: `threshold` is the per-pixel colour distance (YIQ, 0 to 1) above which a pixel
 * counts as different, `maxDiffPixelRatio` the share of all pixels that may differ). Report-only until its first green run on the macOS runner
 * (ER-41), recorded by run id where the CI job is declared.
 *
 * One limit for every picture, measured 2026-11-16 on the Mac that makes the tracked pictures (ER-12: a limit comes with its measurement). Since the
 * runner holds the clock, the mock rounds what its polled replies read of it to a minute and every picture is taken in the middle of a minute with the
 * live updates stopped, two regenerations of the whole set (92 pictures: the pages, both themes, the dialogs and the helper cards; run after the tracked
 * set was made) matched it with 0 different pixels at threshold 0.2, network connections and the recorder included. Before that (the old spec, a ticking
 * clock) the same pictures differed by up to 0.36% between two runs (recorder-dark) and 0.024% (network connections), and the stale tracked pictures
 * differed from a fresh run by 0.030% at the least (throttling) or in size: so a limit of 0 pixels is both measured to hold and the only one that
 * would see a changed number. If a picture ever fails by a few pixels with nothing changed (another OS update, a font), measure again before
 * allowing a ratio, and say so here.
 */
export const DEFAULT_TOLERANCE = { threshold: 0.2, maxDiffPixelRatio: 0 } as const;

/** Pictures that need a looser limit, each with the largest difference measured between two runs of one tree. None today. */
export const LOOSER: Readonly<Record<string, { threshold: number; maxDiffPixelRatio: number }>> = {};
