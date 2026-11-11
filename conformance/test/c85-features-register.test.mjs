import assert from 'node:assert/strict';
import { test } from 'node:test';
import { parseToml } from '../src/util/mini-toml.mjs';
import { judge, sources, testExists, untilState, vmStepIds } from '../src/checks/C85-features-register.mjs';
import { findingsOf, makeApp } from './helpers.mjs';

const now = new Date('2026-11-12T12:00:00Z');
const NAV = `export const NAV = [\n  { id: 'models', label: 'Models' },\n  { id: 'chat', label: 'Chat' },\n];\n`;
const CONTRIB = `const a = DIALOG_CONTRACTS['pw.quit'];\ndefineSettingsPage({\n    id: 'pw.general',\n  });\ndefineSettingsPage({ id: 'pw.chats' });\n`;
const RS = `fn spec(kind: &'static str) {}\nfn a() { let s = spec("load-model", "t"); let o = os_prompt_spec(\n  "install-service",\n  "x"); }\n#[cfg(test)]\nmod t { fn x() { spec("only-in-tests"); } }\n`;
const CMDS = `export const APP_COMMANDS = {\n  overview: { kind: 'query', windows: [] },\n  start_load_model: { kind: 'mutation', windows: [] },\n};\n`;
const SPEC = `test('loads a model and reads it back', () => {});\n`;
const APP_FILES = {
  'app/src/app/shell/nav.ts': NAV,
  'app/src/app/shell/contributions.ts': CONTRIB,
  'app/src-tauri/src/app_commands.rs': RS,
  'app/src/app/generated/app-commands.ts': CMDS,
  'app/e2e-tauri/models.e2e.ts': SPEC,
};
const GOOD = `[[feature]]
id = "models.load"
title = "Load a model"
surfaces = ["route:/models", "route:/chat", "dialog:pw.quit", "settings:pw.general", "settings:pw.chats", "job:load-model", "job:install-service"]
wires = ["app-command:overview", "app-command:start_load_model"]
platforms = ["macos", "windows", "linux"]
tests = { tauri = "app/e2e-tauri/models.e2e.ts#loads a model and reads it back", outcome = "app/e2e-tauri/models.e2e.ts#loads a model", layout = "app/e2e-tauri/models.e2e.ts#loads a model" }
`;
const build = (extra = {}, features = GOOD) => makeApp({ ...APP_FILES, 'features.toml': features, ...extra });
const msgs = (f) => f.map((x) => x.message);

test('the register reader takes strings, arrays over several lines, inline tables and comments, and names what it cannot read', () => {
  const r = parseToml('top = 1\n[[feature]]\nid = "x" # why\nsurfaces = [\n  "a", # one\n  "b",\n]\ntests = { tauri = "p#t", vm = ["windows:s"] }\n[caps]\nunlisted = 5\nbad line\nid2 = nope\n');
  assert.deepEqual(r.tables.feature[0].surfaces, ['a', 'b']);
  assert.deepEqual(r.tables.feature[0].tests, { tauri: 'p#t', vm: ['windows:s'] });
  assert.equal(r.tables.caps.unlisted, 5);
  assert.deepEqual(r.problems.map((p) => p.line), [11, 12]);
  assert.match(parseToml('a = "x"\na = "y"\n').problems[0].message, /set twice/);
});

test('C85 reads routes, dialogs, Settings pages, job kinds (not the test ones) and request kinds from the shell app layout', () => {
  const read = (f) => APP_FILES[f] ?? null;
  const found = Object.fromEntries(sources('app').map((s) => [s.kind, s.ids(read(s.file))]));
  assert.deepEqual(found.route, ['route:/models', 'route:/chat']);
  assert.deepEqual(found.dialog, ['dialog:pw.quit']);
  assert.deepEqual(found.settings, ['settings:pw.general', 'settings:pw.chats']);
  assert.deepEqual(found.job, ['job:load-model', 'job:install-service']);
  assert.deepEqual(found['app-command'], ['app-command:overview', 'app-command:start_load_model']);
});

test('a complete register of an app passes with no finding', async () => {
  assert.deepEqual(await findingsOf('C85', build(), { app: 'demo', options: { now } }), []);
});

test('C85 is report-only: every finding is a warning, and a missing register is one warning', async () => {
  const f = await findingsOf('C85', makeApp({ ...APP_FILES }), { options: { now } });
  assert.equal(f.length, 1);
  assert.equal(f[0].severity, 'warn');
  assert.match(f[0].message, /no features\.toml/);
});

