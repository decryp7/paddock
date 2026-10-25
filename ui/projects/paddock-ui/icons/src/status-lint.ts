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

/** A status-like key mapped to a glyph in TypeScript (`{ error: 'alert' }`, `bad: 'close'`): a second status → glyph map. */
const MAP_ENTRY = /\b(error|errors|bad|fail|failed|failure|danger|critical|warn|warning|warnings)\s*:\s*['"](?:pd:)?(alert|error|info|check|close)['"]/g;

/**
 * The places in a TypeScript source that map a status to a glyph themselves instead of deriving from `PD_STATUS_GLYPHS`
 * (review 4 P2-3: the template lint did not see maps in TypeScript or bound `[name]` values). Pure; status.spec.ts runs it.
 */
export function statusMapProblems(path: string, text: string): string[] {
  if (path.endsWith('/icons/src/status.ts')) return [];
  const out: string[] = [];
  const lines = text.split('\n');
  for (const m of text.matchAll(MAP_ENTRY)) {
    const line = text.slice(0, m.index).split('\n').length;
    // A map to statuses (typed `PdStatus`), not to glyphs: `bad: 'error'` names the status, which the shared map draws.
    if (lines.slice(Math.max(0, line - 4), line).some((l) => /\bPdStatus\b/.test(l))) continue;
    out.push(`${path}:${line}: ${m[1]}: '${m[2]}' (derive it from PD_STATUS_GLYPHS / statusGlyph)`);
  }
  return out;
}
