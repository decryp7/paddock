// A shared action reads settings from its job's environment, and a job that forgets them fails only on CI (or worse, quietly
// takes the slow path: the first three jobs of train 2026-11-11-5 without PADDOCK_DEV_TOKEN built ./dev from source). One
// table says which action needs which settings; every workflow job that uses the action is checked against it.
import assert from 'node:assert/strict';
import { existsSync, readdirSync, readFileSync } from 'node:fs';
import { join } from 'node:path';
import { test } from 'node:test';
import { enforceShrinkOnly } from '../src/util/baseline.mjs';
import { parseJobs, parseSteps } from '../src/util/workflows.mjs';
import { PADDOCK } from './helpers.mjs';

const WORKFLOWS = join(PADDOCK, '.gitea', 'workflows');

/**
 * Shared action -> the environment a job that uses it must set (at the workflow, the job or the step that uses it).
 * `when` is a predicate on the value of another variable of the same job; a requirement is skipped when it is false.
 * - PADDOCK_DEV_DOWNLOAD: the release expression (`'0'` on a tag or release run, so ./dev is built from the tagged tree) or the
 *   literal "0" of a release job. Without it a tag run would download a stored tool binary (ADR 0022 amendment).
 * - PADDOCK_DEV_TOKEN: the read credential for that download; a job that never downloads (literal "0") needs none.
 */
