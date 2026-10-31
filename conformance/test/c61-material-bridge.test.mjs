import assert from 'node:assert/strict';
import { test } from 'node:test';
import { ownTheme, styleEntries } from '../src/checks/C61-material-bridge.mjs';
import { findingsOf, makeApp } from './helpers.mjs';

const NG = (styles) => JSON.stringify({ projects: { app: { architect: { build: { options: { styles } } } } } });
const BRIDGE = ['../../../ui/dist/paddock-ui/css/paddock.css', '../../../ui/dist/paddock-ui/css/material.css', 'src/styles.scss'];

test('C61 reads the style entries and recognises an own theme', () => {
  assert.deepEqual(styleEntries(NG(['a.css', { input: 'b.scss' }])), ['a.css', 'b.scss']);
  assert.equal(styleEntries('not json'), null);
  assert.equal(ownTheme("@use '@angular/material' as mat;\nhtml { @include mat.theme((density: -2)); }"), true);
  assert.equal(ownTheme('html { @include mat.all-component-themes($t); }'), true);
  assert.equal(ownTheme('$x: mat.theme( (color: x) );'), true);
  assert.equal(ownTheme('a { color: red; }'), false);
});

test('C61 warns for an app without the bridge and for an own theme, and passes a clean app', async () => {
  const bad = makeApp({ 'app/angular.json': NG(['src/styles.scss']), 'app/src/styles.scss': 'html { @include mat.theme((density: 0)); }\n' });
  const f = await findingsOf('C61', bad);
  assert.deepEqual(f.map((x) => [x.file, x.severity]), [['app/angular.json', 'warn'], ['app/src/styles.scss', 'warn']]);
  const clean = makeApp({ 'app/angular.json': NG(BRIDGE), 'app/src/styles.scss': 'a { color: red; }\n' });
  assert.deepEqual(await findingsOf('C61', clean), []);
  // No angular.json (a demo, an app without a UI folder): nothing to check.
  assert.deepEqual(await findingsOf('C61', makeApp({ 'a.txt': 'x' })), []);
});
