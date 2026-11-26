// conformance/ci/playwright-browsers.sh: the one place a CI job installs Playwright's browsers. On macOS they go to one fixed folder
// on the boot disk (never the runner's tool cache on the external work volume, where the headless shell is its own responsible
// process and a LaunchDaemon cannot show the Removable Volumes prompt), and then to the Mac CI signer as a CI hook. npx and the
// signer are stand-ins here, so nothing is installed or signed.
import assert from 'node:assert/strict';
import { spawnSync } from 'node:child_process';
import { chmodSync, mkdtempSync, readFileSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { dirname, join } from 'node:path';
import { test } from 'node:test';
import { fileURLToPath } from 'node:url';

const SCRIPT = join(dirname(fileURLToPath(import.meta.url)), '..', 'ci', 'playwright-browsers.sh');

function machine(os) {
  const root = mkdtempSync(join(tmpdir(), 'paddock-pw-browsers-'));
  const bin = join(root, 'bin');
  const log = join(root, 'calls.log');
  spawnSync('mkdir', ['-p', bin, join(root, 'home')]);
  const stub = (name, body = '') => {
    writeFileSync(join(bin, name), `#!/bin/bash\necho "${name} PLAYWRIGHT_BROWSERS_PATH=\${PLAYWRIGHT_BROWSERS_PATH:-unset} $*" >> "${log}"\n${body}\n`);
    chmodSync(join(bin, name), 0o755);
  };
  stub('npx');
  stub('signer.sh');
  writeFileSync(join(bin, 'uname'), `#!/bin/bash\necho ${os}\n`);
  chmodSync(join(bin, 'uname'), 0o755);
  const ghEnv = join(root, 'github-env');
  writeFileSync(ghEnv, '');
  const run = (args, env = {}) =>
    spawnSync('bash', [SCRIPT, ...args], {
      encoding: 'utf8',
      env: { PATH: `${bin}:/usr/bin:/bin`, HOME: join(root, 'home'), GITHUB_ENV: ghEnv, PADDOCK_MAC_SIGNING: join(bin, 'signer.sh'), ...env },
    });
  const calls = () => readFileSync(log, 'utf8').trim().split('\n');
  return { root, run, calls, ghEnv };
}

test('macOS: one fixed folder on the boot disk for every Playwright version, exported, installed there, then signed as a CI hook', () => {
  const m = machine('Darwin');
  const r = m.run(['chromium']);
  assert.equal(r.status, 0, r.stderr);
  const dir = join(m.root, 'home', 'Library', 'Caches', 'paddock-ci', 'ms-playwright');
  assert.deepEqual(m.calls(), [`npx PLAYWRIGHT_BROWSERS_PATH=${dir} playwright install chromium`, `signer.sh PLAYWRIGHT_BROWSERS_PATH=${dir} sign-playwright --ci-hook ${dir}`]);
  assert.equal(readFileSync(m.ghEnv, 'utf8'), `PLAYWRIGHT_BROWSERS_PATH=${dir}\n`);
  assert.doesNotMatch(dir, /tool_cache|\/Volumes\//, 'never on the runner\'s work volume');
});

test('macOS: PADDOCK_PLAYWRIGHT_BROWSERS overrides the folder', () => {
  const m = machine('Darwin');
  assert.equal(m.run(['chromium', 'webkit'], { PADDOCK_PLAYWRIGHT_BROWSERS: '/x/pw' }).status, 0);
  assert.deepEqual(m.calls(), ['npx PLAYWRIGHT_BROWSERS_PATH=/x/pw playwright install chromium webkit', 'signer.sh PLAYWRIGHT_BROWSERS_PATH=/x/pw sign-playwright --ci-hook /x/pw']);
});

test('other systems: Playwright\'s own folder and no signing', () => {
  const m = machine('Linux');
  const r = m.run(['--with-deps', 'chromium']);
  assert.equal(r.status, 0, r.stderr);
  assert.deepEqual(m.calls(), ['npx PLAYWRIGHT_BROWSERS_PATH=unset playwright install --with-deps chromium']);
  assert.equal(readFileSync(m.ghEnv, 'utf8'), '');
});

test('a failed install or signature fails the step with one ::error:: line', () => {
  const m = machine('Darwin');
  writeFileSync(join(m.root, 'bin', 'npx'), '#!/bin/bash\nexit 3\n');
  const r = m.run(['chromium']);
  assert.equal(r.status, 1);
  assert.match(r.stdout, /::error::playwright-browsers: `npx playwright install chromium` failed \(exit 3\)/);
  const s = machine('Darwin');
  writeFileSync(join(s.root, 'bin', 'signer.sh'), '#!/bin/bash\nexit 1\n');
  const f = s.run(['chromium']);
  assert.equal(f.status, 1);
  assert.match(f.stdout, /::error::playwright-browsers: signing the browsers in .* failed/);
});

test('no browser named is a usage error', () => {
  const r = machine('Darwin').run([]);
  assert.equal(r.status, 2);
  assert.match(r.stderr, /name the browsers/);
});
