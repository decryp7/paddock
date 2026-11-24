// ER-51 (start, hold, watch, end): "a check flags a start whose handle is not kept". paddock-dev's raw dispatch
// (ci_runs::dispatch::run) sends a workflow and returns no run; only `./dev ci dispatch` and queue::real::dispatch_held call it, each
// with #[expect(clippy::disallowed_methods)], and both then find the run by its dispatch token. tools/paddock-dev/clippy.toml bans the
// path; this is the ban's positive control (ER-33): a fixture crate with the same crate and module names, under the real entry, must
// fail Clippy on a new caller, pass with the #[expect] or with no caller, and fail when the ban stops resolving (the #[expect]
// sites are then unfulfilled).
import assert from 'node:assert/strict';
import { spawnSync } from 'node:child_process';
import { readFileSync, rmSync, writeFileSync } from 'node:fs';
import { join } from 'node:path';
import { after, test } from 'node:test';
import { makeApp, PADDOCK } from './helpers.mjs';

const hasClippy = spawnSync('cargo', ['clippy', '--version']).status === 0;
const skip = hasClippy ? false : 'clippy is not installed here: run the conformance tests where cargo clippy is (CI image)';
const PATH = 'paddock_dev::ci_runs::dispatch::run';

/** The entry of the real tools/paddock-dev/clippy.toml that bans the raw dispatch. */
function entry() {
  const line = readFileSync(join(PADDOCK, 'tools', 'paddock-dev', 'clippy.toml'), 'utf8')
    .split('\n')
    .find((l) => l.includes(`"${PATH}"`));
  assert.ok(line, `tools/paddock-dev/clippy.toml bans ${PATH}`);
  assert.doesNotMatch(line, /allow-invalid/, 'the ban must resolve: allow-invalid would hide a renamed path');
  return line;
}

const roots = [];
after(() => roots.forEach((r) => rmSync(r, { recursive: true, force: true })));

const STAND_IN = 'pub mod ci_runs {\n    pub mod dispatch {\n        pub fn run() -> i32 {\n            0\n        }\n    }\n}\n';

function clippy(caller, line = entry()) {
  const root = makeApp({
    'Cargo.toml': '[package]\nname = "paddock-dev"\nversion = "0.1.0"\nedition = "2021"\n\n[dependencies]\n',
    'src/lib.rs': `${STAND_IN}${caller}`,
  });
  roots.push(root);
  writeFileSync(join(root, 'clippy.toml'), `disallowed-methods = [\n${line}\n]\n`);
  return spawnSync('cargo', ['clippy', '--offline', '--', '-D', 'warnings'], {
    cwd: root,
    encoding: 'utf8',
    env: { ...process.env, RUSTC_WRAPPER: '', CARGO_TARGET_DIR: join(root, 'target') },
  });
}

test('ER-51: a new caller of the raw dispatch fails Clippy, and the ban names the way that keeps the run', { skip }, () => {
  const bad = clippy('pub fn new_caller() -> i32 {\n    crate::ci_runs::dispatch::run()\n}\n');
  assert.notEqual(bad.status, 0, bad.stderr);
  assert.match(bad.stderr, /use of a disallowed method `paddock_dev::ci_runs::dispatch::run`/);
  assert.match(bad.stderr, /dispatch_held/);
});

test('ER-51: an allowed site with #[expect] passes, and so does code without a caller', { skip }, () => {
  const allowed = clippy(
    'pub fn held() -> i32 {\n    #[expect(clippy::disallowed_methods, reason = "holds the run")]\n    let r = crate::ci_runs::dispatch::run();\n    r\n}\n',
  );
  assert.equal(allowed.status, 0, allowed.stderr);
  const none = clippy('pub fn other() -> i32 {\n    1\n}\n');
  assert.equal(none.status, 0, none.stderr);
});

// Clippy only warns on a ban path that resolves to nothing (`does not refer to a reachable function`), and -D warnings does not turn
// that warning into an error. What makes a renamed or moved `run` fail in the real crate is the two #[expect] sites: with the ban
// resolving to nothing they expect a lint that never fires, and `unfulfilled_lint_expectations` under -D warnings is an error.
test('ER-51: a ban path that resolves to nothing is reported, and the #[expect] sites then fail Clippy', { skip }, () => {
  const held = 'pub fn held() -> i32 {\n    #[expect(clippy::disallowed_methods, reason = "holds the run")]\n    let r = crate::ci_runs::dispatch::run();\n    r\n}\n';
  const typo = clippy(held, entry().replace(PATH, `${PATH}x`));
  assert.match(typo.stderr, /`paddock_dev::ci_runs::dispatch::runx` does not refer to a reachable function/);
  assert.notEqual(typo.status, 0, 'an #[expect] that the ban no longer fulfils must fail');
  assert.match(typo.stderr, /unfulfilled_lint_expectations|this lint expectation is unfulfilled/);
});