export const REQUIRED_ENV = {
  'setup-rust': [
    { name: 'PADDOCK_DEV_DOWNLOAD', value: /^"0"$|refs\/tags\//, why: 'the release expression (or "0" in a release job)' },
    { name: 'PADDOCK_DEV_TOKEN', value: /secrets\.BUILD_INPUTS_TOKEN/, unless: (env) => env.PADDOCK_DEV_DOWNLOAD === '"0"', why: 'secrets.BUILD_INPUTS_TOKEN' },
  ],
  // job-isolation builds and runs ./dev (its acquire step), so it needs the same download setting and read credential.
  'job-isolation': [
    { name: 'PADDOCK_DEV_DOWNLOAD', value: /^"0"$|refs\/tags\//, why: 'the release expression (or "0" in a release job)' },
    { name: 'PADDOCK_DEV_TOKEN', value: /secrets\.BUILD_INPUTS_TOKEN/, unless: (env) => env.PADDOCK_DEV_DOWNLOAD === '"0"', why: 'secrets.BUILD_INPUTS_TOKEN' },
  ],
};

const indentOf = (l) => l.length - l.trimStart().length;

/** `KEY: value` entries of an `env:` block whose key sits at `indent` (value text as written, comments cut). */
export function envBlocks(text, indent) {
  const lines = text.split('\n');
  const env = {};
  for (let i = 0; i < lines.length; i++) {
    if (indentOf(lines[i]) !== indent || !/^\s*env:\s*$/.test(lines[i])) continue;
    for (let j = i + 1; j < lines.length; j++) {
      if (lines[j].trim() === '' || lines[j].trim().startsWith('#')) continue;
      if (indentOf(lines[j]) <= indent) break;
      const m = /^\s*([A-Za-z_][\w]*):\s*(.*?)\s*$/.exec(lines[j]);
      if (m) env[m[1]] = m[2].replace(/\s+#.*$/, '');
    }
  }
  return env;
}

/** Problems of one workflow file: `file: job (line): what is missing`; the key before the first colon is `file:job:VAR`. */
export function missingEnv(file, text, table = REQUIRED_ENV) {
  const workflowEnv = envBlocks(text.split(/^jobs:/m)[0], 0);
  const problems = [];
  for (const job of parseJobs(text)) {
    const jobEnv = envBlocks(job.text, 4);
    for (const [action, required] of Object.entries(table)) {
      const users = parseSteps(job.text).filter((s) => new RegExp(`^\\./(\\.paddock/)?\\.gitea/actions/${action}/?$`).test((s.uses || '').trim()));
      for (const step of users) {
        const env = { ...workflowEnv, ...jobEnv, ...step.env };
        for (const r of required) {
          if (r.unless?.(env)) continue;
          const got = env[r.name];
          if (got === undefined) problems.push(`${file}:${job.id}:${r.name}: job ${job.id} (line ${job.line}) uses ${action} without ${r.name} (${r.why})`);
          else if (!r.value.test(got)) problems.push(`${file}:${job.id}:${r.name}: job ${job.id} (line ${job.line}) sets ${r.name}: ${got}, expected ${r.why}`);
        }
      }
    }
  }
  return problems;
}

// The jobs below predate the table. The list only shrinks (conformance/src/util/baseline.mjs): a job that is not listed must be right, a
// listed job that is now right only warns "remove it", and an entry that the merge base's list lacks fails. ci.yml's wiki, heavy and
// crates are fixed by feat/ci-in-rust-setup-rust; dev-tool.yml runs on main only and publishes the tool itself.
const BASELINE_PATH = 'conformance/baselines/ci-action-env.json';
const BASELINE = join(PADDOCK, BASELINE_PATH);

/** The problems not in `listed`, and the listed keys no problem matches any more. */
export function judge(found, listed) {
  const keys = new Set(found.map((p) => p.split(': ')[0]));
  return { fresh: found.filter((p) => !listed.has(p.split(': ')[0])), gone: [...listed].filter((k) => !keys.has(k)) };
}

test('every job that uses a shared action sets the environment that action needs (table: REQUIRED_ENV), except the listed ones that only shrink', () => {
  const listed = new Set(existsSync(BASELINE) ? JSON.parse(readFileSync(BASELINE, 'utf8')).jobs : []);
  const found = readdirSync(WORKFLOWS)
    .filter((f) => f.endsWith('.yml'))
    .flatMap((f) => missingEnv(f, readFileSync(join(WORKFLOWS, f), 'utf8')));
  const { fresh, gone } = judge(found, listed);
  assert.deepEqual(fresh, [], `${fresh.length} job(s) lack settings. Next step: copy the env lines of a sibling job of the same file.`);
  enforceShrinkOnly({ root: PADDOCK, path: BASELINE_PATH, current: [...listed], gone, keyOf: (k) => k, entriesOf: (j) => j.jobs });
});

test('judge: a fixed listed job is gone (a warning, not a failure); an unlisted problem is fresh', () => {
  const r = judge(['a.yml:j:V: x'], new Set(['a.yml:j:V', 'b.yml:k:V']));
  assert.deepEqual(r, { fresh: [], gone: ['b.yml:k:V'] });
  assert.equal(judge(['c.yml:z:V: x'], new Set()).fresh.length, 1);
});

test('the checker itself: a job without the settings is named, one with them (job, workflow or step level) is not, a release job needs no token', () => {
  const wf = (jobBody, top = '') => `name: t\n${top}jobs:\n  build:\n    runs-on: x\n${jobBody}    steps:\n      - uses: ./.gitea/actions/setup-rust\n`;
  const expr = "${{ startsWith(github.ref, 'refs/tags/') && '0' || '' }}";
  const bad = missingEnv('t.yml', wf(''));
  assert.equal(bad.length, 2, bad.join('\n'));
  assert.match(bad[0], /job build .* uses setup-rust without PADDOCK_DEV_DOWNLOAD/);
  const job = `    env:\n      PADDOCK_DEV_TOKEN: \${{ secrets.BUILD_INPUTS_TOKEN }}\n      PADDOCK_DEV_DOWNLOAD: ${expr}\n`;
  assert.deepEqual(missingEnv('t.yml', wf(job)), []);
  assert.deepEqual(missingEnv('t.yml', wf('', `env:\n  PADDOCK_DEV_TOKEN: \${{ secrets.BUILD_INPUTS_TOKEN }}\n  PADDOCK_DEV_DOWNLOAD: ${expr}\n`)), []);
  assert.deepEqual(missingEnv('t.yml', wf('    env:\n      PADDOCK_DEV_DOWNLOAD: "0"\n')), [], 'a release job never downloads, so it needs no token');
  const wrong = missingEnv('t.yml', wf('    env:\n      PADDOCK_DEV_TOKEN: x\n      PADDOCK_DEV_DOWNLOAD: ""\n'));
  assert.equal(wrong.length, 2, wrong.join('\n'));
  const stepLevel = "name: t\njobs:\n  b:\n    runs-on: x\n    steps:\n      - uses: ./.gitea/actions/setup-rust\n        env:\n          PADDOCK_DEV_DOWNLOAD: \"0\"\n";
  assert.deepEqual(missingEnv('t.yml', stepLevel), [], 'env on the step that uses the action counts');
});

test('job-isolation needs the same settings as setup-rust (it builds and runs ./dev)', () => {
  const wf = (env) => `name: t\njobs:\n  b:\n    runs-on: x\n${env}    steps:\n      - uses: ./.gitea/actions/job-isolation\n`;
  const bad = missingEnv('t.yml', wf(''));
  assert.equal(bad.length, 2, bad.join('\n'));
  assert.match(bad[0], /uses job-isolation without PADDOCK_DEV_DOWNLOAD/);
  assert.deepEqual(missingEnv('t.yml', wf('    env:\n      PADDOCK_DEV_DOWNLOAD: "0"\n')), []);
});
