import { PD_ICON_PATHS, PD_STATUS_GLYPHS, type PdStatus } from '@decryp7/paddock-ui/icons';

/** The drawn path of each status's glyph, and back. */
const SHAPE_STATUS = new Map<string, PdStatus>(
  (Object.entries(PD_STATUS_GLYPHS) as [PdStatus, keyof typeof PD_ICON_PATHS][]).map(([status, glyph]) => [PD_ICON_PATHS[glyph], status]),
);

/**
 * What a rendered component draws as a status without the suite's one map (review 4 follow-up, rule 40): every glyph whose
 * path is a status shape (circle-×, triangle, circle-i, check) must sit in a `<pd-status-glyph>` whose status draws that
 * shape. It reads the drawn paths, so a status glyph bound by `[name]` (which the source lint cannot see) is caught too. Call
 * it on a component's root in its status states; `[]` is clean.
 */
export function statusGlyphRenderProblems(root: ParentNode): string[] {
  const out: string[] = [];
  for (const path of Array.from(root.querySelectorAll('svg path'))) {
    const status = SHAPE_STATUS.get(path.getAttribute('d') ?? '');
    if (!status) continue;
    const host = path.closest('pd-status-glyph');
    const declared = host?.getAttribute('data-status');
    if (!host) out.push(`a ${status} shape outside <pd-status-glyph> (in ${describe(path)})`);
    else if (declared !== status) out.push(`<pd-status-glyph status="${declared}"> draws the ${status} shape`);
  }
  return out;
}

function describe(el: Element): string {
  const owner = el.closest('[data-testid], [class]');
  const id = owner?.getAttribute('data-testid');
  return id ? `[data-testid=${id}]` : `<${owner?.tagName.toLowerCase() ?? 'svg'} class="${owner?.getAttribute('class') ?? ''}">`;
}

/**
 * Throws unless `root` draws at least one status glyph and draws every status shape through the suite's one map
 * (`statusGlyphRenderProblems`): for a component's status-state test.
 */
export function assertStatusGlyphs(root: ParentNode): void {
  if (!root.querySelector('pd-status-glyph')) throw new Error('expected a status glyph (<pd-status-glyph>) in this state');
  const problems = statusGlyphRenderProblems(root);
  if (problems.length) throw new Error(`status glyphs not from PD_STATUS_GLYPHS:\n- ${problems.join('\n- ')}`);
}
