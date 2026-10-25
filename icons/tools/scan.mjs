// Seam scan for the 1024 px masters: a smooth tile must not show a hard step in a vertical column.
//
// Rule: render the tile background alone (`masterSvg(app, { bare: true })`: no glyph, no livery sweep), then walk every pixel
// column. A pixel is "tile body" when it and the pixels 8 px to its left, right, above and below are fully opaque (this drops the
// rounded edge, its anti-aliasing, the edge and rim strokes and the drop shadow). Along each run of adjacent body pixels no
// channel may change by more than `maxStep` (3) levels between neighbours. The livery bands and the glyph are intentional hard
// edges and are not part of the bare render.
import { Resvg } from '@resvg/resvg-js';

export const MAX_STEP = 3;
const MARGIN = 8;

/** @returns {{x:number,y:number,channel:string,from:number,to:number}[]} the violations, empty when the tile is smooth */
export function scanSeams({ pixels, width, height }, { maxStep = MAX_STEP, margin = MARGIN } = {}) {
  const opaque = (x, y) => x >= 0 && y >= 0 && x < width && y < height && pixels[(y * width + x) * 4 + 3] === 255;
  const body = (x, y) => opaque(x, y) && opaque(x - margin, y) && opaque(x + margin, y) && opaque(x, y - margin) && opaque(x, y + margin);
  const out = [];
  for (let x = 0; x < width; x++) {
    for (let y = 1; y < height; y++) {
      if (!body(x, y) || !body(x, y - 1)) continue;
      const a = (y * width + x) * 4, b = a - width * 4;
      for (let c = 0; c < 3; c++) {
        if (Math.abs(pixels[a + c] - pixels[b + c]) > maxStep) out.push({ x, y, channel: 'rgb'[c], from: pixels[b + c], to: pixels[a + c] });
      }
    }
  }
  return out;
}

/** Render an SVG at `px` wide into raw RGBA. */
export const render = (svg, px = 1024) => {
  const r = new Resvg(svg, { fitTo: { mode: 'width', value: px }, shapeRendering: 2, textRendering: 0 }).render();
  return { pixels: r.pixels, width: r.width, height: r.height };
};
