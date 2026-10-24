import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { join } from 'node:path';
import { test } from 'node:test';
import { findingsOf, makeApp, messages, PADDOCK } from './helpers.mjs';
import { disallowedPaths, lintLevels } from '../src/checks/C13-rust-lints.mjs';

const opts = { options: { paddock: PADDOCK } };
const preset = (f) => readFileSync(join(PADDOCK, 'lint', f), 'utf8');
const run = (files) => findingsOf('C13', makeApp(files), opts);

const goodRoot = `[workspace]\nmembers = ["crates/*"]\nexclude = ["fuzz"]\n\n${preset('workspace-lints.toml')}\n[profile.dev]\ndebug = 1\n`;
const member = '[package]\nname = "a-core"\n\n[lints]\nworkspace = true\n';
const good = () => ({
  'Cargo.toml': goodRoot,
  'clippy.toml': preset('clippy.toml').replace('use the app\'s process helper (paddock-proc)', 'use pitcrew_service::process::command'),
  'crates/a-core/Cargo.toml': member,
  'fuzz/Cargo.toml': '[package]\nname = "fuzz"\n',
});

test('C13 good: the preset itself, reworded reasons, extra lints and paths, stricter levels, excluded fuzz crate', async () => {
  assert.deepEqual(await run(good()), []);
  const more = good();
  more['Cargo.toml'] = goodRoot.replace('unsafe_code = "deny"', 'unsafe_code = "forbid"') + '\n[workspace.lints.clippy.extra]\n';
  more['clippy.toml'] += '\n# own\n';
  more['Cargo.toml'] = more['Cargo.toml'].replace('expect_used = "warn"', 'expect_used = { level = "deny", priority = 1 }\ndbg_macro = "warn"');
  assert.deepEqual(await run(more), []);
});

test('C13 bad: a missing lint, a weaker level, a crate that does not opt in, a dropped disallowed path', async () => {
  const bad = good();
  bad['Cargo.toml'] = goodRoot.replace('unwrap_used = "warn"\n', '').replace('unsafe_code = "deny"', 'unsafe_code = "warn"');
  bad['crates/b/Cargo.toml'] = '[package]\nname = "b"\n';
  bad['crates/c/Cargo.toml'] = '[package]\nname = "c"\nlints.workspace = true\n';
  bad['clippy.toml'] = 'disallowed-methods = [\n    { path = "std::process::Command::new", reason = "x" },\n]\n';
  const m = messages(await run(bad));
  assert.match(m, /lacks unwrap_used = "warn"/);
  assert.match(m, /unsafe_code = "warn" is weaker than the preset's "deny"/);
  assert.match(m, /crates\/b\/Cargo\.toml does not opt in/);
  assert.doesNotMatch(m, /crates\/c\//);
  assert.match(m, /lacks std::env::temp_dir/);
  assert.match(m, /lacks reqwest::Client::new/);
});

test('C13 bad: no clippy.toml, no workspace manifest, no lints table at all', async () => {
  const noClippy = good();
  delete noClippy['clippy.toml'];
  assert.match(messages(await run(noClippy)), /clippy\.toml is missing/);
  assert.match(messages(await run({ 'a.txt': 'x' })), /no workspace Cargo\.toml/);
  const bare = good();
  bare['Cargo.toml'] = '[workspace]\nmembers = []\n';
  assert.equal((await run(bare)).length, 5);
});

test('C13 warns without a Paddock checkout', async () => {
  const f = await findingsOf('C13', makeApp(good()), { options: {} });
  assert.deepEqual(f.map((x) => x.severity), ['warn']);
});

test('parsers: lint levels and disallowed paths', () => {
  assert.deepEqual(lintLevels(['a = "warn"', 'b = { level = "deny", priority = -1 }', '# c = "x"', 'd = true']), { a: 'warn', b: 'deny' });
  assert.deepEqual(disallowedPaths('# c\ndisallowed-methods = [\n  { path = "a::b", reason = "r" },\n  { path = "c::d" },\n]\nother = 1\n'), ['a::b', 'c::d']);
  assert.deepEqual(disallowedPaths('x = 1'), []);
});
