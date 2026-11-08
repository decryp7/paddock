// C99 layers 3 and 4 (ADR 0057 §3): what the dependencies of a shipped macOS build can reach.
//
// Layer 4, the graph probe: `cargo tree --target aarch64-apple-darwin` (features resolved per target) for every shipped
// Cargo.lock names the crates the macOS build links; `cargo metadata` supplies their folders. The keyring crates are not allowed in it at all (the one door, paddock-secrets, enables
// its `keyring` feature off macOS only). The Keychain binding crates (security-framework, security-framework-sys,
// objc2-security) are allowed only below a reviewed dependent (TLS trust, login items), so a new dependent that pulls one
// in is a finding until somebody reads what it calls.
// Layer 3, the dependency source scan: every other crate of that graph is read for the SecItem/SecKeychain symbols
// (tests, examples and benches of a dependency do not ship and are skipped).
// Positive controls (ER-33): a tree that does not parse, that names a package the metadata lacks, without a workspace member, or whose registry
// crates have no readable source (not fetched) is "check broken", never a pass.
import { spawnSync } from 'node:child_process';
import { readdirSync, readFileSync } from 'node:fs';
import { dirname, join } from 'node:path';

/** The macOS target the probe resolves for (any Apple triple resolves the same cfgs). */
export const MACOS_TARGET = 'aarch64-apple-darwin';

/** The Keychain symbols of Security.framework (SecItem, SecKeychain, certificate and PKCS12 import into a keychain). */
export const SYMBOLS = /\b(SecItemCopyMatching|SecItemAdd|SecItemUpdate|SecItemDelete|SecItemImport|SecKeychain[A-Z]\w*|SecCertificateAddToKeychain|SecPKCS12Import)\b/g;

/** Crates only paddock-secrets may use (and only off macOS). */
export const ONE_DOOR = new Set(['keyring', 'keyring-core', 'apple-native-keyring-store', 'windows-native-keyring-store', 'zbus-secret-service-keyring-store']);

/** Crates that bind the Keychain API. */
export const MACOS_ONLY_BANNED = new Set(['security-framework', 'security-framework-sys', 'objc2-security']);

/**
 * Crates reviewed to depend on a binding crate without touching a keychain item (2026-11-08): rustls-platform-verifier
 * (SecTrust evaluation), objc2-service-management (SMAppService login items), the bindings themselves and the hakari crate
 * (a feature list, no code). A crate not listed here that depends on a binding crate is a finding.
 */
export const REVIEWED_DEPENDENTS = new Set(['rustls-platform-verifier', 'objc2-service-management', 'paddock-workspace-hack', ...MACOS_ONLY_BANNED]);

const SOURCE = /\.(rs|c|m|h)$/;
const SKIP_DIRS = new Set(['tests', 'examples', 'benches', 'target', '.git']);

/** The source files of one crate (relative POSIX paths), without its tests, examples and benches. */
export function crateSources(dir) {
  const out = [];
  const walk = (d, rel) => {
    let entries;
    try {
      entries = readdirSync(d, { withFileTypes: true });
    } catch {
      return;
    }
    for (const e of entries) {
      if (e.isDirectory()) {
        if (!SKIP_DIRS.has(e.name)) walk(join(d, e.name), `${rel}${e.name}/`);
      } else if (e.isFile() && SOURCE.test(e.name)) out.push(`${rel}${e.name}`);
    }
  };
  walk(dir, '');
  return out.sort();
}

const label = (p) => `${p.name}@${p.version}`;

/**
 * Reads the two documents of a probe: `meta` (cargo metadata, for the packages, their folders and the workspace members) and
 * `tree` (the text of `cargo tree --prefix depth --format {p}` for the macOS target, which resolves features per target:
 * `cargo metadata` unifies them across targets, so a `keyring` feature enabled for Windows only would show up in it).
 * Returns { broken } when they cannot be trusted, else { crates, dependents }: the non-member packages of the tree, and for
 * each (key name@version) the names of the packages that depend on it.
 */
