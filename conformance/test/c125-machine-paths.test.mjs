import assert from 'node:assert/strict';
import { test } from 'node:test';
import { globRegExp, machinePaths, parseAllow } from '../src/checks/C125-machine-paths.mjs';
import { findingsOf, makeApp } from './helpers.mjs';

const ALLOW = 'conformance/c125-allow.json';
const lists = ({ history = [], allow = [] } = {}) => JSON.stringify({ history, allow });

test('C125 bad: the 2026-11-14 shapes, a stale disk default in a guard, a script and a config file', () => {
  assert.deepEqual(machinePaths('MAIN = os.environ.get("WORKTREE_GUARD_MAIN", "/Volumes/d8sst7/dev.decryptology.net/paddock")\n'), [
    { line: 1, literal: '/Volumes/d8sst7/dev.decryptology.net/paddock' },
  ]);
  assert.deepEqual(machinePaths('# a\nexport TART_HOME=${TART_HOME:-/Volumes/d8sst7/vm-data/tart-from-home}\n').map((m) => m.line), [2]);
  // one finding per literal, two on one line
  assert.deepEqual(machinePaths('roots = ["/Volumes/a/x", "/Volumes/B"]\n').map((m) => m.literal), ['/Volumes/a/x', '/Volumes/B']);
  assert.deepEqual(machinePaths('<string>/Volumes/disk 2/x</string>').map((m) => m.literal), ['/Volumes/disk']);
});

test('C125 bad: an escaped literal (a regular expression) is a machine path too, reported unescaped', () => {
  // 2026-11-14: scripts/vm-win.test.mjs matched /only drives \/Volumes\/d8sst7\/vm-data\/.../ and C125 did not see it.
  assert.deepEqual(machinePaths('assert.match(e, /only drives \\/Volumes\\/d8sst7\\/vm-data\\/x/);\n'), [
    { line: 1, literal: '/Volumes/d8sst7/vm-data/x/' }, // the closing / of the regular expression joins it, as a trailing / always does
  ]);
  assert.deepEqual(machinePaths(String.raw`re = r"\/Volumes\/Data"`).map((m) => m.literal), ['/Volumes/Data']);
  assert.deepEqual(machinePaths(String.raw`firmlink \/System\/Volumes\/Data and \/Volumes\/<disk>`), []);
});

test('C125 good: placeholders, other paths that contain Volumes, no Volumes at all', () => {
  assert.deepEqual(machinePaths('Worktree: <a folder under /Volumes/<disk>/dev-targets/wt/>\n'), []);
  assert.deepEqual(machinePaths('see /Volumes/... and /Volumes/$DISK and /Volumes/*\n'), []);
  assert.deepEqual(machinePaths('firmlink /System/Volumes/Data and ~/Volumes/x and C:/Users/a/Volumes/b\n'), []);
  assert.deepEqual(machinePaths('let p = home.join("Volumes");\n'), []);
});

test('C125 path patterns: ** any folders, * one name, a trailing / the folder', () => {
  assert.equal(globRegExp('docs/adr/').test('docs/adr/0001-x.md'), true);
  assert.equal(globRegExp('docs/adr/').test('docs/adrs.md'), false);
  assert.equal(globRegExp('apps/*/docs/adr/').test('apps/dyno/docs/adr/0001.md'), true);
  assert.equal(globRegExp('apps/*/docs/adr/').test('apps/dyno/x/docs/adr/0001.md'), false);
  assert.equal(globRegExp('**/CHANGELOG.md').test('CHANGELOG.md'), true);
  assert.equal(globRegExp('**/CHANGELOG.md').test('apps/dyno/CHANGELOG.md'), true);
  assert.equal(globRegExp('docs/lessons.md').test('docs/lessons.md.bak'), false);
});

