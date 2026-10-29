/** One timeline label: where it starts (px from the plot's left edge) and its estimated width (px). */
export interface LabelBox {
  x: number;
  w: number;
}

/** Where a label goes: a row, and whether it hangs left of its anchor (its right end at `x`) to stay inside the plot. */
export interface LabelSlot {
  row: number;
  flip: boolean;
}

/**
 * Places labels, in the order given, on the first row where they touch no other (4 px apart). A label that would run past
 * the plot's right edge (`widthPx`) and has room on its left hangs left of its anchor instead. Pure.
 */
export function placeLabels(labels: readonly LabelBox[], widthPx: number, gap = 4): LabelSlot[] {
  const rows: Array<Array<[number, number]>> = [];
  return labels.map(({ x, w }) => {
    const flip = x + w > widthPx && x >= w;
    const [start, end] = flip ? [x - w, x] : [x, x + w];
    let row = rows.findIndex((r) => r.every(([s, e]) => end + gap <= s || start >= e + gap));
    if (row < 0) row = rows.push([]) - 1;
    rows[row]!.push([start, end]);
    return { row, flip };
  });
}
