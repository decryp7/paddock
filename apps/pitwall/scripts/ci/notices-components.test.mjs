import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';
import { test } from 'node:test';
import { normalise } from '../../../../conformance/src/notices/generate.mjs';

const app = join(dirname(fileURLToPath(import.meta.url)), '..', '..');
const read = (rel) => readFileSync(join(app, rel), 'utf8');

test('the notices carry the Microsoft Visual C++ Redistributable component text, as paddock-notices appends it', () => {
  const components = JSON.parse(read('paddock.ci.json')).notices.components;
  assert.deepEqual(components, ['app/src-tauri/resources/microsoft-vc-redist.md']);
  const tail = `\n## Bundled components\n\n${normalise(read(components[0]))}\n`;
  assert.ok(read('app/src-tauri/resources/THIRD_PARTY_NOTICES.md').endsWith(tail), 'regenerate the notices: scripts/notices.sh pitwall');
  assert.match(tail, /You may not modify these files/, 'the VC\\redist clause is quoted');
  assert.match(tail, /VC\\redist/);
});