test('C125 allow file: both lists, a reason on every entry, a count of at least 1', () => {
  const { history, allow } = parseAllow(lists({ history: [{ path: 'docs/adr/', reason: 'records history' }], allow: [{ path: 'a.rs', site: '/Volumes/Data', reason: 'invented test data', count: 2 }] }));
  assert.equal(history[0].path, 'docs/adr/');
  assert.deepEqual(allow, [{ path: 'a.rs', site: '/Volumes/Data', reason: 'invented test data', count: 2 }]);
  assert.equal(parseAllow(lists({ allow: [{ path: 'a.rs', site: 'x', reason: 'long enough' }] })).allow[0].count, 1);
  assert.throws(() => parseAllow(lists({ history: [{ path: 'docs/' }] })), /reason/);
  assert.throws(() => parseAllow(lists({ allow: [{ path: 'a.rs', reason: 'long enough' }] })), /site/);
  assert.throws(() => parseAllow(lists({ allow: [{ path: 'a.rs', site: 'x', reason: 'long enough', count: 0 }] })), /count/);
  assert.throws(() => parseAllow('{"allow": []}'), /history/);
  assert.throws(() => parseAllow('nope'), /not JSON/);
});

test('C125 through the runner: warnings only, history skipped, sites allowed by count, a missing list is a broken check', async () => {
  const root = makeApp({
    [ALLOW]: lists({
      history: [{ path: 'docs/adr/', reason: 'ADRs record what was true then' }],
      allow: [{ path: 'apps/dyno/src/vol.rs', site: '/Volumes/Data', reason: 'invented volume names of the disk map', count: 2 }],
    }),
    '.claude/hooks/worktree-guard.py': 'MAIN = os.environ.get("M", "/Volumes/d8sst7/dev.decryptology.net/paddock")\n',
    'docs/adr/0001-x.md': 'built in /Volumes/d7sst7/dev-targets/x\n',
    // an allowed name does not hide another literal in the same file
    'apps/dyno/src/vol.rs': 'let a = "/Volumes/Data";\nlet b = "/Volumes/Data/x";\nlet c = "/Volumes/d7sst7";\n',
    'icons/a.png': 'bytes /Volumes/d7sst7',
    '.git': 'gitdir: /Volumes/d7sst7/paddock/.git/worktrees/x\n',
  });
  const found = await findingsOf('C125', root);
  assert.deepEqual(found.map((f) => [f.file, f.line, f.severity]).sort(), [
    ['.claude/hooks/worktree-guard.py', 1, 'warn'],
    ['apps/dyno/src/vol.rs', 3, 'warn'],
  ]);
  assert.match(found[0].message, /\/Volumes\/d/);
  const broken = await findingsOf('C125', makeApp({ 'a.sh': 'x=/Volumes/a\n' }));
  assert.equal(broken.some((f) => f.outcome === 'check-broken'), true);
});

test('C125 allow file: stale and ambiguous entries and a history entry that names no file are warnings', async () => {
  const f = 'scripts/x.sh';
  const run = (allow, history = [], text = 'A=/Volumes/Data\nB=/Volumes/Data\n') =>
    findingsOf('C125', makeApp({ [ALLOW]: lists({ history, allow }), [f]: text }));
  const entry = (site, count, path = f) => ({ path, site, reason: 'invented test data', count });
  assert.deepEqual(await run([entry('/Volumes/Data', 2)]), []);
  assert.match((await run([entry('/Volumes/Data', 3)])).map((x) => x.message).join('\n'), /stale allow entry .* matches only 2 of its 3 sites/);
  assert.match((await run([entry('/Volumes/Data', 1)])).map((x) => x.message).join('\n'), /ambiguous allow entry .* matches 2 sites/);
  assert.match((await run([entry('/Volumes/Data', 2), entry('/Volumes/Gone', 1)])).map((x) => x.message).join('\n'), /stale allow entry .*Gone.* matches no site/);
  assert.match((await run([entry('/Volumes/x', 1, 'scripts/gone.sh')])).map((x) => x.message).join('\n'), /stale allow entry/);
  const hist = await run([entry('/Volumes/Data', 2)], [{ path: 'docs/research/', reason: 'research notes record history' }]);
  assert.deepEqual(hist.map((x) => [x.file, x.message.replace(/ names.*/, '')]), [[ALLOW, 'stale history entry (docs/research/)']]);
});
