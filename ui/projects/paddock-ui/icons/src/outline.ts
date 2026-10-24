/** The grid every `pd:` glyph is drawn on (px). */
export const ICON_GRID = 24;
/** The stroke every `pd:` glyph is drawn with (px on the 24 px grid), as Pitcrew drew it. */
export const ICON_STROKE = 1.75;

/** Strips anything that could end an attribute value or start markup: path data only ever needs digits, letters, spaces, signs and dots. */
function safePath(d: string): string {
  return d.replace(/[^a-zA-Z0-9 .,+-]/g, '');
}

/**
 * The one outline helper: an SVG literal for `paths` on the 24 px grid, stroked with `currentColor` (so the icon takes the
 * colour of its text and the theme), 1.75 px wide, round caps and joins, no fill. Apps draw their own domain glyphs with
 * it and register them in their own namespace, so they look like the `pd:` set.
 */
export function outlineIcon(...paths: readonly string[]): string {
  const body = paths.map((d) => `<path d="${safePath(d)}"/>`).join('');
  return (
    `<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 ${ICON_GRID} ${ICON_GRID}" fill="none" stroke="currentColor" ` +
    `stroke-width="${ICON_STROKE}" stroke-linecap="round" stroke-linejoin="round">${body}</svg>`
  );
}
