/** The grid every `pd:` glyph is drawn on (px). */
export const ICON_GRID = 24;
/**
 * The stroke every `pd:` glyph is drawn with, in screen pixels at any size (the design guide's 1.5 px): the paths carry
 * `vector-effect="non-scaling-stroke"`, so `<pd-glyph>` and `<mat-icon svgIcon="pd:...">` draw the same line weight.
 */
export const ICON_STROKE = 1.5;

/** A filled dot on the 24 px grid: centre and radius. */
export type OutlineDot = readonly [cx: number, cy: number, r: number];

const num = (n: number): number => (Number.isFinite(n) ? Math.round(n * 1000) / 1000 : 0);

/** The filled circles of `dots` as SVG markup (no stroke, so every engine draws them solid). */
export function dotsMarkup(dots: readonly OutlineDot[]): string {
  return dots.map(([cx, cy, r]) => `<circle cx="${num(cx)}" cy="${num(cy)}" r="${num(r)}" fill="currentColor" stroke="none"/>`).join('');
}

/** Strips anything that could end an attribute value or start markup: path data only ever needs digits, letters, spaces, signs and dots. */
function safePath(d: string): string {
  return d.replace(/[^a-zA-Z0-9 .,+-]/g, '');
}

/**
 * The one outline helper: an SVG literal for `paths` on the 24 px grid, stroked with `currentColor` (so the icon takes the
 * colour of its text and the theme), 1.5 screen px wide at any size, round caps and joins, no fill. Apps draw their own
 * domain glyphs with it and register them in their own namespace, so they look like the `pd:` set. Dots are never stroked
 * (a stroke wider than the circle leaves a ring): give them to `outlineIconWithDots`.
 */
export function outlineIcon(...paths: readonly string[]): string {
  return outlineIconWithDots(paths, []);
}

/** Like `outlineIcon`, plus solid filled dots (`[cx, cy, r]` on the 24 px grid). */
export function outlineIconWithDots(paths: readonly string[], dots: readonly OutlineDot[]): string {
  const body = paths.filter((d) => d !== '').map((d) => `<path d="${safePath(d)}" vector-effect="non-scaling-stroke"/>`).join('') + dotsMarkup(dots);
  return (
    `<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 ${ICON_GRID} ${ICON_GRID}" fill="none" stroke="currentColor" ` +
    `stroke-width="${ICON_STROKE}" stroke-linecap="round" stroke-linejoin="round">${body}</svg>`
  );
}
