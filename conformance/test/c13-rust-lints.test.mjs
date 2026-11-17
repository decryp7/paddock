import assert from 'node:assert/strict';
import { execFileSync } from 'node:child_process';
import { readFileSync } from 'node:fs';
import { join } from 'node:path';
import { test } from 'node:test';
import { findingsOf, makeApp, messages, PADDOCK } from './helpers.mjs';
import { disallowedPaths, lintLevels, ratchetClippyNotYet, ratchetNotYet } from '../src/checks/C13-rust-lints.mjs';

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

test('C13 bad: an app that drops a Keychain method or type from its clippy.toml (ADR 0057, C99 layer 5)', async () => {
  const noTypes = good();
  noTypes['clippy.toml'] = preset('clippy.toml').split('\ndisallowed-types')[0].replace(/\n# ADR 0057 \(rule 72, C99\): the Keychain types[^\n]*(\n#[^\n]*)*$/, '\n');
  const m = messages(await run(noTypes));
  assert.match(m, /disallowed-types lacks keyring::Entry/);
  assert.match(m, /disallowed-types lacks security_framework::os::macos::keychain::SecKeychain/);
  assert.doesNotMatch(m, /disallowed-methods lacks/);
  const noMethod = good();
  noMethod['clippy.toml'] = noMethod['clippy.toml'].replace(/^.*"security_framework::passwords::get_generic_password".*\n/m, '');
  assert.match(messages(await run(noMethod)), /disallowed-methods lacks security_framework::passwords::get_generic_password/);
  assert.match(preset('clippy.toml'), /SecItemAdd/, 'the preset carries the objc2-security entries');
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
  assert.deepEqual(disallowedPaths('disallowed-methods = [\n  { path = "m::a" },\n]\n# { path = "x::y" }\ndisallowed-types = [\n  { path = "t::A", reason = "r" },\n]\n', 'disallowed-types'), ['t::A']);
});

const ci = (cargo) => JSON.stringify(cargo === undefined ? {} : { cargo });

test('C13 probe (ER-33): an app without Rust is "does not apply", an app whose workspace is src-tauri is read there', async () => {
  // Pacenotes and Sighter have no Rust yet: no finding, not "no workspace Cargo.toml"
  assert.deepEqual(await run({ 'paddock.ci.json': ci(undefined), 'README.md': 'x' }), []);
  // shell-demo and workbench-demo: "cargo": "src-tauri" in their paddock.ci.json
  const files = {};
  for (const [k, v] of Object.entries(good())) files[`src-tauri/${k}`] = v;
  files['paddock.ci.json'] = ci('src-tauri');
  assert.deepEqual(await run(files), []);
  // bad: the same app with a member that does not opt in names the file below src-tauri
  files['src-tauri/crates/b/Cargo.toml'] = '[package]\nname = "b"\n';
  assert.match(messages(await run(files)), /src-tauri\/crates\/b\/Cargo\.toml does not opt in/);
  // bad: a declared Cargo workspace whose manifest is not there is still a finding
  assert.match(messages(await run({ 'paddock.ci.json': ci('src-tauri'), 'README.md': 'x' })), /no workspace Cargo\.toml/);
});

const REGISTER = '[[ratchet]]\nid = "C13"\napp = "demo"\nnot_yet = ["xtask", "examples/x"]\nowner = "o"\ntarget = "demo-v9.0.0"\napproved = "user"\n';

test('C13 ratchet (ADR 0060 3.2): members in not_yet need no opt-in, a member in neither list fails', async () => {
  const files = { ...good(), 'paddock.ci.json': ci('.'), 'xtask/Cargo.toml': '[package]\nname = "xtask"\n', 'examples/x/Cargo.toml': '[package]\nname = "x"\n' };
  const withRegister = { options: { paddock: PADDOCK, gatesText: REGISTER } };
  assert.deepEqual(await findingsOf('C13', makeApp(files), withRegister), []);
  // bad: without the list the same two members fail, and a member added later is not exempt
  assert.match(messages(await findingsOf('C13', makeApp(files), opts)), /xtask\/Cargo\.toml does not opt in/);
  files['newcrate/Cargo.toml'] = '[package]\nname = "newcrate"\n';
  const m = messages(await findingsOf('C13', makeApp(files), withRegister));
  assert.match(m, /newcrate\/Cargo\.toml does not opt in/);
  assert.doesNotMatch(m, /xtask|examples/);
});

test('ratchetNotYet reads the entry of this check and app only', () => {
  assert.deepEqual(ratchetNotYet(REGISTER, 'demo'), ['xtask', 'examples/x']);
  assert.deepEqual(ratchetNotYet(REGISTER, 'other'), []);
  assert.deepEqual(ratchetNotYet(REGISTER.replace('id = "C13"', 'id = "C99"'), 'demo'), []);
  assert.deepEqual(ratchetNotYet('', 'demo'), []);
});

test('C13 ratchet: clippy paths listed in clippy_not_yet may be missing from clippy.toml, any other missing path still fails', async () => {
  const files = { ...good(), 'paddock.ci.json': ci('.') };
  files['clippy.toml'] = preset('clippy.toml').split('\n').filter((l) => !/std::env::temp_dir|reqwest::Client::new/.test(l)).join('\n');
  const reg = (list) => `[[ratchet]]\nid = "C13"\napp = "demo"\nnot_yet = []\nclippy_not_yet = [${list}]\nowner = "o"\ntarget = "demo-v9.0.0"\napproved = "user"\n`;
  // bad: no exemption, the missing paths fail
  assert.match(messages(await findingsOf('C13', makeApp(files), { options: { paddock: PADDOCK, gatesText: reg('') } })), /lacks std::env::temp_dir/);
  // good: exactly the exempt paths missing
  const exempt = reg('"std::env::temp_dir", "reqwest::Client::new"');
  assert.deepEqual(await findingsOf('C13', makeApp(files), { options: { paddock: PADDOCK, gatesText: exempt } }), []);
  // bad: a path outside the list is still required
  const m = messages(await findingsOf('C13', makeApp(files), { options: { paddock: PADDOCK, gatesText: reg('"std::env::temp_dir"') } }));
  assert.match(m, /lacks reqwest::Client::new/);
  assert.doesNotMatch(m, /lacks std::env::temp_dir/);
  assert.deepEqual(ratchetClippyNotYet(exempt, 'demo'), ['std::env::temp_dir', 'reqwest::Client::new']);
  assert.deepEqual(ratchetClippyNotYet(exempt, 'other'), []);
});

// ER-47 / rule 84: clearing a child's environment goes through paddock_env::clear_env, which keeps the restricting variables. The
// one lint over every clearing site is Clippy's ban on `Command::env_clear`; this test says the ban is in the preset and in every
// workspace's clippy.toml, and the positive control says C13 sees a copy that dropped it (so the ban cannot be lost quietly).
const BANNED = ['std::process::Command::env_clear', 'std::env::set_var', 'std::env::remove_var', 'std::env::vars'];

test('the preset and every workspace clippy.toml ban Command::env_clear, the process-wide env writes and std::env::vars', () => {
  const tracked = execFileSync('git', ['ls-files', '*clippy.toml'], { cwd: PADDOCK, encoding: 'utf8' }).split('\n').filter(Boolean);
  assert.ok(tracked.includes('lint/clippy.toml') && tracked.length >= 8, `too few clippy.toml files tracked: ${tracked}`);
  for (const f of tracked) {
    const have = new Set(disallowedPaths(readFileSync(join(PADDOCK, f), 'utf8')));
    for (const p of BANNED) assert.ok(have.has(p), `${f} lacks ${p}`);
  }
});

test('C13 positive control: an app clippy.toml that dropped the env_clear ban is reported', async () => {
  const files = good();
  files['clippy.toml'] = files['clippy.toml'].split('\n').filter((l) => !l.includes('std::process::Command::env_clear')).join('\n');
  assert.match(messages(await run(files)), /lacks std::process::Command::env_clear/);
});
