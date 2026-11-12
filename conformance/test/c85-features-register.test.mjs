import assert from 'node:assert/strict';
import { execFileSync } from 'node:child_process';
import { readFileSync, writeFileSync } from 'node:fs';
import { join } from 'node:path';
import { test } from 'node:test';
import { parseToml } from '../src/checks/lib/mini-toml.mjs';
import { judge, owedSummary, parseDate, rustProduction, sources, testExists, untilState, vmStepIds } from '../src/checks/C85-features-register.mjs';
import C85 from '../src/checks/C85-features-register.mjs';
import { main } from '../src/cli.mjs';
import { findingsOf, makeApp } from './helpers.mjs';

const now = new Date('2026-11-12T12:00:00Z');
const NAV = `export const NAV = [\n  { id: 'models', label: 'Models' },\n  { id: 'chat', label: 'Chat' },\n];\n`;
const CONTRIB = `const a = DIALOG_CONTRACTS['pw.quit'];\ndefineSettingsPage({\n    id: 'pw.general',\n  });\ndefineSettingsPage({ id: 'pw.chats' });\n`;
const RS = `fn spec(kind: &'static str) {}\nfn a() { let s = spec("load-model", "t"); let o = os_prompt_spec(\n  "install-service",\n  "x"); }\n#[cfg(test)]\nmod t { fn x() { spec("only-in-tests"); } }\nfn later() { let j = JobSpec { kind: "after-tests", ..d() }; }\n#[cfg(test)]\nfn helper() { spec("second-test-item"); }\n`;
const CMDS = `export const APP_COMMANDS = {\n  overview: { kind: 'query', windows: [] },\n  start_load_model: { kind: 'mutation', windows: [] },\n};\n`;
const SPEC = `test('loads a model and reads it back', () => {});\n`;
const CONTRACTS = `export const DIALOG_CONTRACTS = {\n  'pw.quit': {\n    id: 'pw.quit',\n    kind: 'decision',\n  },\n};\n`;
const APP_FILES = {
  'app/src/app/shell/nav.ts': NAV,
  'app/src/app/shell/contributions.ts': CONTRIB,
  'app/src-tauri/src/app_commands.rs': RS,
  'app/src/app/generated/app-commands.ts': CMDS,
  'app/src/app/generated/dialog-contracts.ts': CONTRACTS,
  'app/e2e-tauri/models.e2e.ts': SPEC,
};
const GOOD = `[[feature]]
id = "models.load"
title = "Load a model"
surfaces = ["route:/models", "route:/chat", "dialog:pw.quit", "settings:pw.general", "settings:pw.chats", "job:load-model", "job:install-service", "job:after-tests"]
wires = ["app-command:overview", "app-command:start_load_model"]
platforms = ["macos", "windows", "linux"]
tests = { tauri = "app/e2e-tauri/models.e2e.ts#loads a model and reads it back", outcome = "app/e2e-tauri/models.e2e.ts#loads a model", layout = "app/e2e-tauri/models.e2e.ts#loads a model" }
`;
const build = (extra = {}, features = GOOD) => makeApp({ ...APP_FILES, 'features.toml': features, ...extra });
const msgs = (f) => f.map((x) => x.message);

test('the register reader takes strings, arrays over several lines, inline tables and comments, and names what it cannot read', () => {
  const r = parseToml('top = 1\n[[feature]]\nid = "x" # why\nsurfaces = [\n  "a", # one\n  "b",\n]\ntests = { tauri = "p#t", vm = ["windows:s"] }\n[caps]\nunlisted = 5\nbad line\nid2 = nope\n');
  assert.deepEqual(r.tables.feature[0].surfaces, ['a', 'b']);
  assert.deepEqual({ ...r.tables.feature[0].tests }, { tauri: 'p#t', vm: ['windows:s'] });
  assert.equal(r.tables.caps.unlisted, 5);
  assert.deepEqual(r.problems.map((p) => p.line), [11, 12]);
  assert.match(parseToml('a = "x"\na = "y"\n').problems[0].message, /set twice/);
});

