// The SBOM step of app-release.yml, run as the runner would: it attaches the app's Rust CycloneDX file to the release
// folder, and does nothing for an app without a Tauri crate.
import assert from 'node:assert/strict';
import { spawnSync } from 'node:child_process';
import { chmodSync, existsSync, mkdirSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { dirname, join } from 'node:path';
import { test } from 'node:test';
import { PADDOCK } from './helpers.mjs';

const text = readFileSync(join(PADDOCK, '.gitea', 'workflows', 'app-release.yml'), 'utf8');
const at = text.indexOf('- name: SBOM (CycloneDX)');
const stepText = text.slice(at);
const body = stepText.slice(stepText.indexOf('run: |\n') + 'run: |\n'.length).split('\n');
const script = body.slice(0, body.findIndex((l, i) => i > 0 && l.trim() !== '' && !l.startsWith('          '))).map((l) => l.slice(10)).join('\n');

function sandbox(withCrate) {
  const dir = mkdtempSync(join(tmpdir(), 'sbom-'));
  mkdirSync(join(dir, 'release'));
  mkdirSync(join(dir, 'bin'));
  if (withCrate) {
    mkdirSync(join(dir, 'apps/x/app/src-tauri'), { recursive: true });
    writeFileSync(join(dir, 'apps/x/app/src-tauri/Cargo.toml'), '');
  }
  // Fake cargo and cargo-cyclonedx: `cargo cyclonedx --manifest-path M --override-filename N` writes N.json next to M.
  writeFileSync(join(dir, 'bin', 'cargo-cyclonedx'), '#!/bin/sh\n');
  writeFileSync(
    join(dir, 'bin', 'cargo'),
    '#!/bin/sh\nwhile [ $# -gt 0 ]; do case "$1" in --manifest-path) m=$2;; --override-filename) n=$2;; esac; shift; done\necho \'{"bomFormat":"CycloneDX"}\' > "$(dirname "$m")/$n.json"\n',
  );
  for (const f of ['cargo', 'cargo-cyclonedx']) chmodSync(join(dir, 'bin', f), 0o755);
  return dir;
}

const run = (dir) =>
  spawnSync('bash', ['-e', '-o', 'pipefail', '-c', script], {
    cwd: dir,
    env: { PATH: `${join(dir, 'bin')}:${dirname(process.execPath)}:/usr/bin:/bin`, APP_ID: 'x', APP_DIR: 'apps/x/app' },
    encoding: 'utf8',
  });

test('the SBOM step puts <app>-rust.cdx.json into the release folder', () => {
  const dir = sandbox(true);
  try {
    const r = run(dir);
    assert.equal(r.status, 0, r.stderr);
    assert.equal(JSON.parse(readFileSync(join(dir, 'release', 'x-rust.cdx.json'), 'utf8')).bomFormat, 'CycloneDX');
  } finally {
    rmSync(dir, { recursive: true, force: true });
  }
});

test('the SBOM step is inert for an app without a Tauri crate', () => {
  const dir = sandbox(false);
  try {
    const r = run(dir);
    assert.equal(r.status, 0, r.stderr);
    assert.match(r.stdout, /no SBOM for this app/);
    assert.equal(existsSync(join(dir, 'release', 'x-rust.cdx.json')), false);
  } finally {
    rmSync(dir, { recursive: true, force: true });
  }
});

test('the SBOM step runs before the release is published', () => {
  assert.ok(at > 0 && at < text.indexOf('publish-release'));
});
