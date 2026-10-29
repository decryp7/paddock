#!/usr/bin/env node
// Fails when a native library or binary we build or ship carries a way to send telemetry to Microsoft: Sighter's and
// Pacenotes' ONNX Runtime builds (Sighter ADR 0001 decision 4: built with --no_telemetry; the check proves it on the
// output) and the binaries that link them. It reads bytes only (no disassembly, no tools), in chunks, so a 400 MB static
// library is fine, and runs the same on every runner.
//
//   node conformance/release/telemetry-bytes.mjs [--allow-telemetry-group] <file>...
//
// A finding (exit 1), each searched as ASCII and as UTF-16LE:
//   - the 1DS SDK (cpp_client_telemetry): its namespace, plain and mangled (Itanium, MSVC);
//   - curl or mbedTLS, which ONNX Runtime 1.30+ links only to upload 1DS events;
//   - a Microsoft telemetry collector host (data.microsoft.com, aria.microsoft.com, telemetry.microsoft.com);
//   - a TraceLogging provider in the Microsoft telemetry provider group: the group GUID's bytes, as a provider's traits
//     store them. ONNX Runtime's public sources define that option as an all-zero group, so our builds must not have it;
//     Microsoft's DirectML.dll does, and its own gate decides about it, so --allow-telemetry-group reports it instead.
// Reported only: the ETW provider names a Windows ONNX Runtime build always compiles in, and DirectML's.
// Exit 0 when every file passes, 1 on a finding, 2 on a usage error or a file that cannot be read.

import { closeSync, openSync, readSync } from 'node:fs';
import { isMain } from '../src/util/is-main.mjs';

/** Strings that must never be present, with why. */
export const FORBIDDEN = [
  ['Microsoft::Applications::Events', 'the 1DS telemetry SDK'],
  ['9Microsoft12Applications6Events', 'the 1DS telemetry SDK (Itanium-mangled)'],
  ['@Events@Applications@Microsoft@@', 'the 1DS telemetry SDK (MSVC-mangled)'],
  ['curl_easy_perform', 'curl (1DS uploads)'],
  ['mbedtls_ssl_handshake', 'mbedTLS (1DS uploads)'],
  ['data.microsoft.com', 'a Microsoft telemetry collector host'],
  ['aria.microsoft.com', 'a Microsoft telemetry collector host'],
  ['telemetry.microsoft.com', 'a Microsoft telemetry collector host'],
];

/** ETW provider names reported, never a finding. */
export const NOTED = ['Microsoft.ML.ONNXRuntime', 'ONNXRuntimeTraceLoggingProvider', 'Microsoft.Windows.AI.MachineLearning.Dml'];

/** {4f50731a-89cf-4782-b3e0-dce8c90476ba}, the Microsoft telemetry provider group, in its in-memory byte order. */
export const TELEMETRY_GROUP = Buffer.from('1a73504fcf898247b3e0dce8c90476ba', 'hex');

const CHUNK = 16 << 20;

const patterns = () => {
  const out = [];
  const both = (s, kind, why) => {
    out.push({ s, kind, why, bytes: Buffer.from(s, 'latin1') });
    out.push({ s, kind, why: `${why}, UTF-16`, bytes: Buffer.from(s, 'utf16le') });
  };
  for (const [s, why] of FORBIDDEN) both(s, 'forbidden', why);
  for (const s of NOTED) both(s, 'noted', 'an ETW provider name');
  out.push({ s: 'Microsoft telemetry provider group', kind: 'group', why: 'a TraceLogging provider in the Microsoft telemetry group', bytes: TELEMETRY_GROUP });
  return out;
};

/**
 * Scans `read(buf, position)` (returns the bytes read) in overlapping chunks; returns the patterns found.
 * @returns {Set<object>}
 */
function scan(read) {
  const pats = patterns();
  const overlap = Math.max(...pats.map((p) => p.bytes.length)) - 1;
  const buf = Buffer.alloc(CHUNK + overlap);
  const found = new Set();
  let carry = 0;
  let pos = 0;
  for (;;) {
    const n = read(buf.subarray(carry, carry + CHUNK), pos);
    if (n <= 0) break;
    pos += n;
    const view = buf.subarray(0, carry + n);
    for (const p of pats) if (!found.has(p) && view.indexOf(p.bytes) >= 0) found.add(p);
    carry = Math.min(overlap, view.length);
    view.copy(buf, 0, view.length - carry);
  }
  return found;
}

/**
 * The result for one buffer or file's bytes.
 * @returns {{ findings: string[], notes: string[] }}
 */
function judge(found, { allowTelemetryGroup = false } = {}) {
  const findings = [];
  const notes = [];
  for (const p of found) {
    if (p.kind === 'forbidden') findings.push(`${p.s} (${p.why})`);
    else if (p.kind === 'noted') notes.push(`ETW provider name ${p.s}${p.why.endsWith('UTF-16') ? ' (UTF-16)' : ''}`);
    else if (allowTelemetryGroup) notes.push(`${p.why} (allowed here: its own gate decides)`);
    else findings.push(p.why);
  }
  return { findings, notes };
}

/** Checks bytes in memory. */
export function checkBytes(bytes, opts) {
  return judge(
    scan((dst, pos) => {
      const src = bytes.subarray(pos, pos + dst.length);
      src.copy(dst);
      return src.length;
    }),
    opts,
  );
}

/** Checks a file, reading it in chunks. Never throws: an unreadable file is `{ error }`. */
export function checkFile(path, opts) {
  let fd;
  try {
    fd = openSync(path, 'r');
    return judge(
      scan((dst, pos) => readSync(fd, dst, 0, dst.length, pos)),
      opts,
    );
  } catch (e) {
    return { error: `cannot be read (${e.code ?? e.message})` };
  } finally {
    if (fd !== undefined) closeSync(fd);
  }
}

if (isMain(import.meta.url)) {
  const args = process.argv.slice(2);
  const allowTelemetryGroup = args[0] === '--allow-telemetry-group';
  const files = allowTelemetryGroup ? args.slice(1) : args;
  if (!files.length || files.some((f) => f.startsWith('--'))) {
    console.error('usage: telemetry-bytes.mjs [--allow-telemetry-group] <file>...');
    process.exit(2);
  }
  let status = 0;
  for (const f of files) {
    const r = checkFile(f, { allowTelemetryGroup });
    if (r.error) {
      console.log(`::error::${f}: ${r.error}`);
      status = Math.max(status, 2);
      continue;
    }
    for (const n of r.notes) console.log(`${f}: note: ${n}`);
    for (const x of r.findings) console.log(`::error::${f}: ${x}`);
    if (r.findings.length) status = Math.max(status, 1);
    else console.log(`${f}: no 1DS, curl, mbedTLS or collector host${allowTelemetryGroup ? '' : ', and no provider in the Microsoft telemetry group'}`);
  }
  process.exit(status);
}
