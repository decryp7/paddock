// C123: a Rust test starts `./dev`, `cargo` or `node` only through `paddock_testkit::TestCommand` (ADR 0100, ER-14, ER-21). A child
// started with a plain `Command` inherits the machine's whole environment, so every new environment lookup in the program under test
// quietly reaches real machine state: the pre-push tests ran the real heavy steps, and `dev()` found the real engineering-rules
// `heavy.sh` through ENGINEERING_RULES and took machine-wide slots in ~/.cache/heavy-slots (fix/precommit-heavy-slot, 2ff5f5072).
// `TestCommand` starts every child from an empty environment plus a listed one. Judged per Rust test file (a path under `tests/`, a
// `tests.rs` or `*_test.rs` file, or the part of any file after `#[cfg(test)]`):
//   1. a constructor (`Command::new`, `paddock_proc::command`, `quiet_command`, `<crate>::process::command`) whose first argument is the
//      binary cargo built for the test (`env!("CARGO_BIN_EXE_<name>")`, any of them: the tool, the slot tool, an app's program), cargo or
//      rustc (`"cargo"`, `"rustc"`, `env!("CARGO")`, `env!("RUSTC")`, `env::var("CARGO")`, `env::var("RUSTC")`), `"rustup"`, `"node"`,
//      `"npm"`, `"npx"`, `"dev"` / `"./dev"` or a path joined to `"dev"`, each also spelled with `.exe` or `.cmd`; in tools/paddock-dev
//      also `"sh"` / `"bash"` (the wrapper script `./dev` and the CI scripts run under one); a name bound to a built binary counts as
//      that argument: a `const`, `static` or `let` set from `env!("CARGO_BIN_EXE_<name>")` or a function that returns one, in the same
//      file, or `pub` in another test file of the same crate (a shared `common` module); a binary handed to the code under test, which
//      starts it through its own spawn API (`SpawnSpec`, `WorkerConfig`), is not a test's own Command and is not judged;
//   2. any other use of the paddock-dev binary or of cargo's path (`env!("CARGO_BIN_EXE_paddock-dev")`, `env!("CARGO")`,
//      `env::var("CARGO")`: handed to a shell, a helper or a variable that starts it) in a function whose code (comments left out) does
//      not name `TestCommand`;
//   3. the machine's `PATH` (`env::var("PATH")`) read in a function that builds a `TestCommand`: it splices the machine back into the
//      curated `PATH`; use `path_first(dir)` with an explicit folder.
// The test part of a file under `src/` starts at its first `#[cfg(..)]` that names `test` (`#[cfg(all(test, unix))]` too, never
// `#[cfg(not(test))]`). Known false negatives (ADR 0100): a program held in a variable (`let p = "cargo"; Command::new(p)`), an
// alias (`use std::process::Command as Cmd`, a `use` of a renamed builder), a block comment that names `TestCommand`, and spawns in
// a non-test helper module that tests call.
// A justified case carries `conformance: test-env — <why>` (any comment form) on the line or the line above. JavaScript tests are not
// judged here (their spawn sites are listed in ADR 0100 as the open follow-up). ER-41: report-only (warnings) until its first green run
// on main; flip SEVERITY to `error` and `mode` to 'gate' then (docs/gates.toml, C123). The Rust host has the same check
// (tools/paddock-dev/src/check/c123.rs); tools/paddock-dev/tests/it/check_c123.rs holds them equal. Paddock root only; a tree with no
// Rust test file is "check broken" (ER-33), never a pass.
import { warn, error, unwaivable } from '../finding.mjs';

