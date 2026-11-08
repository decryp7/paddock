import assert from 'node:assert/strict';
import { mkdirSync, mkdtempSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { test } from 'node:test';
import { createHash } from 'node:crypto';
import { checkInstaller, checkNsisVersion, nsisVersionOf, checkSetupSubsystem, subsystemOf, checkSelfTest, find7z, checkSignatureName, trustedCommentOf, findInstallers, hasAdministratorManifest, includeVersionOf, listedFiles, checkSetupDescribed, resourceStringOf, crateVersion, checkReleasedHash } from '../release/check-per-machine-installer.mjs';

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
  // Only bundle/nsis: a setup.exe elsewhere under the folder (a dependency's test installer) is not the product's.
  mkdirSync(join(d, 'deps/x'), { recursive: true });
  writeFileSync(join(d, 'deps/x/Other_setup.exe'), 'x');
  writeFileSync(join(d, 'bundle/stray-setup.exe'), 'x');
  assert.deepEqual(findInstallers(d).map((p) => p.split('/').pop()), ['Dyno_1.1.0_x64-setup.exe']);
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

test('7z is found on PATH, then in Program Files, else the error says how to install it', () => {
  const ok = (cmd, args) => ({ status: cmd === '7z' || cmd === 'C:\\Program Files\\7-Zip\\7z.exe' ? 0 : 1 });
  assert.deepEqual(find7z({ probe: ok }), { cmd: '7z' });
  assert.deepEqual(find7z({ probe: (c) => ({ status: c === 'C:\\Program Files\\7-Zip\\7z.exe' ? 0 : 1 }) }), { cmd: 'C:\\Program Files\\7-Zip\\7z.exe' });
  const none = find7z({ probe: () => ({ status: 1, error: new Error('ENOENT') }) });
  assert.match(none.error, /7-Zip \(7z\) was not found on PATH or in C:\\Program Files\\7-Zip/);
  assert.match(none.error, /install 7-Zip on the runner|winget install 7zip\.7zip/);
});

test('self-test names the identifier and says stop-all is present', () => {
  const out = 'identifier=net.decryptology.dyno\nproduct=Dyno\nstop_all=present\n';
  assert.deepEqual(checkSelfTest({ stdout: out, status: 0, identifier: 'net.decryptology.dyno', name: 'a-setup.exe' }), []);
  assert.match(checkSelfTest({ stdout: out.replace('present', 'absent'), status: 0, identifier: 'net.decryptology.dyno', name: 'a-setup.exe' }).join(), /stop_all=present/);
  assert.match(checkSelfTest({ stdout: out, status: 0, identifier: 'other', name: 'a-setup.exe' }).join(), /does not name other/);
  assert.match(checkSelfTest({ stdout: '', status: 1, identifier: 'x', name: 'a-setup.exe' }).join(), /self-test/);
  assert.match(checkSelfTest({ stdout: undefined, status: undefined, identifier: 'x', name: 'n' }).join(), /did not run/);
});

/** A minimal PE32+ header with the given optional-header Subsystem. */
const pe = (subsystem) => {
  const b = Buffer.alloc(0x200);
  b.writeUInt16LE(0x5a4d, 0);
  b.writeUInt32LE(0x80, 0x3c);
  b.writeUInt32LE(0x4550, 0x80);
  b.writeUInt16LE(0x20b, 0x80 + 24);
  b.writeUInt16LE(subsystem, 0x80 + 24 + 68);
  return b;
};

test('paddock-setup.exe must be a GUI-subsystem program: a console one flashes a command prompt during install', () => {
  assert.equal(subsystemOf(pe(2)), 2);
  assert.deepEqual(checkSetupSubsystem({ bytes: pe(2), name: 'a-setup.exe' }), []);
  assert.match(checkSetupSubsystem({ bytes: pe(3), name: 'a-setup.exe' }).join(), /subsystem 3.*command prompt/s);
  assert.match(checkSetupSubsystem({ bytes: Buffer.from('MZ'), name: 'a-setup.exe' }).join(), /not a Windows program/);
  assert.match(checkSetupSubsystem({ bytes: null, name: 'a-setup.exe' }).join(), /could not be extracted/);
});

const brand = (v, wide = false) => Buffer.concat([Buffer.from('MZ....'), wide ? Buffer.from(`Nullsoft Install System v${v}`, 'utf16le') : Buffer.from(`Nullsoft Install System v${v}`)]);

test('the installer must be made by NSIS 3.12 or later (3.11 and earlier can run elevated code from a shared $PLUGINSDIR)', () => {
  assert.equal(nsisVersionOf(brand('3.13')), '3.13');
  assert.equal(nsisVersionOf(brand('3.12', true)), '3.12', 'UTF-16 too');
  assert.equal(nsisVersionOf(Buffer.from('MZ no branding')), null);
  assert.deepEqual(checkNsisVersion({ bytes: brand('3.13'), name: 'a-setup.exe' }), []);
  assert.deepEqual(checkNsisVersion({ bytes: brand('3.12'), name: 'a-setup.exe' }), []);
  assert.deepEqual(checkNsisVersion({ bytes: brand('4.0'), name: 'a-setup.exe' }), []);
  assert.match(checkNsisVersion({ bytes: brand('3.11'), name: 'a-setup.exe' }).join(), /NSIS 3\.11.*3\.12/s);
  assert.match(checkNsisVersion({ bytes: brand('3.08'), name: 'a-setup.exe' }).join(), /NSIS 3\.08/);
  assert.match(checkNsisVersion({ bytes: Buffer.from('MZ'), name: 'a-setup.exe' }).join(), /NSIS version was not measured/, 'no branding: not measured, so not passed (ER-33)');
});

const described = ({ omit = [], over = {}, extra = '' } = {}) => {
  const fields = { CompanyName: 'decryptology', FileDescription: 'Dyno update', FileVersion: '1.2.0', InternalName: 'paddock-setup', LegalCopyright: 'Copyright (C) decryptology', OriginalFilename: 'paddock-setup.exe', ProductName: 'Dyno', ProductVersion: '1.2.0', ...over };
  const parts = [Buffer.from('MZ..')];
  for (const [k, v] of Object.entries(fields)) if (!omit.includes(k)) parts.push(utf16(k), Buffer.from([0, 0]), utf16(v), Buffer.from([0, 0]));
  parts.push(Buffer.from('<requestedExecutionLevel level="asInvoker" uiAccess="false"/><supportedOS Id="{x}"/>'), Buffer.from(extra));
  return Buffer.concat(parts);
};
const args = (bytes) => ({ bytes, name: 'Dyno_1.1.1_x64-setup.exe', expectVersion: '1.2.0', product: 'Dyno' });

test('paddock-setup.exe carries every version resource field, its own version and an asInvoker manifest', () => {
  assert.deepEqual(checkSetupDescribed(args(described())), []);
  assert.equal(resourceStringOf(described(), 'OriginalFilename'), 'paddock-setup.exe');
  for (const k of ['CompanyName', 'LegalCopyright', 'OriginalFilename', 'InternalName', 'ProductName', 'FileVersion']) {
    assert.match(checkSetupDescribed(args(described({ omit: [k] }))).join('\n'), new RegExp(`no ${k}`), k);
  }
  assert.match(checkSetupDescribed(args(described({ over: { FileVersion: '1.1.1' } }))).join('\n'), /FileVersion is "1\.1\.1", expected "1\.2\.0".*crate version, not the app version/);
  assert.match(checkSetupDescribed(args(null)).join(''), /could not be extracted/);
});

test('the manifest must be asInvoker with supportedOS; a build path in the program fails the gate', () => {
  const noManifest = Buffer.concat([described().subarray(0, described().indexOf('<requestedExecutionLevel'))]);
  const msgs = checkSetupDescribed(args(noManifest)).join('\n');
  assert.match(msgs, /no asInvoker manifest/);
  assert.match(msgs, /no supportedOS/);
  assert.match(checkSetupDescribed(args(described({ extra: 'C:\\Users\\decryp7\\.cargo\\registry\\src\\x' }))).join('\n'), /build path \(C:\\Users\\decryp7\)/);
  assert.match(checkSetupDescribed(args(described({ extra: '/home/x/.cargo/registry/src' }))).join('\n'), /build path/);
  assert.deepEqual(checkSetupDescribed(args(described({ extra: '/cargo/registry/src/x /rustc/abc/library' }))), []);
});

test("the expected version is the crate's own, read from its [package] table", () => {
  assert.equal(crateVersion('[package]\nname = "paddock-setup"\nversion = "1.2.0"\n[dependencies]\nsemver = { version = "1" }\n'), '1.2.0');
  assert.equal(crateVersion('[package]\nname = "x"\n[dependencies]\nversion = "3"\n'), null);
});

test('a missing crate version fails the gate instead of skipping the FileVersion check (ER-33)', () => {
  const msgs = checkSetupDescribed({ ...args(described()), expectVersion: null }).join('\n');
  assert.match(msgs, /version was not measured.*do not release/);
  assert.equal(crateVersion('[package]\nname = "x"\nversion.workspace = true\n'), null);
});

test('a build folder on any drive, or one named by the caller, in the program fails the gate', () => {
  assert.match(checkSetupDescribed(args(described({ extra: 'D:\\a\\_work\\paddock\\crates\\paddock-setup\\src\\main.rs' }))).join('\n'), /build path/);
  assert.match(checkSetupDescribed(args(described({ extra: 'C:\\act\\slot3\\paddock\\crates\\x\\Cargo.toml' }))).join('\n'), /build path/);
  const roots = ['E:\\runner\\target-dyno-3'];
  assert.match(checkSetupDescribed({ ...args(described({ extra: 'E:/runner/target-dyno-3/release' })), roots }).join('\n'), /build folder E:\/runner\/target-dyno-3/);
  assert.deepEqual(checkSetupDescribed({ ...args(described({ extra: 'x' })), roots }), []);
});

test('the same version with other bytes fails; a version without a record only notes its sha256', () => {
  const bytes = described();
  const sha = createHash('sha256').update(bytes).digest('hex');
  const a = { bytes, name: 'i.exe', identifier: 'net.x.dyno', version: '1.2.0' };
  assert.deepEqual(checkReleasedHash({ ...a, records: { 'net.x.dyno': { '1.2.0': sha } } }), { problems: [], notes: [] });
  const bad = checkReleasedHash({ ...a, records: { 'net.x.dyno': { '1.2.0': 'ab'.repeat(32) } } });
  assert.match(bad.problems.join(''), /changed without a version bump.*Bump the version/);
  const none = checkReleasedHash({ ...a, records: {} });
  assert.deepEqual(none.problems, []);
  assert.match(none.notes.join(''), new RegExp(sha));
  // another app's record or another version is not this one's
  assert.deepEqual(checkReleasedHash({ ...a, records: { 'net.x.other': { '1.2.0': 'ff' } } }).problems, []);
});
