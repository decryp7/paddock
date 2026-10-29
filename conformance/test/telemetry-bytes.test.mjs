// The telemetry byte check (release/telemetry-bytes.mjs). Every input is synthetic: invented bytes around the patterns.
import { test } from 'node:test';
import assert from 'node:assert/strict';
import { spawnSync } from 'node:child_process';
import { writeFileSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';
import { FORBIDDEN, TELEMETRY_GROUP, checkBytes, checkFile } from '../release/telemetry-bytes.mjs';
import { makeApp } from './helpers.mjs';

const CLI = join(dirname(fileURLToPath(import.meta.url)), '../release/telemetry-bytes.mjs');
const noise = (n, seed = 1) => {
  const b = Buffer.alloc(n);
  for (let i = 0; i < n; i++) b[i] = (seed = (seed * 1103515245 + 12345) >>> 0) >>> 24;
  return b;
};
const around = (...parts) => Buffer.concat([noise(1000, 3), ...parts.map((p) => (typeof p === 'string' ? Buffer.from(p, 'latin1') : p)), noise(1000, 5)]);
// A provider's metadata as TraceLogging lays it out: the name, then a group trait (size 0x13, type 1, the GUID).
const provider = (name, guid) => Buffer.concat([Buffer.from(`${name}\0`, 'latin1'), Buffer.from([0x13, 0x00, 0x01]), guid]);

test('a clean file passes', () => {
  assert.deepEqual(checkBytes(around('onnxruntime_c_api', 'OrtGetApiBase')), { findings: [], notes: [] });
});

test('every forbidden string is found, in ASCII and in UTF-16LE', () => {
  for (const [s] of FORBIDDEN) {
    assert.equal(checkBytes(around(s)).findings.length, 1, s);
    assert.equal(checkBytes(around(Buffer.from(s, 'utf16le'))).findings.length, 1, `${s} (UTF-16)`);
  }
  // A mangled 1DS symbol as an archive's symbol table holds it.
  const r = checkBytes(around('_ZN9Microsoft12Applications6Events9LogManager10InitializeEv'));
  assert.deepEqual(r.findings, ['9Microsoft12Applications6Events (the 1DS telemetry SDK (Itanium-mangled))']);
  assert.equal(checkBytes(around('https://mobile.events.data.microsoft.com/OneCollector/1.0')).findings.length, 1);
});

test('a provider in the Microsoft telemetry group fails, unless allowed; a zero group passes', () => {
  const grouped = around(provider('Microsoft.Windows.AI.MachineLearning.Dml', TELEMETRY_GROUP));
  const strict = checkBytes(grouped);
  assert.deepEqual(strict.findings, ['a TraceLogging provider in the Microsoft telemetry group']);
  assert.deepEqual(strict.notes, ['ETW provider name Microsoft.Windows.AI.MachineLearning.Dml']);
  const allowed = checkBytes(grouped, { allowTelemetryGroup: true });
  assert.deepEqual(allowed.findings, []);
  assert.equal(allowed.notes.length, 2);
  // ONNX Runtime's public build: the provider exists, its group is all zeros.
  assert.deepEqual(checkBytes(around(provider('Microsoft.ML.ONNXRuntime', Buffer.alloc(16)))).findings, []);
});

test('a pattern across a chunk boundary is found when reading a file', () => {
  const root = makeApp({});
  const p = join(root, 'big.a');
  const at = (16 << 20) - 9; // the pattern starts 9 bytes before the first chunk ends
  writeFileSync(p, Buffer.concat([Buffer.alloc(at), Buffer.from('curl_easy_perform'), Buffer.alloc(100)]));
  assert.deepEqual(checkFile(p).findings, ['curl_easy_perform (curl (1DS uploads))']);
  assert.match(checkFile(join(root, 'missing')).error, /cannot be read/);
});

test('the CLI: 0 when clean, 1 on a finding, 2 on a usage error or an unreadable file', () => {
  const root = makeApp({});
  const clean = join(root, 'clean.dll');
  const dirty = join(root, 'dirty.dll');
  const dml = join(root, 'DirectML.dll');
  writeFileSync(clean, around('hello'));
  writeFileSync(dirty, around('aria.microsoft.com'));
  writeFileSync(dml, around(provider('Microsoft.Windows.AI.MachineLearning.Dml', TELEMETRY_GROUP)));
  const run = (...a) => spawnSync(process.execPath, [CLI, ...a], { encoding: 'utf8' });
  assert.equal(run(clean).status, 0);
  assert.match(run(clean).stdout, /no 1DS, curl, mbedTLS or collector host, and no provider in the Microsoft telemetry group/);
  assert.equal(run(dirty).status, 1);
  assert.equal(run(dml).status, 1);
  const allowed = run('--allow-telemetry-group', dml);
  assert.equal(allowed.status, 0, allowed.stdout);
  assert.match(allowed.stdout, /allowed here/);
  assert.equal(run().status, 2);
  assert.equal(run(join(root, 'missing')).status, 2);
});
