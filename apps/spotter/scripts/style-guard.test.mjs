import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { test } from 'node:test';

const css = readFileSync(new URL('../app/src/styles/_controls.scss', import.meta.url), 'utf8');
const block = (sel) => css.match(new RegExp(`^${sel.replace('.', '\\.')} \\{([^}]*)\\}`, 'm'))?.[1] ?? '';

test('version numbers use the UI font with aligned figures, not monospace', () => {
  const ver = block('.ver');
  assert.ok(ver, '.ver rule exists');
  assert.doesNotMatch(ver, /font-mono/);
  assert.match(ver, /tabular-nums/);
  assert.match(ver, /font-weight: var\(--pd-weight-semibold\)/);
  assert.match(ver, /letter-spacing: -0\.01em/);
});

test('monospace stays available for aligned columns, hashes and paths', () => {
  assert.match(block('.mono'), /font-mono/);
});
