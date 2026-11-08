// C99 layer 5 (ADR 0057): the Clippy preset must stop code that uses the Keychain API. A fixture crate calls stand-ins for
// keyring::Entry and security_framework::passwords::get_generic_password under the real lint/clippy.toml; Clippy has to
// fail on both and pass once they are gone (ER-33: the ban has shown it can see the thing).
import assert from 'node:assert/strict';
import { spawnSync } from 'node:child_process';
import { readFileSync, rmSync, writeFileSync } from 'node:fs';
import { join } from 'node:path';
import { after, test } from 'node:test';
import { makeApp, PADDOCK } from './helpers.mjs';

const hasClippy = spawnSync('cargo', ['clippy', '--version']).status === 0;
const pkg = (name, deps = '') => `[package]\nname = "${name}"\nversion = "0.1.0"\nedition = "2021"\n\n[dependencies]\n${deps}`;

const roots = [];
after(() => roots.forEach((r) => rmSync(r, { recursive: true, force: true })));
const env = { ...process.env, RUSTC_WRAPPER: '' };

function fixture(lib, config = readFileSync(join(PADDOCK, 'lint', 'clippy.toml'), 'utf8')) {
  const root = makeApp({
    'Cargo.toml': pkg('demo-app', 'keyring = { path = "vendor/keyring" }\nsecurity-framework = { path = "vendor/security-framework" }\n'),
    'src/lib.rs': lib,
    'vendor/keyring/Cargo.toml': pkg('keyring'),
    'vendor/keyring/src/lib.rs': 'pub struct Entry;\nimpl Entry { pub fn new() -> Entry { Entry } }\n',
    'vendor/security-framework/Cargo.toml': pkg('security-framework'),
    'vendor/security-framework/src/lib.rs': 'pub mod passwords { pub fn get_generic_password() -> i32 { 0 } }\n',
  });
  roots.push(root);
  writeFileSync(join(root, 'clippy.toml'), config);
  return root;
}
const clippy = (root) => spawnSync('cargo', ['clippy', '--offline', '--', '-D', 'warnings'], { cwd: root, encoding: 'utf8', env: { ...env, CARGO_TARGET_DIR: join(root, 'target') } });

test('C99 Clippy preset: the Keychain method and type are refused, clean code passes', { skip: hasClippy ? false : 'clippy is not installed here: run the conformance tests where cargo clippy is (CI image)' }, () => {
  const bad = clippy(fixture('pub fn a() -> i32 { security_framework::passwords::get_generic_password() }\npub fn b() -> keyring::Entry { keyring::Entry::new() }\n'));
  assert.notEqual(bad.status, 0, bad.stderr);
  assert.match(bad.stderr, /use of a disallowed method `security_framework::passwords::get_generic_password`/);
  assert.match(bad.stderr, /use of a disallowed type `keyring::Entry`/);
  assert.match(bad.stderr, /owner-only file/);
  const ok = clippy(fixture('pub fn a() -> i32 { 1 }\n'));
  assert.equal(ok.status, 0, ok.stderr);
});

// `allow-invalid = true` makes a mistyped path pass silently. The fixture resolves every path (stand-in crates), so the
// two entries are tried without it: the right path still bites, and a typo is itself an error.
test('C99 Clippy preset: without allow-invalid a right path bites and a mistyped path fails', { skip: hasClippy ? false : 'clippy is not installed here' }, () => {
  const lines = readFileSync(join(PADDOCK, 'lint', 'clippy.toml'), 'utf8').split('\n');
  const pick = (needle) => {
    const l = lines.find((x) => x.includes(needle));
    assert.ok(l, `the preset has ${needle}`);
    return l.replace('allow-invalid = true, ', '');
  };
  const method = pick('"security_framework::passwords::get_generic_password"');
  const type = pick('"keyring::Entry"');
  const config = (m) => `disallowed-methods = [\n${m}\n]\ndisallowed-types = [\n${type}\n]\n`;
  const lib = 'pub fn a() -> i32 { security_framework::passwords::get_generic_password() }\npub fn b() -> keyring::Entry { keyring::Entry::new() }\n';
  const right = clippy(fixture(lib, config(method)));
  assert.notEqual(right.status, 0, right.stderr);
  assert.match(right.stderr, /disallowed method `security_framework::passwords::get_generic_password`/);
  assert.match(right.stderr, /disallowed type `keyring::Entry`/);
  const typo = clippy(fixture(lib, config(method.replace('get_generic_password', 'get_generic_passwrd'))));
  assert.notEqual(typo.status, 0, 'a mistyped path must not pass');
  assert.match(typo.stderr, /get_generic_passwrd/);
  assert.match(typo.stderr, /(not refer to a reachable|could not resolve|unresolved)/i);
});

// The preset's paths against the REAL crates (not stand-ins), allow-invalid stripped from every entry: a mistyped or renamed
// path would be reported as unreachable, and the real types and a real function must be refused. Needs the Apple-only
// crates to compile, so it runs on macOS only and is reported as skipped everywhere else (ER-33: it is not silently green).
const onMac = process.platform === 'darwin';
test('C99 Clippy preset: every entry resolves in the real security-framework, objc2-security, keyring and keyring-core', { skip: !onMac ? 'macOS only: security-framework and objc2-security build on Apple targets; not measured on this OS' : !hasClippy ? 'clippy is not installed here' : false }, () => {
  const config = readFileSync(join(PADDOCK, 'lint', 'clippy.toml'), 'utf8').replaceAll('allow-invalid = true, ', '');
  const root = makeApp({
    'Cargo.toml': `${pkg('real')}security-framework = "=3.7.0"\nobjc2-security = { version = "=0.3.2", features = ["SecItem", "SecKeychain", "SecKeychainItem", "SecKeychainSearch", "SecImportExport", "SecIdentitySearch"] }\nkeyring = "=4.2.0"\nkeyring-core = "=1.0.1"\n`,
    'src/lib.rs': [
      'use objc2_security as _;',
      'use security_framework as _;',
      'pub fn a() -> Option<security_framework::os::macos::keychain::SecKeychain> { None }',
      'pub fn b() -> Option<keyring::Entry> { None }',
      'pub fn c() -> Option<keyring_core::Entry> { None }',
      'pub fn d() -> Option<objc2_security::SecKeychain> { None }',
      'pub fn e() { let _ = security_framework::passwords::get_generic_password("s", "a"); }',
      '',
    ].join('\n'),
  });
  roots.push(root);
  writeFileSync(join(root, 'clippy.toml'), config);
  const r = clippy(root);
  assert.notEqual(r.status, 0, 'the real Keychain API is refused');
  for (const what of ['type `security_framework::os::macos::keychain::SecKeychain`', 'type `objc2_security::SecKeychain`', 'type `keyring::Entry`', 'type `keyring_core::Entry`', 'method `security_framework::passwords::get_generic_password`']) {
    assert.ok(r.stderr.includes(`use of a disallowed ${what}`), `${what} is refused:\n${r.stderr.slice(0, 1500)}`);
  }
  assert.doesNotMatch(r.stderr, /does not refer to a reachable|could not resolve/i, 'every preset path resolves in the real crates');
});
