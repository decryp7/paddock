// The one way a test runs git (ER-21 class, ER-45, ER-47). A test that starts git inherits the caller's environment: under a
// git hook (the pre-push gate runs the tests) or a person's `GIT_DIR=... node --test`, GIT_DIR, GIT_WORK_TREE and GIT_INDEX_FILE
// point at the REAL repository, and `git init`, `git config`, `git checkout -b` or `git commit` in a scratch folder then write
// there (2026-11-19: `core.worktree` and `user.*` landed in the shared .git/config and broke every worktree).
// So this helper
//   - drops every GIT_* variable (a caller that needs one passes it in `env`; the repository-redirecting ones are refused),
//   - pins GIT_CONFIG_GLOBAL and GIT_CONFIG_SYSTEM to /dev/null and passes the identity with -c, never through `git config`,
//   - sets GIT_CEILING_DIRECTORIES to the temp folder, so repository discovery never walks above it,
//   - refuses to run unless the folder is inside the machine's temp folder, and (for every command but `init` and `clone`)
//     unless the repository git resolves there has its git dir inside that folder too,
//   - refuses git's own arguments that leave the folder: the global options -C, --git-dir, --work-tree and --namespace (both
//     spellings), `config --file/-f/--global/--system`, a configuration key that moves the work tree or the hooks, the options
//     --separate-git-dir, --template, --reference and --upload-pack, and any path argument (absolute, `~` or with a `..`
//     segment, also after `--opt=` or a glued short option, or a relative one whose first segment exists, so a symlink inside the
//     folder is followed) that resolves outside the temp folder: so `clone` into, `init` in, `worktree add` at, `bundle create`,
//     `archive --output`, `push`, `fetch` or `remote add` of a path outside it is refused before git starts.
// Read-only calls on the real tree (`git ls-files` over PADDOCK) are not scratch work and carry `// conformance: raw-git — <why>`
// (check C142 reports a raw git spawn in a test that has neither this helper nor that comment).
import { spawnSync } from 'node:child_process';
import { existsSync, realpathSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { dirname, isAbsolute, relative, resolve } from 'node:path';

/** Variables that point git at another repository: never passed through, never accepted in `env`. */
const REDIRECTING = /^GIT_(DIR|WORK_TREE|INDEX_FILE|OBJECT_DIRECTORY|COMMON_DIR|NAMESPACE|ALTERNATE_OBJECT_DIRECTORIES|CEILING_DIRECTORIES|CONFIG_COUNT|CONFIG_PARAMETERS)$|^GIT_CONFIG_(KEY|VALUE)_/;

/** The temp folder, resolved (macOS: /var is a link to /private/var). */
export const SCRATCH_ROOT = realpathSync(tmpdir());

/** Whether `dir` (resolved) lies inside the scratch root. */
export function insideScratch(dir) {
  let real;
  try {
    real = realpathSync(resolve(dir));
  } catch {
    return false;
  }
  const rel = relative(SCRATCH_ROOT, real);
  return rel !== '' && !rel.startsWith('..') && !isAbsolute(rel);
}

/**
 * The environment of a scratch git: the caller's, less every GIT_* variable, plus the pinned configuration and `extra`
 * (`{ HOME: ... }`; `PATH` may be replaced). Throws when `extra` names a repository-redirecting variable.
 */
export function scratchEnv(extra = {}) {
  const env = Object.fromEntries(Object.entries(process.env).filter(([k]) => !k.startsWith('GIT_')));
  for (const k of Object.keys(extra)) if (REDIRECTING.test(k)) throw new Error(`scratch-git: ${k} would point git at another repository; refused`);
  return { ...env, ...extra, GIT_CONFIG_GLOBAL: '/dev/null', GIT_CONFIG_SYSTEM: '/dev/null', GIT_CONFIG_NOSYSTEM: '1', GIT_CEILING_DIRECTORIES: SCRATCH_ROOT, GIT_TERMINAL_PROMPT: '0' };
}

/** The identity and defaults every scratch git gets, as -c pairs. */
const CONFIG = ['user.name=t', 'user.email=t@example.invalid', 'commit.gpgsign=false', 'init.defaultBranch=main', 'protocol.file.allow=always'].flatMap((c) => ['-c', c]);

const checked = new Set();

/** Global options (before the command) that point git at another repository, folder or namespace. */
const GLOBAL_OPTION = /^(?:-C|--git-dir|--work-tree|--namespace|--exec-path|--super-prefix|--config-env)(?:=|$)/;
/** `git config` options that choose another file than the repository's own. */
const CONFIG_FILE_OPTION = /^(?:--global|--system|--file|-f|--blob|--worktree)(?:=|$)/;
/** Options of init and clone that put a git dir, template or object store somewhere else. */
const PLACEMENT_OPTION = /^(?:--separate-git-dir|--template|--reference|--reference-if-able|--upload-pack|-u)(?:=|$)/;
/** Configuration keys that move the work tree or run code from outside (in `-c k=v` and in `config k v`). */
const DANGEROUS_KEY = /\b(?:core\.worktree|core\.hookspath|include\.path|includeif\.[^=\s]*\.path)\b/i;

/**
 * The path `arg` names, or null when it is no path: an absolute path, `~`, a `file://` URL, a `..` segment, or a relative path whose
 * first segment EXISTS under `cwd` (so `lnk/x` through a symlink `lnk` is read, `HEAD` and `main` are not). `--opt=path` and a glued
 * short option (`-o/abs`, `-ofile`) are read too.
 */
function pathOf(cwd, arg) {
  let a = arg.replace(/^--[a-z-]+=/, '');
  const glued = /^-[A-Za-z]([^=\s].*)$/.exec(a);
  if (glued && !a.startsWith('--')) a = glued[1];
  if (a.startsWith('file://')) a = a.slice('file://'.length);
  if (a === '' || a.startsWith('-') || a === '/dev/null' || a === 'NUL') return null;
  if (a.startsWith('~') || isAbsolute(a) || /(?:^|[\\/])\.\.(?:[\\/]|$)/.test(a)) return a;
  const first = a.split(/[\\/]/)[0];
  return first !== '' && existsSync(resolve(cwd, first)) ? a : null;
}

/** Whether `path` (made absolute against `cwd`) is inside the scratch root, through its nearest existing ancestor. */
function pathInside(cwd, path) {
  if (path.startsWith('~')) return false;
  let p = resolve(cwd, path);
  const rest = [];
  for (;;) {
    try {
      const real = realpathSync(p);
      const rel = relative(SCRATCH_ROOT, resolve(real, ...rest.reverse()));
      return rel !== '' && !rel.startsWith('..') && !isAbsolute(rel);
    } catch {
      rest.push(p.slice(dirname(p).length + 1));
      const up = dirname(p);
      if (up === p) return false;
      p = up;
    }
  }
}

/** Throws when `args` (git's arguments, before the identity pairs are added) would let git act outside the scratch folder. */
export function refuseEscapingArgs(cwd, args) {
  const refuse = (why, arg) => {
    throw new Error(`scratch-git: ${why} (\`git ${args.join(' ')}\`) refused: ${arg} could leave the scratch folder; run it in the scratch folder with a path inside it`);
  };
  let verbAt = -1;
  for (let i = 0; i < args.length; i++) {
    const a = args[i];
    if (a === '-c') {
      i++;
      continue;
    }
    if (a === '-C' || GLOBAL_OPTION.test(a)) refuse('a global option', a);
    if (!a.startsWith('-')) {
      verbAt = i;
      break;
    }
  }
  const verb = verbAt >= 0 ? args[verbAt] : '';
  for (let i = 0; i < args.length; i++) {
    const a = args[i];
    if (DANGEROUS_KEY.test(a)) refuse('a configuration key that moves the work tree or the hooks', a);
    if (i > verbAt && verbAt >= 0) {
      if (verb === 'config' && CONFIG_FILE_OPTION.test(a)) refuse('`git config` naming another file', a);
      if ((verb === 'init' || verb === 'clone') && PLACEMENT_OPTION.test(a)) refuse(`\`git ${verb}\` placing its git dir elsewhere`, a);
    }
    if (args[i - 1] === '-c') continue; // a `-c key=value` pair: the key was checked above, the value is not a path
    const path = pathOf(cwd, a);
    if (path !== null && !pathInside(cwd, path)) refuse('a path outside the temp folder', a);
  }
}

function refuseOutside(cwd, env) {
  if (!insideScratch(cwd)) throw new Error(`scratch-git: ${cwd} is not inside the temp folder ${SCRATCH_ROOT}; a scratch git never runs elsewhere`);
  const real = realpathSync(resolve(cwd));
  if (checked.has(real)) return;
  const r = spawnSync('git', ['rev-parse', '--absolute-git-dir'], { cwd: real, env, encoding: 'utf8' });
  if (r.status !== 0 || !insideScratch(r.stdout.trim())) {
    throw new Error(`scratch-git: the repository git finds from ${cwd} is ${JSON.stringify(r.stdout.trim() || r.stderr.trim())}, not one inside ${SCRATCH_ROOT}; run \`git init\` there first`);
  }
  checked.add(real);
}

/**
 * Runs `git <args>` in the scratch folder `cwd` and returns { status, stdout, stderr } (strings; never throws on a git failure).
 * `opts`: `input` (stdin), `env` (extra variables, see scratchEnv).
 */
export function gitResult(cwd, args, opts = {}) {
  const env = scratchEnv(opts.env);
  refuseEscapingArgs(cwd, args);
  const verb = args.find((a, i) => !a.startsWith('-') && args[i - 1] !== '-c' && args[i - 1] !== '-C');
  if (verb === 'init' || verb === 'clone') {
    if (!insideScratch(cwd)) throw new Error(`scratch-git: ${cwd} is not inside the temp folder ${SCRATCH_ROOT}; a scratch git never runs elsewhere`);
  } else refuseOutside(opts.repo ?? cwd, env);
  const r = spawnSync('git', [...CONFIG, ...args], { cwd, env, input: opts.input, encoding: 'utf8', maxBuffer: 64 << 20 });
  if (verb === 'init') checked.delete(realpathSync(resolve(cwd)));
  return { status: r.status, stdout: r.stdout ?? '', stderr: r.stderr ?? '' };
}

/** Like `gitResult` but returns stdout and throws (naming the command and its stderr) when git fails: execFileSync's contract. */
export function git(cwd, ...args) {
  const r = gitResult(cwd, args);
  if (r.status !== 0) throw new Error(`git ${args.join(' ')} (in ${cwd}) exited ${r.status}: ${r.stderr.trim()}`);
  return r.stdout;
}

/** `git` with options (`{ env, input }`, see gitResult): returns stdout, throws when git fails. */
export function gitIn(cwd, opts, ...args) {
  const r = gitResult(cwd, args, opts);
  if (r.status !== 0) throw new Error(`git ${args.join(' ')} (in ${cwd}) exited ${r.status}: ${r.stderr.trim()}`);
  return r.stdout;
}
