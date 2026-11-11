// ADR 0085 D7 (WP-4), rule 78: the updater's private key and its password reach a workflow step only on a tag run. A step that
// names the secret gates it with `startsWith(github.ref, 'refs/tags/') && secrets.<name> || ''`; a reusable workflow call's
// `secrets:` block is not a step (the called workflow gates it where a step uses it). The step then hands the key to
// `tauri build` only (updater-key.sh, tested in ci-scripts.test.mjs).
import assert from 'node:assert/strict';
import { readdirSync, readFileSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { test } from 'node:test';
import { fileURLToPath } from 'node:url';
import { parseSteps } from '../src/util/workflows.mjs';

const WORKFLOWS = join(dirname(fileURLToPath(import.meta.url)), '..', '..', '.gitea', 'workflows');
const SECRET = /secrets\.(?:PADDOCK_)?TAURI_SIGNING_PRIVATE_KEY(?:_PASSWORD)?\b/;
const GATED = /\$\{\{ startsWith\(github\.ref, 'refs\/tags\/'\) && secrets\.(?:PADDOCK_)?TAURI_SIGNING_PRIVATE_KEY(?:_PASSWORD)? \|\| '' \}\}/;

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
  assert.deepEqual(found, [], 'gate each with `startsWith(github.ref, \'refs/tags/\') && secrets.<name> || \'\'` (ADR 0085 D7)');
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
    "          TAURI_SIGNING_PRIVATE_KEY_PASSWORD: ${{ startsWith(github.ref, 'refs/tags/') && secrets.PADDOCK_TAURI_SIGNING_PRIVATE_KEY_PASSWORD || '' }}",
    '        run: npx tauri build',
  ].join('\n');
  assert.deepEqual(ungated('x.yml', wf), ['x.yml:11: TAURI_SIGNING_PRIVATE_KEY: ${{ secrets.PADDOCK_TAURI_SIGNING_PRIVATE_KEY }}']);
});
