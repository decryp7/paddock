// C169: a file is written whole through `paddock_env::write_atomic`, never by a hand-written "write a temporary file, then rename"
// (user decision 2026-11-25; the ER-21 class: two writers that shared one temporary name lost data twice, Spotter's avatar
// `write_atomic` with `path.with_extension("tmp")` and the pre-push reuse store). `paddock_env::write_atomic` (and
// `write_atomic_created` where the caller creates the file, for a mode or a security descriptor) names its temporary file with a
// per-process counter, flushes it, renames it and removes it on failure, so concurrent writers never share one.
//
// Flagged, in Rust production code (test items are not read; comments and string insides are masked) outside `crates/paddock-env/`:
// a `rename(` call (`std::fs::rename`, `fs::rename`, `tokio::fs::rename`) whose first argument is a name that says it is a temporary
// file: `tmp`, `temp` or a name made of them (`tmp_path`, `self.tmp`, `temp_file`). Streaming writers (`.part` downloads, a zip or a
// segment written in pieces), directory moves and rotations name their source `part`, `staging`, `aside` and so on and are not
// flagged; a site that is fine anyway (a rename used as a lock claim) carries `// conformance: write-atomic — <why>` on its line or
// the line above.
// ER-41: report-only (warnings) until its first green run on the real tree, recorded in docs/gates.toml.
import { warn } from '../finding.mjs';
import { withoutTestItems } from '../util/rust-test-items.mjs';
import { maskRust } from '../util/mask-rust.mjs';

const WAIVER = /conformance:\s*write-atomic\s*[—–-]+\s*\S.{2,}/;
const SKIP_PATH = /(?:^|\/)(?:target|vendor)\/|(?:^|\/)fixtures?\/|^crates\/paddock-env\/|(?:^|\/)(?:tests|benches|examples)\/.+\.rs$|(?:^|\/)tests\.rs$|_tests?\.rs$|^apps\/debrief\/docs\/research\//;
// `rename(` after an optional path (`std::fs::`, `fs::`, `tokio::fs::`), then the first argument: `&`, `self.`, `mut`-less names.
const SITE = /(?<![\w.])(?:(?:std|tokio)::)?(?:fs::)?rename\s*\(\s*&?\s*(?:self\.)?([A-Za-z_]\w*)\b/g;
const TEMPORARY = /^(?:tmp|temp)(?:_\w+|\d*)$|^\w+_(?:tmp|temp)$/;
const FIX = 'Write the bytes with `paddock_env::write_atomic(path, bytes, private)` (a temporary name of its own per write, flushed, renamed, removed on failure), or `paddock_env::write_atomic_created` when the file needs a mode or a security descriptor; `From<EnvError> for std::io::Error` keeps an io::Result. A site that is fine carries `// conformance: write-atomic — <why>`.';

/** The sites of one file: [{ line }]. Empty for a file that is not Rust production code. */
export function writeAtomicSites(text, file) {
  if (!file.endsWith('.rs') || SKIP_PATH.test(file)) return [];
  const masked = withoutTestItems(maskRust(text));
  const lines = text.split('\n');
  const out = [];
  for (const m of masked.matchAll(SITE)) {
    if (!TEMPORARY.test(m[1])) continue;
    const line = masked.slice(0, m.index).split('\n').length;
    if (WAIVER.test(lines[line - 1] ?? '') || WAIVER.test(lines[line - 2] ?? '')) continue;
    if (!out.some((o) => o.line === line)) out.push({ line });
  }
  return out;
}

export default {
  id: 'C169',
  scope: 'root',
  mode: 'report-only',
  title: 'A file is written whole through paddock_env::write_atomic, never a hand-written temporary file and rename',
  rule: 55,
  run(ctx) {
    const out = [];
    for (const f of ctx.files()) {
      if (!f.endsWith('.rs')) continue;
      const text = ctx.read(f);
      if (!text || !text.includes('rename')) continue;
      for (const s of writeAtomicSites(text, f)) {
        out.push(warn('C169', 55, f, s.line, 'renames a temporary file over its target by hand: writers that share the temporary name lose data (Spotter avatar, the pre-push reuse store)', FIX));
      }
    }
    return out;
  },
};
