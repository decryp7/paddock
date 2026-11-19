// The one way a test runs git (ER-21 class, ER-45, ER-47). A test that starts git inherits the caller's environment: under a
// git hook (the pre-push gate runs the tests) or a person's `GIT_DIR=... node --test`, GIT_DIR, GIT_WORK_TREE and GIT_INDEX_FILE
// point at the REAL repository, and `git init`, `git config`, `git checkout -b` or `git commit` in a scratch folder then write
// there (2026-11-19: `core.worktree` and `user.*` landed in the shared .git/config and broke every worktree).
// So this helper
//   - drops every GIT_* variable (a caller that needs one passes it in `env`; the repository-redirecting ones are refused),
//   - pins GIT_CONFIG_GLOBAL and GIT_CONFIG_SYSTEM to /dev/null and passes the identity with -c, never through `git config`,
//   - sets GIT_CEILING_DIRECTORIES to the temp folder, so repository discovery never walks above it,
//   - refuses to run unless the folder is inside the machine's temp folder, and (for every command but `init` and `clone`)
//     unless the repository git resolves there has its git dir inside that folder too.
// Read-only calls on the real tree (`git ls-files` over PADDOCK) are not scratch work and carry `// conformance: raw-git — <why>`
// (check C142 reports a raw git spawn in a test that has neither this helper nor that comment).
import { spawnSync } from 'node:child_process';
import { realpathSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { isAbsolute, relative, resolve } from 'node:path';

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
