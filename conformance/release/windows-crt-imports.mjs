#!/usr/bin/env node
// Fails when a Windows executable or DLL we ship imports the Visual C++ runtime (ADR 0032). VCRUNTIME140.dll, MSVCP140.dll
// and their kin are not part of Windows: on a PC without Microsoft's VC++ redistributable such a program does not start
// (0xC0000135, "STATUS_DLL_NOT_FOUND"). Every Paddock binary links the runtime statically (`+crt-static`, the root
// .cargo/config.toml); this check proves it on what was built. The Universal CRT (ucrtbase.dll, api-ms-win-crt-*) is part
// of Windows 10 and later and is allowed.
//
//   node conformance/release/windows-crt-imports.mjs --src-tauri <dir> --bin-dir <dir> [--config <file.json>]...
//   node conformance/release/windows-crt-imports.mjs <file-or-folder>...
//
// The first form checks exactly what the installer ships, named by the app's Tauri configuration (tauri.conf.json, then
// tauri.windows.conf.json, then each --config in order, merged like Tauri does): the main binary (mainBinaryName, else
// Cargo's default-run, else the package name) and every bundle.externalBin from --bin-dir, the folder `tauri build` wrote
// (<build folder>/<profile>), and every .exe and .dll among bundle.resources (relative to --src-tauri). A missing main
// binary or sidecar is an error; other files in the build folder (stale ones a kept runner folder still holds) are never
// read. The second form checks the given files and, recursively, the .exe and .dll files of the given folders (symbolic
// links to folders are not followed). Exit 0 when every file is clean, 1 on a finding (an import of the runtime, a file
// that is missing, unreadable or not a valid PE), 2 when nothing was found to check. Pure Node: runs on the Windows
// runner, the Linux image and a Mac alike, no dumpbin or objdump needed.
//
// Limits: a DLL loaded at run time with LoadLibrary is not in any import table, so this check cannot see what it needs;
// such a DLL must be shipped and listed (bundle.resources) to be checked itself.

import { existsSync, lstatSync, readdirSync, readFileSync, statSync } from 'node:fs';
import { basename, dirname, extname, join } from 'node:path';
import { isMain } from '../src/util/is-main.mjs';

/**
 * DLLs of the Visual C++ redistributable: a family name, its version and any suffix (`vcruntime140_1`,
 * `vcruntime140_threads`, `msvcp140_atomic_wait`, `mfc140u`, `mfcm140u`) and the debug builds, which are never on a
 * user's PC (`msvcp140d`, `msvcp140d_atomic_wait`, `vcruntime140_1d`, `ucrtbased`). The Universal CRT (ucrtbase.dll,
 * api-ms-win-crt-*.dll) and the system msvcrt.dll are part of Windows and are not listed.
 */
export const REDIST_DLL = /^(?:(?:vcruntime|msvcp|msvcr|concrt|vccorlib|vcomp|vcamp|mfcm?)\d+[a-z0-9_]*|ucrtbased)\.dll$/i;

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
    const walk = (entry, step, nameAt, vaBased = () => false) => {
      const out = [];
      const d = dir(entry);
      if (!d || !d.rva) return out;
      let o = off(d.rva);
      if (o === null) throw new Error(`directory ${entry} outside every section`);
      for (let n = 0; n < MAX_DESCRIPTORS; n++, o += step) {
        if (o + step > b.length) throw new Error(`directory ${entry} cut off`);
        if (b.subarray(o, o + step).every((x) => x === 0)) return out;
        if (vaBased(o)) throw new Error('a delay-load descriptor uses virtual addresses (the format before Visual C++ 7), which this check does not read: inspect the file with dumpbin /imports');
        const dll = name(u32(o + nameAt));
        if (dll === null) throw new Error(`directory ${entry} names an unreadable DLL`);
        out.push(dll);
      }
      throw new Error(`directory ${entry} has no end`);
    };
    // Delay-load descriptors: bit 0 of Attributes set means the fields are RVAs; clear is the old VA-based format.
    return { imports: walk(1, 20, 12), delayImports: walk(13, 32, 4, (o) => (u32(o) & 1) === 0) };
  } catch (e) {
    return { error: e.message };
  }
}

