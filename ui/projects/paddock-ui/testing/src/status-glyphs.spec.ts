import { describe, expect, it } from 'vitest';
import { PD_ICON_PATHS } from '@decryp7/paddock-ui/icons';
import { assertStatusGlyphs, statusGlyphRenderProblems } from './status-glyphs';

/** A glyph as `pd-glyph` draws it, optionally inside a `pd-status-glyph` host of `status`. */
function glyph(d: string, status?: string): string {
  const svg = `<svg><path d="${d}"></path></svg>`;
  return status ? `<pd-status-glyph data-status="${status}"><pd-glyph>${svg}</pd-glyph></pd-status-glyph>` : `<pd-glyph>${svg}</pd-glyph>`;
}

function dom(html: string): HTMLElement {
  const root = document.createElement('div');
  root.innerHTML = html;
  return root;
}

describe('statusGlyphRenderProblems (review 4 follow-up)', () => {
  it('accepts status shapes drawn through the map and any other glyph', () => {
    const root = dom(glyph(PD_ICON_PATHS.error, 'error') + glyph(PD_ICON_PATHS.alert, 'warning') + glyph(PD_ICON_PATHS.refresh));
    expect(statusGlyphRenderProblems(root)).toEqual([]);
    expect(() => assertStatusGlyphs(root)).not.toThrow();
  });

  it('finds a status shape bound by name outside the map, and a host that draws another status', () => {
    const root = dom(`<p data-testid="x">${glyph(PD_ICON_PATHS.alert)}</p>` + glyph(PD_ICON_PATHS.alert, 'error'));
    expect(statusGlyphRenderProblems(root)).toEqual(['a warning shape outside <pd-status-glyph> (in [data-testid=x])', '<pd-status-glyph status="error"> draws the warning shape']);
    expect(() => assertStatusGlyphs(root)).toThrow('status glyphs not from PD_STATUS_GLYPHS');
    expect(statusGlyphRenderProblems(dom(`<span class="c">${glyph(PD_ICON_PATHS.info)}</span>`))).toEqual(['a info shape outside <pd-status-glyph> (in <span class="c">)']);
  });

  it('refuses a status state that draws no status glyph at all', () => {
    expect(() => assertStatusGlyphs(dom(glyph(PD_ICON_PATHS.refresh)))).toThrow('expected a status glyph');
  });
});
