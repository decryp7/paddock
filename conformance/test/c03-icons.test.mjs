import assert from 'node:assert/strict';
import { test } from 'node:test';
import { cornerIcon, findingsOf, fullIcon, makeApp, messages } from './helpers.mjs';

const names = ['icon.png', '128x128@2x.png', '128x128.png', '32x32.png'];
const icons = (make, extra = {}) => Object.fromEntries([...names.map((n) => [`app/src-tauri/icons/${n}`, make()]), ...Object.entries(extra)]);
const ci = { '.gitea/workflows/build.yml': 'jobs:\n  a:\n    steps:\n      - run: node scripts/ci/check-icons.mjs\n' };

test('C03 good: icons fill the canvas and CI checks them', async () => {
  const f = await findingsOf('C03', makeApp({ ...icons(fullIcon), ...ci }));
  assert.deepEqual(f.filter((x) => x.severity === 'error'), []);
});

test('C03 bad: artwork in a corner, no CI icon check', async () => {
  const f = await findingsOf('C03', makeApp(icons(cornerIcon)));
  assert.match(messages(f), /covers only \d+% of the width/);
  assert.match(messages(f), /no CI step checks the icons/);
});

test('C03 bad: missing icons folder fails closed', async () => {
  const f = await findingsOf('C03', makeApp({ 'README.md': 'x' }));
  assert.match(messages(f), /does not exist/);
});

test('C03 hash: equal to the Paddock reference passes, a differing file fails', async () => {
  const paddock = makeApp(Object.fromEntries(names.map((n) => [`icons/demo/tauri/${n}`, fullIcon()])));
  const good = await findingsOf('C03', makeApp({ ...icons(fullIcon), ...ci }), { options: { paddock } });
  assert.deepEqual(good.filter((x) => x.severity === 'error'), []);
  const other = () => fullIcon(32);
  const bad = await findingsOf('C03', makeApp({ ...icons(fullIcon), 'app/src-tauri/icons/32x32.png': other(), ...ci }), { options: { paddock } });
  assert.match(messages(bad), /32x32\.png differs from paddock/);
});
