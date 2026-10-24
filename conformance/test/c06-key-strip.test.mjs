import assert from 'node:assert/strict';
import { test } from 'node:test';
import { findingsOf, makeApp, messages } from './helpers.mjs';

const wf = (run, env = true) => `name: b\njobs:\n  bundle:\n    runs-on: ubuntu-latest\n    steps:\n      - name: Build installers\n${env ? '        env:\n          TAURI_SIGNING_PRIVATE_KEY: ${{ secrets.K }}\n          TAURI_SIGNING_PRIVATE_KEY_PASSWORD: ${{ secrets.P }}\n          TAURI_UPDATER_PUBKEY: ${{ secrets.U }}\n' : ''}        run: |\n${run.split('\n').map((l) => `          ${l}`).join('\n')}\n`;
const W = '.gitea/workflows/build.yml';

test('C06 good: both keys stripped inline', async () => {
  const run = `export TAURI_SIGNING_PRIVATE_KEY="$(printf '%s' "$TAURI_SIGNING_PRIVATE_KEY" | tr -d ' \\t\\r\\n')"\nexport TAURI_UPDATER_PUBKEY="$(printf '%s' "$TAURI_UPDATER_PUBKEY" | tr -d ' \\t\\r\\n')"\nnpx tauri build`;
  assert.deepEqual(await findingsOf('C06', makeApp({ [W]: wf(run) })), []);
});

test('C06 good: the shared helper, and a script the step calls', async () => {
  assert.deepEqual(await findingsOf('C06', makeApp({ [W]: wf('eval "$(node conformance/release/strip-keys.mjs)"\nnpx tauri build') })), []);
  const script = `export TAURI_SIGNING_PRIVATE_KEY="$(printf '%s' "$TAURI_SIGNING_PRIVATE_KEY" | tr -d ' \\t\\r\\n')"\nexport TAURI_UPDATER_PUBKEY="$(printf '%s' "$TAURI_UPDATER_PUBKEY" | tr -d ' \\t\\r\\n')"\n`;
  assert.deepEqual(await findingsOf('C06', makeApp({ [W]: wf('bash scripts/ci/tauri-bundle.sh deb'), 'scripts/ci/tauri-bundle.sh': script })), []);
});

test('C06 bad: keys reach the build unstripped (Pitcrew)', async () => {
  const f = await findingsOf('C06', makeApp({ [W]: wf('npx tauri build') }));
  assert.equal(f.length, 2);
  assert.match(messages(f), /TAURI_SIGNING_PRIVATE_KEY/);
  assert.match(messages(f), /TAURI_UPDATER_PUBKEY/);
  assert.equal(f[0].line, 6);
});

test('C06 bad: only one of the two stripped; and a script that does not strip', async () => {
  const run = `export TAURI_SIGNING_PRIVATE_KEY="$(printf '%s' "$TAURI_SIGNING_PRIVATE_KEY" | tr -d ' \\t\\r\\n')"\nnpx tauri build`;
  const f = await findingsOf('C06', makeApp({ [W]: wf(run) }));
  assert.equal(f.length, 1);
  assert.match(f[0].message, /TAURI_UPDATER_PUBKEY/);
  assert.equal((await findingsOf('C06', makeApp({ [W]: wf('bash s.sh'), 's.sh': 'tauri build\n' }))).length, 2);
});

test('C06 fails closed without workflows; a step without keys is fine', async () => {
  assert.match(messages(await findingsOf('C06', makeApp({ 'a.txt': 'x' }))), /no workflow files/);
  assert.deepEqual(await findingsOf('C06', makeApp({ [W]: wf('npm test', false) })), []);
});
