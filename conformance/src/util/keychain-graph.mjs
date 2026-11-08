// C99 layers 3 and 4 (ADR 0057 §3): what the dependencies of a shipped macOS build can reach.
//
// Layer 4, the graph probe: `cargo tree --target <Apple triple> --all-features` (features resolved per target) for each
// workspace a paddock.ci.json names, plus crates/ (without --all-features: its crates define the optional features), on
// both Apple triples. `cargo metadata` supplies the packages and their folders. In the macOS graph:
//   - the keyring crates are not allowed at all (the one door, paddock-secrets, enables `keyring` off macOS only);
//   - a Keychain binding crate (security-framework, security-framework-sys, objc2-security) is allowed only below a parent
//     reviewed for it, at the reviewed major series (BINDING_PARENTS), so a new parent or a new series is a finding until
//     somebody reads what it calls;
//   - objc2-security must not have a Sec* Keychain feature on (the SecItem/SecKeychain items are feature-gated).
// Layer 3, the dependency source scan: every non-binding crate of that graph is read for the SecItem/SecKeychain symbols
// (.rs .c .cc .cpp .m .mm .h .swift; a dependency's tests, examples and benches at its crate root do not ship). A crate
// that ships a prebuilt binary (.a .dylib .framework ...) cannot be read: it is listed as "binary, not scanned".
// Positive controls (ER-33): a tree that does not parse, that names a package the metadata lacks, two packages with one
// name@version, no workspace member, or registry crates with no readable source (not fetched) are "check broken", never a pass.
import { spawnSync } from 'node:child_process';
import { readdirSync, readFileSync } from 'node:fs';
import { dirname, join } from 'node:path';

/** The Apple triples the probe resolves (both resolve the same cfgs today; a cfg on the architecture would differ). */
export const MACOS_TARGETS = ['aarch64-apple-darwin', 'x86_64-apple-darwin'];

/** The Keychain symbols of Security.framework (SecItem, SecKeychain, certificate and PKCS12 import into a keychain). */
export const SYMBOLS = /\b(SecItemCopyMatching|SecItemAdd|SecItemUpdate|SecItemDelete|SecItemImport|SecKeychain[A-Z]\w*|SecCertificateAddToKeychain|SecPKCS12Import)\b/g;

/** Crates only paddock-secrets may use (and only off macOS). */
export const ONE_DOOR = new Set(['keyring', 'keyring-core', 'apple-native-keyring-store', 'windows-native-keyring-store', 'zbus-secret-service-keyring-store']);

/** Crates that bind the Keychain API. */
export const MACOS_ONLY_BANNED = new Set(['security-framework', 'security-framework-sys', 'objc2-security']);

/**
 * The parents reviewed to depend on each binding crate without touching a keychain item (2026-11-08), with the reviewed
 * series of the parent ("0.7" for 0.7.x, "3" for 3.x): rustls-platform-verifier (SecTrust evaluation),
 * objc2-service-management (SMAppService login items), paddock-workspace-hack (a feature list, no code), and the bindings
 * on each other. A parent outside its row, or at another series, is a finding.
 */
export const BINDING_PARENTS = {
  'security-framework': { 'rustls-platform-verifier': '0.7' },
  'security-framework-sys': { 'security-framework': '3', 'rustls-platform-verifier': '0.7', 'paddock-workspace-hack': '0.1' },
  'objc2-security': { 'objc2-service-management': '0.3' },
};

/**
 * Crates that ship prebuilt binaries which cannot hold a Keychain call (reviewed 2026-11-08): ring's pregenerated assembler
 * objects of its crypto routines, evtx's and vcpkg's sample Windows DLLs. Any other crate with a binary is listed as "binary, not scanned".
 */
export const REVIEWED_BINARIES = new Set(['ring', 'evtx', 'vcpkg']);

/** An objc2-security feature that switches a Keychain item on (the crate gates each Sec* item behind its own feature). */
export const KEYCHAIN_FEATURE = /^Sec|Keychain/;

/** The reviewed series of a version: 0.y.z gives "0.y", x.y.z gives "x". */
export const series = (version) => (version.startsWith('0.') ? version.split('.').slice(0, 2).join('.') : version.split('.')[0]);

const SOURCE = /\.(rs|c|cc|cpp|m|mm|h|swift)$/;
const BINARY = /\.(a|dylib|so|lib|o|dll|framework|xcframework)$/;
const SKIP_ROOT_DIRS = new Set(['tests', 'examples', 'benches', 'target']);

