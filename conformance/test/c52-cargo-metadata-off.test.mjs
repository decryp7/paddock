import assert from 'node:assert/strict';
import { test } from 'node:test';
import { enablingLines } from '../src/checks/C52-cargo-metadata-off.mjs';
import { findingsOf, makeApp, messages } from './helpers.mjs';

const GOOD = '[dependencies]\npaddock-repo-graph = { path = "../../../../crates/paddock-repo-graph" }\n';

test('enablingLines finds each way of switching the feature on', () => {
  assert.deepEqual(enablingLines(GOOD), []);
  assert.deepEqual(enablingLines('[dependencies]\npaddock-repo-graph = { path = "x", features = ["cargo-metadata"] }\n'), [2]);
  assert.deepEqual(enablingLines('[dependencies.paddock-repo-graph]\npath = "x"\nfeatures = ["cargo-metadata"]\n'), [3]);
  assert.deepEqual(enablingLines('[workspace.dependencies]\npaddock-repo-graph = { path = "x", features = ["a", \'cargo-metadata\'] }\n'), [2]);
  assert.deepEqual(enablingLines('[features]\nmeta = ["paddock-repo-graph/cargo-metadata"]\n'), [2]);
  assert.deepEqual(enablingLines('[features]\nmeta = ["paddock-repo-graph?/cargo-metadata"]\n'), [2]);
});

test('enablingLines ignores comments and other crates with the same feature name', () => {
  assert.deepEqual(enablingLines('# paddock-repo-graph = { features = ["cargo-metadata"] }\n'), []);
  assert.deepEqual(enablingLines('[dependencies]\nother = { path = "x", features = ["cargo-metadata"] }\n'), []);
  assert.deepEqual(enablingLines('[dependencies.other]\nfeatures = ["cargo-metadata"]\n'), []);
  assert.deepEqual(enablingLines('[dependencies.paddock-repo-graph]\nfeatures = ["other"]\n[dependencies.x]\nfeatures = ["cargo-metadata"]\n'), []);
});

test('C52 good: Pacenotes manifests without the feature, and other apps that use it', async () => {
  const files = {
    'apps/pacenotes/Cargo.toml': '[workspace]\nmembers = ["crates/*"]\n',
    'apps/pacenotes/crates/pacenotes-graph/Cargo.toml': GOOD,
    'apps/debrief/crates/x/Cargo.toml': '[dependencies]\npaddock-repo-graph = { path = "x", features = ["cargo-metadata"] }\n',
  };
  assert.deepEqual(await findingsOf('C52', makeApp(files)), []);
});

test('C52 bad: a fixture manifest enabling the feature fails, naming file and line', async () => {
  const files = {
    'apps/pacenotes/crates/pacenotes-graph/Cargo.toml': '[dependencies]\npaddock-repo-graph = { path = "x", features = ["cargo-metadata"] }\n',
  };
  const f = await findingsOf('C52', makeApp(files));
  assert.equal(f.length, 1);
  assert.equal(f[0].severity, 'error');
  assert.equal(f[0].file, 'apps/pacenotes/crates/pacenotes-graph/Cargo.toml');
  assert.equal(f[0].line, 2);
  assert.match(messages(f), /enables paddock-repo-graph's cargo-metadata feature/);
});
