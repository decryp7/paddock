// C142 (reserved on origin, refs/paddock/check-ids/C142, 2026-11-19): a test starts git through the scratch-git helper
// (conformance/test/lib/scratch-git.mjs), never with a raw `spawnSync('git', ...)` / `execFileSync('git', ...)`.
// The class (ER-21, ER-45, ER-47): a test's git inherits the caller's GIT_DIR, GIT_WORK_TREE and GIT_INDEX_FILE (a git hook, such
// as the pre-push gate that runs the tests, sets them), so `git init`, `git config` or `git checkout -b` in a scratch folder wrote
// into the real repository: on 2026-11-19 `core.worktree` and `user.*` landed in the shared .git/config and broke every worktree.
// The helper drops every GIT_* variable, pins the global and system config, passes the identity by -c and refuses a folder outside
// the temp folder. Earlier instances: the same inheritance in union-merge.test.mjs and c85-features-register.test.mjs, which
// passed `...process.env` through.
// A site is a spawn of `git` (spawnSync, execFileSync, spawn, execFile; execSync with a `git ...` string) in a test file whose first
// non-option argument is not a literal READ-ONLY verb (ls-files, rev-parse, log, show, diff, status, cat-file, check-ignore,
// check-attr, rev-list, ls-tree, for-each-ref, merge-base, describe): a write verb, a verb the check cannot read (a spread or a
// variable) and `git -C` through variables are all sites. A read of the real tree that must stay raw carries
// `// conformance: raw-git — <why>` on its line or the line above. Text inside a string literal (a test of a lint that names
// `spawnSync("git", ...)`) is not a site. Read: `*.test.{mjs,js,cjs,ts}` and the `.mjs` files directly in conformance/test/ (the
// helper itself and fixtures are not read).
// ER-41: report-only (warnings) until its first green run on the real tree, recorded by run id in docs/gates.toml; then SEVERITY
// flips to error.
import { warn } from '../finding.mjs';

export const SEVERITY = warn;
const WAIVER = /conformance:\s*raw-git\s*[—–-]+\s*\S.{2,}/;
const TEST_FILE = /(?:\.test\.(?:mjs|js|cjs|ts)$)|^conformance\/test\/(?:lib\/)?[^/]+\.mjs$/;
const SKIP = /(?:^|\/)(?:node_modules|dist|target|fixtures?)\/|^conformance\/test\/lib\/scratch-git\.mjs$/;
const READ_ONLY = new Set(['ls-files', 'rev-parse', 'log', 'show', 'diff', 'status', 'cat-file', 'check-ignore', 'check-attr', 'rev-list', 'ls-tree', 'for-each-ref', 'merge-base', 'describe']);
const FIX =
  "Run it through `git`, `gitIn` or `gitResult` of conformance/test/lib/scratch-git.mjs (it drops GIT_*, pins the config and refuses a folder outside the temp folder). A read of the real tree that must stay raw carries `// conformance: raw-git — <why>`.";
/** `spawnSync('git'`, `execFileSync("git"`, `spawn(\`git\``, ... : the call and where its arguments start. */
const SPAWN = /\b(?:spawnSync|execFileSync|spawn|execFile)\(\s*(['"`])git\1\s*,?\s*/g;
/** `execSync('git ...')`, `execSync(\`git ...\`)`: a shell string, never read-only-checked. */
const SHELL = /\bexecSync\(\s*(['"`])git\s/g;

/** Whether the text before `index` on its line is inside a string literal (an odd number of one quote kind) or a `//` comment. */
function inString(code, index) {
  const before = code.slice(code.lastIndexOf('\n', index - 1) + 1, index);
  if (before.includes('//')) return true; // a comment: not code
  return ["'", '"', '`'].some((q) => (before.split(q).length - 1) % 2 === 1);
}

/** The first literal non-option word of the array that starts at `rest` (`['-c', 'x=y', 'init', ...]`), or null when not readable. */
function verbOf(rest) {
  if (rest[0] !== '[') return null;
  const m = /^\[([^\]]*)\]/.exec(rest);
  const inner = (m ? m[1] : rest.slice(1, 200)).trim();
  const words = [];
  for (const part of inner.split(',')) {
    const w = part.trim();
    const lit = /^(['"`])([^'"`$]*)\1$/.exec(w);
    if (!lit) {
      words.push(null);
      break;
    }
    words.push(lit[2]);
  }
  for (let i = 0; i < words.length; i++) {
    const w = words[i];
    if (w === null) return null;
    if (w === '-c' || w === '-C') {
      i++;
      continue;
    }
    if (w.startsWith('-')) continue;
    return w;
  }
  return null;
}

/** The raw git spawns of one file that are sites: [{ line, text }]. */
export function rawGitSites(text, file = 'x.test.mjs') {
  if (!TEST_FILE.test(file) || SKIP.test(file) || !/\bgit\b/.test(text)) return [];
  const raw = text.split('\n');
  const lineOf = (i) => text.slice(0, i).split('\n').length;
  const out = [];
  const add = (index) => {
    const line = lineOf(index);
    if (WAIVER.test(raw[line - 1] ?? '') || WAIVER.test(raw[line - 2] ?? '') || out.some((s) => s.line === line)) return;
    out.push({ line, text: (raw[line - 1] ?? '').trim() });
  };
  for (const m of text.matchAll(SPAWN)) {
    if (inString(text, m.index)) continue;
    const verb = verbOf(text.slice(m.index + m[0].length));
    if (verb === null || !READ_ONLY.has(verb)) add(m.index);
  }
  for (const m of text.matchAll(SHELL)) if (!inString(text, m.index)) add(m.index);
  return out.sort((a, b) => a.line - b.line);
}

export default {
  id: 'C142',
  scope: 'root',
  mode: 'report-only',
  title: 'A test starts git through the scratch-git helper, never raw',
  rule: 45,
  run(ctx) {
    const out = [];
    for (const f of ctx.files()) {
      if (!TEST_FILE.test(f) || SKIP.test(f)) continue;
      const text = ctx.read(f);
      if (!text) continue;
      for (const s of rawGitSites(text, f)) {
        out.push(SEVERITY('C142', 45, f, s.line, `a raw git spawn in a test (\`${s.text.slice(0, 120)}\`) inherits GIT_DIR and GIT_WORK_TREE from a git hook and can write into the real repository`, FIX));
      }
    }
    return out;
  },
};