test('a surface, a job kind and a request kind with no feature are found, and [[unlisted]] and [[unwired]] clear them', async () => {
  const thin = GOOD.replace('"route:/chat", ', '').replace(', "job:install-service"', '').replace(', "app-command:start_load_model"', '');
  const f = await findingsOf('C85', build({}, thin), { options: { now } });
  assert.deepEqual(msgs(f).map((m) => m.split(' ')[0] + ' ' + m.split(' ')[1]).sort(), ['job job:install-service', 'request kind', 'route route:/chat'].sort());
  const cleared = build({ 'wiring.toml': '[[unwired]]\nwire = "app-command"\nkind = "start_load_model"\nwhy = "later"\nuntil = "demo-v1.1.0"\n' }, `${thin}\n[[unlisted]]\nsurface = "route:/chat"\nwhy = "the shell owns it"\n\n[[unlisted]]\nsurface = "job:install-service"\nwhy = "the shell owns it"\n`);
  assert.deepEqual(await findingsOf('C85', cleared, { options: { now } }), []);
});

test('a stale surface and a stale request kind named by a row are found', async () => {
  const stale = GOOD.replace('"route:/chat"', '"route:/gone"').replace('"app-command:overview"', '"app-command:removed"');
  const m = msgs(await findingsOf('C85', build({}, stale), { options: { now } })).join('\n');
  assert.match(m, /names route:\/gone, which/);
  assert.match(m, /names app-command:removed, which/);
  assert.match(m, /route route:\/chat maps to no feature/);
  assert.match(m, /request kind app-command:overview belongs to no feature/);
});

