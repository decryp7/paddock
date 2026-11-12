// A test that passes only on a retry fails the run (ER-12): every nextest `[profile.ci]` allows exactly one retry and sets
// `flaky-result = "fail"` (cargo-nextest >= 0.9.131), so the run stays red and the report marks the test FLAKY for `./dev ci flakes`.
import assert from 'node:assert/strict';
import { execFileSync } from 'node:child_process';
import { readFileSync } from 'node:fs';
import { join } from 'node:path';
import { test } from 'node:test';
import { fileURLToPath } from 'node:url';

const ROOT = fileURLToPath(new URL('..', import.meta.url));

/** The text of the `[profile.ci]` table, up to the next table header (so `[profile.ci.junit]` is not part of it). */
export function ciProfile(toml) {
  const i = toml.search(/^\[profile\.ci\]/m);
  if (i < 0) return null;
  const rest = toml.slice(i + '[profile.ci]'.length);
  const end = rest.search(/^\[/m);
  return end < 0 ? rest : rest.slice(0, end);
}

/** Every `[[profile.ci.overrides]]` table that sets `retries`. */
export function ciOverrideRetries(toml) {
  return [...toml.matchAll(/^\[\[profile\.ci\.overrides\]\]\n((?:(?!\[).*\n?)*)/gm)]
    .map((m) => /^retries\s*=\s*(\d+)/m.exec(m[1])?.[1])
    .filter((v) => v !== undefined);
}

const code = (s) => s.replace(/#[^\n]*/g, '');

test('the profile reader tells a good ci profile from a bad one', () => {
  const good = '[profile.default]\nretries = 2\n[profile.ci]\nretries = 1\nflaky-result = "fail"\n[profile.ci.junit]\npath = "x"\n';
  assert.match(ciProfile(good), /^retries = 1$/m);
  assert.doesNotMatch(ciProfile('[profile.ci]\nretries = 1\n[profile.ci.junit]\nflaky-result = "fail"\n'), /flaky-result/);
  assert.deepEqual(ciOverrideRetries('[[profile.ci.overrides]]\nfilter = "all()"\nretries = 0\n\n[[profile.default.overrides]]\nretries = 5\n'), ['0']);
});

test('every nextest ci profile allows one retry and fails the run when it was needed', () => {
  const files = execFileSync('git', ['ls-files', '*nextest*.toml'], { cwd: ROOT, encoding: 'utf8' }).split('\n').filter(Boolean);
  let checked = 0;
  for (const rel of files) {
    const toml = code(readFileSync(join(ROOT, rel), 'utf8'));
    const profile = ciProfile(toml);
    if (profile === null) continue;
    checked += 1;
    assert.match(profile, /^retries\s*=\s*1\s*$/m, `${rel}: [profile.ci] needs retries = 1`);
    assert.match(profile, /^flaky-result\s*=\s*"fail"\s*$/m, `${rel}: [profile.ci] needs flaky-result = "fail" (a retry must not turn the run green, ER-12)`);
    for (const r of ciOverrideRetries(toml)) assert.equal(r, '1', `${rel}: a ci override sets retries = ${r}; ER-12 allows one`);
  }
  assert.ok(checked >= 9, `only ${checked} nextest ci profiles found`);
});