test('C85 reads routes, dialogs, Settings pages, job kinds (not the test ones) and request kinds from the shell app layout', () => {
  const read = (f) => APP_FILES[f] ?? null;
  const found = Object.fromEntries(sources('app').map((s) => [s.kind, s.ids(read(s.file)).map((x) => x.name)]));
  assert.deepEqual(found.route, ['route:/models', 'route:/chat']);
  assert.deepEqual(found.dialog, ['dialog:pw.quit']);
  assert.deepEqual(found.settings, ['settings:pw.general', 'settings:pw.chats']);
  assert.deepEqual(found.job, ['job:load-model', 'job:install-service', 'job:after-tests'], 'a JobSpec literal after a test module is read; every #[cfg(test)] item is blanked, not only the first');
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
  const row = (extra) => `[[feature]]\nid = "x"\ntitle = "X"\nsurfaces = ["route:/models", "route:/chat", "dialog:pw.quit", "settings:pw.general", "settings:pw.chats", "job:load-model", "job:install-service", "job:after-tests"]\nwires = ["app-command:overview", "app-command:start_load_model"]\nplatforms = ["macos", "windows", "linux"]\n${extra}\n`;
  let text;
  const TAGS = ['demo-v1.0.0', 'demo-v1.0.3'];
  const run = (extra, tags = TAGS) => ((text = row(extra)), judge({ app: 'demo', appDir: 'app', read, now, vmManifest: null, tags }).map((f) => f.message));
  assert.equal(run('owed = ["tauri", "outcome", "layout"]\nowed_why = "no kit yet"\nowed_until = "demo-v1.1.0"').length, 0);
  assert.match(run('owed = ["tauri", "outcome", "layout"]\nowed_until = "demo-v1.1.0"').join('\n'), /gives no `owed_why`/);
  assert.match(run('owed = ["tauri", "outcome", "layout"]\nowed_why = "w"').join('\n'), /names no `owed_until`/);
  assert.match(run('owed = ["tauri", "outcome", "layout"]\nowed_why = "w"\nowed_until = "2026-11-01"').join('\n'), /has passed/);
  assert.match(run('owed = ["tauri", "outcome", "layout"]\nowed_why = "w"\nowed_until = "2027-06-01"').join('\n'), /more than 60 days/);
  assert.match(run('owed = ["tauri", "outcome", "layout", "vm:solaris"]\nowed_why = "w"\nowed_until = "demo-v1.1.0"').join('\n'), /"vm:solaris" is not one of/);
  const none = run('').join('\n');
  for (const k of ['tauri', 'outcome', 'layout']) assert.match(none, new RegExp(`no tests\\.${k}: neither a test nor`));
  assert.deepEqual([untilState('2026-12-01', 'demo', now).ok, untilState('demo-v1.1.0', 'demo', now, { tags: TAGS }).ok, untilState('pitwall-v1.1.0', 'demo', now, { tags: TAGS }).ok], [true, true, false]);
});

