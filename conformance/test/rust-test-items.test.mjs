// The test-item blanker has a Rust twin (`without_test_items`, tools/paddock-dev/src/check/lex/rust.rs). Both run the rows of one
// shared corpus, so a change to one that the other does not follow fails here or there (the C115 corpus pattern).
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { test } from 'node:test';
import { maskRust } from '../src/util/mask-rust.mjs';
import { impliesTest, onlyTestItems, withoutTestItems } from '../src/util/rust-test-items.mjs';

const rows = readFileSync(new URL('../../tools/paddock-dev/tests/fixtures/test-items-corpus.tsv', import.meta.url), 'utf8')
  .split('\n')
  .filter((l) => l && !l.startsWith('#'))
  .map((l) => {
    const [name, source, visible, hidden] = l.split('\t');
    return { name, source: source.replaceAll('\\n', '\n'), visible: visible.split(',').filter(Boolean), hidden: (hidden ?? '').split(',').filter(Boolean) };
  });

test('withoutTestItems agrees with the shared corpus (its Rust twin reads the same file)', () => {
  assert.ok(rows.length >= 10, 'the corpus is read');
  const wrong = [];
  for (const r of rows) {
    const masked = maskRust(r.source);
    const out = withoutTestItems(masked);
    if (out.length !== masked.length || out.split('\n').length !== masked.split('\n').length) wrong.push(`${r.name}: offsets or newlines moved`);
    for (const w of r.visible) if (!out.includes(w)) wrong.push(`${r.name}: ${w} should stay visible`);
    for (const w of r.hidden) if (out.includes(w)) wrong.push(`${r.name}: ${w} should be hidden`);
    // The other direction: only the test items are left.
    const only = onlyTestItems(masked);
    for (const w of r.hidden) if (!only.includes(w)) wrong.push(`${r.name}: ${w} should be in the test items`);
    for (const w of r.visible) if (only.includes(w)) wrong.push(`${r.name}: ${w} should not be in the test items`);
  }
  assert.deepEqual(wrong, []);
});

test('impliesTest: test, all(.. test ..) and any(.. test ..) are test-only; not(..), other cfgs and key = value are not', () => {
  for (const p of ['test', ' test ', 'all(unix, test)', 'all(test, target_os = "linux")', 'any(test, feature = "testing")', 'all(unix, any(test, windows))']) {
    assert.equal(impliesTest(p), true, p);
  }
  for (const p of ['unix', 'not(test)', 'all(unix, not(test))', 'feature = "test"', 'all(unix, windows)', 'any(unix, windows)', 'testing']) {
    assert.equal(impliesTest(p), false, p);
  }
});