const isBinary = (p) => ['.exe', '.dll'].includes(extname(p).toLowerCase());

/**
 * Every *.exe and *.dll below `paths` (files are taken as given; symbolic links to folders are not followed). Never
 * throws: a path that cannot be read becomes a problem.
 * @param {string[]} paths
 * @param {{ readdirSync: typeof readdirSync }} [io] the folder lister, injectable so tests can make a folder unreadable under any user
 * @returns {{ files: string[], problems: string[] }}
 */
export function collect(paths, io = { readdirSync }) {
  const files = [];
  const problems = [];
  const visit = (p, explicit) => {
    let st;
    try {
      st = explicit ? statSync(p) : lstatSync(p);
    } catch (e) {
      problems.push(`${p}: cannot be read (${e.code ?? e.message})`);
      return;
    }
    if (st.isSymbolicLink()) {
      // Not followed when it is a folder (a loop, or a tree outside the bundle); a linked file is read like any other.
      let target;
      try {
        target = statSync(p);
      } catch (e) {
        problems.push(`${p}: a broken symbolic link (${e.code ?? e.message})`);
        return;
      }
      if (target.isFile() && isBinary(p)) files.push(p);
      return;
    }
    if (st.isDirectory()) {
      let entries;
      try {
        entries = io.readdirSync(p).sort();
      } catch (e) {
        problems.push(`${p}: cannot be listed (${e.code ?? e.message})`);
        return;
      }
      for (const e of entries) visit(join(p, e), false);
    } else if (explicit || isBinary(p)) {
      files.push(p);
    }
  };
  for (const p of paths) visit(p, true);
  return { files, problems };
}

/** RFC 7396 JSON merge patch, the way Tauri merges a platform or --config file over tauri.conf.json. */
export function mergePatch(target, patch) {
  if (patch === null || typeof patch !== 'object' || Array.isArray(patch)) return patch;
  const out = target !== null && typeof target === 'object' && !Array.isArray(target) ? { ...target } : {};
  for (const [k, v] of Object.entries(patch)) {
    if (v === null) delete out[k];
    else out[k] = mergePatch(out[k], v);
  }
  return out;
}