/** The severity of a finding: report-only for now (ER-41). */
export const SEVERITY = warn;
const WAIVER = /conformance:\s*test-env[ \t]*[—–-]+[ \t]*(\S.{2,})/;
const BARE_WAIVER = /conformance:\s*test-env/;
const ARG = String.raw`env!\("CARGO_BIN_EXE_[0-9A-Za-z_-]+"\)|env!\("(?:CARGO|RUSTC)"\)|"(?:cargo|rustc|rustup|node|npm|npx|dev|\./dev)(?:\.exe|\.cmd)?"|(?:std::)?env::var(?:_os)?\("(?:CARGO|RUSTC)"\)|[0-9A-Za-z_:.&()]*\.join\("(?:\./)?dev"\)`;
const ctor = (arg) => new RegExp(String.raw`(?:\bCommand::new|\b(?:\w+::)*(?:quiet_)?command)\(\s*(${arg})`, 'g');
const CTOR = ctor(ARG);
/** In the tests of paddock-dev a shell starts the wrapper script `./dev` and the CI scripts, so a shell is one of the programs judged. */
const CTOR_SHELL = ctor(String.raw`${ARG}|"(?:sh|bash)(?:\.exe)?"`);
const BOUND = /(?:const|static|let(?:\s+mut)?)\s+([A-Za-z_][A-Za-z0-9_]*)\s*(?::[^=;]*)?=\s*[^;]*?env!\("CARGO_BIN_EXE_/g;
const SHELL_SCOPE = /^tools\/paddock-dev\//;
const PATH_VAR = /\b(?:env::)?var(?:_os)?\("PATH"\)/g;
const DEV_BIN = /env!\("CARGO_BIN_EXE_paddock-dev"\)|env!\("CARGO"\)|\b(?:env::)?var(?:_os)?\("CARGO"\)/g;
const FN_LINE = /^(\s*)(?:pub(?:\([^)]*\))?\s+)?(?:async\s+)?(?:unsafe\s+)?fn\s+\w/;
const TEST_PATH = /(?:^|\/)tests?\/|(?:^|\/)tests?\.rs$|_tests?\.rs$/;
/** A `#[cfg(..)]` that names `test` (`#[cfg(test)]`, `#[cfg(all(test, unix))]`), but not `#[cfg(not(test))]`. */
const CFG_TEST = /#\[cfg\([^\]]*\btest\b[^\]]*\)\]/;
const NOT_TEST = /\bnot\(\s*test\b/;
/** Whether a line opens a test part: a `#[cfg(..)]` naming `test`, not `not(test)`. */
export const isCfgTest = (line) => { const m = CFG_TEST.exec(line); return m !== null && !NOT_TEST.test(m[0]); };
/** A function's name and whether its signature returns something (`fn child_program() -> PathBuf`). */
const FN_NAME = /\bfn\s+([A-Za-z_][A-Za-z0-9_]*)[^{;]*->/;
const FIX = 'Start the child with `paddock_testkit::TestCommand::new(..)` (an empty environment plus the listed one, a private HOME and TMPDIR); add what the test needs with `.env("NAME", value)`. A justified case carries `// conformance: test-env — <why>`.';

/** The crate a Rust file belongs to: its path up to the `tests/` or `src/` folder (the whole path when it has neither). */
export const crateOf = (path) => path.replace(/\/(?:tests|src)\/.*$/, '');

/** Whether `path` is a Rust file this check reads. */
export const isRust = (path) => path.endsWith('.rs');

/**
 * The names a file binds to a built binary: a `const`, `static` or `let` set from `env!("CARGO_BIN_EXE_..")`, and a function that
 * returns one (`fn child_program() -> PathBuf { ..env!(..) }`). [{ name, exported }]: `exported` for a `pub` const, static or fn,
 * which the other test files of the same crate use too (a shared `common` module).
 */
export function boundNames(text) {
  const lines = text.split('\n').map((l) => l.replace(/\r$/, ''));
  const out = [...text.matchAll(BOUND)].map((m) => ({ name: m[1], exported: /\bpub(?:\([^)]*\))?\s+$/.test(text.slice(Math.max(0, m.index - 16), m.index)) }));
  lines.forEach((l, i) => {
    const m = FN_LINE.test(l) && FN_NAME.exec(`${l} ${lines[i + 1] ?? ''}`);
    if (m && code(functionText(lines, i)).includes('env!("CARGO_BIN_EXE_')) out.push({ name: m[1], exported: /^\s*pub\b/.test(l) });
  });
  return out;
}

/**
 * The spawn sites of one Rust file: [{ line, what, waived }] (1-based lines). `waived` is undefined, 'no-reason' or the reason.
 * `exported`: the names other files of the tree export bound to a built binary ([`boundNames`]).
 */
export function spawnSites(text, path, exported = []) {
  const lines = text.split('\n').map((l) => l.replace(/\r$/, ''));
  const starts = [];
  let off = 0;
  for (const l of text.split('\n')) { starts.push(off); off += l.length + 1; }
  const lineOf = (idx) => { let lo = 0; let hi = starts.length - 1; while (lo < hi) { const mid = (lo + hi + 1) >> 1; if (starts[mid] <= idx) lo = mid; else hi = mid - 1; } return lo; };
  const cfg = lines.findIndex(isCfgTest);
  const wholeFile = TEST_PATH.test(path);
  const inTest = (i) => wholeFile || (cfg >= 0 && i >= cfg);
  const out = [];
  const waiver = (i) => {
    const near = `${i > 0 ? lines[i - 1] : ''}\n${lines[i]}`;
    const w = WAIVER.exec(near);
    return w ? w[1] : BARE_WAIVER.test(near) ? 'no-reason' : undefined;
  };
  const comment = (i) => lines[i].trimStart().startsWith('//');
  const seen = new Set();
  const bound = [...new Set([...boundNames(text).map((b) => b.name), ...exported])];
  const base = SHELL_SCOPE.test(path) ? String.raw`${ARG}|"(?:sh|bash)(?:\.exe)?"` : ARG;
  const ctorRe = bound.length ? ctor(`${base}|&?(?:${bound.join('|')})\\b`) : SHELL_SCOPE.test(path) ? CTOR_SHELL : CTOR;
  for (const m of text.matchAll(ctorRe)) {
    const i = lineOf(m.index);
    if (comment(i) || !inTest(i)) continue;
    seen.add(i);
    seen.add(lineOf(m.index + m[0].length - m[1].length)); // the argument may sit on the next line
    out.push({ line: i + 1, what: m[1], waived: waiver(i) });
  }
  for (const m of text.matchAll(DEV_BIN)) {
    const i = lineOf(m.index);
    if (comment(i) || !inTest(i) || seen.has(i)) continue;
    if (code(functionText(lines, i)).includes('TestCommand')) continue;
    out.push({ line: i + 1, what: m[0], waived: waiver(i) });
  }
  for (const m of text.matchAll(PATH_VAR)) {
    const i = lineOf(m.index);
    if (comment(i) || !inTest(i) || seen.has(i)) continue;
    if (!code(functionText(lines, i)).includes('TestCommand')) continue;
    out.push({ line: i + 1, what: m[0], waived: waiver(i) });
  }
  return out.sort((a, b) => a.line - b.line);
}

/** `text` without its line comments (`//` outside a string to the end of the line), so a word in a comment does not count. */
export function code(text) {
  return text
    .split('\n')
    .map((l) => {
      let inStr = false;
      for (let k = 0; k < l.length; k++) {
        if (l[k] === '\\' && inStr) k++;
        else if (l[k] === '"') inStr = !inStr;
        else if (!inStr && l[k] === '/' && l[k + 1] === '/') return l.slice(0, k);
      }
      return l;
    })
    .join('\n');
}

/** The text of the function around line `i` (from its `fn` line to its closing brace), or the line itself outside a function. */
function functionText(lines, i) {
  for (let s = i; s >= 0; s--) {
    const m = FN_LINE.exec(lines[s]);
    if (!m) continue;
    const close = `${m[1]}}`;
    let e = s + 1;
    while (e < lines.length && lines[e] !== close) e++;
    return e >= i ? lines.slice(s, e + 1).join('\n') : lines[i];
  }
  return lines[i];
}

export default {
  id: 'C123',
  title: 'a Rust test starts ./dev, cargo or node only through paddock_testkit::TestCommand',
  rule: 3,
  scope: 'root',
  mode: 'report-only',
  run(ctx) {
    if (ctx.app !== 'paddock') return [];
    const files = ctx.files().filter(isRust).sort();
    const testFiles = files.filter((f) => TEST_PATH.test(f) || (ctx.read(f) ?? '').split('\n').some(isCfgTest));
    if (testFiles.length === 0) {
      return [unwaivable(error('C123', 3, 'crates', 1, 'check broken: no Rust test file was found to read', 'Run it with --root at the repository root.'))];
    }
    // A `pub` name bound to a built binary counts in every test file of its crate (the folder above `tests/` or `src/`).
    const exported = new Map();
    for (const f of testFiles) {
      const names = boundNames(ctx.read(f) ?? '').filter((b) => b.exported).map((b) => b.name);
      exported.set(crateOf(f), [...(exported.get(crateOf(f)) ?? []), ...names]);
    }
    const out = [];
    for (const file of testFiles) {
      const text = ctx.read(file);
      if (text === null || text === undefined) {
        out.push(unwaivable(error('C123', 3, file, 1, 'check broken: the file cannot be read', 'Fix the file permissions or remove the file.')));
        continue;
      }
      for (const s of spawnSites(text, file, exported.get(crateOf(file)) ?? [])) {
        if (s.waived && s.waived !== 'no-reason') continue;
        const bare = s.waived === 'no-reason' ? ' (the waiver comment has no reason)' : '';
        out.push(SEVERITY('C123', 3, file, s.line, `a test starts \`${s.what}\` without paddock_testkit::TestCommand${bare} (ER-14: the child inherits the machine's environment)`, FIX));
      }
    }
    return out;
  },
};
