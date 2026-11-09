// Records tests/golden/expected/*.json: what scripts/affected.mjs answers for tests/golden/cases.json on the fixture repository.
// Run once from the repository root (`node tools/paddock-dev/tests/golden/generate.mjs`) and commit the result; the Rust golden
// test (tests/golden.rs) compares against the recorded files, so it keeps working after affected.mjs is gone (ADR 0022 R4).
// The fixture repository is built the same way in both places: tests/golden/base-overlay over tests/golden/fixture is the
// commit tagged `fixture-base`, tests/golden/fixture alone (the overlay-only files removed) is the commit tagged `fixture-head`.
import { execFileSync, spawnSync } from 'node:child_process';
import { cpSync, existsSync, mkdirSync, mkdtempSync, readdirSync, readFileSync, realpathSync, renameSync, rmSync, statSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { dirname, join, relative } from 'node:path';
import { fileURLToPath } from 'node:url';
import { planFor } from '../../../../scripts/affected.mjs';

const here = dirname(fileURLToPath(import.meta.url));
const git = (cwd, ...args) =>
  execFileSync('git', ['-c', 'user.name=t', '-c', 'user.email=t@example.invalid', '-c', 'commit.gpgsign=false', '-c', 'core.autocrlf=false', ...args], { cwd, encoding: 'utf8', stdio: ['ignore', 'pipe', 'pipe'] });
const walk = (dir) => readdirSync(dir).flatMap((n) => (statSync(join(dir, n)).isDirectory() ? walk(join(dir, n)) : [join(dir, n)]));

// A lockfile is stored in the fixture as package-lock.fixture.json so the repository's own one-lockfile check ignores it.
const place = (from, root) => {
  cpSync(from, root, { recursive: true });
  if (existsSync(join(root, 'package-lock.fixture.json'))) renameSync(join(root, 'package-lock.fixture.json'), join(root, 'package-lock.json'));
};

const root = realpathSync(mkdtempSync(join(tmpdir(), 'paddock-golden-')));
try {
  git(root, 'init', '-q', '-b', 'main');
  place(join(here, 'fixture'), root);
  place(join(here, 'base-overlay'), root);
  git(root, 'add', '-A');
  git(root, 'commit', '-q', '-m', 'base');
  git(root, 'tag', 'fixture-base');
  place(join(here, 'fixture'), root);
  for (const f of walk(join(here, 'base-overlay'))) {
    const rel = relative(join(here, 'base-overlay'), f);
    try {
      statSync(join(here, 'fixture', rel));
    } catch {
      rmSync(join(root, rel));
    }
  }
  git(root, 'add', '-A');
  git(root, 'commit', '-q', '-m', 'head');
  git(root, 'tag', 'fixture-head');

  mkdirSync(join(here, 'expected'), { recursive: true });
  for (const c of JSON.parse(readFileSync(join(here, 'cases.json'), 'utf8'))) {
    let out;
    try {
      out = planFor({ files: c.files ?? null, base: c.base ?? null, head: c.head ?? 'HEAD', all: !!c.all, explain: true, root }).plan;
    } catch (e) {
      out = { error: true };
    }
    writeFileSync(join(here, 'expected', `${c.name}.json`), `${JSON.stringify(out, null, 1)}\n`);
  }

  // The command line cases (ci-cases.json): `--ci`, `--commit-range` and the argument forms, run through the script itself. The script
  // finds its repository from its own location, so an untracked copy of it (and of the helper it imports) sits in the fixture repository.
  // What is recorded is what a caller sees: the exit status, the job outputs and summary of `--ci`, the answer of a mode that answers
  // on standard output (a ci run without `--app` logs there instead, and the port logs on standard error), and the first line of an error.
  // Commit ids are masked: they differ per run.
  mkdirSync(join(root, 'scripts'), { recursive: true });
  mkdirSync(join(root, 'conformance/src/util'), { recursive: true });
  cpSync(join(here, '../../../../scripts/affected.mjs'), join(root, 'scripts/affected.mjs'));
  cpSync(join(here, '../../../../conformance/src/util/is-main.mjs'), join(root, 'conformance/src/util/is-main.mjs'));
  git(root, 'update-ref', 'refs/remotes/origin/main', 'fixture-base');
  const headSha = git(root, 'rev-parse', 'fixture-head').trim();
  const mask = (t) => (t === null ? null : t.replace(/\b[0-9a-f]{40}\b/g, '<sha>'));
  const canonical = (t) => {
    if (!t.startsWith('{')) return t.trim();
    const v = JSON.parse(t);
    for (const k of Object.keys(v.why ?? {})) v.why[k].sort();
    return v;
  };
  mkdirSync(join(here, 'expected/cli'), { recursive: true });
  for (const c of JSON.parse(readFileSync(join(here, 'ci-cases.json'), 'utf8'))) {
    const dir = mkdtempSync(join(tmpdir(), 'paddock-golden-ci-'));
    const env = { PATH: process.env.PATH, HOME: process.env.HOME, GITHUB_OUTPUT: join(dir, 'out'), GITHUB_STEP_SUMMARY: join(dir, 'summary') };
    for (const [k, v] of Object.entries(c.env)) env[k] = v === '__HEAD__' ? headSha : v;
    const r = spawnSync('node', ['scripts/affected.mjs', ...c.args], { cwd: root, env, encoding: 'utf8' });
    const read = (f) => (existsSync(join(dir, f)) ? mask(readFileSync(join(dir, f), 'utf8')) : null);
    const logsToStdout = c.args.includes('--ci') && !c.args.includes('--app');
    const answer = canonical(r.stdout);
    const rec = {
      code: r.status,
      stdout: logsToStdout ? null : typeof answer === 'string' ? mask(answer) : answer,
      outputs: read('out'),
      summary: read('summary'),
      error: r.status === 0 ? null : r.stderr.split('\n')[0],
    };
    rmSync(dir, { recursive: true, force: true });
    writeFileSync(join(here, 'expected/cli', `${c.name}.json`), `${JSON.stringify(rec, null, 1)}\n`);
  }
} finally {
  rmSync(root, { recursive: true, force: true });
}
