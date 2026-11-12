// ADR 0085 D7 (WP-4), rule 78: the updater's private key and its password reach a workflow step only on the app's release tag.
// A step that names the secret gates it with `startsWith(github.ref, <the app's release tag prefix>) && secrets.<name> || ''`:
// `format('refs/tags/{0}v', inputs.tag-prefix)` in the shared app-release.yml, `'refs/tags/<app>-v'` in an app's own workflow.
// A reusable workflow call's `secrets:` block is not a step (the called workflow gates it where a step uses it). Inside the
// step the key reaches `tauri build` only (updater-key.sh, tested in ci-scripts.test.mjs). The tag's other inputs are fresh:
// no kept npm or Angular cache, no kept node_modules (rule 78; tag-node-caches.sh).
import assert from 'node:assert/strict';
import { existsSync, lstatSync, mkdirSync, mkdtempSync, readdirSync, readFileSync, symlinkSync, writeFileSync } from 'node:fs';
import { spawnSync } from 'node:child_process';
import { tmpdir } from 'node:os';
import { dirname, join } from 'node:path';
import { test } from 'node:test';
import { fileURLToPath } from 'node:url';
import { parseSteps } from '../src/util/workflows.mjs';
import { expr, job, loadWorkflow, stepsOf } from './lib/workflow.mjs';

const WORKFLOWS = join(dirname(fileURLToPath(import.meta.url)), '..', '..', '.gitea', 'workflows');
const SECRET = /secrets\.(?:PADDOCK_)?TAURI_SIGNING_PRIVATE_KEY(?:_PASSWORD)?\b/;
const GATED = /\$\{\{ startsWith\(github\.ref, (?:format\('refs\/tags\/\{0\}v', inputs\.tag-prefix\)|'refs\/tags\/[a-z][a-z0-9-]*-v') *\) && secrets\.(?:PADDOCK_)?TAURI_SIGNING_PRIVATE_KEY(?:_PASSWORD)? \|\| '' \}\}/;

/** The step lines that name the signing secret without the tag gate: [`file:line: text`]. */
export function ungated(file, text) {
  const out = [];
  for (const step of parseSteps(text)) {
    step.text.split('\n').forEach((l, i) => {
      if (SECRET.test(l) && !GATED.test(l)) out.push(`${file}:${step.line + i}: ${l.trim()}`);
    });
  }
  return out;
}

test('every workflow step gets the updater signing key on a tag run only', () => {
  const files = readdirSync(WORKFLOWS).filter((f) => /\.ya?ml$/.test(f));
  assert.ok(files.length > 10, 'the workflows were found');
  const found = files.flatMap((f) => ungated(f, readFileSync(join(WORKFLOWS, f), 'utf8')));
  assert.deepEqual(found, [], 'gate each with the app\'s release tag prefix (ADR 0085 D7)');
  // The two steps that build installers do use it, gated.
  const steps = ['app-release.yml', 'debrief.yml'].map((f) => readFileSync(join(WORKFLOWS, f), 'utf8'));
  for (const t of steps) assert.match(t, GATED);
});

test('the gate check sees an ungated step and leaves a reusable call\'s secrets block alone', () => {
  const wf = [
    'jobs:',
    '  release:',
    '    uses: ./.gitea/workflows/app-release.yml',
    '    secrets:',
    '      TAURI_SIGNING_PRIVATE_KEY: ${{ secrets.PADDOCK_TAURI_SIGNING_PRIVATE_KEY }}',
    '  bundle:',
    '    runs-on: x',
    '    steps:',
    '      - name: build',
    '        env:',
    '          TAURI_SIGNING_PRIVATE_KEY: ${{ secrets.PADDOCK_TAURI_SIGNING_PRIVATE_KEY }}',
    "          TAURI_SIGNING_PRIVATE_KEY_PASSWORD: ${{ startsWith(github.ref, 'refs/tags/demo-v') && secrets.PADDOCK_TAURI_SIGNING_PRIVATE_KEY_PASSWORD || '' }}",
    "          OTHER: ${{ startsWith(github.ref, 'refs/tags/') && secrets.PADDOCK_TAURI_SIGNING_PRIVATE_KEY || '' }}",
    '        run: npx tauri build',
  ].join('\n');
  assert.deepEqual(ungated('x.yml', wf), [
    'x.yml:11: TAURI_SIGNING_PRIVATE_KEY: ${{ secrets.PADDOCK_TAURI_SIGNING_PRIVATE_KEY }}',
    "x.yml:13: OTHER: ${{ startsWith(github.ref, 'refs/tags/') && secrets.PADDOCK_TAURI_SIGNING_PRIVATE_KEY || '' }}",
  ], 'any tag is too wide: the gate names the app\'s release tags');
});