test('a feature that depends on a platform needs a vm step (or an owed entry) for each OS it lists, Windows included', () => {
  const manifest = "const A = row('helper-smart', /x/), row('cold-start', /y/);";
  assert.deepEqual([...vmStepIds(manifest)], ['helper-smart', 'cold-start']);
  const base = (platforms, wires, tests, extra = '') =>
    `[[feature]]\nid = "x"\ntitle = "X"\nsurfaces = ["route:/models", "route:/chat", "dialog:pw.quit", "settings:pw.general", "settings:pw.chats", "job:load-model", "job:install-service", "job:after-tests"]\nwires = ${wires}\nplatforms = ${platforms}\ntests = ${tests}\n${extra}\n`;
  const T = 'app/e2e-tauri/models.e2e.ts#loads a model and reads it back';
  const tests = (vm) => `{ tauri = "${T}", outcome = "${T}", layout = "${T}"${vm ? `, vm = ${vm}` : ''} }`;
  const run = (text) => judge({ app: 'demo', appDir: 'app', read: (f) => (f === 'features.toml' ? text : APP_FILES[f] ?? null), now, vmManifest: manifest, tags: ['demo-v1.0.3'] }).map((f) => f.message).filter((m) => !/maps to no feature|belongs to no feature/.test(m));
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

const quiet = { log() {}, err() {} };
const exitOf = (root, ...more) => main(['--app', 'demo', '--root', root, '--only', 'C85', ...more], quiet);

test('a source the app lacks is "not measured" (exit 6), and a reader that finds nothing is "check broken" (exit 7): never a pass', async () => {
  const noNav = makeApp({ ...APP_FILES, 'app/src/app/shell/nav.ts': 'export const NAV = [];\n', 'features.toml': GOOD });
  const broken = await findingsOf('C85', noNav, { options: { now } });
  assert.match(msgs(broken).join('\n'), /check broken: no route was found/);
  assert.deepEqual([broken[0].severity, broken[0].unwaivable, broken[0].outcome], ['error', true, 'check-broken']);
  assert.equal(await exitOf(noNav), 7);
  const files = { ...APP_FILES, 'features.toml': GOOD };
  delete files['app/src-tauri/src/app_commands.rs'];
  const missing = makeApp(files);
  const f = await findingsOf('C85', missing, { options: { now } });
  assert.match(msgs(f).join('\n'), /not measured: app\/src-tauri\/src\/app_commands\.rs does not exist/);
  assert.deepEqual([f[0].severity, f[0].unwaivable, f[0].outcome], ['error', true, 'not-measured']);
  assert.equal(await exitOf(missing), 6);
  // nav.ts deleted altogether
  const noFile = { ...APP_FILES, 'features.toml': GOOD };
  delete noFile['app/src/app/shell/nav.ts'];
  assert.equal(await exitOf(makeApp(noFile)), 6);
  // a clean app exits 0
  assert.equal(await exitOf(build()), 0);
});

test('a vm step with no manifest to look it up in is "not measured" once (exit 6), not a pass', async () => {
  const vmRow = GOOD.replace('platforms = ["macos", "windows", "linux"]', 'platforms = ["windows"]').replace('layout = "app/e2e-tauri/models.e2e.ts#loads a model" }', 'layout = "app/e2e-tauri/models.e2e.ts#loads a model", vm = ["windows:a", "windows:b"] }');
  const f = judge({ app: 'demo', appDir: 'app', read: (p) => (p === 'features.toml' ? vmRow : APP_FILES[p] ?? null), now, vmManifest: null });
  const nm = f.filter((x) => x.outcome === 'not-measured');
  assert.equal(nm.length, 1);
  assert.match(nm[0].message, /manifest\.mjs was not read/);
  const root = build({ 'paddock-root/none.txt': '' }, vmRow);
  assert.equal(await exitOf(root, '--paddock', join(root, 'paddock-root')), 6);
});

test('a malformed register line is reported with its line, never skipped', async () => {
  const f = await findingsOf('C85', build({}, `${GOOD}\nthis is not toml\n`), { options: { now } });
  assert.match(msgs(f)[0], /features\.toml: cannot read this line/);
  assert.equal(f[0].line > 1, true);
});

// ---- owed bounds (P1-2) ----------------------------------------------------------------------------------------------------
const TAGS = ['demo-v1.0.0', 'demo-v1.0.2', 'demo-v1.0.3'];
const owedRow = (until, why = 'w') => `${GOOD}owed = ["layout"]\nowed_why = "${why}"\nowed_until = "${until}"\n`.replace(', layout = "app/e2e-tauri/models.e2e.ts#loads a model" }', ' }');
const owedRun = (until, opts = {}) =>
  judge({ app: 'demo', appDir: 'app', read: (f) => (f === 'features.toml' ? owedRow(until) : APP_FILES[f] ?? null), now, vmManifest: null, tags: TAGS, ...opts });
const owedMsgs = (until, opts) => owedRun(until, opts).map((f) => f.message).join('\n');

test('owed_until: a tag that already exists, an older one, and one past the next release line are findings; the next line is fine', () => {
  assert.equal(owedRun('demo-v1.1.0').length, 0);
  assert.equal(owedRun('demo-v1.0.4').length, 0, 'a patch after the latest tag is inside the current line');
  assert.match(owedMsgs('demo-v1.0.3'), /is not past the latest tag demo-v1\.0\.3: its tests were due/);
  assert.match(owedMsgs('demo-v1.0.1'), /its tests were due/);
  assert.match(owedMsgs('demo-v1.2.0'), /past the next release line \(demo-v1\.1\.0\)/);
  assert.match(owedMsgs('demo-v9.9.9'), /past the next release line/);
  assert.match(owedMsgs('demo-v1.1.5'), /past the next release line/);
});

test('owed_until: tags that are not known make the run "not measured" (exit 6), never a pass', () => {
  for (const tags of [null, []]) {
    const f = owedRun('demo-v1.1.0', { tags });
    assert.equal(f.length, 1);
    assert.deepEqual([f[0].severity, f[0].unwaivable, f[0].outcome], ['error', true, 'not-measured']);
  }
  assert.equal(untilState('demo-v1.1.0', 'demo', now, { tags: [] }).kind, 'not-measured');
});

test('owed_until: a date is a real date, in the future, and at most 60 days after the entry was written', () => {
  assert.equal(parseDate('2026-02-30'), null);
  assert.equal(parseDate('2026-13-01'), null);
  assert.equal(parseDate('2026-00-10'), null);
  assert.ok(parseDate('2028-02-29'));
  assert.equal(parseDate('2027-02-29'), null);
  assert.match(owedMsgs('2026-02-30'), /is not a real date/);
  assert.match(owedMsgs('2026-13-45'), /is not a real date/);
  assert.match(owedMsgs('next week'), /neither demo-vX\.Y\.Z nor a date/);
  assert.match(owedMsgs('2026-11-01'), /has passed/);
  assert.match(owedMsgs('2026-11-12'), /is today/);
  assert.equal(owedRun('2026-12-31').length, 0);
  assert.match(owedMsgs('2027-01-20'), /more than 60 days after the entry was written \(2026-11-12\)/);
  // the 60 days count from the commit that wrote the entry, where git can say
  const old = () => new Date('2026-09-20T00:00:00Z');
  assert.match(owedMsgs('2026-12-31', { since: old }), /more than 60 days after the entry was written \(2026-09-20\)/);
  assert.equal(owedRun('2026-11-15', { since: old }).length, 0);
});

test('an expired or reached owed entry is a warning in a release run too: C85 is report-only and never blocks a tag (ADR 0052, user 2026-11-12)', () => {
  const warn = owedRun('demo-v1.0.3');
  assert.deepEqual(warn.map((f) => f.severity), ['warn']);
  const rel = owedRun('demo-v1.0.3', { release: true });
  assert.deepEqual(rel.map((f) => f.severity), ['warn']);
  assert.equal(rel[0].unwaivable, undefined);
  // and another gap in the register stays a warning in a release run
  const other = judge({ app: 'demo', appDir: 'app', read: (f) => (f === 'features.toml' ? GOOD.replace('title = "Load a model"\n', '') : APP_FILES[f] ?? null), now, vmManifest: null, tags: TAGS, release: true });
  assert.deepEqual(other.map((f) => f.severity), ['warn']);
});

test('the CLI reads the tags and the commit date from git: an owed entry is a warning (exit 0) in a branch run and with --version too', async () => {
  const git = (root, ...a) => execFileSync('git', ['-C', root, '-c', 'user.name=t', '-c', 'user.email=t@example.com', '-c', 'commit.gpgsign=false', ...a], { encoding: 'utf8', env: { ...process.env, GIT_CONFIG_GLOBAL: '/dev/null', GIT_CONFIG_SYSTEM: '/dev/null' } });
  const root = build({}, owedRow('demo-v1.0.3'));
  git(root, 'init', '-q');
  git(root, 'add', '-A');
  git(root, 'commit', '-q', '-m', 'fixture');
  git(root, 'tag', 'demo-v1.0.3');
  const r = (...more) => main(['--app', 'demo', '--root', root, '--only', 'C85', '--paddock', root, ...more], quiet);
  assert.equal(await r(), 0, 'report-only: the finding is a warning');
  assert.equal(await r('--strict', 'C85'), 1);
  assert.equal(await r('--version', 'demo-v1.0.3'), 0, 'a release run: the owed finding is reported, the tag is not blocked');
  // an entry owed for the next line is fine in both, and a date more than 60 days after its commit is not (the commit is today)
  writeFileSync(join(root, 'features.toml'), owedRow('demo-v1.1.0'));
  assert.equal(await r('--version', 'demo-v1.0.3'), 0);
  writeFileSync(join(root, 'features.toml'), owedRow('2099-01-01'));
  git(root, 'commit', '-q', '-am', 'far date');
  assert.equal(await r('--version', 'demo-v1.0.3'), 0);
});

test('C85 reads the app\'s tags and the entry\'s commit date through its options (pure inputs), and writes the owed summary', async () => {
  const f = await findingsOf('C85', build({}, owedRow('demo-v1.0.3')), { options: { now, paddock: '/nonexistent', tags: TAGS, since: () => null } });
  assert.match(msgs(f)[0], /is not past the latest tag/);
  assert.equal(f[0].severity, 'warn');
  const rel = await findingsOf('C85', build({}, owedRow('demo-v1.0.3')), { options: { now, paddock: '/nonexistent', tags: TAGS, version: 'demo-v1.1.0' } });
  assert.equal(rel[0].severity, 'warn');
});

test('the job summary line says how many tests are owed and the nearest expiry', async () => {
  const features = parseToml(`${owedRow('demo-v1.1.0')}\n${owedRow('2026-12-01').replace('id = "models.load"', 'id = "b"')}\n${owedRow('2026-11-20').replace('id = "models.load"', 'id = "c"')}`).tables.feature;
  assert.equal(owedSummary(features, 'demo'), 'C85 demo: owed 3 tests on 3 features; nearest expiry: 2026-11-20, demo-v1.1.0; not measured: panes, tray popovers, widget kinds');
  assert.equal(owedSummary([], 'demo'), 'C85 demo: owed 0 tests on 0 features; nearest expiry: none; not measured: panes, tray popovers, widget kinds');
  const file = join(makeApp({ 'x': '' }), 'summary.md');
  const before = process.env.GITHUB_STEP_SUMMARY;
  process.env.GITHUB_STEP_SUMMARY = file;
  try {
    await findingsOf('C85', build({}, owedRow('demo-v1.1.0')), { options: { now, paddock: '/nonexistent', tags: TAGS } });
  } finally {
    if (before === undefined) delete process.env.GITHUB_STEP_SUMMARY;
    else process.env.GITHUB_STEP_SUMMARY = before;
  }
  assert.match(readFileSync(file, 'utf8'), /C85 demo: owed 1 tests on 1 features; nearest expiry: demo-v1\.1\.0/);
});

// ---- the smaller items ------------------------------------------------------------------------------------------------------
test('na with a reason stands in for a kind of test that does not apply; without a reason, or beside a test, it is a finding', () => {
  const T = 'app/e2e-tauri/models.e2e.ts#loads a model and reads it back';
  const row = (extra, tests = `{ tauri = "${T}" }`) => GOOD.replace(/^tests = .*$/m, `tests = ${tests}`) + extra;
  const run = (text) => judge({ app: 'demo', appDir: 'app', read: (f) => (f === 'features.toml' ? text : APP_FILES[f] ?? null), now, vmManifest: null, tags: TAGS }).map((f) => f.message).join('\n');
  assert.equal(run(row('na = ["outcome", "layout"]\nna_why = "no UI"\n')), '');
  assert.match(run(row('na = ["outcome", "layout"]\n')), /gives no `na_why`/);
  assert.match(run(row('na = ["layout", "vm:macos"]\nna_why = "x"\n')), /na "vm:macos" is not one of/);
  assert.match(run(row('na = ["tauri", "outcome", "layout"]\nna_why = "x"\n')), /names tests\.tauri and also marks it not applicable/);
  assert.match(run(row('owed = ["outcome"]\nowed_why = "w"\nowed_until = "demo-v1.1.0"\nna = ["outcome", "layout"]\nna_why = "x"\n')), /owes tests\.outcome and also marks it not applicable/);
  assert.match(run(row('')), /no tests\.outcome: neither a test nor/);
});

test('a TypeScript test must be a live test( or it( call that starts with the title; .skip, .fixme, a comment and a string do not count', () => {
  const read = (p) =>
    ({
      'a.ts': "test('does it', () => {});\ntest.only(\"quoted \\\"x\\\"\", () => {});\nit(`tpl ${n}`, () => {});\ntest(\"what's up\", () => {});\ntest(`the default popup fits 520 px: ${s}`, () => {});",
      'b.ts': "test.skip('skipped one', () => {});\ntest.fixme('fixme one', () => {});\n// test('commented', () => {})\nconst s = \"test('in a string'\";\nconst title = 'mentioned: does it';\ntestHelper('helper title', 1);",
      'c.txt': 'test("x")',
    })[p] ?? null;
  assert.equal(testExists('a.ts#does it', read).ok, true);
  assert.equal(testExists('a.ts#tpl ', read).ok, true);
  assert.equal(testExists('a.ts#what\'s up', read).ok, true, 'a quote in the title');
  assert.equal(testExists('a.ts#the default popup fits 520 px without scrolling', read).ok, false);
  assert.equal(testExists('a.ts#the default popup fits 520 px', read).ok, true, 'a template-literal title with a variable is matched by its fixed start');
  assert.match(testExists('b.ts#skipped one', read).why, /is skipped/);
  assert.match(testExists('b.ts#fixme one', read).why, /is skipped/);
  assert.match(testExists('b.ts#commented', read).why, /no test titled/);
  assert.match(testExists('b.ts#in a string', read).why, /no test titled/);
  assert.match(testExists('b.ts#mentioned: does it', read).why, /no test titled/);
  assert.match(testExists('b.ts#helper title', read).why, /no test titled/);
  assert.match(testExists('c.txt#x', read).why, /not a file type/);
  assert.match(testExists('a.rs#bad name', () => '').why, /named by its fn name/);
});

test('a surface or wire kind that is not on the allow-list is a finding (a typo is not silently a new kind)', () => {
  const run = (text) => judge({ app: 'demo', appDir: 'app', read: (f) => (f === 'features.toml' ? text : APP_FILES[f] ?? null), now, vmManifest: null, tags: TAGS }).map((f) => f.message).join('\n');
  assert.match(run(GOOD.replace('"dialog:pw.quit"', '"dialg:pw.quit"')), /surface "dialg:pw\.quit" is not written kind:name with a kind of route, dialog, settings/);
  assert.match(run(GOOD.replace('"settings:pw.chats"', '"setting:pw.chats"')), /surface "setting:pw\.chats"/);
  assert.match(run(GOOD.replace('"app-command:overview"', '"app-comand:overview"')), /wire "app-comand:overview" is not written kind:name/);
  assert.match(run(GOOD.replace('"job:load-model"', '"job"')), /surface "job"/);
  assert.match(run(`${GOOD}\n[[unlisted]]\nsurface = "wat:x"\nwhy = "w"\n`), /surface "wat:x"/);
  assert.equal(run(GOOD.replace('"job:load-model"', '"pane:logs", "tray:popup", "widget:cpu", "http:/v1", "job:load-model"').replace('"app-command:overview"', '"privileged:Smart", "app-agent:Hardware", "app-command:overview"')).includes('is not written'), false);
});

test('a malformed supporting list is a finding, not skipped', () => {
  const run = (text) => judge({ app: 'demo', appDir: 'app', read: (f) => (f === 'features.toml' ? text : APP_FILES[f] ?? null), now, vmManifest: null, tags: TAGS }).map((f) => f.message).join('\n');
  assert.match(run(`${GOOD}supporting = "app/e2e-tauri/models.e2e.ts#loads a model"\n`), /`supporting` must be a list of "path#title" strings/);
  assert.match(run(`${GOOD}supporting = [1, 2]\n`), /`supporting` must be a list/);
  assert.equal(run(`${GOOD}supporting = ["app/e2e-tauri/models.e2e.ts#loads a model"]\n`), '');
});

test('the dialogs of the generated contracts are authoritative, and contributions.ts must define the same ones', () => {
  const run = (files) => judge({ app: 'demo', appDir: 'app', read: (f) => (f === 'features.toml' ? GOOD : files[f] ?? null), now, vmManifest: null, tags: TAGS }).map((f) => f.message).join('\n');
  const more = `${CONTRACTS}  'pw.extra': {\n    id: 'pw.extra',\n  },\n`;
  const m = run({ ...APP_FILES, 'app/src/app/generated/dialog-contracts.ts': more });
  assert.match(m, /dialog:pw\.extra is in app\/src\/app\/generated\/dialog-contracts\.ts but app\/src\/app\/shell\/contributions\.ts does not define it \(2 dialogs here, 1 there\)/);
  assert.match(m, /dialog dialog:pw\.extra maps to no feature/);
  const extra = run({ ...APP_FILES, 'app/src/app/shell/contributions.ts': `${CONTRIB}const b = DIALOG_CONTRACTS['pw.ghost'];\n` });
  assert.match(extra, /dialog:pw\.ghost is defined in app\/src\/app\/shell\/contributions\.ts but app\/src\/app\/generated\/dialog-contracts\.ts \(generated from Rust\) does not list it/);
  const f = judge({ app: 'demo', appDir: 'app', read: (p) => (p === 'features.toml' ? GOOD : { ...APP_FILES, 'app/src/app/generated/dialog-contracts.ts': undefined }[p] ?? null), now, vmManifest: null, tags: TAGS });
  assert.ok(f.some((x) => x.outcome === 'not-measured' && /dialog-contracts\.ts does not exist/.test(x.message)));
});

test('rustProduction blanks every #[cfg(test)] item (a mod, a fn, a use) and keeps the lines', () => {
  const src = '#[cfg(test)]\nuse x::y;\nfn a() { spec("one"); }\n#[cfg(test)]\n#[allow(dead_code)]\nmod t { fn f() { let s = "}"; let c = \'}\'; /* } */ spec("t1"); }\n  mod inner { } }\nfn b() { spec("two"); }\n#[cfg(test)]\nfn c() { spec("t2"); }\nfn d() { spec("three"); }\n';
  const prod = rustProduction(src);
  assert.equal(prod.split('\n').length, src.split('\n').length);
  assert.deepEqual([...prod.matchAll(/spec\("(\w+)"\)/g)].map((m) => m[1]), ['one', 'two', 'three']);
  assert.equal(rustProduction('fn a() {}'), 'fn a() {}');
});

test('a finding carries the line of the thing it is about', async () => {
  const thin = GOOD.replace('"route:/chat", ', '');
  const f = await findingsOf('C85', build({}, thin), { options: { now } });
  assert.equal(f[0].file, 'app/src/app/shell/nav.ts');
  assert.equal(f[0].line, 3, 'the chat route is on the third line of nav.ts');
  const stale = await findingsOf('C85', build({}, GOOD.replace('"route:/chat"', '"route:/gone"')), { options: { now } });
  const gone = stale.find((x) => /names route:\/gone/.test(x.message));
  assert.equal(gone.line, GOOD.split('\n').findIndex((l) => l.includes('route:/gone') || l.includes('surfaces =')) + 1);
});

test('na = ["tauri"] never hides the real-engine test: a warning in a branch run and in a release run', () => {
  const T = 'app/e2e-tauri/models.e2e.ts#loads a model and reads it back';
  const text = GOOD.replace(/^tests = .*$/m, `tests = { outcome = "${T}", layout = "${T}" }`) + 'na = ["tauri"]\nna_why = "no UI"\n';
  const run = (release) => judge({ app: 'demo', appDir: 'app', read: (f) => (f === 'features.toml' ? text : APP_FILES[f] ?? null), now, vmManifest: null, tags: TAGS, release });
  const branch = run(false).filter((f) => /na "tauri"/.test(f.message));
  assert.deepEqual(branch.map((f) => f.severity), ['warn']);
  assert.deepEqual(run(true).filter((f) => /na "tauri"/.test(f.message)).map((f) => f.severity), ['warn']);
  // and tauri is still missing: neither a test nor owed
  assert.ok(run(false).some((f) => /has no tests\.tauri: neither a test nor/.test(f.message)), 'na does not stand in for tauri');
});
