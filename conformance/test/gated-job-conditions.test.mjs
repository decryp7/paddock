// A job gated on `needs.gate` must not use `always()`: it would also run once the workflow is cancelled. `!cancelled()` plus the
// explicit result checks is the condition (the gate's own failure or cancellation still stops the job).
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { join } from 'node:path';
import { test } from 'node:test';
import { PADDOCK } from './helpers.mjs';

const GATED = [
  ['canary.yml', 'app'],
  ['debrief-fuzz.yml', 'fuzz'],
  ['service-smoke.yml', 'macos-vm'],
  ['dev-tool.yml', 'publish'],
  ['dev-tool.yml', 'publish-macos'],
];

/** The `if:` line of a top-level job. */
function jobIf(text, job) {
  const lines = text.split('\n');
  const start = lines.findIndex((l) => l === `  ${job}:`);
  assert.ok(start >= 0, `job ${job} not found`);
  const end = lines.findIndex((l, i) => i > start && /^ {2}[\w-]+:/.test(l));
  const body = lines.slice(start + 1, end < 0 ? undefined : end);
  return body.find((l) => /^ {4}if:/.test(l)) ?? '';
}

for (const [file, job] of GATED) {
  test(`${file} ${job} is gated with !cancelled() and the gate's results, never always()`, () => {
    const cond = jobIf(readFileSync(join(PADDOCK, '.gitea', 'workflows', file), 'utf8'), job);
    assert.doesNotMatch(cond, /always\(\)/, cond);
    assert.match(cond, /!cancelled\(\)/);
    assert.match(cond, /needs\.gate\.result != 'failure'/);
    assert.match(cond, /needs\.gate\.result != 'cancelled'/);
    assert.match(cond, /needs\.gate\.outputs\.run != 'false'/);
  });
}