/** The names Cargo may give the app's binary, from its Cargo.toml: `default-run`, then the package name. */
export function cargoBinaryNames(cargoToml) {
  const pkg = /^\[package\]\s*\n([\s\S]*?)(?=^\[|(?![\s\S]))/m.exec(cargoToml)?.[1] ?? '';
  const value = (key) => new RegExp(`^${key}\\s*=\\s*"([^"]+)"`, 'm').exec(pkg)?.[1];
  return [value('default-run'), value('name')].filter(Boolean);
}

/** Paths a `bundle.resources` source names: a file, a folder (recursively), or a `*` pattern in its last segment. */
function resourcePaths(srcTauri, source) {
  const full = join(srcTauri, source);
  if (!basename(source).includes('*')) return existsSync(full) ? [full] : [];
  const re = new RegExp(`^${basename(source).split('*').map((x) => x.replace(/[.+?^${}()|[\]\\]/g, '\\$&')).join('.*')}$`);
  try {
    return readdirSync(dirname(full)).filter((f) => re.test(f)).sort().map((f) => join(dirname(full), f));
  } catch {
    return [];
  }
}

/**
 * The Windows binaries the installer of the app in `srcTauri` ships, built into `binDir`, from its Tauri configuration
 * (`configs`: extra --config files, merged after tauri.conf.json and tauri.windows.conf.json). Never throws.
 * @returns {{ files: string[], problems: string[] }}
 */
export function shippedFiles({ srcTauri, binDir, configs = [] }) {
  const problems = [];
  const read = (p, what) => {
    try {
      return readFileSync(p, 'utf8');
    } catch (e) {
      problems.push(`${p}: cannot read ${what} (${e.code ?? e.message})`);
      return null;
    }
  };
  let conf = {};
  const layers = [join(srcTauri, 'tauri.conf.json'), ...(existsSync(join(srcTauri, 'tauri.windows.conf.json')) ? [join(srcTauri, 'tauri.windows.conf.json')] : []), ...configs];
  for (const p of layers) {
    const text = read(p, 'the Tauri configuration');
    if (text === null) continue;
    try {
      conf = mergePatch(conf, JSON.parse(text));
    } catch (e) {
      problems.push(`${p}: not valid JSON (${e.message})`);
    }
  }
  const files = [];
  const cargo = read(join(srcTauri, 'Cargo.toml'), 'the Cargo manifest');
  const names = [...new Set([conf.mainBinaryName, ...(cargo ? cargoBinaryNames(cargo) : [])].filter(Boolean))];
  const main = names.map((n) => join(binDir, `${n}.exe`)).find((p) => existsSync(p));
  if (main) files.push(main);
  else problems.push(`the app's binary is missing: none of ${names.map((n) => `${n}.exe`).join(', ') || '(no name found)'} in ${binDir}`);
  for (const e of conf.bundle?.externalBin ?? []) {
    const p = join(binDir, `${basename(e)}.exe`);
    if (existsSync(p)) files.push(p);
    else problems.push(`the sidecar ${e} is missing: no ${basename(e)}.exe in ${binDir}`);
  }
  const resources = conf.bundle?.resources;
  const sources = Array.isArray(resources) ? resources : Object.keys(resources ?? {});
  for (const source of sources) {
    for (const p of resourcePaths(srcTauri, source)) {
      const found = collect([p]);
      files.push(...found.files.filter(isBinary));
      problems.push(...found.problems);
    }
  }
  return { files, problems };
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

/** Parses the command line: `{ srcTauri, binDir, configs }` or `{ paths }`; throws on a usage error. */
export function parseArgs(argv) {
  const opts = { srcTauri: null, binDir: null, configs: [], paths: [] };
  for (let i = 0; i < argv.length; i++) {
    const a = argv[i];
    if (a === '--src-tauri' || a === '--bin-dir' || a === '--config') {
      const v = argv[++i];
      if (!v) throw new Error(`${a} needs a value`);
      if (a === '--config') opts.configs.push(v);
      else opts[a === '--src-tauri' ? 'srcTauri' : 'binDir'] = v;
    } else if (a.startsWith('--')) {
      throw new Error(`unknown option ${a}`);
    } else {
      opts.paths.push(a);
    }
  }
  if (!!opts.srcTauri !== !!opts.binDir) throw new Error('--src-tauri and --bin-dir go together');
  if (opts.srcTauri && opts.paths.length) throw new Error('give either --src-tauri/--bin-dir or paths, not both');
  if (!opts.srcTauri && !opts.paths.length) throw new Error('nothing to check');
  return opts;
}

if (isMain(import.meta.url)) {
  let opts;
  try {
    opts = parseArgs(process.argv.slice(2));
  } catch (e) {
    console.error(`${e.message}\nusage: windows-crt-imports.mjs --src-tauri <dir> --bin-dir <dir> [--config <file.json>]...\n       windows-crt-imports.mjs <file-or-folder>...`);
    process.exit(2);
  }
  const { files, problems } = opts.srcTauri ? shippedFiles(opts) : collect(opts.paths);
  const findings = [...problems, ...check(files)];
  for (const f of findings) console.log(`::error::${f}`);
  if (findings.length) process.exit(1);
  if (!files.length) {
    console.error(`::error::no .exe or .dll found to check`);
    process.exit(2);
  }
  console.log(`${files.length} Windows binaries checked: none needs the VC++ redistributable.`);
}