const read = (f) => readFileSync(join(WORKFLOWS, f), 'utf8');
/** The text of the step named `name` in the job `job` of a workflow. */
function stepOf(text, job, name) {
  const start = text.indexOf(`\n  ${job}:\n`);
  assert.ok(start >= 0, `job ${job}`);
  const next = text.slice(start + 1).search(/\n  [a-z0-9-]+:\n/);
  const body = next < 0 ? text.slice(start) : text.slice(start, start + 1 + next);
  const steps = parseSteps(`jobs:\n${body}`);
  const step = steps.find((s) => s.name === name);
  assert.ok(step, `step "${name}" in ${job}`);
  return { step, steps };
}

test('Debrief\'s Build installers: the key leaves the environment before any npm ci, and only the one tauri build gets it', () => {
  const { step } = stepOf(read('debrief.yml'), 'bundle', 'Build installers');
  const run = step.run.split('\n').filter((l) => !l.trim().startsWith('#')).join('\n');
  const at = (s) => run.indexOf(s);
  assert.ok(at('source ../../../conformance/ci/updater-key.sh') >= 0);
  assert.ok(at('source ../../../conformance/ci/updater-key.sh') < at('updater_key_take'));
  assert.ok(at('updater_key_take') < at('npm ci'), 'updater_key_take before the first npm ci');
  assert.equal(run.match(/tauri build/g).length, 1, 'one tauri build');
  assert.match(run, /^updater_key_exec npx tauri build .* --config \.\.\/target\/no-hook\.conf\.json -- --profile dist$/m, 'through updater_key_exec, the hook emptied last');
  assert.ok(at('before-build.mjs --app-dir . --config src-tauri/tauri.sidecars.conf.json') < at('updater_key_exec npx tauri build'), 'the hook runs first, without the key');
  assert.match(run, /refs\/tags\/\*\) \(cd \.\.\/\.\.\/\.\. && npm ci\)/, 'a tag always runs npm ci');
  assert.match(run, /updater-key-check\.mjs --pubkey "\$updater_pubkey" --dir "\$sigdir"/);
  assert.doesNotMatch(run, /\benv \$\{?updater/, 'no env K=V prefix');
});

