// The one JavaScript reader of the `[[report_only]]` rows of docs/gates.toml (the gate register, ADR 0060): which checks are
// report-only (ER-41). A check that is report-only warns; when its first green run on the real tree is recorded, the row is removed in
// that step and the check blocks. Nothing else in the JavaScript sources reads the register's text (ER-42).
// The subset it reads: `[[report_only]]` tables of `key = "text"` and `key = YYYY-MM-DD` lines; other tables are skipped.

/** The `[[report_only]]` rows of the register's text, as objects of their keys (every value text), each with its `line`. */
export function reportOnlyRows(text) {
  const rows = [];
  let cur = null;
  text.split('\n').forEach((raw, i) => {
    const line = raw.replace(/^\s+|\s+$/g, '');
    if (!line || line.startsWith('#')) return;
    if (line.startsWith('[')) {
      cur = line === '[[report_only]]' ? { line: i + 1 } : null;
      if (cur) rows.push(cur);
      return;
    }
    if (!cur) return;
    const m = /^(\w+)\s*=\s*(?:"((?:[^"\\]|\\.)*)"|(\d{4}-\d{2}-\d{2}))\s*(?:#.*)?$/.exec(line);
    if (m) cur[m[1]] = (m[2] ?? m[3]).replace(/\\(["\\])/g, '$1');
  });
  return rows;
}

/** Whether the register lists `id` as report-only. */
export function isReportOnly(text, id) {
  return reportOnlyRows(text).some((r) => r.id === id);
}
