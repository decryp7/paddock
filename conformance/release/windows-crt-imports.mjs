#!/usr/bin/env node
// Fails when a Windows executable or DLL we ship imports the Visual C++ runtime (ADR 0032). VCRUNTIME140.dll, MSVCP140.dll
// and their kin are not part of Windows: on a PC without Microsoft's VC++ redistributable such a program does not start
// (0xC0000135, "STATUS_DLL_NOT_FOUND"). Every Paddock binary links the runtime statically (`+crt-static`, the root
// .cargo/config.toml); this check proves it on what was built. The Universal CRT (ucrtbase.dll, api-ms-win-crt-*) is part
// of Windows 10 and later and is allowed.
//
//   node conformance/release/windows-crt-imports.mjs <file-or-folder>...
//
// A file is read as PE whatever its name; a folder is searched recursively for *.exe and *.dll. Exit 0 when every file is
// clean, 1 when one imports the runtime (or is not a valid PE), 2 when nothing was found to check. Pure Node: runs on the
// Windows runner, the Linux image and a Mac alike, no dumpbin or objdump needed.

import { readdirSync, readFileSync, statSync } from 'node:fs';
import { extname, join } from 'node:path';
import { isMain } from '../src/util/is-main.mjs';

/**
 * DLLs of the Visual C++ redistributable (and its debug builds, which are never on a user's PC). The Universal CRT
 * (ucrtbase.dll, api-ms-win-crt-*.dll) and the system msvcrt.dll are part of Windows and are not listed.
 */
export const REDIST_DLL = /^(?:vcruntime\d+(?:_\d+)?|msvcp\d+(?:_[a-z0-9_]+)?|msvcr\d+|concrt\d+|vccorlib\d+|vcomp\d+|vcamp\d+|mfc\d+[a-z]*)d?\.dll$|^ucrtbased\.dll$/i;

/** Whether importing `dll` needs the VC++ redistributable. */
export function needsRedist(dll) {
  return REDIST_DLL.test(dll);
}

/** Bounds on untrusted input: more descriptors or sections than this is a broken file, not a real binary. */
const MAX_SECTIONS = 96;
const MAX_DESCRIPTORS = 4096;
const MAX_NAME = 260;

/**
 * The DLL names a PE file imports, normally and delay-loaded. Never throws: a file that is not a valid PE returns
 * `{ error }` instead.
 * @param {Uint8Array} bytes
 * @returns {{ imports: string[], delayImports: string[] } | { error: string }}
 */
export function peImports(bytes) {
  try {
    const b = Buffer.from(bytes.buffer, bytes.byteOffset, bytes.byteLength);
    const u16 = (o) => (o >= 0 && o + 2 <= b.length ? b.readUInt16LE(o) : null);
    const u32 = (o) => (o >= 0 && o + 4 <= b.length ? b.readUInt32LE(o) : null);
    if (u16(0) !== 0x5a4d) return { error: 'no MZ header' };
    const pe = u32(0x3c);
    if (pe === null || u32(pe) !== 0x00004550) return { error: 'no PE signature' };
    const sections = u16(pe + 6);
    const optSize = u16(pe + 20);
    if (sections === null || optSize === null || sections > MAX_SECTIONS) return { error: 'bad COFF header' };
    const opt = pe + 24;
    const magic = u16(opt);
    if (magic !== 0x10b && magic !== 0x20b) return { error: 'unknown optional header' };
    const plus = magic === 0x20b;
    const dirCount = u32(opt + (plus ? 108 : 92));
    const dirs = opt + (plus ? 112 : 96);
    const dir = (i) => (dirCount !== null && i < dirCount && (i + 1) * 8 <= optSize - (dirs - opt) ? { rva: u32(dirs + i * 8), size: u32(dirs + i * 8 + 4) } : null);

    const table = opt + optSize;
    const secs = [];
    for (let i = 0; i < sections; i++) {
      const s = table + i * 40;
      const va = u32(s + 12), vsize = u32(s + 8), raw = u32(s + 20), rawSize = u32(s + 16);
      if (va === null || raw === null) return { error: 'section table cut off' };
      secs.push({ va, size: Math.max(vsize, rawSize), raw });
    }
    const off = (rva) => {
      for (const s of secs) if (rva >= s.va && rva < s.va + s.size) return s.raw + (rva - s.va);
      return null;
    };
    const name = (rva) => {
      const o = off(rva);
      if (o === null || o >= b.length) return null;
      const end = b.indexOf(0, o);
      if (end < 0 || end - o > MAX_NAME || end === o) return null;
      return b.toString('latin1', o, end);
    };
    const walk = (entry, step, nameAt) => {
      const out = [];
      const d = dir(entry);
      if (!d || !d.rva) return out;
      let o = off(d.rva);
      if (o === null) throw new Error(`directory ${entry} outside every section`);
      for (let n = 0; n < MAX_DESCRIPTORS; n++, o += step) {
        if (o + step > b.length) throw new Error(`directory ${entry} cut off`);
        if (b.subarray(o, o + step).every((x) => x === 0)) return out;
        const dll = name(u32(o + nameAt));
        if (dll === null) throw new Error(`directory ${entry} names an unreadable DLL`);
        out.push(dll);
      }
      throw new Error(`directory ${entry} has no end`);
    };
    return { imports: walk(1, 20, 12), delayImports: walk(13, 32, 4) };
  } catch (e) {
    return { error: e.message };
  }
}

/** Every *.exe and *.dll below `paths` (files are taken as given). */
export function collect(paths) {
  const out = [];
  const visit = (p, explicit) => {
    let st;
    try {
      st = statSync(p);
    } catch {
      if (explicit) out.push(p); // reported as unreadable by check()
      return;
    }
    if (st.isDirectory()) {
      for (const e of readdirSync(p).sort()) visit(join(p, e), false);
    } else if (explicit || ['.exe', '.dll'].includes(extname(p).toLowerCase())) {
      out.push(p);
    }
  };
  for (const p of paths) visit(p, true);
  return out;
}

/**
 * Checks `files`; returns one line per finding (empty when every file is clean).
 * @param {string[]} files
 */
export function check(files) {
  const findings = [];
  for (const f of files) {
    let bytes;
    try {
      bytes = readFileSync(f);
    } catch (e) {
      findings.push(`${f}: cannot be read (${e.code ?? e.message})`);
      continue;
    }
    const r = peImports(bytes);
    if ('error' in r) {
      findings.push(`${f}: not a valid PE file (${r.error})`);
      continue;
    }
    const bad = [...r.imports, ...r.delayImports].filter(needsRedist);
    if (bad.length) findings.push(`${f}: imports ${[...new Set(bad)].join(', ')} (the VC++ redistributable; link with +crt-static, ADR 0032)`);
  }
  return findings;
}

if (isMain(import.meta.url)) {
  const args = process.argv.slice(2);
  if (!args.length || args.includes('--help')) {
    console.error('usage: windows-crt-imports.mjs <file-or-folder>...');
    process.exit(2);
  }
  const files = collect(args);
  if (!files.length) {
    console.error(`::error::no .exe or .dll found in ${args.join(', ')}`);
    process.exit(2);
  }
  const findings = check(files);
  for (const f of findings) console.log(`::error::${f}`);
  if (findings.length) process.exit(1);
  console.log(`${files.length} Windows binaries checked: none needs the VC++ redistributable.`);
}
