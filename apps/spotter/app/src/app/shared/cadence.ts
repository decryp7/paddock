/** Release cadence for the timeline cards: where on a 0..100 axis the buckets that hold releases sit. */

export interface CadenceMark {
  /** Position on the axis, 0 (oldest bucket) to 100 (newest). */
  x: number;
  /** Releases in that bucket (at least 1). */
  count: number;
}

/** The marks for a series of per-bucket release counts, oldest first. */
export function cadenceMarks(releases: readonly number[]): CadenceMark[] {
  const n = releases.length;
  if (n === 0) return [];
  return releases.flatMap((count, i) => (count > 0 ? [{ x: n === 1 ? 50 : (i / (n - 1)) * 100, count }] : []));
}

/** Total releases in the series. */
export function cadenceTotal(releases: readonly number[]): number {
  return releases.reduce((a, b) => a + b, 0);
}
