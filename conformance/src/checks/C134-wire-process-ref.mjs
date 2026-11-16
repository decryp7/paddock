// C134 (report-only until its first green run, ER-41): a wire type names a process by `ProcessRef`, never by a pid field of its
// own (rule 53: a pid is reused by the OS, so a pid alone names whichever process holds it now; ADR 0033). Dyno's wire crate
// (`apps/dyno/crates/dyno-core/src`) is the one place a process crosses to the UI, the CLI and the helper: a struct or enum that
// serialises there (`#[derive(.. Serialize ..)]`, also inside `cfg_attr`, or a hand-written `impl Serialize for`) and has a field that
// names a process by number is flagged, except `ProcessRef` itself. A field names a process by number when
// - its name is `pid`, `pids`, `ppid`, `parent`, `process_id`, or ends in `pid` or `pids` in any case (`parentPid`, `via_pid`), or
//   its `#[serde(rename = "..")]` is one of those, or
// - its type holds the word `Pid` (`Pid`, `Option<Pid>`, `Vec<Pid>`), or
// - it is a tuple field of type `Pid` (a tuple struct or variant has no names to read).
//
// A field that is not a process reference carries `// conformance: wire-pid — <why>` in a plain `//` comment (not `///` or `//!`) on
// the same line or on a comment-only line right above it (the process's own sample, a list joined to the same sample, the helper's
// table that holds a start time next to the pid, a parent pid that the tree builder checks against the child's start time). A waiver
// with no reason is a finding that says so. The waivers may only shrink (ER-46): one that the merge base does not have is a finding
// (the base is `PADDOCK_BASE_REF`, default `origin/main`; a tree where the base has no waiver at all is the first introduction).
// The scan of a file stops at its `mod tests` and skips `tests.rs` and `tests/`. A tree with no Rust file in the wire crate is
// "check broken" (ER-33), never a pass. Cost: linear in the size of the files (one pass over each line, bounded regular expressions).
import { execFileSync } from 'node:child_process';
import { warn, error, unwaivable } from '../finding.mjs';

/** The severity of a finding: report-only for now (ER-41). */
export const SEVERITY = warn;
const WIRE_DIR = 'apps/dyno/crates/dyno-core/src';
const WIRE_CRATE = /^apps\/dyno\/crates\/dyno-core\/src\/.+\.rs$/;
const SKIPPED = /(?:^|\/)tests?(?:\/|\.rs$)/;
/** A plain `//` comment (never `///` or `//!`) with the waiver and a reason. */
const WAIVER = /(?<!\/)\/\/(?![/!])\s*conformance:\s*wire-pid[ \t]*[—–-]+[ \t]*(\S.{2,})/;
const BARE_WAIVER = /(?<!\/)\/\/(?![/!])\s*conformance:\s*wire-pid/;
const NAME = /^(?:pid|pids|ppid|parent|process_?id|.*pids?)$/i;
const ITEM = /^\s*(?:pub(?:\([^)]*\))?\s+)?(struct|enum)\s+([A-Za-z_][A-Za-z0-9_]*)/;
const MOD_TESTS = /^\s*mod\s+tests?\b/;
const HAND_IMPL = /\bimpl(?:\s*<[^>]*>)?\s+(?:serde::)?Serialize\s+for\s+([A-Za-z_][A-Za-z0-9_]*)/g;
const FIX = 'Use `process: ProcessRef` (dyno_core::process, ProcessRef in api.types.ts), which carries the start time, so a reused pid is another process. A field that is not a process reference carries `// conformance: wire-pid — <why>`.';

/** The line without strings (their content is not code) and without a trailing `//` comment. */
const code = (line) => line.replace(/"(?:[^"\\]|\\.)*"/g, '""').replace(/\/\/.*$/, '');

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

/** The fields of one item body line that name a process by number. `rename` is the pending `#[serde(rename = "..")]`. */
function numberedFields(body, rename) {
  const out = [];
  // Named fields: `[pub] name: Type,` (several can share a line in a variant `{ pid: u32, name: String }`).
  for (const m of body.matchAll(/(?<![\w.:])([A-Za-z_][A-Za-z0-9_]*)\s*:(?!:)\s*([^,{}]*)/g)) {
    const [, name, type] = m;
    if (name === 'pub') continue;
    if (NAME.test(name) || (rename !== null && NAME.test(rename)) || /\bPid\b/.test(type)) out.push(name);
    rename = null;
  }
  // Tuple fields: `(pub Pid, String)` of a tuple struct or variant.
  const tuple = /\(([^()]*)\)/.exec(body);
  if (out.length === 0 && tuple && !body.includes(':')) {
    for (const part of tuple[1].split(',')) if (/^\s*(?:pub(?:\([^)]*\))?\s+)?(?:Option<\s*)?Pid\b/.test(part)) out.push('(tuple field)');
  }
  return out;
}

