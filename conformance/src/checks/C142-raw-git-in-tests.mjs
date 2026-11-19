// C142 (reserved on origin, refs/paddock/check-ids/C142, 2026-11-19): a test starts git through the scratch-git helper
// (conformance/test/lib/scratch-git.mjs; Rust: `paddock_testkit::scratch_git()`), never with a raw spawn.
// The class (ER-21, ER-45, ER-47; project rules 82 and 84): a test's git inherits the caller's GIT_DIR, GIT_WORK_TREE and
// GIT_INDEX_FILE (a git hook, such as the pre-push gate that runs the tests, sets them), so `git init`, `git config` or `git
// checkout -b` in a scratch folder wrote into the real repository: on 2026-11-19 `core.worktree` and `user.*` landed in the
// shared .git/config and broke every worktree. The helper drops every GIT_* variable, pins the global and system config, passes
// the identity by -c and refuses a folder or an argument outside the temp folder. Earlier instances: the same inheritance in
// union-merge.test.mjs and c85-features-register.test.mjs, which passed `...process.env` through.
// A site is a start of `git` in a test file, however it is spelled: spawnSync, execFileSync, spawn, execFile, async exec, execSync
// (a string or a template that names `git`), execa, zx's $`git ...`, a variable that holds 'git', or a shell (`sh -c`, `bash -c`)
// whose script runs git; unless its first non-option argument is a literal READ-ONLY verb (ls-files, rev-parse, log, show, diff,
// cat-file, check-ignore, check-attr, rev-list, ls-tree, for-each-ref, merge-base, describe). `status` is a write: it refreshes
// the index, so it is a read only with `--no-optional-locks`. A verb the check cannot read (a spread, a variable) is a site. A
// read of the real tree that must stay raw carries `// conformance: raw-git — <why>` on its line or the line above. Text inside a
// string literal (a test of a lint that names `spawnSync("git", ...)`) and a real comment are not sites; a `//` inside a string
// (a URL) does not make the rest of the line a comment.
// Read: `*.test.{mjs,js,cjs,ts}`, `*.spec.{mjs,js,cjs,ts}` and the `.mjs` files directly in conformance/test/ (the helper itself
// and fixtures are not read).
// Known gaps: a git started through a wrapper function of another file, and an argument list assembled in several statements.
// ER-41: report-only (warnings) until its first green run on the real tree, recorded by run id in docs/gates.toml; then SEVERITY
// flips to error.
import { warn } from '../finding.mjs';

export const SEVERITY = warn;
const WAIVER = /conformance:\s*raw-git\s*[—–-]+\s*\S.{2,}/;
const TEST_FILE = /(?:\.(?:test|spec)\.(?:mjs|js|cjs|ts)$)|^conformance\/test\/(?:lib\/)?[^/]+\.mjs$/;
const SKIP = /(?:^|\/)(?:node_modules|dist|target|fixtures?)\/|^conformance\/test\/lib\/scratch-git\.mjs$/;
const READ_ONLY = new Set(['ls-files', 'rev-parse', 'log', 'show', 'diff', 'cat-file', 'check-ignore', 'check-attr', 'rev-list', 'ls-tree', 'for-each-ref', 'merge-base', 'describe', 'status']);
const FIX =
  'Run it through `git`, `gitIn` or `gitResult` of conformance/test/lib/scratch-git.mjs (it drops GIT_*, pins the config and refuses a folder or argument outside the temp folder; Rust: `paddock_testkit::scratch_git()`). A read of the real tree that must stay raw carries `// conformance: raw-git — <why>`.';

