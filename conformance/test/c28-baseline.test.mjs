import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { join } from 'node:path';
import { test } from 'node:test';
import { baselineFiles, findingsOf, makeApp, messages, PADDOCK } from './helpers.mjs';
import { applySections, sectionDiffs, tomlTables } from '../src/sync/baseline.mjs';
import { main } from '../src/cli.mjs';

const opts = { options: { paddock: PADDOCK } };
const run = (files) => findingsOf('C28', makeApp(files), opts);

test('C28 good: an app repo equal to the templates', async () => {
  assert.deepEqual(await run(baselineFiles()), []);
});

test('C28 good: an app may keep its own tables, extra rules and Cargo content', async () => {
  const files = baselineFiles();
  files['deny.toml'] += '\n[extra]\nx = 1\n';
  files['Cargo.toml'] += '\n[profile.release]\nlto = true\n';
  files['renovate.json'] = JSON.stringify({ extends: ['local>decryp7/paddock//renovate/app'], packageRules: [{ matchPackageNames: ['x'], enabled: false }] });
  assert.deepEqual(await run(files), []);
});

test('C28 bad: a missing file, a drifted file, a drifted table, a missing preset', async () => {
  const files = baselineFiles();
  delete files['rustfmt.toml'];
  files['.editorconfig'] += 'x\n';
  files['deny.toml'] = files['deny.toml'].replace('"MIT",', '"MIT", "GPL-3.0",');
  files['Cargo.toml'] = files['Cargo.toml'].replace('opt-level = 2', 'opt-level = 3');
  files['renovate.json'] = '{"extends":["config:recommended"]}';
  const f = await run(files);
  const m = messages(f);
  assert.match(m, /rustfmt\.toml: the file is missing/);
  assert.match(m, /\.editorconfig: it differs/);
  assert.match(m, /deny\.toml: table\(s\) \[licenses\]/);
  assert.match(m, /Cargo\.toml: table\(s\) \[profile\.dev\.package\."\*"\]/);
  assert.match(m, /renovate\.json: "extends" lacks local>decryp7\/paddock\/\/renovate\/app/);
  assert.equal(f.length, 5);
  assert.ok(f.every((x) => x.severity === 'error' && /sync --write/.test(x.fix)));
});

test('C28 bad: renovate.json that is not JSON; a Cargo.toml without the tables', async () => {
  const files = baselineFiles();
  files['renovate.json'] = '{ nope';
  files['Cargo.toml'] = '[workspace]\nmembers = []\n';
  const m = messages(await run(files));
  assert.match(m, /not valid JSON/);
  assert.match(m, /Cargo\.toml: table\(s\)/);
});

test('C28 follows appDir and honours baselineSkip and waivers', async () => {
  const files = baselineFiles();
  files['ui/.nvmrc'] = files['app/.nvmrc'];
  files['ui/tsconfig.json'] = files['app/tsconfig.json'];
  delete files['app/.nvmrc'];
  delete files['app/tsconfig.json'];
  files['paddock.conformance.json'] = JSON.stringify({ appDir: 'ui' });
  assert.deepEqual(await run(files), []);
  const drift = baselineFiles();
  drift['lefthook.yml'] += '    privacy:\n';
  assert.equal((await run(drift)).length, 1);
  drift['paddock.conformance.json'] = JSON.stringify({ baselineSkip: ['lefthook.yml'] });
  assert.deepEqual(await run(drift), []);
  const waived = baselineFiles();
  waived['lefthook.yml'] += 'x';
  waived['paddock.waivers.toml'] = '[[waiver]]\ncheck = "C28"\npath = "lefthook.yml"\nreason = "privacy hook"\nexpires = "2099-01-01"\n';
  assert.deepEqual(await run(waived), []);
});

test('C28 warns, and does not pass, without a Paddock checkout', async () => {
  const f = await findingsOf('C28', makeApp(baselineFiles()), { options: {} });
  assert.equal(f.length, 1);
  assert.equal(f[0].severity, 'warn');
  assert.match(f[0].message, /--paddock/);
});

test('placeholders: the app name reaches cliff.toml and deny.toml', async () => {
  const files = baselineFiles('pitcrew');
  assert.match(files['cliff.toml'], /All notable changes to Pitcrew/);
  const wrong = { ...files, 'cliff.toml': files['cliff.toml'].replace('Pitcrew', 'Other') };
  assert.match(messages(await findingsOf('C28', makeApp(wrong), { app: 'pitcrew', ...opts })), /cliff\.toml/);
  assert.deepEqual(await findingsOf('C28', makeApp(files), { app: 'pitcrew', ...opts }), []);
});

test('toml table helpers: split, compare, replace and append', () => {
  const src = '[a]\nx = 1\n\n[b]\ny = 2\n';
  assert.deepEqual(tomlTables(src).tables.map((t) => t.header), ['[a]', '[b]']);
  assert.deepEqual(sectionDiffs('[a]\nx = 1\n[b]\ny = 3\n', src, ['a', 'b']), ['b']);
  assert.equal(applySections('[a]\nx = 9\n\n[c]\nz = 1\n', src, ['a', 'b']), '[a]\nx = 1\n\n[c]\nz = 1\n\n[b]\ny = 2\n');
  assert.deepEqual(sectionDiffs('', src, ['zzz']), []);
});

test('sync: reports differences, --write repairs them, a second run is clean', async () => {
  const files = baselineFiles();
  delete files['rustfmt.toml'];
  files['deny.toml'] = files['deny.toml'].replace('"ISC",', '');
  files['renovate.json'] = '{"extends":[],"packageRules":[{"matchPackageNames":["x"]}]}';
  const root = makeApp(files);
  const out = [];
  const io = { log: (s) => out.push(s), err: (s) => out.push(s) };
  const cli = (...a) => main(['sync', '--app', 'demo', '--root', root, '--paddock', PADDOCK, ...a], io);
  assert.equal(await cli(), 1);
  assert.equal(await cli('--write'), 0);
  assert.equal(await cli(), 0);
  assert.match(out.join('\n'), /in sync with Paddock/);
  assert.match(readFileSync(join(root, 'rustfmt.toml'), 'utf8'), /max_width = 140/);
  assert.match(readFileSync(join(root, 'deny.toml'), 'utf8'), /"ISC"/);
  const renovate = JSON.parse(readFileSync(join(root, 'renovate.json'), 'utf8'));
  assert.equal(renovate.extends[0], 'local>decryp7/paddock//renovate/app');
  assert.equal(renovate.packageRules.length, 1, 'the app rules are kept');
});

test('sync without a Paddock checkout is a usage error', async () => {
  const errs = [];
  const root = makeApp({ 'a.txt': 'x' });
  assert.equal(await main(['sync', '--app', 'demo', '--root', root, '--paddock', join(root, 'none')], { log() {}, err: (s) => errs.push(s) }), 2);
  assert.match(errs.join(), /no Paddock checkout/);
});