export function readGraph(meta, tree) {
  const byKey = new Map((meta?.packages ?? []).map((p) => [`${p.name}@${p.version}`, p]));
  const members = new Set((meta?.workspace_members ?? []).map((id) => (meta.packages ?? []).find((p) => p.id === id)?.name).filter(Boolean));
  if (members.size === 0) return { broken: 'the metadata lists no workspace member' };
  const lines = String(tree ?? '').split('\n').filter(Boolean);
  const stack = [];
  const dependents = new Map();
  const seen = new Map();
  for (const line of lines) {
    const m = /^(\d+)([^\s]+) v(\S+)/.exec(line);
    if (!m) return { broken: `the cargo tree output has a line this probe cannot read: ${line.slice(0, 80)}` };
    const depth = Number(m[1]);
    const key = `${m[2]}@${m[3]}`;
    stack.length = depth;
    stack[depth] = m[2];
    if (depth > 0) (dependents.get(key) ?? dependents.set(key, new Set()).get(key)).add(stack[depth - 1]);
    const pkg = byKey.get(key);
    if (!pkg) return { broken: `${key} is in the cargo tree but not in the cargo metadata` };
    seen.set(key, pkg);
  }
  if (![...seen.values()].some((p) => members.has(p.name))) return { broken: 'the cargo tree contains no workspace member' };
  return { crates: [...seen.values()].filter((p) => !members.has(p.name)), dependents, members: [...members] };
}

/** Layer 4: the keyring crates and unreviewed users of a binding crate in the macOS graph: [{ what }]. Pure. */
export function graphFindings(graph) {
  const out = [];
  for (const p of graph.crates) {
    if (ONE_DOOR.has(p.name)) {
      const by = [...new Set(graph.dependents.get(label(p)) ?? [])].join(', ') || 'the workspace';
      out.push({ what: `${label(p)} is in the macOS build (pulled in by ${by}); only paddock-secrets may use it, and only off macOS` });
    } else if (MACOS_ONLY_BANNED.has(p.name)) {
      const unreviewed = [...new Set(graph.dependents.get(label(p)) ?? [])].filter((d) => !REVIEWED_DEPENDENTS.has(d));
      if (unreviewed.length) out.push({ what: `${label(p)} is in the macOS build below ${unreviewed.join(', ')}, which have not been reviewed to leave keychain items alone` });
    }
  }
  return out;
}

/**
 * Layer 3: the Keychain symbols in the sources of the graph's dependencies: { findings: [{ what }], read, unfetched }.
 * `readSources(manifestPath)` returns [{ file, text }] (injectable); the default reads the crate folder.
 */
export function sourceScan(graph, readSources = defaultReadSources) {
  const findings = [];
  let read = 0;
  const unfetched = [];
  for (const p of graph.crates) {
    if (MACOS_ONLY_BANNED.has(p.name)) continue; // the bindings define the symbols; their dependents are reviewed above
    const files = readSources(p.manifest_path);
    if (files.length === 0) {
      if (p.source) unfetched.push(label(p));
      continue;
    }
    read += files.length;
    for (const { file, text } of files) {
      const seen = new Set();
      for (const m of text.matchAll(SYMBOLS)) seen.add(m[1]);
      for (const s of seen) findings.push({ what: `${label(p)} (${file}) uses ${s}` });
    }
  }
  return { findings, read, unfetched };
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

/**
 * Fetches and resolves one workspace for macOS: { meta, tree } or { broken }. `spawn` is injectable; `extra` adds cargo flags
 * (a test passes --offline). Needs a Cargo.lock (--locked: a lock that would change is a broken check, not a silent update).
 */
export function probeWorkspace(dir, { spawn = spawnSync, extra = [] } = {}) {
  const run = (args) => spawn('cargo', [...args, ...extra], { cwd: dir, encoding: 'utf8', maxBuffer: 256 << 20, timeout: 900_000 });
  const fail = (what, r) => ({ broken: `${what}: ${r.error ? r.error.message : `exit ${r.status}: ${String(r.stderr ?? '').trim().split('\n').slice(-3).join(' | ')}`}` });
  const fetched = run(['fetch', '--locked', '--target', MACOS_TARGET]);
  if (fetched.error || fetched.status !== 0) return fail('cargo fetch failed', fetched);
  const r = run(['metadata', '--locked', '--format-version', '1', '--filter-platform', MACOS_TARGET]);
  if (r.error || r.status !== 0) return fail('cargo metadata failed', r);
  const tree = run(['tree', '--workspace', '--locked', '--target', MACOS_TARGET, '--edges', 'normal,build', '--prefix', 'depth', '--format', '{p}']);
  if (tree.error || tree.status !== 0) return fail('cargo tree failed', tree);
  try {
    return { meta: JSON.parse(r.stdout), tree: tree.stdout };
  } catch (e) {
    return { broken: `cargo metadata printed no JSON (${e.message})` };
  }
}
