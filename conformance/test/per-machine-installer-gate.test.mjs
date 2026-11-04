import assert from 'node:assert/strict';
import { mkdirSync, mkdtempSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { test } from 'node:test';
import { checkInstaller, checkSignatureName, trustedCommentOf, findInstallers, hasAdministratorManifest, includeVersionOf, listedFiles } from '../release/check-per-machine-installer.mjs';

const utf16 = (s) => Buffer.from(s, 'utf16le');
const installer = ({ admin = true, version = '1' } = {}) =>
  Buffer.concat([
    Buffer.from('MZ....'),
    Buffer.from(admin ? '<requestedExecutionLevel level="requireAdministrator" uiAccess="false"/>' : '<requestedExecutionLevel level="asInvoker"/>'),
    utf16('PaddockInclude'),
    Buffer.from([0, 0]),
    utf16(version),
  ]);

test('the manifest asks for administrator level', () => {
  assert.equal(hasAdministratorManifest(installer()), true);
  assert.equal(hasAdministratorManifest(installer({ admin: false })), false);
});

test("the include's version is read from the version resource", () => {
  assert.equal(includeVersionOf(installer({ version: '7' })), '7');
  assert.equal(includeVersionOf(Buffer.from('MZ no resource')), null);
});

test('7z -slt output is read into the file names inside the installer', () => {
  const out = 'Path = Dyno_1.1.0_x64-setup.exe\nType = Nsis\n\n----------\nPath = $PLUGINSDIR/nsis_tauri_utils.dll\nSize = 3\n\nPath = paddock-setup.exe\nSize = 9\n';
  assert.deepEqual(listedFiles(out), ['$PLUGINSDIR/nsis_tauri_utils.dll', 'paddock-setup.exe']);
});

test('installers are found under the bundle folder', () => {
  const d = mkdtempSync(join(tmpdir(), 'gate-'));
  mkdirSync(join(d, 'bundle/nsis'), { recursive: true });
  writeFileSync(join(d, 'bundle/nsis/Dyno_1.1.0_x64-setup.exe'), 'x');
  writeFileSync(join(d, 'bundle/nsis/other.txt'), 'x');
  assert.deepEqual(findInstallers(d).map((p) => p.split('/').pop()), ['Dyno_1.1.0_x64-setup.exe']);
  assert.deepEqual(findInstallers(join(d, 'nothing')), []);
});

test('the gate names every way an installer fails it, with the next step', () => {
  const ok = checkInstaller({ bytes: installer(), files: ['$PLUGINSDIR/x.dll', 'paddock-setup.exe'], expectVersion: '1', name: 'a-setup.exe' });
  assert.deepEqual(ok, []);
  const bad = checkInstaller({ bytes: installer({ admin: false, version: '0' }), files: ['x.dll'], expectVersion: '1', name: 'a-setup.exe' });
  assert.equal(bad.length, 3, bad.join('\n'));
  assert.match(bad.join('\n'), /administrator/);
  assert.match(bad.join('\n'), /does not contain paddock-setup.exe/);
  assert.match(bad.join('\n'), /include version 0.*expected 1/);
  const none = checkInstaller({ bytes: Buffer.from('MZ'), files: null, expectVersion: '1', name: 'a-setup.exe' });
  assert.match(none.join('\n'), /could not be listed/);
});

const sig = (comment) => Buffer.from(`untrusted comment: signature from tauri secret key\nRUQxxxx\ntrusted comment: ${comment}\nRUQyyyy\n`).toString('base64');

test('the signature names the installer as paddock-setup expects (product_version_arch-setup.exe)', () => {
  assert.equal(trustedCommentOf(sig('timestamp:1\tfile:Dyno_1.1.0_x64-setup.exe')), 'timestamp:1\tfile:Dyno_1.1.0_x64-setup.exe');
  assert.equal(trustedCommentOf('not base64 !!'), null);
  const ok = (comment, file = 'Dyno_1.1.0_x64-setup.exe') => checkSignatureName({ sig: sig(comment), file, product: 'Dyno', version: '1.1.0' });
  assert.deepEqual(ok('timestamp:1\tfile:Dyno_1.1.0_x64-setup.exe'), []);
  assert.deepEqual(ok('timestamp:1\tfile:Dyno_1.1.0_arm64-setup.exe', 'Dyno_1.1.0_arm64-setup.exe'), []);
  assert.match(ok('timestamp:1\tfile:Dyno_1.1.1_x64-setup.exe').join(), /names Dyno_1.1.1_x64-setup.exe, expected Dyno_1.1.0_x64-setup.exe/);
  assert.match(ok('timestamp:1').join(), /no file: name/);
  assert.match(checkSignatureName({ sig: 'garbage!', file: 'Dyno_1.1.0_x64-setup.exe', product: 'Dyno', version: '1.1.0' }).join(), /unreadable/);
  assert.match(ok('timestamp:1\tfile:Dyno_1.1.0_x64-setup.exe', 'Other_1.1.0_x64-setup.exe').join(), /file name Other_1.1.0_x64-setup.exe is not Dyno_1.1.0_/);
});
