import assert from 'node:assert/strict';
import { createHash } from 'node:crypto';
import { mkdirSync, mkdtempSync, readFileSync, rmSync, writeFileSync, existsSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join, dirname } from 'node:path';
import { fileURLToPath } from 'node:url';
import { test } from 'node:test';
import { readPin, stage, verify } from './stage-vc-redist.mjs';

const here = dirname(fileURLToPath(import.meta.url));
const realLock = JSON.parse(readFileSync(join(here, '../../app/src-tauri/windows/vc-redist.lock.json'), 'utf8'));
const bytes = new TextEncoder().encode('not really an installer');
const hash = createHash('sha256').update(bytes).digest('hex');
const pinOf = (over = {}) => ({ x64: { name: 'vc_redist.x64.exe', url: `https://download.visualstudio.microsoft.com/download/pr/g/${hash.toUpperCase()}/VC_redist.x64.exe`, sha256: hash, size: bytes.length, ...over } });

function appWith(lock) {
  const app = mkdtempSync(join(tmpdir(), 'pitwall-vc-'));
  const dir = join(app, 'app', 'src-tauri', 'windows');
  mkdirSync(dir, { recursive: true });
  if (lock) writeFileSync(join(dir, 'vc-redist.lock.json'), JSON.stringify(lock));
  return { app, dest: join(dir, 'vc_redist.x64.exe') };
}

test('the committed pin is valid and carries its hash in the url', () => {
  const pin = readPin(realLock);
  assert.equal(pin.name, 'vc_redist.x64.exe');
});

test('the installer hooks compare the registry with the pinned version, and its messages name the stable download link', () => {
  const hooks = readFileSync(join(here, '../../app/src-tauri/windows/installer-hooks.nsh'), 'utf8');
  const [major, minor, bld] = ['MAJOR', 'MINOR', 'BLD'].map((k) => hooks.match(new RegExp(`!define PITWALL_VC_${k} (\\d+)`))?.[1]);
  assert.equal(`${major}.${minor}.${bld}`, realLock.x64.version.split('.').slice(0, 3).join('.'), 'the NSIS defines and vc-redist.lock.json name one version');
  assert.match(hooks, /nsExec::Exec \/TIMEOUT=\d+ '"\$PLUGINSDIR\\vc_redist\.x64\.exe" \/install \/quiet \/norestart'\n\s+Pop \$0/, 'the package runs with a time limit and its result (exit code, "error" or "timeout") is read');
  assert.doesNotMatch(hooks, /ExecWait/, 'no unbounded wait on the package');
  assert.match(hooks, /\$0 == "timeout"/, 'a package that did not finish has its own words');
  assert.match(hooks, /SetRebootFlag true/);
  assert.match(hooks, /1618/);
  assert.doesNotMatch(hooks, /aka\.ms\/vs\/17/);
});

test('a pin on another host, with a hash the url lacks or a bad size is refused', () => {
  assert.throws(() => readPin(pinOf({ url: 'https://example.com/x/' + hash + '/a.exe' })), /must start with/);
  assert.throws(() => readPin(pinOf({ url: 'https://download.visualstudio.microsoft.com/download/pr/g/' + 'a'.repeat(64) + '/a.exe' })), /does not carry/);
  assert.throws(() => readPin(pinOf({ sha256: 'ABC' })), /64 lowercase/);
  assert.throws(() => readPin(pinOf({ size: 0 })), /positive integer/);
  assert.throws(() => readPin({}), /no "x64" entry/);
});

test('verify names a wrong size and a wrong hash', () => {
  const pin = readPin(pinOf());
  verify(bytes, pin);
  assert.throws(() => verify(new Uint8Array(3), pin), /bytes, the pin says/);
  assert.throws(() => verify(new TextEncoder().encode('not really an installer!'.slice(0, bytes.length - 1) + 'X'), pin), /has sha256/);
});

test('stage downloads, verifies and writes the file; a good staged file is not downloaded again', async () => {
  const { app, dest } = appWith(pinOf());
  try {
    let calls = 0;
    const fetchBytes = async () => (calls++, bytes);
    assert.equal((await stage({ app, fetchBytes })).downloaded, true);
    assert.deepEqual(new Uint8Array(readFileSync(dest)), bytes);
    assert.equal((await stage({ app, fetchBytes })).downloaded, false);
    assert.equal(calls, 1);
    writeFileSync(dest, 'damaged');
    assert.equal((await stage({ app, fetchBytes })).downloaded, true);
  } finally {
    rmSync(app, { recursive: true, force: true });
  }
});

test('stage fails closed on a wrong download and leaves no file', async () => {
  const { app, dest } = appWith(pinOf());
  const noLock = appWith(null);
  try {
    await assert.rejects(stage({ app, fetchBytes: async () => new TextEncoder().encode('a different file!!!!!!') }), /pin says/);
    assert.equal(existsSync(dest), false);
    await assert.rejects(stage({ app: noLock.app }), /is missing/);
  } finally {
    rmSync(app, { recursive: true, force: true });
    rmSync(noLock.app, { recursive: true, force: true });
  }
});
