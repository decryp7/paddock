// C134 (report-only until its first green run, ER-41): a wire type names a process by `ProcessRef`, never by a pid field of its
// own (rule 53: a pid is reused by the OS, so a pid alone names whichever process holds it now; ADR 0033). Dyno's wire crate
// (`apps/dyno/crates/dyno-core/src`) is the one place a process crosses to the UI, the CLI and the helper: a struct or enum
// that derives `Serialize` there and has a field named `pid`, `pids` or `*_pid` is flagged, except `ProcessRef` itself.
//
// A field that is not a process reference carries `conformance: wire-pid — <why>` in a comment on the same line or on a
// comment-only line right above it (the process's own sample, a list joined to the same sample, the helper's table that holds a
// start time next to the pid, a socket row the agent resolves to an owner). A waiver with no reason is a finding that says so.
// Code after the first `#[cfg(test)]` of a file is not read. A tree with no Rust file in the wire crate is "check broken" (ER-33),
// never a pass. Cost: linear in the size of the files (one pass over each line, one bounded regular expression per line).
import { warn, error, unwaivable } from '../finding.mjs';

/** The severity of a finding: report-only for now (ER-41). */
export const SEVERITY = warn;
const WIRE_CRATE = /^apps\/dyno\/crates\/dyno-core\/src\/.+\.rs$/;
const WAIVER = /conformance:\s*wire-pid[ \t]*[—–-]+[ \t]*(\S.{2,})/;
const BARE_WAIVER = /conformance:\s*wire-pid/;
const PID_FIELD = /(?<![\w.])(pid|pids|[a-z0-9_]+_pid)\s{0,4}:(?!:)/g;
const ITEM = /^\s*(?:pub(?:\([^)]*\))?\s+)?(struct|enum)\s+([A-Za-z_][A-Za-z0-9_]*)/;
const CFG_TEST = /#\[cfg\([^\]]*\btest\b[^\]]*\)\]/;
const FIX = 'Use `process: ProcessRef` (dyno_core::process, ProcessRef in api.types.ts), which carries the start time, so a reused pid is another process. A field that is not a process reference carries `// conformance: wire-pid — <why>`.';

const stripComment = (line) => {
  const i = line.indexOf('//');
  return i < 0 ? line : line.slice(0, i);
};

/** The waiver on a line or on the comment-only lines right above it: a reason, 'no-reason', or null. */
function waiverFor(lines, i) {
  const seen = [lines[i]];
  for (let j = i - 1; j >= 0 && lines[j].trim().startsWith('//'); j--) seen.push(lines[j]);
  for (const l of seen) {
    const m = WAIVER.exec(l);
    if (m) return m[1];
  }
  return seen.some((l) => BARE_WAIVER.test(l)) ? 'no-reason' : null;
}

/** The pid fields of the serializing items of one Rust file: `{ line, field, item, waived }`, `waived` a reason, 'no-reason' or null. */
export function pidFields(text) {
  const lines = text.split('\n');
  const out = [];
  let attrs = ''; // the attributes right above the next item
  let open = 0; // unclosed `[` of an attribute that spans lines
  let item = null; // the serializing item being read: { name, depth, opened }
  for (let i = 0; i < lines.length; i++) {
    if (CFG_TEST.test(lines[i])) break;
    const code = stripComment(lines[i]);
    const t = code.trim();
    if (item === null) {
      if (open > 0 || t.startsWith('#[')) {
        attrs += ` ${code}`;
        open += (code.match(/\[/g) ?? []).length - (code.match(/\]/g) ?? []).length;
        continue;
      }
      const m = ITEM.exec(code);
      if (!m) {
        if (t !== '') attrs = '';
        continue;
      }
      const serializes = /#\[derive\([^\]]*\bSerialize\b/.test(attrs);
      attrs = '';
      if (!serializes) continue;
      item = { name: m[2], depth: 0, opened: false };
    }
    for (const ch of code) {
      if (ch === '{') {
        item.depth++;
        item.opened = true;
      } else if (ch === '}') item.depth--;
    }
    if (item.name !== 'ProcessRef' && item.opened) {
      for (const f of code.matchAll(PID_FIELD)) out.push({ line: i + 1, field: f[1], item: item.name, waived: waiverFor(lines, i) });
    }
    if ((item.opened && item.depth <= 0) || (!item.opened && code.includes(';'))) item = null;
  }
  return out;
}

export default {
  id: 'C134',
  title: 'a wire type names a process by ProcessRef, not by a pid field',
  rule: 53,
  scope: 'root',
  mode: 'report-only',
  run(ctx) {
    if (ctx.app !== 'paddock') return [];
    const files = ctx.files().filter((f) => WIRE_CRATE.test(f)).sort();
    if (files.length === 0) {
      return [unwaivable(error('C134', 53, 'apps/dyno/crates/dyno-core/src', 1, 'check broken: no Rust file of the wire crate was found to read', 'Run it with --root at the repository root.'))];
    }
    const out = [];
    for (const file of files) {
      const text = ctx.read(file);
      if (text === null || text === undefined) {
        out.push(unwaivable(error('C134', 53, file, 1, 'not measured: the file cannot be read, so a pid field in it cannot be seen', 'Fix the file permissions or remove the file.'), 'not-measured'));
        continue;
      }
      for (const f of pidFields(text)) {
        if (f.waived && f.waived !== 'no-reason') continue;
        const bare = f.waived === 'no-reason' ? ' (the waiver comment has no reason)' : '';
        out.push(SEVERITY('C134', 53, file, f.line, `\`${f.item}\` has a wire field \`${f.field}\` that names a process by pid alone${bare}`, FIX));
      }
    }
    return out;
  },
};