function walkCrate(dir, accept) {
  const out = [];
  const walk = (d, rel) => {
    let entries;
    try {
      entries = readdirSync(d, { withFileTypes: true });
    } catch {
      return;
    }
    for (const e of entries) {
      if (e.name === '.git') continue;
      if (e.isDirectory()) {
        if (rel === '' && SKIP_ROOT_DIRS.has(e.name)) continue;
        if (BINARY.test(e.name) && accept === BINARY) out.push(`${rel}${e.name}`); // a bundle: .framework, .xcframework
        walk(join(d, e.name), `${rel}${e.name}/`);
      } else if (e.isFile() && accept.test(e.name)) out.push(`${rel}${e.name}`);
    }
  };
  walk(dir, '');
  return out.sort();
}

/** The source files of one crate (relative POSIX paths); tests, examples and benches only count below src/ and friends. */
export const crateSources = (dir) => walkCrate(dir, SOURCE);

/** The prebuilt binaries one crate ships (relative POSIX paths), which no text scan can read. */
export const crateBinaries = (dir) => walkCrate(dir, BINARY);

const label = (p) => `${p.name}@${p.version}`;

/**
 * Reads the two documents of a probe: `meta` (cargo metadata: packages, folders, workspace members) and `tree` (the text of
 * `cargo tree --prefix depth --format "{p}|{f}"` for one Apple target, which resolves features per target: `cargo metadata`
 * unifies them across targets, so a `keyring` feature enabled for Windows only would show up in it).
 * Returns { broken } when they cannot be trusted, else { crates, parents, features, members }: the non-member packages of
 * the tree, for each key (name@version) the keys of the packages that depend on it, and its enabled features.
 */
export function readGraph(meta, tree) {
  const packages = meta?.packages ?? [];
  const byKey = new Map();
  for (const p of packages) {
    const k = label(p);
    if (byKey.has(k) && byKey.get(k).id !== p.id) return { broken: `two packages are named ${k} (${byKey.get(k).id} and ${p.id}), so the tree cannot say which one it means` };
    byKey.set(k, p);
  }
  const members = new Set((meta?.workspace_members ?? []).map((id) => packages.find((p) => p.id === id)?.name).filter(Boolean));
  if (members.size === 0) return { broken: 'the metadata lists no workspace member' };
  const stack = [];
  const parents = new Map();
  const features = new Map();
  const seen = new Map();
  for (const line of String(tree ?? '').split('\n').filter(Boolean)) {
    const m = /^(\d+)([^\s]+) v(\S+)(?: \([^)]*\))?\|(.*?)(?: \(\*\))?$/.exec(line);
    if (!m) return { broken: `the cargo tree output has a line this probe cannot read: ${line.slice(0, 80)}` };
    const depth = Number(m[1]);
    const key = `${m[2]}@${m[3]}`;
    stack.length = depth;
    stack[depth] = key;
    if (depth > 0) (parents.get(key) ?? parents.set(key, new Set()).get(key)).add(stack[depth - 1]);
    const pkg = byKey.get(key);
    if (!pkg) return { broken: `${key} is in the cargo tree but not in the cargo metadata` };
    seen.set(key, pkg);
    const f = features.get(key) ?? features.set(key, new Set()).get(key);
    for (const x of m[4].split(',').filter(Boolean)) f.add(x);
  }
  if (![...seen.values()].some((p) => members.has(p.name))) return { broken: 'the cargo tree contains no workspace member' };
  return { crates: [...seen.values()].filter((p) => !members.has(p.name)), parents, features, members: [...members] };
}

/**
 * Layer 4: the keyring crates, unreviewed parents of a binding crate and Keychain features of objc2-security in the macOS
 * graph: [{ key, what }]. `key` names the finding (it is the waiver path suffix: `Cargo.lock#<key>`). Pure.
 */
export function graphFindings(graph) {
  const out = [];
  const nameOf = (k) => k.slice(0, k.lastIndexOf('@'));
  for (const p of graph.crates) {
    const k = label(p);
    const parents = [...(graph.parents.get(k) ?? [])];
    if (ONE_DOOR.has(p.name)) {
      const by = parents.map(nameOf).join(', ') || 'the workspace';
      out.push({ key: p.name, what: `${k} is in the macOS build (pulled in by ${by}); only paddock-secrets may use it, and only off macOS` });
    } else if (MACOS_ONLY_BANNED.has(p.name)) {
      const reviewed = BINDING_PARENTS[p.name] ?? {};
      for (const par of parents) {
        const [pn, pv] = [nameOf(par), par.slice(par.lastIndexOf('@') + 1)];
        if (reviewed[pn] === undefined) out.push({ key: `${p.name}<-${pn}`, what: `${k} is in the macOS build below ${par}, which has not been reviewed to leave keychain items alone` });
        else if (reviewed[pn] !== series(pv)) out.push({ key: `${p.name}<-${pn}`, what: `${k} is in the macOS build below ${par}; only the ${reviewed[pn]} series of ${pn} was reviewed` });
      }
      if (p.name === 'objc2-security') {
        const on = [...(graph.features.get(k) ?? [])].filter((f) => KEYCHAIN_FEATURE.test(f));
        if (on.length) out.push({ key: 'objc2-security:features', what: `${k} has the Keychain features ${on.join(', ')} on in the macOS build` });
      }
    }
  }
  return out;
}

