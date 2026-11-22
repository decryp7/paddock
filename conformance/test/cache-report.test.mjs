// conformance/ci/cache-report.sh (ADR 0115): the one reader of an actions/cache step's outputs. Exact, partial and miss are told apart
// (node-caches called a partial restore "miss"), and the `cache-report:` line is the format `ci cache-audit` reads.
import assert from 'node:assert/strict';
import { spawnSync } from 'node:child_process';
import { join } from 'node:path';
import { test } from 'node:test';
import { PADDOCK } from './helpers.mjs';

const SCRIPT = join(PADDOCK, 'conformance', 'ci', 'cache-report.sh');
const sh = (...args) => {
  const r = spawnSync('bash', [SCRIPT, ...args], { env: { PATH: '/usr/bin:/bin' }, encoding: 'utf8' });
  return { status: r.status, text: r.stdout.trim() };
};

test('the outcome comes from cache-hit and the matched key: true is exact, a matched key without true is partial, nothing is a miss', () => {
  assert.equal(sh('outcome', 'true', 'k').text, 'exact');
  assert.equal(sh('outcome', 'false', 'older-k').text, 'partial');
  assert.equal(sh('outcome', 'false', '').text, 'miss');
  assert.equal(sh('outcome', '', '').text, 'miss', 'a step that did not run is not a hit');
});

test('the line names restore and save, and says pending for a post-step save instead of claiming it', () => {
  const partial = sh('line', 'npm', 'npm-k', 'false', 'npm-older', 'pending').text;
  assert.match(partial, /- npm cache `npm-k`: restored: partial hit \(matched `npm-older`\); saved: pending/);
  assert.match(partial, /^cache-report: name=npm outcome=partial saved=pending key=npm-k$/m);
  assert.match(sh('line', 'x', 'k', '', '', 'no').text, /restored: miss; saved: NO/);
  assert.match(sh('line', 'x', 'k', 'true', 'k', 'n/a').text, /restored: exact hit; saved: not by this job/);
});

test('an unknown call is refused with the usage line', () => {
  assert.equal(sh('nope').status, 2);
});
