import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { test } from 'node:test';
import { fileURLToPath } from 'node:url';

const ROOT = join(dirname(fileURLToPath(import.meta.url)), '..', '..');
const MATCHERS = ['matchPackageNames', 'matchDepNames', 'excludePackageNames', 'matchCurrentVersion'];

/** The packageRules entries whose pattern list mixes a bare `*` or `**` with other patterns (Renovate's validator rejects them). */
export function mixedWildcards(config) {
  const problems = [];
  (config.packageRules ?? []).forEach((rule, i) => {
    for (const key of MATCHERS) {
      const list = rule[key];
      if (Array.isArray(list) && list.length > 1 && list.some((p) => p === '*' || p === '**')) {
        problems.push(`packageRules[${i}].${key}: "*" or "**" together with other patterns; list only the negations instead`);
      }
    }
  });
  return problems;
}

test('mixedWildcards: a lone * passes, * with other patterns fails', () => {
  assert.deepEqual(mixedWildcards({ packageRules: [{ matchPackageNames: ['*'] }, { matchPackageNames: ['!a', '!b'] }] }), []);
  assert.equal(mixedWildcards({ packageRules: [{}, { matchPackageNames: ['*', '!a'] }] }).length, 1);
  assert.match(mixedWildcards({ packageRules: [{ matchDepNames: ['**', 'x'] }] })[0], /packageRules\[0\]\.matchDepNames/);
});

test('renovate.json never mixes * with other patterns (Renovate stops opening PRs and files an issue)', () => {
  const config = JSON.parse(readFileSync(join(ROOT, 'renovate.json'), 'utf8'));
  assert.deepEqual(mixedWildcards(config), []);
});
