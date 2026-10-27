import assert from 'node:assert/strict';
import { chmodSync, mkdirSync, writeFileSync } from 'node:fs';
import { join } from 'node:path';
import { test } from 'node:test';
import { findingsOf, makeApp, messages } from './helpers.mjs';

const macConf = (extra) => JSON.stringify({ bundle: { macOS: { signingIdentity: '-', ...extra } } });
const verifyWf = '.gitea/workflows/build.yml';
const goodWf = 'jobs:\n  b:\n    steps:\n      - run: bash conformance/release/macos-verify-bundle.sh\n';

test('C04 good: ad hoc, hardened runtime off, verify script in CI', async () => {
  const f = await findingsOf('C04', makeApp({ 'app/src-tauri/tauri.macos.conf.json': macConf({ hardenedRuntime: false }), [verifyWf]: goodWf }));
  assert.deepEqual(f, []);
});

test('C04 bad: hardened runtime on by default, no verify step', async () => {
  const f = await findingsOf('C04', makeApp({ 'app/src-tauri/tauri.macos.conf.json': macConf({}), [verifyWf]: 'jobs:\n  b:\n    steps:\n      - run: echo hi\n' }));
  assert.match(messages(f), /hardenedRuntime is not false/);
  assert.match(messages(f), /no CI step runs `codesign --verify/);
});

test('C04 bad: .app verified but not the DMG', async () => {
  const wf = 'jobs:\n  b:\n    steps:\n      - run: codesign --verify --deep --strict "$APP"\n';
  const f = await findingsOf('C04', makeApp({ 'app/src-tauri/tauri.macos.conf.json': macConf({ hardenedRuntime: false }), [verifyWf]: wf }));
  assert.match(messages(f), /not the app inside the DMG/);
});

test('C04 bad: a secret APPLE_SIGNING_IDENTITY reaches tauri build empty (it overrides signingIdentity "-" with "")', async () => {
  const env = 'jobs:\n  b:\n    steps:\n      - env:\n          APPLE_SIGNING_IDENTITY: ${{ secrets.APPLE_SIGNING_IDENTITY }}\n        run: |\n          bash conformance/release/macos-verify-bundle.sh\n          npx tauri build\n';
  const files = { 'app/src-tauri/tauri.macos.conf.json': macConf({ hardenedRuntime: false }) };
  const bad = await findingsOf('C04', makeApp({ ...files, [verifyWf]: env }));
  assert.match(messages(bad), /APPLE_SIGNING_IDENTITY.*empty/);
  const guarded = env.replace('npx tauri build', '[ -n "${APPLE_SIGNING_IDENTITY:-}" ] || unset APPLE_SIGNING_IDENTITY\n          npx tauri build');
  assert.deepEqual(await findingsOf('C04', makeApp({ ...files, [verifyWf]: guarded })), []);
  // The guard may live in a script the workflow runs.
  const viaScript = env.replace('npx tauri build', 'bash scripts/ci/bundle.sh');
  assert.deepEqual(await findingsOf('C04', makeApp({ ...files, [verifyWf]: viaScript, 'scripts/ci/bundle.sh': 'unset APPLE_SIGNING_IDENTITY\n' })), []);
});

test('C04 bad: grep -q after a pipe under pipefail is found, the capture form is not', async () => {
  const bad = 'set -euo pipefail\nsig=$(codesign -dv "$app" 2>&1)\necho "$sig" | grep -q Signature=adhoc\n';
  const good = 'set -euo pipefail\nsig=$(codesign -dv "$app" 2>&1)\ngrep -q Signature=adhoc <<<"$sig"\n';
  const base = { 'app/src-tauri/tauri.macos.conf.json': macConf({ hardenedRuntime: false }), [verifyWf]: goodWf };
  assert.match(messages(await findingsOf('C04', makeApp({ ...base, 'scripts/verify.sh': bad }))), /grep -q` under pipefail/);
  assert.doesNotMatch(messages(await findingsOf('C04', makeApp({ ...base, 'scripts/verify.sh': good }))), /pipefail/);
});

/** A bundle folder with shimmed codesign/file/otool so the shared script runs on any OS. */
function bundleFixture({ linkerSigned = false, sidecarExit = 0 } = {}) {
  const root = makeApp({
    'bundle/macos/Demo.app/Contents/MacOS/demo': '#!/bin/sh\n',
    'bundle/macos/Demo.app/Contents/MacOS/demo-worker': `#!/bin/sh\nexit ${sidecarExit}\n`,
    'app/src-tauri/tauri.macos.conf.json': macConf({ hardenedRuntime: false }),
    [verifyWf]: goodWf,
    'paddock.conformance.json': JSON.stringify({ sidecars: [{ name: 'demo-worker', args: ['--self-test'] }] }),
  });
  for (const f of ['demo', 'demo-worker']) chmodSync(join(root, 'bundle/macos/Demo.app/Contents/MacOS', f), 0o755);
  const bin = join(root, 'shim');
  mkdirSync(bin);
  const sh = (name, body) => {
    writeFileSync(join(bin, name), `#!/bin/sh\n${body}\n`);
    chmodSync(join(bin, name), 0o755);
  };
  const flags = linkerSigned ? 'flags=0x20002(adhoc,linker-signed)' : 'flags=0x2(adhoc)';
  sh('codesign', `case "$1" in --verify) exit 0;; esac\necho "Signature=adhoc" >&2; echo "${flags}" >&2; echo "Sealed Resources version=2 rules=13 files=4" >&2; echo "TeamIdentifier=not set" >&2`);
  sh('file', 'echo "Mach-O 64-bit executable arm64"');
  sh('otool', 'true');
  return { root, bin };
}

test('C04 bundle good: the shared script passes and starts the sidecar', async () => {
  const { root, bin } = bundleFixture();
  const f = await findingsOf('C04', root, { options: { bundle: join(root, 'bundle'), bundleEnv: { PATH: `${bin}:${process.env.PATH}` } } });
  assert.deepEqual(f, []);
});

test('C04 bundle bad: linker-signed seal and a sidecar that does not start', async () => {
  const a = bundleFixture({ linkerSigned: true });
  const fa = await findingsOf('C04', a.root, { options: { bundle: join(a.root, 'bundle'), bundleEnv: { PATH: `${a.bin}:${process.env.PATH}` } } });
  assert.match(messages(fa), /only linker-signed/);
  const b = bundleFixture({ sidecarExit: 3 });
  const fb = await findingsOf('C04', b.root, { options: { bundle: join(b.root, 'bundle'), bundleEnv: { PATH: `${b.bin}:${process.env.PATH}` } } });
  assert.match(messages(fb), /sidecar demo-worker .* does not start/);
});
