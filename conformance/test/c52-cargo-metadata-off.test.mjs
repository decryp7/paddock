import assert from 'node:assert/strict';
import { test } from 'node:test';
import { enablingLines, entries, splitKey, stripComment } from '../src/checks/C52-cargo-metadata-off.mjs';
import { findingsOf, makeApp, messages } from './helpers.mjs';

const GOOD = '[dependencies]\npaddock-repo-graph = { path = "../../../../crates/paddock-repo-graph" }\n';

test('enablingLines finds each way of switching the feature on', () => {
  assert.deepEqual(enablingLines(GOOD), []);
  assert.deepEqual(enablingLines('[dependencies]\npaddock-repo-graph = { path = "x", features = ["cargo-metadata"] }\n'), [2]);
  assert.deepEqual(enablingLines('[dependencies.paddock-repo-graph]\npath = "x"\nfeatures = ["cargo-metadata"]\n'), [3]);
  assert.deepEqual(enablingLines('[workspace.dependencies]\npaddock-repo-graph = { path = "x", features = ["a", \'cargo-metadata\'] }\n'), [2]);
  assert.deepEqual(enablingLines('[features]\nmeta = ["paddock-repo-graph/cargo-metadata"]\n'), [2]);
  assert.deepEqual(enablingLines('[features]\nmeta = ["paddock-repo-graph?/cargo-metadata"]\n'), [2]);
  assert.deepEqual(enablingLines('[target.\'cfg(unix)\'.dependencies]\npaddock-repo-graph = { path = "x", features = ["cargo-metadata"] }\n'), [2]);
  assert.deepEqual(enablingLines('[build-dependencies]\npaddock-repo-graph.features = ["cargo-metadata"]\n'), [2]);
});

test('a multi-line features array is read', () => {
  const text = '[dependencies]\npaddock-repo-graph = {\n  path = "x",\n  features = [\n    "git-history", # fine\n    "cargo-metadata",\n  ],\n}\n';
  assert.deepEqual(enablingLines(text), [2]);
  assert.deepEqual(enablingLines('[dependencies.paddock-repo-graph]\nfeatures = [\n  "cargo-metadata"\n]\n'), [2]);
});

test('a quoted table header is read', () => {
  assert.deepEqual(enablingLines('[dependencies."paddock-repo-graph"]\nfeatures = ["cargo-metadata"]\n'), [2]);
  assert.deepEqual(enablingLines("[dependencies.'paddock-repo-graph']\nfeatures = ['cargo-metadata']\n"), [2]);
  assert.deepEqual(enablingLines('[dependencies]\n"paddock-repo-graph" = { features = ["cargo-metadata"] }\n'), [2]);
});

test('a renamed dependency (package = ...) and a forward through its alias are found', () => {
  assert.deepEqual(enablingLines('[dependencies]\ngraph = { package = "paddock-repo-graph", path = "x", features = ["cargo-metadata"] }\n'), [2]);
  assert.deepEqual(enablingLines('[dependencies.graph]\npackage = "paddock-repo-graph"\nfeatures = ["cargo-metadata"]\n'), [3]);
  const forward = '[dependencies]\ngraph = { package = "paddock-repo-graph", path = "x" }\n\n[features]\nmeta = ["graph/cargo-metadata"]\n';
  assert.deepEqual(enablingLines(forward), [5]);
  assert.deepEqual(enablingLines('[features]\nmeta = ["other/cargo-metadata"]\n'), []);
});

test('a # inside a string is not a comment, and a commented line is ignored', () => {
  assert.equal(stripComment('a = "x # y" # c'), 'a = "x # y" ');
  assert.equal(stripComment("a = 'x # y' # c"), "a = 'x # y' ");
  assert.equal(stripComment('a = "q\\" # still string" # c'), 'a = "q\\" # still string" ');
  const text = '[dependencies]\npaddock-repo-graph = { path = "a#b", features = ["cargo-metadata"] }\n';
  assert.deepEqual(enablingLines(text), [2], 'the hash in the path does not hide the rest of the line');
  assert.deepEqual(enablingLines('# paddock-repo-graph = { features = ["cargo-metadata"] }\n'), []);
  assert.deepEqual(enablingLines('[dependencies]\npaddock-repo-graph = { path = "x", features = ["a"] } # "cargo-metadata"\n'), []);
});

test('other crates with the same feature name, and other features, are not flagged', () => {
  assert.deepEqual(enablingLines('[dependencies]\nother = { path = "x", features = ["cargo-metadata"] }\n'), []);
  assert.deepEqual(enablingLines('[dependencies.other]\nfeatures = ["cargo-metadata"]\n'), []);
  assert.deepEqual(enablingLines('[dependencies.paddock-repo-graph]\nfeatures = ["git-history"]\n[dependencies.x]\nfeatures = ["cargo-metadata"]\n'), []);
});

test('the reader helpers', () => {
  assert.deepEqual(splitKey('a."b.c".d'), ['a', 'b.c', 'd']);
  assert.deepEqual(entries('[t]\nk = [\n 1,\n 2\n]\n'), [{ table: ['t'], key: ['k'], value: '[ 1, 2 ]', line: 2 }]);
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
