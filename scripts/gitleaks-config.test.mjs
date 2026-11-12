// The secret scan (.gitea/actions/gitleaks, .gitleaks.toml): the allow-list holds only reasoned, path-scoped entries, and the
// action's pin is complete. The scan itself runs in CI (the conformance job); gitleaks is not needed here.
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { join } from 'node:path';
import { test } from 'node:test';
import { fileURLToPath } from 'node:url';

const ROOT = fileURLToPath(new URL('..', import.meta.url));
const read = (rel) => readFileSync(join(ROOT, rel), 'utf8');

/** The `[[allowlists]]` tables of a gitleaks config as {description, targetRules, paths} (the subset this repository uses). */
export function allowlists(toml) {
  return toml.split(/^\[\[allowlists\]\]\n/m).slice(1).map((block) => ({
    description: /^description\s*=\s*"([^"]*)"/m.exec(block)?.[1] ?? '',
    targetRules: [...(/^targetRules\s*=\s*\[([^\]]*)\]/m.exec(block)?.[1] ?? '').matchAll(/"([^"]+)"/g)].map((m) => m[1]),
    paths: [...block.matchAll(/'''([^']+)'''/g)].map((m) => m[1]),
  }));
}

test('the reader finds the entries of an allow-list', () => {
  const [a] = allowlists('x\n[[allowlists]]\ndescription = "why"\ntargetRules = ["jwt"]\npaths = [\n  \'\'\'^a/b\\.rs$\'\'\',\n]\n');
  assert.deepEqual(a, { description: 'why', targetRules: ['jwt'], paths: ['^a/b\\.rs$'] });
});

test('every allow-list entry has a reason, names rules and anchors each path to one file or lockfile', () => {
  const toml = read('.gitleaks.toml');
  assert.match(toml, /^useDefault\s*=\s*true/m, 'the default rules must stay on');
  const entries = allowlists(toml);
  assert.ok(entries.length > 0);
  for (const e of entries) {
    assert.ok(e.description.length >= 20, `an allow-list entry needs a reason: ${JSON.stringify(e.paths)}`);
    assert.ok(e.targetRules.length > 0, `entry "${e.description}" must name its rules (no blanket path)`);
    assert.ok(e.paths.length > 0, `entry "${e.description}" has no path`);
    for (const p of e.paths) {
      assert.ok(p.startsWith('^') && p.endsWith('$'), `${p}: anchor the path with ^ and $`);
      assert.doesNotMatch(p, /\.\*|\+/, `${p}: name one file, not a pattern that can swallow a new one`);
    }
  }
});

test('the action pins a version with a checksum for every platform it supports', () => {
  const action = read('.gitea/actions/gitleaks/action.yml');
  assert.match(action, /VERSION=\d+\.\d+\.\d+/);
  const sums = [...action.matchAll(/SUM=([0-9a-f]+)/g)].map((m) => m[1]);
  assert.ok(sums.length >= 2);
  for (const s of sums) assert.equal(s.length, 64, 'a SHA-256 is 64 hex digits');
  assert.match(action, /--redact=100/, 'secret values must never reach the log');
});

test('the conformance job runs the scan', () => {
  assert.match(read('.gitea/workflows/ci.yml'), /uses: \.\/\.gitea\/actions\/gitleaks/);
});
