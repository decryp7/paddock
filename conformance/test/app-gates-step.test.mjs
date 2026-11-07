// The Rust step of the shared action app-gates runs under `bash --noprofile --norc -e -o pipefail` (Gitea/act's `shell: bash`).
// Its `step` helper must survive a failing check: every check runs, each failure prints one ::error::, the group closes and
// the step exits non-zero at the end.
import { test } from 'node:test';
import assert from 'node:assert/strict';
import { spawnSync } from 'node:child_process';
import { readFileSync } from 'node:fs';
import { fileURLToPath } from 'node:url';

const actionPath = fileURLToPath(new URL('../../.gitea/actions/app-gates/action.yml', import.meta.url));

/** The `step() { ... }` helper of the Rust step, dedented. */
function stepFunction() {
  const text = readFileSync(actionPath, 'utf8');
  const m = text.match(/^( +)step\(\) \{\n[\s\S]*?^\1\}\n/m);
  assert.ok(m, 'step() helper not found in app-gates/action.yml');
  const indent = m[1].length;
  return m[0].split('\n').map((l) => l.slice(indent)).join('\n');
}

test('app-gates step(): a failing check does not stop later checks (bash -e -o pipefail)', () => {
  const script = [
    'set -uo pipefail',
    'status=0',
    stepFunction(),
    "fake() { printf '        FAIL [   0.012s] (1/3) dyno-agent::agent a_test\\n'; return 100; }",
    'step fake',
    'step echo second-step-ran',
    'exit $status',
  ].join('\n');
  const r = spawnSync('bash', ['--noprofile', '--norc', '-e', '-o', 'pipefail', '-c', script], { encoding: 'utf8' });
  const out = r.stdout;
  assert.match(out, /second-step-ran/, `the second check must run\n${out}`);
  assert.match(out, /::error::failed \(exit 100\): fake \| failing tests: dyno-agent::agent a_test/, out);
  assert.equal((out.match(/::endgroup::/g) || []).length, 2, `both groups must close\n${out}`);
  assert.equal(r.status, 1, `the step must exit non-zero\n${out}`);
});
