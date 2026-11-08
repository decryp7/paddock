// C99 layer 5 (ADR 0057): the Clippy preset must stop code that uses the Keychain API. A fixture crate calls stand-ins for
// keyring::Entry and security_framework::passwords::get_generic_password under the real lint/clippy.toml; Clippy has to
// fail on both and pass once they are gone (ER-33: the ban has shown it can see the thing).
import assert from 'node:assert/strict';
import { spawnSync } from 'node:child_process';
import { copyFileSync } from 'node:fs';
import { join } from 'node:path';
import { test } from 'node:test';
import { makeApp, PADDOCK } from './helpers.mjs';

const hasClippy = spawnSync('cargo', ['clippy', '--version']).status === 0;
const pkg = (name, deps = '') => `[package]\nname = "${name}"\nversion = "0.1.0"\nedition = "2021"\n\n[dependencies]\n${deps}`;

function fixture(lib) {
  const root = makeApp({
    'Cargo.toml': pkg('demo-app', 'keyring = { path = "vendor/keyring" }\nsecurity-framework = { path = "vendor/security-framework" }\n'),
    'src/lib.rs': lib,
    'vendor/keyring/Cargo.toml': pkg('keyring'),
    'vendor/keyring/src/lib.rs': 'pub struct Entry;\nimpl Entry { pub fn new() -> Entry { Entry } }\n',
    'vendor/security-framework/Cargo.toml': pkg('security-framework'),
    'vendor/security-framework/src/lib.rs': 'pub mod passwords { pub fn get_generic_password() -> i32 { 0 } }\n',
  });
  copyFileSync(join(PADDOCK, 'lint', 'clippy.toml'), join(root, 'clippy.toml'));
  return root;
}
const clippy = (root) => spawnSync('cargo', ['clippy', '--offline', '--', '-D', 'warnings'], { cwd: root, encoding: 'utf8', env: { ...process.env, CARGO_TARGET_DIR: join(root, 'target') } });

test('C99 Clippy preset: the Keychain method and type are refused, clean code passes', { skip: hasClippy ? false : 'clippy is not installed here: run the conformance tests where cargo clippy is (CI image)' }, () => {
  const bad = clippy(fixture('pub fn a() -> i32 { security_framework::passwords::get_generic_password() }\npub fn b() -> keyring::Entry { keyring::Entry::new() }\n'));
  assert.notEqual(bad.status, 0, bad.stderr);
  assert.match(bad.stderr, /use of a disallowed method `security_framework::passwords::get_generic_password`/);
  assert.match(bad.stderr, /use of a disallowed type `keyring::Entry`/);
  assert.match(bad.stderr, /owner-only file/);
  const ok = clippy(fixture('pub fn a() -> i32 { 1 }\n'));
  assert.equal(ok.status, 0, ok.stderr);
});
