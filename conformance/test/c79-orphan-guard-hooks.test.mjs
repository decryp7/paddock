import assert from 'node:assert/strict';
import { test } from 'node:test';
import C79, { ADOPTING, guardedMains, lockGraph, reaches } from '../src/checks/C79-orphan-guard-hooks.mjs';

const LOCK = `version = 4

[[package]]
name = "app"
version = "0.1.0"
dependencies = [
 "mid",
 "serde 1.0.0",
]

[[package]]
name = "mid"
version = "0.1.0"
dependencies = [
 "paddock-proc",
]

[[package]]
name = "paddock-proc"
version = "0.1.0"

[[package]]
name = "plain"
version = "0.1.0"
`;

const tree = (files) => ({
  files: () => Object.keys(files),
  read: (p) => files[p] ?? null,
});

const HOOK = 'fn main() {\n    paddock_proc::guardian::run_if_requested();\n}\n';
const BARE = 'fn main() {}\n';

test('C79 reads a Cargo.lock graph and follows it to paddock-proc', () => {
  const g = lockGraph(LOCK);
  assert.deepEqual(g.app, ['mid', 'serde']);
  assert.ok(reaches(g, 'app') && reaches(g, 'mid'));
  assert.ok(!reaches(g, 'plain') && !reaches(g, 'serde'));
});

test('C79 finds the mains that reach paddock-proc, hooked or not', () => {
  const files = {
    'w/Cargo.lock': LOCK,
    'w/app/Cargo.toml': '[package]\nname = "app"\n',
    'w/app/src/main.rs': HOOK,
    'w/app/src/bin/tool.rs': BARE,
    'w/app/src/bin/two/main.rs': BARE,
    'w/app/tests/it/main.rs': BARE,
    'w/plain/Cargo.toml': '[package]\nname = "plain"\n',
    'w/plain/src/main.rs': BARE,
  };
  const t = tree(files);
  assert.deepEqual(guardedMains(t.files(), t.read), [
    { file: 'w/app/src/main.rs', hooked: true },
    { file: 'w/app/src/bin/tool.rs', hooked: false },
    { file: 'w/app/src/bin/two/main.rs', hooked: false },
  ]);
});

test('C79: a missing hook is an error, an adopting one a warning, and the list only shrinks', () => {
  const files = {
    'Cargo.lock': LOCK,
    'Cargo.toml': '[package]\nname = "app"\n',
    'src/main.rs': BARE,
    'src/bin/listed.rs': BARE,
    'src/bin/done.rs': HOOK,
  };
  const adopting = new Map([
    ['src/bin/listed.rs', 'adopts later'],
    ['src/bin/done.rs', 'adopts later'],
  ]);
  const f = C79.run(tree(files), adopting);
  const by = Object.fromEntries(f.map((x) => [x.file, x.severity]));
  assert.deepEqual(by, { 'src/main.rs': 'error', 'src/bin/listed.rs': 'warn', 'src/bin/done.rs': 'error' });
});

test('C79 ADOPTING names only main files', () => {
  for (const [file, why] of ADOPTING) {
    assert.match(file, /src\/(main\.rs|bin\/.+\.rs)$/);
    assert.ok(why.length > 5);
  }
});

test('C79 matches a listed main from an app folder (paths relative to the checked root)', () => {
  const files = { 'Cargo.lock': LOCK, 'Cargo.toml': '[package]\nname = "app"\n', 'src/main.rs': BARE };
  const f = C79.run(tree(files), new Map([['apps/x/src/main.rs', 'adopts later']]));
  assert.deepEqual(f.map((x) => x.severity), ['warn']);
});
