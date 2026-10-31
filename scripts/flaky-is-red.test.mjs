// A test that passes only on a retry is a failure (rules 5 and 57, ER-12): every CI retry setting is paired with the setting
// that turns a flaky pass red. Playwright: `failOnFlakyTests`; cargo-nextest (>= 0.9.131): `flaky-result = "fail"`.
import assert from 'node:assert/strict';
import { readFileSync, readdirSync, existsSync } from 'node:fs';
import { join } from 'node:path';
import { test } from 'node:test';
import { fileURLToPath } from 'node:url';

const ROOT = fileURLToPath(new URL('..', import.meta.url));
const read = (rel) => readFileSync(join(ROOT, rel), 'utf8');

/** Playwright configs of the apps and the UI workspace that exist in this checkout (a sparse checkout may lack some). */
function playwrightConfigs() {
  const found = [];
  for (const app of readdirSync(join(ROOT, 'apps'))) {
    const rel = `apps/${app}/app/playwright.config.ts`;
    if (existsSync(join(ROOT, rel))) found.push(rel);
  }
  if (existsSync(join(ROOT, 'ui/playwright.config.ts'))) found.push('ui/playwright.config.ts');
  return found;
}

/** True when the config retries on CI (`retries: CI ? 2 : 0`, `retries: process.env['CI'] ? 1 : 0`, ...). */
export const retriesOnCi = (src) => /^\s*retries:\s*[^\n]*\bCI\b[^\n]*\?\s*[1-9]/m.test(src.replace(/\/\/[^\n]*/g, ''));
export const failsOnFlaky = (src) => /^\s*failOnFlakyTests:\s*(?!false\b)\S/m.test(src.replace(/\/\/[^\n]*/g, ''));

/** The text of the `[profile.ci]` table (up to the next table header that is not `[profile.ci.` something). */
export function ciProfile(toml) {
  const i = toml.search(/^\[profile\.ci\]/m);
  if (i < 0) return null;
  const rest = toml.slice(i + '[profile.ci]'.length);
  const end = rest.search(/^\[/m);
  return end < 0 ? rest : rest.slice(0, end);
}

test('the checks tell a retrying config without failOnFlakyTests from a good one', () => {
  assert.equal(retriesOnCi("retries: CI ? 1 : 0,"), true);
  assert.equal(retriesOnCi("retries: process.env['CI'] ? 1 : 0,"), true);
  assert.equal(retriesOnCi("retries: 0,"), false);
  assert.equal(retriesOnCi("// retries: CI ? 2 : 0\nretries: 0,"), false);
  assert.equal(failsOnFlaky("retries: CI ? 1 : 0,\n  failOnFlakyTests: CI,"), true);
  assert.equal(failsOnFlaky("failOnFlakyTests: false,"), false);
  assert.equal(failsOnFlaky("retries: CI ? 1 : 0,"), false);
  assert.match(ciProfile('[profile.default]\na=1\n[profile.ci]\nretries = 0\nflaky-result = "fail"\n[profile.ci.junit]\nx=1\n'), /flaky-result/);
  assert.doesNotMatch(ciProfile('[profile.ci]\nretries = 0\n[profile.ci.junit]\nflaky-result = "fail"\n'), /flaky-result/);
});

test('every Playwright config that retries on CI also fails on flaky tests, with exactly one retry', () => {
  const configs = playwrightConfigs();
  assert.ok(configs.length > 0, 'no Playwright config found');
  for (const rel of configs) {
    const src = read(rel);
    if (!retriesOnCi(src)) continue;
    assert.ok(failsOnFlaky(src), `${rel}: CI retries but has no failOnFlakyTests (a flaky pass would be green)`);
    assert.match(src.replace(/\/\/[^\n]*/g, ''), /retries:\s*[^\n]*\bCI\b[^\n]*\?\s*1\s*:/, `${rel}: CI allows one retry only`);
  }
});

test('every nextest ci profile turns a flaky pass into a failure', () => {
  const files = ['crates/.config/nextest.toml', 'crates/.config/nextest-demos.toml', 'tools/paddock-dev/.config/nextest.toml'];
  for (const app of readdirSync(join(ROOT, 'apps'))) files.push(`apps/${app}/.config/nextest.toml`);
  let checked = 0;
  for (const rel of files) {
    if (!existsSync(join(ROOT, rel))) continue;
    const profile = ciProfile(read(rel));
    if (profile === null) continue;
    checked += 1;
    assert.match(profile, /^flaky-result\s*=\s*"fail"/m, `${rel}: [profile.ci] needs flaky-result = "fail"`);
  }
  assert.ok(checked > 0, 'no nextest ci profile found');
});