test('a test that does not exist is found; one that exists is not; a Rust test is a fn name', () => {
  const read = (p) => ({ 'a.ts': "test('does it', () => {})", 'b.rs': '#[test]\nfn it_works() {}\n#[tokio::test(flavor = "x")]\n#[ignore]\nasync fn it_waits() {}\nfn helper() {}\n// fn fake_one' })[p] ?? null;
  assert.equal(testExists('a.ts#does it', read).ok, true);
  assert.match(testExists('a.ts#other', read).why, /no test titled/);
  assert.match(testExists('c.ts#x', read).why, /does not exist/);
  assert.equal(testExists('b.rs#it_works', read).ok, true);
  assert.equal(testExists('b.rs#it_waits', read).ok, true);
  assert.equal(testExists('b.rs#nothing', read).ok, false);
  assert.equal(testExists('b.rs#helper', read).ok, false, 'a helper fn is not a test');
  assert.equal(testExists('b.rs#fake_one', read).ok, false);
  assert.match(testExists('no-hash', read).why, /path#title/);
});

test('a row that names a missing test, or owes and names the same one, is found', async () => {
  const bad = GOOD.replace('loads a model and reads it back"', 'a title nobody wrote"');
  assert.match(msgs(await findingsOf('C85', build({}, bad), { options: { now } }))[0], /tests\.tauri names a test that does not exist/);
  const both = `${GOOD}owed = ["layout"]\nowed_why = "w"\nowed_until = "demo-v1.1.0"\n`;
  assert.match(msgs(await findingsOf('C85', build({}, both), { options: { now } }))[0], /names tests\.layout and also owes it/);
});

test('a missing kind of test needs an owed entry with a reason and a bounded date', () => {
  const read = (f) => (f === 'features.toml' ? text : APP_FILES[f] ?? null);
  const row = (extra) => `[[feature]]\nid = "x"\ntitle = "X"\nsurfaces = ["route:/models", "route:/chat", "dialog:pw.quit", "settings:pw.general", "settings:pw.chats", "job:load-model", "job:install-service"]\nwires = ["app-command:overview", "app-command:start_load_model"]\nplatforms = ["macos", "windows", "linux"]\n${extra}\n`;
  let text;
  const run = (extra) => ((text = row(extra)), judge({ app: 'demo', appDir: 'app', read, now, vmManifest: null }).map((f) => f.message));
  assert.equal(run('owed = ["tauri", "outcome", "layout"]\nowed_why = "no kit yet"\nowed_until = "demo-v1.1.0"').length, 0);
  assert.match(run('owed = ["tauri", "outcome", "layout"]\nowed_until = "demo-v1.1.0"').join('\n'), /gives no `owed_why`/);
  assert.match(run('owed = ["tauri", "outcome", "layout"]\nowed_why = "w"').join('\n'), /names no `owed_until`/);
  assert.match(run('owed = ["tauri", "outcome", "layout"]\nowed_why = "w"\nowed_until = "2026-11-01"').join('\n'), /has passed/);
  assert.match(run('owed = ["tauri", "outcome", "layout"]\nowed_why = "w"\nowed_until = "2027-06-01"').join('\n'), /more than 60 days/);
  assert.match(run('owed = ["tauri", "outcome", "layout", "vm:solaris"]\nowed_why = "w"\nowed_until = "demo-v1.1.0"').join('\n'), /"vm:solaris" is not one of/);
  const none = run('').join('\n');
  for (const k of ['tauri', 'outcome', 'layout']) assert.match(none, new RegExp(`no tests\\.${k}: neither a test nor`));
  assert.deepEqual([untilState('2026-12-01', 'demo', now).ok, untilState('demo-v1.1.0', 'demo', now).ok, untilState('pitwall-v1.1.0', 'demo', now).ok], [true, true, false]);
});

test('a feature that depends on a platform needs a vm step (or an owed entry) for each OS it lists, Windows included', () => {
  const manifest = "const A = row('helper-smart', /x/), row('cold-start', /y/);";
  assert.deepEqual([...vmStepIds(manifest)], ['helper-smart', 'cold-start']);
  const base = (platforms, wires, tests, extra = '') =>
    `[[feature]]\nid = "x"\ntitle = "X"\nsurfaces = ["route:/models", "route:/chat", "dialog:pw.quit", "settings:pw.general", "settings:pw.chats", "job:load-model", "job:install-service"]\nwires = ${wires}\nplatforms = ${platforms}\ntests = ${tests}\n${extra}\n`;
  const T = 'app/e2e-tauri/models.e2e.ts#loads a model and reads it back';
  const tests = (vm) => `{ tauri = "${T}", outcome = "${T}", layout = "${T}"${vm ? `, vm = ${vm}` : ''} }`;
  const run = (text) => judge({ app: 'demo', appDir: 'app', read: (f) => (f === 'features.toml' ? text : APP_FILES[f] ?? null), now, vmManifest: manifest }).map((f) => f.message).filter((m) => !/maps to no feature|belongs to no feature/.test(m));
  const all = '["macos", "windows", "linux"]';
  // all three OS, no privileged wire: the real-engine test is enough.
  assert.deepEqual(run(base(all, '[]', tests())), []);
  // a privileged wire: a step per OS.
  const priv = run(base(all, '["agent-helper:Smart"]', tests('["macos:helper-smart"]'))).join('\n');
  assert.match(priv, /no vm-smoke step for windows/);
  assert.match(priv, /no vm-smoke step for linux/);
  const fix = judge({ app: 'demo', appDir: 'app', read: (f) => (f === 'features.toml' ? base(all, '["agent-helper:Smart"]', tests()) : APP_FILES[f] ?? null), now, vmManifest: manifest }).find((f) => /for windows/.test(f.message)).fix;
  assert.match(fix, /Windows is never left out/);
  assert.doesNotMatch(priv, /for macos/);
  // a step that exists on the OS passes; an owed entry covers the rest.
  assert.deepEqual(run(base('["windows"]', '[]', tests('["windows:helper-smart"]'))), []);
  assert.deepEqual(run(base('["windows", "macos"]', '[]', tests('["windows:cold-start"]'), 'owed = ["vm:macos"]\nowed_why = "w"\nowed_until = "demo-v1.1.0"')), []);
  // a step id that does not exist, and a step for an OS the row does not list.
  assert.match(run(base('["windows"]', '[]', tests('["windows:nope"]'))).join('\n'), /does not exist in scripts\/vm-smoke\/manifest\.mjs/);
  assert.match(run(base('["windows"]', '[]', tests('["windows:cold-start", "macos:cold-start"]'))).join('\n'), /for an OS the feature does not list/);
});

test('more [[unlisted]] rows than the cap, and an unlisted row with no reason, are found', async () => {
  const rows = (n) => Array.from({ length: n }, (_, i) => `[[unlisted]]\nsurface = "other:${i}"\nwhy = "w"\n`).join('\n');
  assert.match(msgs(await findingsOf('C85', build({}, `${GOOD}\n${rows(6)}`), { options: { now } }))[0], /6 \[\[unlisted\]\] rows exceed the cap of 5/);
  assert.match(msgs(await findingsOf('C85', build({ 'wiring.toml': '[caps]\nunlisted = 1\n' }, `${GOOD}\n${rows(2)}`), { options: { now } }))[0], /exceed the cap of 1/);
  assert.match(msgs(await findingsOf('C85', build({}, `${GOOD}\n[[unlisted]]\nsurface = "x"\n`), { options: { now } }))[0], /needs `surface` and a non-empty `why`/);
});

test('a source the app lacks is "not measured", and a reader that finds nothing is a failed positive control', async () => {
  const noNav = makeApp({ ...APP_FILES, 'app/src/app/shell/nav.ts': 'export const NAV = [];\n', 'features.toml': GOOD });
  assert.match(msgs(await findingsOf('C85', noNav, { options: { now } })).join('\n'), /check broken: no route was found/);
  const files = { ...APP_FILES, 'features.toml': GOOD };
  delete files['app/src-tauri/src/app_commands.rs'];
  assert.match(msgs(await findingsOf('C85', makeApp(files), { options: { now } })).join('\n'), /not measured: app\/src-tauri\/src\/app_commands\.rs does not exist/);
});

test('a malformed register line is reported with its line, never skipped', async () => {
  const f = await findingsOf('C85', build({}, `${GOOD}\nthis is not toml\n`), { options: { now } });
  assert.match(msgs(f)[0], /features\.toml: cannot read this line/);
  assert.equal(f[0].line > 1, true);
});
