import assert from 'node:assert/strict';
import { test } from 'node:test';
import { fakedRoots, optOuts, parseAllow } from '../src/checks/C135-leak-detector-opt-outs.mjs';
import { findingsOf, makeApp } from './helpers.mjs';

const ALLOW = 'conformance/c135-allow.json';
const list = (allow = []) => JSON.stringify({ allow });
const entry = (path, site = "mode: 'off'", count = 1) => ({ path, site, reason: 'a suite that leaks on purpose', count });

test('C135 bad: mode off and skip in the call are found, with their lines; fail and report are not', () => {
  const text = "installLeakDetector({ mode: 'fail' });\ninstallLeakDetector({\n  mode: 'off',\n});\ninstallLeakDetector({ skip: ['timer'] });\ninstallLeakDetector();\n";
  assert.deepEqual(optOuts(text), [
    { line: 2, site: "mode: 'off'" },
    { line: 5, site: 'skip:' },
  ]);
  assert.deepEqual(optOuts("const x = { mode: 'off' };\n"), []);
});

test('C135 allow file: a path, a site, a reason and a count', () => {
  assert.deepEqual(parseAllow(list([entry('a.ts')])), [entry('a.ts')]);
  assert.throws(() => parseAllow(list([{ path: 'a.ts', reason: 'long enough' }])), /site/);
});

test('C135 through the runner: listed opt-outs pass, others are errors, the list may not grow against the merge base', async () => {
  const files = {
    [ALLOW]: list([entry('apps/a/src/leaky.spec.ts')]),
    'apps/a/src/leaky.spec.ts': "describe('x', () => {\n  installLeakDetector({ mode: 'off' });\n});\n",
    'apps/a/src/other.spec.ts': "installLeakDetector({ mode: 'off' });\n",
    'apps/a/src/fine.ts': "installLeakDetector({ mode: 'fail' });\n",
  };
  const base = (text) => ({ git: (args) => (args[0] === 'show' ? text : 'abc123') });
  // the merge base has the same list: only the unlisted opt-out is reported
  const same = await findingsOf('C135', makeApp(files), { options: base(list([entry('apps/a/src/leaky.spec.ts')])) });
  assert.deepEqual(same.map((f) => [f.file, f.line, f.severity]), [['apps/a/src/other.spec.ts', 1, 'error']]);
  // the merge base has an empty list: the entry was added by this branch
  const grown = await findingsOf('C135', makeApp(files), { options: base(list()) });
  assert.ok(grown.some((f) => f.severity === 'error' && /the list grew/.test(f.message)));
  // a stale entry
  const stale = await findingsOf('C135', makeApp({ ...files, 'apps/a/src/leaky.spec.ts': 'nothing\n' }), { options: base(list([entry('apps/a/src/leaky.spec.ts')])) });
  assert.ok(stale.some((f) => /stale allow entry/.test(f.message)));
});

test('C135 a broken allow file is one finding, nothing else', async () => {
  for (const text of [null, 'nope']) {
    const found = await findingsOf('C135', makeApp({ 'apps/a/src/x.spec.ts': "installLeakDetector({ mode: 'off' });\n", ...(text === null ? {} : { [ALLOW]: text }) }));
    assert.deepEqual(found.map((f) => [f.file, f.outcome]), [[ALLOW, 'check-broken']]);
  }
});

test('C135 flags a spec that fakes a TestBed root with ng-version, and nothing else', async () => {
  assert.deepEqual(fakedRoots("tab.setAttribute('ng-version', 'x');\nconst a = 1;\nhtml = '<div ng-version=\"0\">';\n"), [{ line: 1 }, { line: 3 }]);
  assert.deepEqual(fakedRoots("el.closest('[ng-version]')\n"), []);
  const found = await findingsOf('C135', makeApp({ [ALLOW]: list(), 'apps/a/src/x.spec.ts': "tab.setAttribute('ng-version', 'leftover');\n", 'apps/a/src/x.ts': "el.setAttribute('ng-version', 'y');\n" }));
  assert.deepEqual(found.map((f) => [f.file, f.line, f.severity]), [['apps/a/src/x.spec.ts', 1, 'error']]);
});
