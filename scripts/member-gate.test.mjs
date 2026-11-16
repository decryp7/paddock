// Tests of scripts/member-gate-windows.sh, the MANUAL Windows test-VM runner (docs/adr/amendments/0087-member-gate-other-os-2026-11-16.md).
// The member gate itself runs on the real CI runners (tools/paddock-dev/src/merge_check/other_os.rs); this script stays for
// running one package natively on the test VM by hand. These tests need no Docker and no VM.
import { test } from 'node:test';
import assert from 'node:assert/strict';
import { spawnSync } from 'node:child_process';
import { readFileSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';

const here = dirname(fileURLToPath(import.meta.url));
const read = (f) => readFileSync(join(here, f), 'utf8');

test('member-gate-windows.sh parses', () => {
  assert.equal(spawnSync('bash', ['-n', join(here, 'member-gate-windows.sh')]).status, 0);
});

test('a call without a spec is refused with exit 2 before it touches the VM', () => {
  const r = spawnSync('bash', [join(here, 'member-gate-windows.sh'), 'sometree'], { encoding: 'utf8' });
  assert.equal(r.status, 2, r.stderr);
  assert.match(r.stderr, /::error::.*no spec given/);
});

test('the Windows run copies the toolchain of rust-toolchain.toml in per run, never runs rustup, and carries no token', () => {
  const s = read('member-gate-windows.sh');
  assert.match(s, /rust-toolchain\.toml/);
  assert.match(s, /RUSTUP_AUTO_INSTALL=0/);
  assert.doesNotMatch(s, /rustup (install|toolchain|update|default|target)|rustup-init/);
  assert.doesNotMatch(s, /Authorization|--token|_TOKEN|credential-provider/);
  assert.match(s, /buildtools-baseline/);
});

test('the script says it is for manual use and that the member gate runs on the real runners', () => {
  assert.match(read('member-gate-windows.sh'), /MANUAL USE/);
});
