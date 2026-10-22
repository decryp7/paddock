/**
 * A cell's heat: a translucent accent wash proportional to the value (0 → none, `full` → strongest). The number is
 * always printed too; colour only reinforces it.
 */
export function heat(value: number, full: number): string | null {
  if (!Number.isFinite(value) || value <= 0 || full <= 0) return null;
  const level = Math.min(1, value / full);
  // Perceptually even steps: small values stay faint, the top end saturates.
  const alpha = Math.round((0.03 + Math.sqrt(level) * 0.27) * 100) / 100;
  return `rgb(var(--heat) / ${alpha})`;
}