test('the bundle jobs read no kept npm or Angular cache and no kept node_modules on a tag', () => {
  for (const [file, job_] of [['app-release.yml', 'bundle'], ['debrief.yml', 'bundle']]) {
    const text = read(file);
    const { steps } = stepOf(text, job_, 'Fresh npm cache and no kept Angular cache on a tag');
    const caches = steps.filter((s) => /\.\/\.gitea\/actions\/node-caches/.test(s.uses ?? ''));
    assert.ok(caches.length >= 1, `${file}: node-caches`);
    const parsed = stepsOf(job(loadWorkflow(file), job_));
    const ifOf = (name, uses) => expr(parsed.find((s) => (name ? s.name === name : true) && (uses ? (s.uses ?? '').includes(uses) : true))?.if);
    const parsedCaches = parsed.filter((s) => (s.uses ?? '').includes('./.gitea/actions/node-caches'));
    assert.equal(parsedCaches.length, caches.length, `${file}: the parsed workflow sees the same node-caches steps`);
    for (const c of parsedCaches) assert.match(expr(c.if), /^(?:runner\.os != 'Windows' && )?!startsWith\(github\.ref, 'refs\/tags\/'\)$/, `${file}: node-caches is skipped on a tag`);
    const fresh = steps.find((s) => s.name === 'Fresh npm cache and no kept Angular cache on a tag');
    assert.equal(ifOf(fresh.name), "startsWith(github.ref, 'refs/tags/')");
    assert.match(fresh.run, /bash conformance\/ci\/tag-node-caches\.sh /);
  }
  const { steps } = stepOf(read('debrief.yml'), 'bundle', 'Build installers');
  const reuse = steps.find((s) => s.name === 'Reuse node_modules for this lockfile (Windows)');
  const debriefSteps = stepsOf(job(loadWorkflow('debrief.yml'), 'bundle'));
  const ifNamed = (name) => expr(debriefSteps.find((s) => s.name === name)?.if);
  assert.ok(reuse);
  assert.equal(ifNamed(reuse.name), "runner.os == 'Windows' && !startsWith(github.ref, 'refs/tags/')", 'no kept node_modules on a tag');
  const unlink = steps.find((s) => s.name === 'No kept node_modules on a tag (Windows)');
  assert.equal(ifNamed(unlink.name), "runner.os == 'Windows' && startsWith(github.ref, 'refs/tags/')");
  assert.match(unlink.run, /windows-node-modules\.sh unlink$/);
  const npmWin = steps.find((s) => s.name === 'npm cache (Windows)');
  assert.equal(ifNamed(npmWin.name), "runner.os == 'Windows' && !startsWith(github.ref, 'refs/tags/')");
  // The release tag names the expected updater key for the per-machine installer gate.
  const gate = stepOf(read('app-release.yml'), 'bundle', 'The per-machine installer check').step;
  assert.match(gate.env.PADDOCK_EXPECT_UPDATE_KEY, /^\$\{\{ startsWith\(github\.ref, format\('refs\/tags\/\{0\}v', inputs\.tag-prefix\)\) && secrets\.TAURI_UPDATER_PUBKEY \|\| '' \}\}$/);
});

test('tag-node-caches.sh: a new npm cache in the job folder, and a kept Angular cache link removed as a link', { skip: process.platform === 'win32' && 'POSIX links' }, () => {
  const root = mkdtempSync(join(tmpdir(), 'paddock-tag-caches-'));
  const kept = join(root, 'kept-angular');
  mkdirSync(join(kept, 'x'), { recursive: true });
  writeFileSync(join(kept, 'x', 'entry'), 'a kept cache entry');
  mkdirSync(join(root, 'ws', 'app', '.angular'), { recursive: true });
  symlinkSync(kept, join(root, 'ws', 'app', '.angular', 'cache'));
  const env = join(root, 'env');
  const script = join(dirname(fileURLToPath(import.meta.url)), '..', 'ci', 'tag-node-caches.sh');
  const r = spawnSync('bash', [script, 'app'], { cwd: join(root, 'ws'), encoding: 'utf8', env: { PATH: '/usr/bin:/bin', RUNNER_TEMP: join(root, 'tmp'), GITHUB_ENV: env, GITHUB_RUN_ID: '42' } });
  assert.equal(r.status, 0, r.stdout + r.stderr);
  assert.equal(readFileSync(env, 'utf8'), `npm_config_cache=${join(root, 'tmp', 'npm-cache-tag-42')}\n`);
  assert.ok(existsSync(join(root, 'tmp', 'npm-cache-tag-42')));
  assert.throws(() => lstatSync(join(root, 'ws', 'app', '.angular', 'cache')), /ENOENT/, 'the link is gone');
  assert.equal(readFileSync(join(kept, 'x', 'entry'), 'utf8'), 'a kept cache entry', 'never deleted through the link');
  const outside = spawnSync('bash', [script], { encoding: 'utf8', env: { PATH: '/usr/bin:/bin' } });
  assert.notEqual(outside.status, 0, 'outside a CI job it refuses');
});