/** The numbered fields of the serialising items of one Rust file: `{ line, field, item, waived }`; `waived` is a reason, 'no-reason' or null. */
export function pidFields(text) {
  const lines = text.split('\n');
  const hand = new Set([...text.matchAll(HAND_IMPL)].map((m) => m[1]));
  const out = [];
  let attrs = ''; // the attributes right above the next item
  let open = 0; // unclosed `[` of an attribute that spans lines
  let item = null; // the serialising item being read: { name, depth, opened }
  let rename = null;
  for (let i = 0; i < lines.length; i++) {
    if (MOD_TESTS.test(lines[i])) break;
    const c = code(lines[i]);
    const t = c.trim();
    let body = c;
    if (item === null) {
      if (open > 0 || t.startsWith('#[')) {
        attrs += ` ${c}`;
        open += (c.match(/\[/g) ?? []).length - (c.match(/\]/g) ?? []).length;
        continue;
      }
      const m = ITEM.exec(c);
      if (!m) {
        if (t !== '') attrs = '';
        continue;
      }
      const serialises = /derive\([^\]]*\bSerialize\b/.test(attrs) || hand.has(m[2]);
      attrs = '';
      if (!serialises) continue;
      item = { name: m[2], depth: 0, opened: false };
      rename = null;
      // the item's own line holds fields only after its opening bracket (a tuple struct, a one-line item)
      body = c.replace(ITEM, '').replace(/^[^({]*(?=[({])/, '');
    }
    {
      const attrRename = /#\[serde\([^\]]*\brename\s*=\s*"([^"]+)"/.exec(lines[i]);
      if (attrRename) rename = attrRename[1];
      // attributes on the field's own line are not the field (`#[serde(default = "x//y")] pub pid: u32,`)
      const rest = body.replace(/#\[[^\]]*\]/g, '');
      for (const field of numberedFields(rest, rename)) {
        // ProcessRef is the one type that holds a pid, next to its start time
        if (item.name === 'ProcessRef' && field === 'pid') continue;
        out.push({ line: i + 1, field, item: item.name, waived: waiverFor(lines, i) });
      }
      if (/[A-Za-z_]\w*\s*:/.test(rest)) rename = null;
    }
    for (const ch of c) {
      if (ch === '{' || ch === '(') {
        item.depth++;
        item.opened = true;
      } else if (ch === '}' || ch === ')') item.depth--;
    }
    if ((item.opened && item.depth <= 0) || (!item.opened && c.includes(';'))) item = null;
  }
  return out;
}

/** The waivers of one file as keys (`file::item::field`), the same way the check reads them. */
export function waiverKeys(file, text) {
  return pidFields(text)
    .filter((f) => f.waived && f.waived !== 'no-reason')
    .map((f) => `${file}::${f.item}::${f.field}`);
}

const gitIn = (root) => (args) => execFileSync('git', args, { cwd: root, encoding: 'utf8', maxBuffer: 64 << 20, stdio: ['ignore', 'pipe', 'pipe'] });

/**
 * The waiver keys at the merge base with the target branch, or `{ reason }` when they cannot be measured, or `{ first: true }` when
 * the base has no waiver at all (the check is being introduced).
 */
export function baseWaivers(root, git = gitIn(root), baseRef = process.env.PADDOCK_BASE_REF ?? 'origin/main') {
  let base;
  try {
    const head = git(['rev-parse', 'HEAD']).trim();
    base = git(['merge-base', 'HEAD', baseRef]).trim();
    if (base === head) base = git(['rev-parse', 'HEAD^1']).trim();
  } catch (e) {
    return { reason: `no merge base (${String(e.message ?? e).split('\n')[0].slice(0, 120)})` };
  }
  try {
    const keys = [];
    for (const f of git(['ls-tree', '-r', '--name-only', base, '--', WIRE_DIR]).split('\n').filter((n) => WIRE_CRATE.test(n) && !SKIPPED.test(n))) {
      keys.push(...waiverKeys(f, git(['show', `${base}:${f}`])));
    }
    return keys.length === 0 ? { first: true, base } : { keys, base };
  } catch (e) {
    return { reason: `the wire crate at ${base.slice(0, 9)} cannot be read (${String(e.message ?? e).split('\n')[0].slice(0, 80)})` };
  }
}

export default {
  id: 'C134',
  title: 'a wire type names a process by ProcessRef, not by a pid field',
  rule: 53,
  scope: 'root',
  mode: 'report-only',
  run(ctx) {
    if (ctx.app !== 'paddock') return [];
    const files = ctx.files().filter((f) => WIRE_CRATE.test(f) && !SKIPPED.test(f)).sort();
    if (files.length === 0) {
      return [unwaivable(error('C134', 53, WIRE_DIR, 1, 'check broken: no Rust file of the wire crate was found to read', 'Run it with --root at the repository root.'))];
    }
    const out = [];
    const waivers = [];
    for (const file of files) {
      const text = ctx.read(file);
      if (text === null || text === undefined) {
        out.push(unwaivable(error('C134', 53, file, 1, 'not measured: the file cannot be read, so a pid field in it cannot be seen', 'Fix the file permissions or remove the file.'), 'not-measured'));
        continue;
      }
      for (const f of pidFields(text)) {
        if (f.waived && f.waived !== 'no-reason') {
          waivers.push({ key: `${file}::${f.item}::${f.field}`, file, line: f.line });
          continue;
        }
        const bare = f.waived === 'no-reason' ? ' (the waiver comment has no reason)' : '';
        out.push(SEVERITY('C134', 53, file, f.line, `\`${f.item}\` has a wire field \`${f.field}\` that names a process by number${bare}`, FIX));
      }
    }
    const base = ctx.root ? baseWaivers(ctx.root) : { reason: 'no root' };
    if (base.reason) {
      out.push(warn('C134', 53, WIRE_DIR, 1, `the waivers' shrink-only rule is not enforced here: ${base.reason}`, 'Run it in a clone with the merge base available (PADDOCK_BASE_REF).'));
    } else if (!base.first) {
      const had = new Set(base.keys);
      for (const w of waivers.filter((x) => !had.has(x.key))) {
        out.push(SEVERITY('C134', 53, w.file, w.line, `the waiver ${w.key} is not at the merge base ${base.base.slice(0, 9)}: the waivers may only shrink (ER-46)`, "Name the process with `ProcessRef` instead; a new waiver needs the user's approval."));
      }
    }
    return out;
  },
};
