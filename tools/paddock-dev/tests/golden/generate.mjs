// Records tests/golden/expected/*.json: what scripts/affected.mjs answers for tests/golden/cases.json on the fixture repository.
// Run once from the repository root (`node tools/paddock-dev/tests/golden/generate.mjs`) and commit the result; the Rust golden
// test (tests/golden.rs) compares against the recorded files, so it keeps working after affected.mjs is gone (ADR 0022 R4).
// The fixture repository is built the same way in both places: tests/golden/base-overlay over tests/golden/fixture is the
// commit tagged `fixture-base`, tests/golden/fixture alone (the overlay-only files removed) is the commit tagged `fixture-head`.
import { execFileSync } from 'node:child_process';
import { cpSync, mkdirSync, mkdtempSync, readdirSync, readFileSync, realpathSync, rmSync, statSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { dirname, join, relative } from 'node:path';
import { fileURLToPath } from 'node:url';
import { planFor } from '../../../../scripts/affected.mjs';

const here = dirname(fileURLToPath(import.meta.url));
const git = (cwd, ...args) =>
  execFileSync('git', ['-c', 'user.name=t', '-c', 'user.email=t@example.invalid', '-c', 'commit.gpgsign=false', '-c', 'core.autocrlf=false', ...args], { cwd, encoding: 'utf8', stdio: ['ignore', 'pipe', 'pipe'] });
const walk = (dir) => readdirSync(dir).flatMap((n) => (statSync(join(dir, n)).isDirectory() ? walk(join(dir, n)) : [join(dir, n)]));

const root = realpathSync(mkdtempSync(join(tmpdir(), 'paddock-golden-')));
try {
  git(root, 'init', '-q', '-b', 'main');
  cpSync(join(here, 'fixture'), root, { recursive: true });
  cpSync(join(here, 'base-overlay'), root, { recursive: true });
  git(root, 'add', '-A');
  git(root, 'commit', '-q', '-m', 'base');
  git(root, 'tag', 'fixture-base');
  cpSync(join(here, 'fixture'), root, { recursive: true });
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
} finally {
  rmSync(root, { recursive: true, force: true });
}
