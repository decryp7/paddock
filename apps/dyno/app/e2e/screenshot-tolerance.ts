/**
 * How far a regenerated documentation screenshot may differ from the tracked one before the check calls the tracked one stale
 * (`screenshots-check.spec.ts`, Playwright's `toMatchSnapshot`: `threshold` is the per-pixel colour distance (YIQ, 0 to 1) above which a pixel
 * counts as different, `maxDiffPixelRatio` the share of all pixels that may differ). Report-only until its first green run on the macOS runner
 * (ER-41), recorded by run id where the CI job is declared.
 *
 * Measured 2026-11-16 on the Mac that makes the tracked pictures (ER-12: a limit comes with its measurement), with that comparator and threshold 0.2:
 * - run to run, the same tree regenerated three times (74 pictures; compared against each other, two pairs): 69 to 71 of 72 tracked pictures had
 *   0 different pixels; network-connections-dark and -light 1197 and 1264 of 5,184,000 pixels (0.0231% and 0.0244%), recorder-dark 22,972 of
 *   6,336,000 (0.3626%) (the pages that poll every 2 s or more: the second their last poll read differs by run);
 * - stale pictures, the tracked ones before this change against a fresh run: 60 of 72 differ; the smallest pixel difference was throttling-dark
 *   with 1,547 of 5,184,000 pixels (0.0298%), the rest larger or of another size (a size difference always fails).
 * The default (0.01%) is above the 0 measured for the stable pictures and a third of the smallest stale one; the two noisy pages get about 1.5 times
 * their measured noise. Three runs are a small sample: if a stable picture ever fails by a few pixels with nothing changed, measure again
 * before raising a limit, and say so here.
 */
export const DEFAULT_TOLERANCE = { threshold: 0.2, maxDiffPixelRatio: 0.0001 } as const;

/** Pictures of polled pages: a looser ratio each, with the largest difference measured between two runs of one tree. */
export const LOOSER: Readonly<Record<string, { threshold: number; maxDiffPixelRatio: number }>> = {
  // measured 0.0231% and 0.0244% (1197 and 1264 pixels): about 1.6 times
  'network-connections-dark.png': { threshold: 0.2, maxDiffPixelRatio: 0.0004 },
  'network-connections-light.png': { threshold: 0.2, maxDiffPixelRatio: 0.0004 },
  // measured 0.3626% (22,972 pixels) in the dark picture, the light one 0 in these runs but 1.77% to 4% by any-channel difference in the first
  // measurement (the chart series shift by a sample): a looser limit for both, about 1.65 times the larger measured
  'recorder-dark.png': { threshold: 0.2, maxDiffPixelRatio: 0.006 },
  'recorder-light.png': { threshold: 0.2, maxDiffPixelRatio: 0.006 },
};