const CALLS = 'spawnSync|execFileSync|spawn|execFile';
/** `spawnSync('git'`, `execFileSync("git"`, `spawn(\`git\``, ... : the call and where its arguments start. */
const SPAWN = new RegExp(`\\b(?:${CALLS})\\(\\s*(['"\`])git\\1\\s*,?\\s*`, 'g');
/** A shell string or template that runs git, also after `cd x &&`: `execSync('git init')`, `exec(\`cd d && git init\`)`. */
const SHELL = /(?<![.\w])(?:execSync|exec)\(\s*(['"`])[^\n]*?(?<![\w./-])git\s/g;
/** A shell started with a script that runs git: `spawnSync('bash', ['-c', 'git init'])`. */
const SHELL_SCRIPT = new RegExp(`\\b(?:${CALLS})\\(\\s*(['"\`])(?:ba|z|da|k)?sh\\1\\s*,\\s*\\[[^\\]]*?(?<![\\w./-])git\\s[^\\]]*\\]`, 'g');
/** execa('git', ...), execaSync('git'...), execaCommand('git ...'), zx's $`git ...` and $`... && git ...`. */
const EXECA = /\bexeca(?:Sync|Command|CommandSync)?\(\s*(['"`])git\b/g;
const ZX = /\$`[^`\n]*?(?<![\w./-])git\s/g;

/** Where `index` is on its line: 'code', 'string' (inside a quote) or 'comment' (after a `//` outside any quote). */
function context(code, index) {
  const before = code.slice(code.lastIndexOf('\n', index - 1) + 1, index);
  let quote = null;
  for (let i = 0; i < before.length; i++) {
    const c = before[i];
    if (quote) {
      if (c === '\\') i++;
      else if (c === quote) quote = null;
    } else if (c === "'" || c === '"' || c === '`') quote = c;
    else if (c === '/' && before[i + 1] === '/') return 'comment';
  }
  return quote ? 'string' : 'code';
}

/** The first literal non-option word of the array that starts at `rest` (`['-c', 'x=y', 'init', ...]`), or null; `noLocks` says the array has --no-optional-locks. */
function verbOf(rest) {
  if (rest[0] !== '[') return { verb: null, noLocks: false };
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
  const noLocks = words.includes('--no-optional-locks');
  for (let i = 0; i < words.length; i++) {
    const w = words[i];
    if (w === null) return { verb: null, noLocks };
    if (w === '-c' || w === '-C') {
      i++;
      continue;
    }
    if (w.startsWith('-')) continue;
    return { verb: w, noLocks };
  }
  return { verb: null, noLocks };
}

/** The raw git starts of one file that are sites: [{ line, text }]. */
export function rawGitSites(text, file = 'x.test.mjs') {
  if (!TEST_FILE.test(file) || SKIP.test(file) || !/\bgit\b|GIT\b/i.test(text)) return [];
  const raw = text.split('\n');
  const lineOf = (i) => text.slice(0, i).split('\n').length;
  const out = [];
  const add = (index) => {
    const line = lineOf(index);
    if (WAIVER.test(raw[line - 1] ?? '') || WAIVER.test(raw[line - 2] ?? '') || out.some((s) => s.line === line)) return;
    out.push({ line, text: (raw[line - 1] ?? '').trim() });
  };
  const code = (m) => context(text, m.index) === 'code';
  for (const m of text.matchAll(SPAWN)) {
    if (!code(m)) continue;
    const { verb, noLocks } = verbOf(text.slice(m.index + m[0].length));
    if (verb === null || !READ_ONLY.has(verb) || (verb === 'status' && !noLocks)) add(m.index);
  }
  for (const re of [SHELL, SHELL_SCRIPT, EXECA, ZX]) for (const m of text.matchAll(re)) if (code(m)) add(m.index);
  // A variable that holds 'git' and is then started: `const GIT = 'git'; spawnSync(GIT, [...])`.
  const names = [...text.matchAll(/\b(\w+)\s*=\s*(['"`])git\2/g)].map((m) => m[1]);
  if (names.length) {
    const viaName = new RegExp(`\\b(?:${CALLS}|execa|execaSync)\\(\\s*(?:${names.join('|')})\\b`, 'g');
    for (const m of text.matchAll(viaName)) if (code(m)) add(m.index);
  }
  return out.sort((a, b) => a.line - b.line);
}

export default {
  id: 'C142',
  scope: 'root',
  mode: 'report-only',
  title: 'A test starts git through the scratch-git helper, never raw',
  rule: 84,
  run(ctx) {
    const out = [];
    for (const f of ctx.files()) {
      if (!TEST_FILE.test(f) || SKIP.test(f)) continue;
      const text = ctx.read(f);
      if (!text) continue;
      for (const s of rawGitSites(text, f)) {
        out.push(
          SEVERITY(
            'C142',
            84,
            f,
            s.line,
            `a raw git start in a test (\`${s.text.slice(0, 120)}\`) inherits GIT_DIR and GIT_WORK_TREE from a git hook and can write into the real repository (ER-45, ER-47)`,
            FIX,
          ),
        );
      }
    }
    return out;
  },
};