/**
 * Layer 3: the Keychain symbols in the sources of the graph's dependencies: { findings: [{ key, what }], read, unfetched,
 * binaries }. `readSources(manifestPath)` returns [{ file, text }] and `listBinaries(manifestPath)` the binaries (both
 * injectable). The binding crates define the symbols and are skipped unless `scanBindings` is set (the positive control
 * scans them to prove the scan sees a real call).
 */
export function sourceScan(graph, { readSources = defaultReadSources, listBinaries = defaultListBinaries, scanBindings = false } = {}) {
  const findings = [];
  const binaries = [];
  let read = 0;
  const unfetched = [];
  for (const p of graph.crates) {
    const bins = REVIEWED_BINARIES.has(p.name) ? [] : listBinaries(p.manifest_path);
    if (bins.length) binaries.push(`${label(p)}: ${bins.slice(0, 3).join(', ')}${bins.length > 3 ? ` and ${bins.length - 3} more` : ''}`);
    if (!scanBindings && MACOS_ONLY_BANNED.has(p.name)) continue; // their parents are reviewed in graphFindings
    const files = readSources(p.manifest_path);
    if (files.length === 0) {
      if (p.source) unfetched.push(label(p));
      continue;
    }
    read += files.length;
    for (const { file, text } of files) {
      const seen = new Set();
      for (const m of text.matchAll(SYMBOLS)) seen.add(m[1]);
      for (const s of seen) findings.push({ key: `src:${label(p)}:${s}`, what: `${label(p)} (${file}) uses ${s}` });
    }
  }
  return { findings, read, unfetched, binaries };
}

function defaultReadSources(manifestPath) {
  const dir = dirname(manifestPath);
  return crateSources(dir).flatMap((file) => {
    try {
      return [{ file, text: readFileSync(join(dir, file), 'utf8') }];
    } catch {
      return [];
    }
  });
}

const defaultListBinaries = (manifestPath) => crateBinaries(dirname(manifestPath));

/**
 * Fetches and resolves one workspace for each Apple target: { meta, runs: [{ target, tree }] } or { broken }. `spawn` is
 * injectable; `extra` adds cargo flags (a test passes --offline); `targets` replaces the Apple triples (the Windows control);
 * `allFeatures` is off for crates/. Needs a Cargo.lock (--locked: a lock that would change is a broken check).
 */
export function probeWorkspace(dir, { spawn = spawnSync, extra = [], targets = MACOS_TARGETS, allFeatures = true, env = process.env } = {}) {
  const run = (args) => spawn('cargo', [...args, ...extra], { cwd: dir, encoding: 'utf8', maxBuffer: 256 << 20, timeout: 900_000, env });
  const fail = (what, r) => ({ broken: `${what}: ${r.error ? r.error.message : `exit ${r.status}: ${String(r.stderr ?? '').trim().split('\n').slice(-3).join(' | ')}`}` });
  const feat = allFeatures ? ['--all-features'] : [];
  for (const t of targets) {
    const f = run(['fetch', '--locked', '--target', t]);
    if (f.error || f.status !== 0) return fail(`cargo fetch --target ${t} failed`, f);
  }
  const r = run(['metadata', '--locked', '--format-version', '1', ...feat]);
  if (r.error || r.status !== 0) return fail('cargo metadata failed', r);
  let meta;
  try {
    meta = JSON.parse(r.stdout);
  } catch (e) {
    return { broken: `cargo metadata printed no JSON (${e.message})` };
  }
  const runs = [];
  for (const t of targets) {
    const tree = run(['tree', '--workspace', '--locked', '--target', t, '--edges', 'normal,build', '--prefix', 'depth', '--format', '{p}|{f}', ...feat]);
    if (tree.error || tree.status !== 0) return fail(`cargo tree --target ${t} failed`, tree);
    runs.push({ target: t, tree: tree.stdout });
  }
  return { meta, runs };
}
