import assert from 'node:assert/strict';
import { test } from 'node:test';
import { hakariPackage, namesPackage, packageName, workspaceMembers, workspaceTests } from '../src/checks/C37-test-binaries.mjs';
import { findingsOf, makeApp, messages } from './helpers.mjs';

const pkg = (name, deps = '') => `[package]\nname = "${name}"\nversion = "0.1.0"\n\n[dependencies]\n${deps}`;
const HACK = 'ws-hack = { version = "0.1", path = "../ws-hack" }\n';
const ci = (body) => ({ '.gitea/workflows/ci.yml': `jobs:\n  crate:\n    steps:\n${body}` });
const step = (cmd) => `      - run: ${cmd}\n`;
const config = (extra) => ({ 'paddock.conformance.json': JSON.stringify({ app: 'demo', ...extra }) });

/** A two-crate workspace in crates/ with cargo-hakari; `over` replaces or adds files. */
const workspace = (over = {}) => ({
  'crates/Cargo.toml': '[workspace]\nresolver = "3"\nmembers = [\n    "alpha", # the first\n    "beta",\n    "ws-hack",\n]\n\n[workspace.package]\nedition = "2024"\n',
  'crates/.config/hakari.toml': 'hakari-package = "ws-hack"\ndep-format-version = "4"\n',
  'crates/ws-hack/Cargo.toml': pkg('ws-hack'),
  'crates/alpha/Cargo.toml': pkg('alpha', HACK),
  'crates/beta/Cargo.toml': pkg('beta', HACK),
  'crates/alpha/tests/it/main.rs': 'mod one;\n',
  'crates/alpha/tests/it/one.rs': '#[test]\nfn t() {}\n',
  ...ci(step('cargo nextest run -p alpha --all-features --profile ci') + step('cargo test -p beta --all-features')),
  ...config({ perCrateSteps: ['crates'] }),
  ...over,
});

test('the parsers read packages, workspaces, hakari settings and -p arguments', () => {
  assert.equal(packageName(pkg('alpha')), 'alpha');
  assert.equal(packageName('[workspace]\nmembers = []\n'), null);
  assert.deepEqual(workspaceMembers('[workspace]\nmembers = [\n  "a", # x\n  "b/*",\n]\n[profile.dev]\ndebug = 1\n'), ['a', 'b/*']);
  assert.equal(workspaceMembers(pkg('alone')), null);
  assert.deepEqual(workspaceMembers('[package]\nname = "app"\n\n[workspace]\n'), []);
  assert.equal(hakariPackage('# x\nhakari-package = "ws-hack"\n'), 'ws-hack');
  assert.ok(namesPackage('cargo nextest run -p alpha --profile ci', 'alpha'));
  assert.ok(namesPackage('cargo test --package=alpha', 'alpha'));
  assert.ok(!namesPackage('cargo test -p alpha-extra', 'alpha'));
});

test('C37 good: tests in tests/it, every member tested by its own step and on the workspace hack', async () => {
  assert.deepEqual(await findingsOf('C37', makeApp(workspace())), []);
});

test('C37 bad: a tests/<name>.rs beside tests/it is an error naming the move', async () => {
  const f = await findingsOf('C37', makeApp(workspace({ 'crates/alpha/tests/two.rs': '#[test]\nfn t() {}\n' })));
  assert.equal(f.length, 1);
  assert.equal(f[0].severity, 'error');
  assert.equal(f[0].file, 'crates/alpha/tests/two.rs');
  assert.match(f[0].fix, /crates\/alpha\/tests\/it\//);
});

test('C37: ownTestBinaries allows a file with a reason; no reason is an error, a stale entry warns', async () => {
  const extra = { 'crates/alpha/tests/installed.rs': '#[test]\nfn t() {}\n' };
  const ok = workspace({ ...extra, ...config({ perCrateSteps: ['crates'], ownTestBinaries: { 'crates/alpha/tests/installed.rs': 'installs the process-wide logger' } }) });
  assert.deepEqual(await findingsOf('C37', makeApp(ok)), []);
  const bare = workspace({ ...extra, ...config({ perCrateSteps: ['crates'], ownTestBinaries: { 'crates/alpha/tests/installed.rs': ' ' } }) });
  assert.match(messages(await findingsOf('C37', makeApp(bare))), /without a reason/);
  const stale = await findingsOf('C37', makeApp(workspace(config({ perCrateSteps: ['crates'], ownTestBinaries: { 'crates/alpha/tests/gone.rs': 'was here' } }))));
  assert.equal(stale.length, 1);
  assert.equal(stale[0].severity, 'warn');
  assert.match(stale[0].message, /gone\.rs, which does not exist/);
});

test('C37 bad: a member without a CI step, and one without the workspace hack', async () => {
  const noStep = await findingsOf('C37', makeApp(workspace(ci(step('cargo nextest run -p alpha --profile ci')))));
  assert.equal(noStep.length, 1);
  assert.equal(noStep[0].file, 'crates/beta/Cargo.toml');
  assert.match(noStep[0].message, /beta is a workspace member that no workflow tests/);
  const noHack = await findingsOf('C37', makeApp(workspace({ 'crates/beta/Cargo.toml': pkg('beta') })));
  assert.equal(noHack.length, 1);
  assert.match(noHack[0].message, /beta does not depend on ws-hack/);
});

test('C37: a --workspace test step covers every member unless the workspace is in perCrateSteps', async () => {
  const wide = workspace({ ...ci(step('cargo nextest run --workspace --profile ci')), ...config({}) });
  assert.deepEqual(await findingsOf('C37', makeApp(wide)), []);
  const strict = workspace(ci(step('cargo nextest run --workspace --profile ci')));
  assert.equal((await findingsOf('C37', makeApp(strict))).length, 2, 'perCrateSteps: crates needs a step per member');
});

test('C37: a --workspace step does not cover a member it excludes', async () => {
  assert.ok(workspaceTests('cargo nextest run --workspace --all-features', 'beta'));
  assert.ok(!workspaceTests('cargo nextest run --workspace --exclude beta', 'beta'));
  assert.ok(!workspaceTests('cargo nextest run --workspace --exclude=beta', 'beta'));
  assert.ok(workspaceTests('cargo nextest run --workspace --exclude beta-extra', 'beta'));
  assert.ok(!workspaceTests('cargo nextest run -p beta', 'beta'));
  const excluded = await findingsOf('C37', makeApp(workspace({ ...ci(step('cargo nextest run --workspace --exclude beta --profile ci')), ...config({}) })));
  assert.equal(excluded.length, 1);
  assert.match(excluded[0].message, /beta is a workspace member that no workflow tests/);
});

test('C37: globbed members, a workspace without hakari, the hack crate itself, and a single crate at the root', async () => {
  const glob = {
    'Cargo.toml': '[workspace]\nmembers = ["plugins/*"]\n',
    'plugins/a/Cargo.toml': pkg('plug-a'),
    'plugins/b/Cargo.toml': pkg('plug-b'),
    ...ci(step('cargo test -p plug-a')),
  };
  const f = await findingsOf('C37', makeApp(glob));
  assert.equal(f.length, 1, 'no hakari: only the missing step');
  assert.match(f[0].message, /plug-b is a workspace member that no workflow tests/);
  const single = { 'Cargo.toml': pkg('solo'), 'tests/one.rs': '#[test]\nfn t() {}\n' };
  assert.match(messages(await findingsOf('C37', makeApp(single))), /tests\/one\.rs is a test binary of its own/);
});
