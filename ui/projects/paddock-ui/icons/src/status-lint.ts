/** A status glyph written by name in a template (`<pd-glyph name="alert">`), which bypasses `PD_STATUS_GLYPHS`. */
const LITERAL = /<pd-glyph\b[^>]*\bname="(?:pd:)?(alert|error|info)"/g;

/**
 * The places in a source file that draw a status glyph by name instead of through `<pd-status-glyph>` or `statusGlyph`
 * (review 3 P2-4, rule 40): `["path:line: alert"]`. Pure, so the check is tested on text; status.spec.ts runs it over
 * every UI project.
 */
export function statusGlyphProblems(path: string, text: string): string[] {
  const out: string[] = [];
  for (const m of text.matchAll(LITERAL)) {
    const line = text.slice(0, m.index).split('\n').length;
    out.push(`${path}:${line}: <pd-glyph name="${m[1]}"> (use <pd-status-glyph status="…"> or statusGlyph)`);
  }
  return out;
}
