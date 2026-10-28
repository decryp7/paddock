// scripts/shell-demo/build-macos-n-n1.sh (the local update smoke's N and N+1 builds), run in a scratch git repo with a fake
// bundler: nothing is compiled and nothing on the machine is touched.
import assert from 'node:assert/strict';
import { spawnSync } from 'node:child_process';
import { copyFileSync, existsSync, mkdirSync, readFileSync, writeFileSync } from 'node:fs';
import { join } from 'node:path';
import { test } from 'node:test';
import { scratch, systemFolder, TOOLS, write } from './helpers.mjs';

const SCRIPT = 'scripts/shell-demo/build-macos-n-n1.sh';
const VERSION = '3.4.5'; // invented: the script reads the version from the repo it runs in, never a pinned one
const BUMPED = '3.4.6';

/**
 * A scratch repo with the shell demo's two manifests and lockfile, committed, and a fake bundler that makes an .app whose
 * agent answers `--self-test` like the real one (a header with the version, then one line per check). Knobs:
 * FAKE_AGENT_VERSION (the version the agent claims), FAKE_FAIL_AT (the version whose bundling fails).
 */
function repo() {
  const root = scratch('paddock-n-n1-');
  const toml = (name) => `[package]\nname = "${name}"\nversion = "${VERSION}"\n\n[dependencies]\nlog = { version = "0.4" }\n`;
  write(root, {
    'apps/shell-demo/src-tauri/Cargo.toml': toml('paddock-shell-demo'),
    'apps/shell-demo/src-tauri/agent/Cargo.toml': toml('shell-demo-agent'),
    'apps/shell-demo/src-tauri/Cargo.lock': `version = 4\n`,
    'fake-bundle.sh': `#!/bin/bash
set -e
v=$(sed -n 's/^version = "\\(.*\\)"/\\1/p' apps/shell-demo/src-tauri/agent/Cargo.toml | head -n1)
[ "$v" != "\${FAKE_FAIL_AT:-}" ] || { echo "bundling $v failed" >&2; exit 3; }
echo "version = 4 # $v" > apps/shell-demo/src-tauri/Cargo.lock
app="$CARGO_TARGET_DIR/release/bundle/macos/Paddock Shell Demo.app/Contents/MacOS"
rm -rf "$CARGO_TARGET_DIR/release/bundle"; mkdir -p "$app"
printf '#!/bin/bash\\nprintf "shell-demo-agent %s self-test\\\\nok executable\\\\nok service spec\\\\n"\\n' "\${FAKE_AGENT_VERSION:-$v}" > "$app/shell-demo-agent"
chmod +x "$app/shell-demo-agent"
`,
  });
  mkdirSync(join(root, 'scripts/shell-demo'), { recursive: true });
  copyFileSync(join(TOOLS, '..', SCRIPT), join(root, SCRIPT));
  const git = (...a) => spawnSync('git', ['-c', 'user.name=t', '-c', 'user.email=t@example.invalid', ...a], { cwd: root, encoding: 'utf8' });
  git('init', '-q');
  git('add', '-A');
  git('commit', '-qm', 'fixture');
  return { root, status: () => git('status', '--porcelain').stdout };
}

function build(r, env = {}) {
  const out = join(r.root, '..', `${r.root.split('/').pop()}-out`);
  const res = spawnSync('bash', [join(r.root, SCRIPT), out], {
    cwd: r.root,
    encoding: 'utf8',
    timeout: 30_000,
    env: { PATH: systemFolder(), HOME: r.root, CARGO_TARGET_DIR: join(r.root, '..', `${r.root.split('/').pop()}-target`), PADDOCK_BUNDLE: 'fake-bundle.sh', ...env },
  });
  return { status: res.status, text: `${res.stdout}${res.stderr}`, out };
}

test('builds N and N+1, reads the version from the self-test header line, and leaves the checkout clean', () => {
  const r = repo();
  const res = build(r);
  assert.equal(res.status, 0, res.text);
  assert.equal(readFileSync(join(res.out, 'versions'), 'utf8').trim(), `${VERSION} ${BUMPED}`);
  for (const b of ['n', 'n1']) assert.ok(existsSync(join(res.out, b, 'Paddock Shell Demo.app/Contents/MacOS/shell-demo-agent')), b);
  assert.equal(r.status(), '', 'the bumped manifests and lockfile are restored');
});

test('fails when build N+1 agent does not report the bumped version, and restores the checkout', () => {
  const r = repo();
  const res = build(r, { FAKE_AGENT_VERSION: VERSION });
  assert.notEqual(res.status, 0, res.text);
  assert.match(res.text, new RegExp(`::error::the agent of the build for ${BUMPED.replaceAll('.', '\\.')} reports '${VERSION.replaceAll('.', '\\.')}'`));
  assert.equal(r.status(), '', 'the bump does not outlive a failed check');
});

test('a failed N+1 bundle restores the checkout too', () => {
  const r = repo();
  const res = build(r, { FAKE_FAIL_AT: BUMPED });
  assert.equal(res.status, 3, res.text);
  assert.equal(r.status(), '');
});

test('a failed first bundle restores the checkout (the lockfile the bundler rewrote)', () => {
  const r = repo();
  const res = build(r, { FAKE_FAIL_AT: VERSION });
  assert.equal(res.status, 3, res.text);
  assert.equal(r.status(), '');
});

test('refuses to start over uncommitted edits, and leaves them alone', () => {
  const r = repo();
  const file = join(r.root, 'apps/shell-demo/src-tauri/Cargo.toml');
  const edited = `${readFileSync(file, 'utf8')}# my edit\n`;
  writeFileSync(file, edited);
  const res = build(r);
  assert.equal(res.status, 1, res.text);
  assert.match(res.text, /uncommitted changes/);
  assert.equal(readFileSync(file, 'utf8'), edited, 'the edit is not thrown away');
});
