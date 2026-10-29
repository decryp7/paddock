/** The accent (it follows `data-accent` and the scheme) at `percent` over nothing: the heat colour, no colour triple needed. */
function accentWash(percent: number): string {
  return `color-mix(in srgb, var(--pd-accent) ${percent}%, transparent)`;
}

/**
 * A cell's heat: a translucent accent wash proportional to the value (0 → none, `full` → strongest). The number is
 * always printed too; colour only reinforces it.
 */
export function heat(value: number, full: number): string | null {
  if (!Number.isFinite(value) || value <= 0 || full <= 0) return null;
  const level = Math.min(1, value / full);
  // Perceptually even steps: small values stay faint, the top end saturates.
  const percent = Math.round((0.03 + Math.sqrt(level) * 0.27) * 100);
  return accentWash(percent);
}

/**
 * A neutral data bar behind a cell (for ranking many rows): a left-aligned fill proportional to `value / full` in a
 * quiet grey, or in the accent on the column the table is sorted by. The number is always printed too.
 */
export function bar(value: number, full: number, accent = false): string | null {
  if (!Number.isFinite(value) || value <= 0 || full <= 0) return null;
  const pct = Math.round(Math.min(1, value / full) * 100);
  const colour = accent ? accentWash(26) : 'color-mix(in srgb, var(--text-3) 22%, transparent)';
  return `linear-gradient(to right, ${colour} ${pct}%, transparent ${pct}%)`;
}
